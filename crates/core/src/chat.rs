//! Chat replays: what viewers wrote during a stream, at each message's time in
//! the VOD. Kick's and Twitch's come from the sites' own web APIs (yt-dlp no
//! longer fetches Twitch's, and doesn't know Kick's), YouTube's from yt-dlp.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::fetch::VodInfo;
use crate::kick;
use crate::tools::YtDlp;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatMessage {
    /// Seconds into the VOD.
    pub time: f64,
    pub author: String,
    /// The author's name color, as `#rrggbb`, if the site gives one.
    pub color: Option<String>,
    pub parts: Vec<ChatPart>,
}

/// A run of a message: text, or an emote shown as an image.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ChatPart {
    Text {
        text: String,
    },
    /// `url` is the image on the emote provider's CDN (see `emotes`); `name`
    /// is what it reads as, which search matches.
    Emote {
        name: String,
        url: String,
    },
}

impl ChatMessage {
    /// The message as plain text, emotes by name, for the search index.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut after_emote = false;
        for part in &self.parts {
            let (s, emote) = match part {
                ChatPart::Text { text } => (text.as_str(), false),
                ChatPart::Emote { name, .. } => (name.as_str(), true),
            };
            // Emotes are words of their own, even when sent back to back.
            let joined = out.is_empty() || out.ends_with(char::is_whitespace) || s.starts_with(char::is_whitespace);
            if (emote || after_emote) && !joined {
                out.push(' ');
            }
            out.push_str(s);
            after_emote = emote;
        }
        out.trim().to_string()
    }
}

/// A VOD's chat, oldest first. `None` when the site has no chat replay for it
/// (a file on this computer, another site, or a YouTube video that wasn't a
/// livestream). Reports a 0..1 fraction while it pages through.
pub fn fetch(
    info: &VodInfo,
    yt_dlp: Option<&YtDlp>,
    work_dir: &Path,
    vod_id: i64,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(f64),
) -> Result<Option<Vec<ChatMessage>>> {
    let mut messages = match info.platform.as_str() {
        "kick" => Some(fetch_kick(info, cancel, &mut on_progress)?),
        "twitch" => Some(fetch_twitch(info, cancel, &mut on_progress)?),
        "youtube" => {
            fetch_youtube(info, yt_dlp.context("The video downloader isn't installed yet.")?, work_dir, vod_id)?
        }
        _ => None,
    };
    if let Some(messages) = &mut messages {
        messages.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
    Ok(messages)
}

/// Kick and Twitch API pages are a few tens of KB.
const PAGE_MAX_BYTES: u64 = 4 << 20;
/// How long to wait before asking again after the site says to slow down.
const RATE_LIMIT_WAIT: Duration = Duration::from_secs(5);

/// GETs or POSTs JSON, waiting and retrying a few times when rate limited.
fn api(url: &str, body: Option<&Value>, headers: &[(&str, &str)], cancel: &AtomicBool) -> Result<Value> {
    for attempt in 0.. {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::Canceled.into());
        }
        let agent = crate::net::agent();
        let resp = match body {
            Some(body) => {
                let mut req = agent.post(url).header("Content-Type", "application/json");
                for (k, v) in headers {
                    req = req.header(*k, *v);
                }
                req.config().http_status_as_error(false).build().send(body.to_string())
            }
            None => {
                let mut req = agent.get(url);
                for (k, v) in headers {
                    req = req.header(*k, *v);
                }
                req.config().http_status_as_error(false).build().call()
            }
        };
        let mut resp = resp.with_context(|| format!("contact {}", host(url)))?;
        let status = resp.status().as_u16();
        if status == 429 && attempt < 5 {
            std::thread::sleep(RATE_LIMIT_WAIT);
            continue;
        }
        if status != 200 {
            bail!("{} answered {status} when asked for the chat.", host(url));
        }
        let text = resp.body_mut().with_config().limit(PAGE_MAX_BYTES).read_to_string()?;
        return serde_json::from_str(&text).with_context(|| format!("unreadable chat from {}", host(url)));
    }
    unreachable!()
}

fn host(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

// Kick: the channel's chat history, read backward from the VOD's end in pages
// of 25. The history is the whole channel's, so the VOD's span is worked out
// from when it started (see `kick::chat_origin`) and its length.

fn fetch_kick(info: &VodInfo, cancel: &AtomicBool, on_progress: &mut dyn FnMut(f64)) -> Result<Vec<ChatMessage>> {
    let origin = kick::chat_origin(&info.platform_id)?;
    let duration = info.duration.filter(|d| *d > 0.0).context("Kick didn't say how long this VOD is.")?;
    let end = origin.start + duration;
    let mut cursor = (end * 1e6) as i64;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    loop {
        let url = format!("https://web.kick.com/api/v1/chat/{}/history?cursor={cursor}", origin.channel);
        let page = api(&url, None, &[], cancel)?;
        let (messages, next) = parse_kick_page(&page)?;
        let mut oldest = end;
        for (at, id, msg) in messages {
            oldest = oldest.min(at);
            if at >= origin.start && at <= end && seen.insert(id) {
                out.push(ChatMessage { time: at - origin.start, ..msg });
            }
        }
        on_progress(((end - oldest) / duration).clamp(0.0, 1.0));
        // Stop at the VOD's start, or when the history runs out or stops moving back.
        match next {
            Some(next) if next < cursor && oldest >= origin.start => cursor = next,
            _ => break,
        }
    }
    Ok(out)
}

/// A history page's messages, as (Unix time, id, message with time 0), and the cursor for the page before it.
#[allow(clippy::type_complexity)]
fn parse_kick_page(page: &Value) -> Result<(Vec<(f64, String, ChatMessage)>, Option<i64>)> {
    let data = page.get("data").context("unreadable chat from Kick")?;
    let mut out = Vec::new();
    for m in data.get("messages").and_then(Value::as_array).into_iter().flatten() {
        let s = |p: &str| m.pointer(p).and_then(Value::as_str);
        let (Some(id), Some(at), Some(content)) = (s("/id"), s("/created_at").and_then(parse_time), s("/content"))
        else {
            continue;
        };
        let msg = ChatMessage {
            time: 0.0,
            author: s("/sender/username").unwrap_or("?").to_string(),
            color: s("/sender/identity/color").filter(|c| is_color(c)).map(str::to_string),
            parts: kick_parts(content),
        };
        out.push((at, id.to_string(), msg));
    }
    let next = data.get("cursor").and_then(|c| c.as_str()?.parse().ok().or(c.as_i64()));
    Ok((out, next))
}

/// Splits Kick's `[emote:<id>:<name>]` codes out of a message.
fn kick_parts(content: &str) -> Vec<ChatPart> {
    let mut parts = Vec::new();
    let mut rest = content;
    while let Some(at) = rest.find("[emote:") {
        let code = &rest[at + 7..];
        let Some(close) = code.find(']') else { break };
        let Some((id, name)) = code[..close].split_once(':') else { break };
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            break;
        }
        push_text(&mut parts, &rest[..at]);
        parts.push(ChatPart::Emote {
            name: name.to_string(),
            url: format!("https://files.kick.com/emotes/{id}/fullsize"),
        });
        rest = &code[close + 1..];
    }
    push_text(&mut parts, rest);
    parts
}

/// Appends text to a message, leaving out Unicode tag characters, which
/// chatters add invisibly to get around duplicate-message filters.
fn push_text(parts: &mut Vec<ChatPart>, text: &str) {
    let text: String = text.chars().filter(|c| !('\u{E0000}'..='\u{E007F}').contains(c)).collect();
    if text.is_empty() {
        return;
    }
    match parts.last_mut() {
        Some(ChatPart::Text { text: last }) => last.push_str(&text),
        _ => parts.push(ChatPart::Text { text }),
    }
}

// Twitch: the GraphQL API its website uses, with the website's public client
// id. Paging with the cursors it returns fails an integrity check that only a
// browser passes, so this asks for the comments from each next second instead.

const TWITCH_GQL: &str = "https://gql.twitch.tv/gql";
const TWITCH_CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";
const TWITCH_QUERY: &str = "query($id: ID!, $offset: Int) { video(id: $id) { comments(contentOffsetSeconds: $offset) {
    edges { node { id contentOffsetSeconds commenter { displayName }
        message { userColor fragments { text emote { emoteID } } } } }
    pageInfo { hasNextPage } } } }";

fn fetch_twitch(info: &VodInfo, cancel: &AtomicBool, on_progress: &mut dyn FnMut(f64)) -> Result<Vec<ChatMessage>> {
    let id = info.platform_id.trim_start_matches('v');
    let duration = info.duration.filter(|d| *d > 0.0);
    let mut offset = 0i64;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    loop {
        let body = json!({ "query": TWITCH_QUERY, "variables": { "id": id, "offset": offset } });
        let page = api(TWITCH_GQL, Some(&body), &[("Client-ID", TWITCH_CLIENT_ID)], cancel)?;
        let (messages, more) = parse_twitch_page(&page)?;
        let last = messages.last().map_or(offset, |(_, m)| m.time as i64);
        for (id, msg) in messages {
            if seen.insert(id) {
                out.push(msg);
            }
        }
        if let Some(d) = duration {
            on_progress((last as f64 / d).clamp(0.0, 1.0));
        }
        if !more {
            break;
        }
        // A page that ends in the second it started from may not have held all
        // of that second's messages; that's rare, and moving on keeps this finite.
        offset = if last > offset { last } else { offset + 1 };
    }
    Ok(out)
}

fn parse_twitch_page(page: &Value) -> Result<(Vec<(String, ChatMessage)>, bool)> {
    if let Some(err) = page.pointer("/errors/0/message").and_then(Value::as_str) {
        bail!("Twitch wouldn't give the chat: {err}");
    }
    let video = page.pointer("/data/video").context("unreadable chat from Twitch")?;
    if video.is_null() {
        bail!("Twitch says this VOD doesn't exist. It may have been deleted.");
    }
    let mut out = Vec::new();
    for edge in video.pointer("/comments/edges").and_then(Value::as_array).into_iter().flatten() {
        let n = &edge["node"];
        let (Some(id), Some(time)) = (n["id"].as_str(), n["contentOffsetSeconds"].as_f64()) else { continue };
        let mut parts = Vec::new();
        for f in n.pointer("/message/fragments").and_then(Value::as_array).into_iter().flatten() {
            let text = f["text"].as_str().unwrap_or_default();
            match f.pointer("/emote/emoteID").and_then(Value::as_str).filter(|id| is_emote_id(id)) {
                Some(emote) => parts.push(ChatPart::Emote {
                    name: text.to_string(),
                    url: format!("https://static-cdn.jtvnw.net/emoticons/v2/{emote}/default/dark/1.0"),
                }),
                None => push_text(&mut parts, text),
            }
        }
        let msg = ChatMessage {
            time,
            author: n.pointer("/commenter/displayName").and_then(Value::as_str).unwrap_or("?").to_string(),
            color: n.pointer("/message/userColor").and_then(Value::as_str).filter(|c| is_color(c)).map(str::to_string),
            parts,
        };
        out.push((id.to_string(), msg));
    }
    let more = video.pointer("/comments/pageInfo/hasNextPage").and_then(Value::as_bool).unwrap_or(false);
    Ok((out, more))
}

/// Twitch emote ids are digits, or `emotesv2_` and hex.
fn is_emote_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

// YouTube: yt-dlp saves a past livestream's chat as a "live_chat" subtitle,
// one JSON action per line.

fn fetch_youtube(info: &VodInfo, yt_dlp: &YtDlp, work_dir: &Path, vod_id: i64) -> Result<Option<Vec<ChatMessage>>> {
    fs::create_dir_all(work_dir)?;
    let stem = work_dir.join(format!("{vod_id}.chat"));
    let file = work_dir.join(format!("{vod_id}.chat.live_chat.json"));
    let _ = fs::remove_file(&file);
    let out = yt_dlp
        .command()
        .args(["--skip-download", "--write-subs", "--sub-langs", "live_chat", "--no-playlist", "--no-warnings", "-o"])
        .arg(&stem)
        .arg("--")
        .arg(&info.url)
        .stdin(Stdio::null())
        .output()
        .context("run yt-dlp")?;
    if !out.status.success() {
        bail!("{}", crate::fetch::clean_error(&String::from_utf8_lossy(&out.stderr)));
    }
    // No file: the video has no chat replay (it wasn't a livestream).
    let Ok(text) = fs::read_to_string(&file) else { return Ok(None) };
    let _ = fs::remove_file(&file);
    Ok(Some(parse_youtube(&text)))
}

fn parse_youtube(text: &str) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(replay) = v.get("replayChatItemAction") else { continue };
        let Some(ms) = replay["videoOffsetTimeMsec"].as_str().and_then(|s| s.parse::<f64>().ok()) else { continue };
        for action in replay["actions"].as_array().into_iter().flatten() {
            let item = &action["addChatItemAction"]["item"];
            let Some(r) = item.get("liveChatTextMessageRenderer").or_else(|| item.get("liveChatPaidMessageRenderer"))
            else {
                continue;
            };
            let mut parts = Vec::new();
            for run in r.pointer("/message/runs").and_then(Value::as_array).into_iter().flatten() {
                if let Some(text) = run["text"].as_str() {
                    push_text(&mut parts, text);
                } else if let Some(emoji) = run.get("emoji") {
                    let name = emoji.pointer("/shortcuts/0").and_then(Value::as_str);
                    let url = emoji.pointer("/image/thumbnails/0/url").and_then(Value::as_str);
                    match (emoji["isCustomEmoji"].as_bool(), name, url) {
                        (Some(true), Some(name), Some(url)) if url.starts_with("https://") => {
                            parts.push(ChatPart::Emote { name: name.to_string(), url: url.to_string() })
                        }
                        // A standard emoji is its own character.
                        _ => push_text(&mut parts, emoji["emojiId"].as_str().unwrap_or_default()),
                    }
                }
            }
            if parts.is_empty() {
                continue;
            }
            out.push(ChatMessage {
                time: ms / 1000.0,
                author: r.pointer("/authorName/simpleText").and_then(Value::as_str).unwrap_or("?").to_string(),
                color: None,
                parts,
            });
        }
    }
    out
}

fn is_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Unix seconds for an RFC 3339 time such as `2026-09-28T21:59:54Z`,
/// `2026-09-28T21:33:22+00:00` or `2026-09-28T20:15:37.046Z`.
pub(crate) fn parse_time(s: &str) -> Option<f64> {
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, min, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if s.get(4..5)? != "-" || s.get(10..11)? != "T" || !(1..=12).contains(&month) {
        return None;
    }
    let mut rest = s.get(19..)?;
    let mut frac = 0.0;
    if let Some(digits) = rest.strip_prefix('.') {
        let n = digits.bytes().take_while(u8::is_ascii_digit).count();
        frac = format!("0.{}", &digits[..n]).parse().ok()?;
        rest = &digits[n..];
    }
    let offset = match rest {
        "Z" => 0,
        _ => {
            let sign = match rest.get(..1)? {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
            let (h, m) = rest.get(1..)?.split_once(':')?;
            sign * (h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60)
        }
    };
    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + hour * 3600 + min * 60 + sec - offset) as f64 + frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_time("2026-09-28T21:59:54Z"), Some(1790632794.0));
        assert_eq!(parse_time("2026-09-28T21:33:22+00:00"), Some(1790631202.0));
        assert_eq!(parse_time("2026-09-28T23:33:22+02:00"), Some(1790631202.0));
        assert_eq!(parse_time("2026-09-28T20:15:37.046Z"), Some(1790626537.046));
        assert_eq!(parse_time("2024-02-29T12:00:00Z"), Some(1709208000.0));
        assert_eq!(parse_time("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_time("yesterday"), None);
    }

    #[test]
    fn splits_kick_emotes() {
        let url = |id: &str| format!("https://files.kick.com/emotes/{id}/fullsize");
        assert_eq!(
            kick_parts("hi [emote:4085540:barkylove][emote:4085540:barkylove] bye"),
            [
                ChatPart::Text { text: "hi ".into() },
                ChatPart::Emote { name: "barkylove".into(), url: url("4085540") },
                ChatPart::Emote { name: "barkylove".into(), url: url("4085540") },
                ChatPart::Text { text: " bye".into() },
            ]
        );
        assert_eq!(kick_parts("[emote:x/../y:z] ok"), [ChatPart::Text { text: "[emote:x/../y:z] ok".into() }]);
        assert_eq!(kick_parts("[emote:12"), [ChatPart::Text { text: "[emote:12".into() }]);
        assert_eq!(kick_parts("oda \u{E0021}"), [ChatPart::Text { text: "oda ".into() }], "tag characters dropped");
    }

    #[test]
    fn reads_a_kick_history_page() {
        let page = json!({ "data": { "cursor": "1790631366565505", "messages": [
            { "id": "a", "content": "[emote:4085574:barkywave]hello", "created_at": "2026-09-28T21:30:01Z",
              "sender": { "username": "sunkai", "identity": { "color": "#F2708A" } } },
            { "id": "b", "content": "@x has subbed", "created_at": "2026-09-28T21:29:00Z",
              "sender": { "username": "KickBot", "identity": { "color": "red; x" } } },
        ] } });
        let (messages, next) = parse_kick_page(&page).unwrap();
        assert_eq!(next, Some(1790631366565505));
        assert_eq!(messages.len(), 2);
        let (at, id, m) = &messages[0];
        assert_eq!(
            (*at, id.as_str(), m.author.as_str(), m.color.as_deref()),
            (1790631001.0, "a", "sunkai", Some("#F2708A"))
        );
        assert_eq!(m.text(), "barkywave hello");
        assert_eq!(messages[1].2.color, None, "only #rrggbb colors");
    }

    #[test]
    fn reads_a_twitch_page() {
        let page = json!({ "data": { "video": { "comments": {
            "edges": [{ "node": { "id": "c", "contentOffsetSeconds": 526, "commenter": { "displayName": "swiftyspiffy" },
                "message": { "userColor": "#FF3AE9", "fragments": [
                    { "text": "item #4 is scary ", "emote": null },
                    { "text": "monkaS", "emote": { "emoteID": "emotesv2_3a2f" } },
                ] } } }],
            "pageInfo": { "hasNextPage": true } } } } });
        let (messages, more) = parse_twitch_page(&page).unwrap();
        assert!(more);
        let m = &messages[0].1;
        assert_eq!((m.time, m.author.as_str()), (526.0, "swiftyspiffy"));
        assert_eq!(m.text(), "item #4 is scary monkaS");
        assert!(matches!(&m.parts[1], ChatPart::Emote { url, .. }
            if url == "https://static-cdn.jtvnw.net/emoticons/v2/emotesv2_3a2f/default/dark/1.0"));

        let refused = json!({ "errors": [{ "message": "failed integrity check" }], "data": null });
        assert!(parse_twitch_page(&refused).unwrap_err().to_string().contains("integrity"));
    }

    #[test]
    fn reads_youtube_live_chat() {
        let line = |offset: &str, item: Value| {
            json!({ "replayChatItemAction": { "videoOffsetTimeMsec": offset,
                "actions": [{ "addChatItemAction": { "item": item } }] } })
            .to_string()
        };
        let text = [
            line(
                "12345",
                json!({ "liveChatTextMessageRenderer": {
                "authorName": { "simpleText": "@viewer" },
                "message": { "runs": [
                    { "text": "nice " },
                    { "emoji": { "emojiId": "UC/x", "isCustomEmoji": true, "shortcuts": [":yt:"],
                        "image": { "thumbnails": [{ "url": "https://yt3.ggpht.com/abc=w24-h24" }] } } },
                    { "emoji": { "emojiId": "😂", "shortcuts": [":joy:"] } },
                ] } } }),
            ),
            line("20000", json!({ "liveChatViewerEngagementMessageRenderer": {} })),
            "not json".into(),
        ]
        .join("\n");
        let messages = parse_youtube(&text);
        assert_eq!(messages.len(), 1);
        assert_eq!((messages[0].time, messages[0].author.as_str()), (12.345, "@viewer"));
        assert_eq!(messages[0].text(), "nice :yt: 😂");
    }
}
