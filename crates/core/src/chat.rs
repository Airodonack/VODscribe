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
    #[serde(default)]
    pub kind: Kind,
    /// For a Twitch sub, its tier (Prime counts as 1); for a gift, how many
    /// subs; for a tip, how many bits or KICKs. A Super Chat's amount is in its
    /// text instead, in whatever currency it was.
    #[serde(default)]
    pub amount: Option<f64>,
    /// When the event happened, if not when the message was sent: a Kick resub
    /// celebration can be shared long after the sub (see `resubs_announced`).
    #[serde(default)]
    pub event_time: Option<f64>,
}

/// What a chat message is: someone writing, or an event the site posts in
/// chat. Recognized from how each site words them (see `twitch_event`,
/// `kick_event` and `parse_youtube`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Message,
    /// A subscription or YouTube membership, new or renewed.
    Sub,
    /// Subs or memberships given to others.
    Gift,
    /// Bits (Twitch), KICKs (Kick), a Super Chat or Super Sticker (YouTube).
    Tip,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Message => "message",
            Kind::Sub => "sub",
            Kind::Gift => "gift",
            Kind::Tip => "tip",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "sub" => Kind::Sub,
            "gift" => Kind::Gift,
            "tip" => Kind::Tip,
            _ => Kind::Message,
        }
    }
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

/// Where a VOD's chat goes as it's fetched, a batch at a time, so it can be
/// shown while the rest arrives, and where the fetch has got to, so one that's
/// interrupted (quitting, a network error) carries on from there.
pub trait ChatStore {
    /// Where an interrupted fetch got to, if one did (a position `save` was given).
    fn resume(&mut self) -> Result<Option<f64>>;
    /// Forgets every saved message and position, to start over.
    fn clear(&mut self) -> Result<()>;
    /// Forgets the saved messages from `time` (seconds into the VOD) on.
    fn drop_from(&mut self, time: f64) -> Result<()>;
    /// Saves `messages` and that the fetch has reached `position`, together.
    fn save(&mut self, messages: &[ChatMessage], position: f64) -> Result<()>;
    /// The saved subs, as (id, message), for `resubs_announced`.
    fn subs(&mut self) -> Result<Vec<(i64, ChatMessage)>>;
    /// Makes these saved messages plain messages.
    fn demote(&mut self, ids: &[i64]) -> Result<()>;
}

/// Fetches a VOD's chat into `store`, carrying on from where an interrupted
/// fetch got to. False when the site has no chat replay for it (a file on this
/// computer, another site, or a YouTube video that wasn't a livestream).
/// Reports a 0..1 fraction while it pages through.
#[allow(clippy::too_many_arguments)]
pub fn fetch(
    info: &VodInfo,
    yt_dlp: Option<&YtDlp>,
    work_dir: &Path,
    vod_id: i64,
    store: &mut dyn ChatStore,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(f64),
) -> Result<bool> {
    match info.platform.as_str() {
        "kick" => fetch_kick(info, store, cancel, &mut on_progress)?,
        "twitch" => fetch_twitch(info, store, cancel, &mut on_progress)?,
        "youtube" => {
            let yt_dlp = yt_dlp.context("The video downloader isn't installed yet.")?;
            if !fetch_youtube(info, yt_dlp, work_dir, vod_id, store)? {
                return Ok(false);
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// A `ChatStore` in memory, for the developer CLI and tests.
#[derive(Default, Debug)]
pub struct MemoryChat {
    pub messages: Vec<ChatMessage>,
    pub position: Option<f64>,
}

impl ChatStore for MemoryChat {
    fn resume(&mut self) -> Result<Option<f64>> {
        Ok(self.position)
    }

    fn clear(&mut self) -> Result<()> {
        self.messages.clear();
        self.position = None;
        Ok(())
    }

    fn drop_from(&mut self, time: f64) -> Result<()> {
        self.messages.retain(|m| m.time < time);
        Ok(())
    }

    fn save(&mut self, messages: &[ChatMessage], position: f64) -> Result<()> {
        self.messages.extend_from_slice(messages);
        self.position = Some(position);
        Ok(())
    }

    fn subs(&mut self) -> Result<Vec<(i64, ChatMessage)>> {
        let subs = self.messages.iter().enumerate().filter(|(_, m)| m.kind == Kind::Sub);
        Ok(subs.map(|(i, m)| (i as i64, m.clone())).collect())
    }

    fn demote(&mut self, ids: &[i64]) -> Result<()> {
        for &id in ids {
            self.messages[id as usize].kind = Kind::Message;
        }
        Ok(())
    }
}

/// Collects messages and saves them every so often, rather than page by page.
struct Batch {
    messages: Vec<ChatMessage>,
    since: std::time::Instant,
}

impl Batch {
    const SIZE: usize = 500;
    const EVERY: Duration = Duration::from_secs(1);

    fn new() -> Self {
        Batch { messages: Vec::new(), since: std::time::Instant::now() }
    }

    /// Saves what's collected if there's enough or it's been a while (or `now`),
    /// with `position`, which must be where the fetch carries on after them.
    fn save(&mut self, store: &mut dyn ChatStore, position: f64, now: bool) -> Result<()> {
        if now || self.messages.len() >= Self::SIZE || self.since.elapsed() >= Self::EVERY {
            store.save(&self.messages, position)?;
            self.messages.clear();
            self.since = std::time::Instant::now();
        }
        Ok(())
    }
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

// Saved as it goes, with the next page's cursor as the position: a page holds
// only messages older than its cursor, so carrying on from it repeats none.

fn fetch_kick(
    info: &VodInfo,
    store: &mut dyn ChatStore,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(f64),
) -> Result<()> {
    let origin = kick::chat_origin(&info.platform_id)?;
    let duration = info.duration.filter(|d| *d > 0.0).context("Kick didn't say how long this VOD is.")?;
    let page = |cursor: i64| {
        let url = format!("https://web.kick.com/api/v1/chat/{}/history?cursor={cursor}", origin.channel);
        parse_kick_page(&api(&url, None, &[], cancel)?)
    };
    kick_into(store, origin.start, duration, page, on_progress)
}

/// Pages back through Kick's history into `store` (see `fetch_kick`); `page`
/// gets the page before a cursor. Apart, so tests can page through their own.
#[allow(clippy::type_complexity)]
fn kick_into(
    store: &mut dyn ChatStore,
    start: f64,
    duration: f64,
    mut page: impl FnMut(i64) -> Result<(Vec<(f64, String, ChatMessage, Option<f64>)>, Option<i64>)>,
    on_progress: &mut dyn FnMut(f64),
) -> Result<()> {
    let end = start + duration;
    let mut cursor = match store.resume()? {
        Some(position) => position as i64,
        None => {
            store.clear()?;
            (end * 1e6) as i64
        }
    };
    let mut batch = Batch::new();
    let mut seen = HashSet::new();
    loop {
        let (messages, next) = page(cursor)?;
        let mut oldest = (cursor as f64 / 1e6).min(end);
        for (at, id, msg, subbed_at) in messages {
            oldest = oldest.min(at);
            if at >= start && at <= end && seen.insert(id) {
                let event_time = subbed_at.map(|t| t - start);
                batch.messages.push(ChatMessage { time: at - start, event_time, ..msg });
            }
        }
        on_progress(((end - oldest) / duration).clamp(0.0, 1.0));
        // Stop at the VOD's start, or when the history runs out or stops moving back.
        match next {
            Some(next) if next < cursor && oldest >= start => {
                cursor = next;
                batch.save(store, next as f64, false)?;
            }
            _ => {
                batch.save(store, cursor as f64, true)?;
                break;
            }
        }
    }
    let announced = resubs_announced(&store.subs()?);
    store.demote(&announced)?;
    Ok(())
}

/// A history page's messages, as (Unix time, id, message with time 0, and for
/// a celebration, when its sub happened), and the cursor for the page before it.
#[allow(clippy::type_complexity)]
fn parse_kick_page(page: &Value) -> Result<(Vec<(f64, String, ChatMessage, Option<f64>)>, Option<i64>)> {
    let data = page.get("data").context("unreadable chat from Kick")?;
    let mut out = Vec::new();
    for m in data.get("messages").and_then(Value::as_array).into_iter().flatten() {
        let s = |p: &str| m.pointer(p).and_then(Value::as_str);
        let (Some(id), Some(at), Some(content)) = (s("/id"), s("/created_at").and_then(parse_time), s("/content"))
        else {
            continue;
        };
        let author = s("/sender/username").unwrap_or("?");
        let metadata = s("/metadata").and_then(|m| serde_json::from_str::<Value>(m).ok());
        let celebration = metadata.as_ref().and_then(|m| m.pointer("/celebration/type")?.as_str());
        let subbed_at = metadata.as_ref().and_then(|m| parse_time(m.pointer("/celebration/created_at")?.as_str()?));
        let (kind, amount) = kick_event(author, content, celebration);
        let msg = ChatMessage {
            time: 0.0,
            author: author.to_string(),
            color: s("/sender/identity/color").filter(|c| is_color(c)).map(str::to_string),
            parts: kick_parts(content),
            kind,
            amount,
            event_time: None,
        };
        out.push((at, id.to_string(), msg, subbed_at.filter(|_| kind == Kind::Sub)));
    }
    let next = data.get("cursor").and_then(|c| c.as_str()?.parse().ok().or(c.as_i64()));
    Ok((out, next))
}

/// What a Kick message is. Events come as KickBot's announcements, when the
/// channel has them on ("@name has subbed for 15 months!", "@name just gifted
/// 1,000 KICKs!"), and as a resubscriber's own message, marked as a celebration.
fn kick_event(author: &str, content: &str, celebration: Option<&str>) -> (Kind, Option<f64>) {
    if celebration == Some("subscription_renewed") {
        return (Kind::Sub, None);
    }
    if !author.eq_ignore_ascii_case("KickBot") {
        return (Kind::Message, None);
    }
    let after = |marker: &str| content.split_once(marker).map(|(_, rest)| leading_number(rest));
    if let Some(Some(n)) = after(" just gifted ").filter(|_| content.contains(" KICKs")) {
        return (Kind::Tip, Some(n));
    }
    if let Some(n) = after(" gifted ").filter(|_| content.contains(" sub")) {
        return (Kind::Gift, n.or(Some(1.0)));
    }
    if content.contains(" has subbed") || content.contains(" just subscribed") || content.contains(" has subscribed") {
        return (Kind::Sub, None);
    }
    (Kind::Message, None)
}

/// The number at the start of `text`, allowing thousands separators ("1,000").
fn leading_number(text: &str) -> Option<f64> {
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit() || *c == ',').filter(|c| *c != ',').collect();
    digits.parse().ok()
}

/// A resub can show twice on Kick: KickBot's announcement when it happens, and
/// the subscriber's celebration message, which they may send much later (its
/// `event_time` is when the sub happened). Of `subs`, the celebrations KickBot
/// announced, which should stay in chat as plain messages.
pub fn resubs_announced(subs: &[(i64, ChatMessage)]) -> Vec<i64> {
    let announced: Vec<(String, f64)> = subs
        .iter()
        .filter(|(_, m)| m.author.eq_ignore_ascii_case("KickBot"))
        .map(|(_, m)| (m.text().to_lowercase(), m.time))
        .collect();
    subs.iter()
        .filter_map(|(id, m)| {
            let subbed_at = m.event_time?;
            let who = format!("@{} ", m.author.to_lowercase());
            announced.iter().any(|(text, at)| (at - subbed_at).abs() < 120.0 && text.contains(&who)).then_some(*id)
        })
        .collect()
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

// Saved as it goes, with the next offset as the position. The next page starts
// at that second again, so carrying on drops what was saved from it first.

fn fetch_twitch(
    info: &VodInfo,
    store: &mut dyn ChatStore,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(f64),
) -> Result<()> {
    let id = info.platform_id.trim_start_matches('v');
    let page = |offset: i64| {
        let body = json!({ "query": TWITCH_QUERY, "variables": { "id": id, "offset": offset } });
        parse_twitch_page(&api(TWITCH_GQL, Some(&body), &[("Client-ID", TWITCH_CLIENT_ID)], cancel)?)
    };
    twitch_into(store, info.duration.filter(|d| *d > 0.0), page, on_progress)
}

/// Pages through Twitch's comments into `store` (see `fetch_twitch`); `page`
/// gets the page from an offset. Apart, so tests can page through their own.
#[allow(clippy::type_complexity)]
fn twitch_into(
    store: &mut dyn ChatStore,
    duration: Option<f64>,
    mut page: impl FnMut(i64) -> Result<(Vec<(String, ChatMessage)>, bool)>,
    on_progress: &mut dyn FnMut(f64),
) -> Result<()> {
    let mut offset = match store.resume()? {
        Some(position) => {
            store.drop_from(position)?;
            position as i64
        }
        None => {
            store.clear()?;
            0
        }
    };
    let mut batch = Batch::new();
    let mut seen = HashSet::new();
    let mut bombs = GiftBombs::default();
    loop {
        let (messages, more) = page(offset)?;
        let last = messages.last().map_or(offset, |(_, m)| m.time as i64);
        // A page that ends in the second it started from may not have held all
        // of that second's messages; that's rare, and moving on keeps this finite.
        let next = if last > offset { last } else { offset + 1 };
        for (id, mut msg) in messages {
            if seen.insert(id) {
                bombs.count_once(&mut msg);
                // Saved now only if before the next page's second, which that page brings again.
                batch.messages.push(msg);
            }
        }
        if let Some(d) = duration {
            on_progress((last as f64 / d).clamp(0.0, 1.0));
        }
        if !more {
            batch.save(store, next as f64, true)?;
            break;
        }
        // Keep the next second's messages for the batch after, so a save's position
        // is never in the middle of what it saved.
        let carry: Vec<ChatMessage> = {
            let (keep, carry): (Vec<_>, Vec<_>) = batch.messages.drain(..).partition(|m| (m.time as i64) < next);
            batch.messages = keep;
            carry
        };
        batch.save(store, next as f64, false)?;
        batch.messages.splice(0..0, carry);
        offset = next;
    }
    Ok(())
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
        let mut msg = ChatMessage {
            time,
            author: n.pointer("/commenter/displayName").and_then(Value::as_str).unwrap_or("?").to_string(),
            color: n.pointer("/message/userColor").and_then(Value::as_str).filter(|c| is_color(c)).map(str::to_string),
            parts,
            kind: Kind::Message,
            amount: None,
            event_time: None,
        };
        (msg.kind, msg.amount) = twitch_event(&msg.author, &msg.text());
        out.push((id.to_string(), msg));
    }
    let more = video.pointer("/comments/pageInfo/hasNextPage").and_then(Value::as_bool).unwrap_or(false);
    Ok((out, more))
}

/// Twitch's own cheermotes: a cheer is one of these and a number of bits
/// ("Cheer100"). Many emotes also end in digits (Squid1, MercyWing1), so only
/// these count; a channel's own cheermotes aren't recognized.
const CHEERMOTES: &[&str] = &[
    "cheer",
    "doodlecheer",
    "biblethump",
    "cheerwhal",
    "corgo",
    "scoops",
    "uni",
    "showlove",
    "party",
    "seemsgood",
    "pride",
    "kappa",
    "frankerz",
    "heyguys",
    "dansgame",
    "elegiggle",
    "trihard",
    "kreygasm",
    "4head",
    "swiftrage",
    "notlikethis",
    "failfish",
    "vohiyo",
    "pjsalt",
    "mrdestructoid",
    "bday",
    "ripcheer",
    "shamrock",
    "bitboss",
    "streamlabs",
    "muxy",
    "holidaycheer",
    "goal",
    "anon",
    "charity",
];

/// What a Twitch comment by `author` is. Subs and gifts are posted in the
/// subscriber's name and start with it ("name subscribed at Tier 1. …", "name
/// is gifting 5 Tier 1 Subs to …"); cheers are cheermotes in the text.
fn twitch_event(author: &str, text: &str) -> (Kind, Option<f64>) {
    if let Some((first, rest)) = text.split_once(' ')
        && first.eq_ignore_ascii_case(author)
    {
        if let Some(tier) = rest.strip_prefix("subscribed at Tier ") {
            return (Kind::Sub, leading_number(tier).or(Some(1.0)));
        }
        if rest.starts_with("subscribed with Prime") {
            return (Kind::Sub, Some(1.0));
        }
        if let Some(gifting) = rest.strip_prefix("is gifting ") {
            return (Kind::Gift, leading_number(gifting).or(Some(1.0)));
        }
        if rest.starts_with("gifted a Tier ") {
            return (Kind::Gift, Some(1.0));
        }
    }
    let bits: f64 = text
        .split_whitespace()
        .filter_map(|word| {
            let split = word.find(|c: char| c.is_ascii_digit())?;
            let (name, n) = word.split_at(split);
            (CHEERMOTES.contains(&name.to_lowercase().as_str()) && n.bytes().all(|b| b.is_ascii_digit()))
                .then(|| n.parse::<f64>().ok())
                .flatten()
        })
        .sum();
    if bits > 0.0 { (Kind::Tip, Some(bits)) } else { (Kind::Message, None) }
}

/// A Twitch gift bomb ("X is gifting 5 Tier 1 Subs to …") can be followed by a
/// line for each sub ("X gifted a Tier 1 sub to Y!"). Those are the same
/// gift: they stay in chat as messages rather than counting again.
#[derive(Default)]
struct GiftBombs {
    /// Each gifter's subs still to be announced one by one, and when they gave them.
    left: std::collections::HashMap<String, (f64, f64)>,
}

impl GiftBombs {
    /// How long after a bomb its lines can still arrive.
    const WITHIN: f64 = 300.0;

    fn count_once(&mut self, msg: &mut ChatMessage) {
        if msg.kind != Kind::Gift {
            return;
        }
        let who = msg.author.to_lowercase();
        if msg.text().contains(" is gifting ") {
            self.left.insert(who, (msg.amount.unwrap_or(1.0), msg.time));
        } else if let Some((left, at)) = self.left.get_mut(&who)
            && *left >= 1.0
            && msg.time - *at <= Self::WITHIN
        {
            *left -= 1.0;
            msg.kind = Kind::Message;
            msg.amount = None;
        }
    }
}

/// Twitch emote ids are digits, or `emotesv2_` and hex.
fn is_emote_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

// YouTube: yt-dlp saves a past livestream's chat as a "live_chat" subtitle,
// one JSON action per line.

/// Has yt-dlp save the chat, then reads it into `store` a line at a time,
/// saving as it goes: a long, busy stream's is hundreds of megabytes. False
/// when the video has no chat replay. yt-dlp hands over the whole chat at the
/// end, so there's nothing to carry on from: a fetch starts over.
fn fetch_youtube(
    info: &VodInfo,
    yt_dlp: &YtDlp,
    work_dir: &Path,
    vod_id: i64,
    store: &mut dyn ChatStore,
) -> Result<bool> {
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
    let Ok(reader) = fs::File::open(&file).map(std::io::BufReader::new) else { return Ok(false) };
    store.clear()?;
    let mut batch = Batch::new();
    for line in std::io::BufRead::lines(reader) {
        parse_youtube_line(&line?, &mut batch.messages);
        batch.save(store, 0.0, false)?;
    }
    batch.save(store, 0.0, true)?;
    let _ = fs::remove_file(&file);
    Ok(true)
}

#[cfg(test)]
fn parse_youtube(text: &str) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    for line in text.lines() {
        parse_youtube_line(line, &mut out);
    }
    out
}

/// Adds the chat messages in one line of yt-dlp's `live_chat.json` to `out`.
fn parse_youtube_line(line: &str, out: &mut Vec<ChatMessage>) {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    let Some(replay) = v.get("replayChatItemAction") else { return };
    let Some(ms) = replay["videoOffsetTimeMsec"].as_str().and_then(|s| s.parse::<f64>().ok()) else { return };
    for action in replay["actions"].as_array().into_iter().flatten() {
        let item = &action["addChatItemAction"]["item"];
        // Which kind of item, where its text is, and anything to say before that.
        let (kind, r, runs, lead) = if let Some(r) = item.get("liveChatTextMessageRenderer") {
            (Kind::Message, r, r.pointer("/message/runs"), None)
        } else if let Some(r) = item.get("liveChatPaidMessageRenderer") {
            (Kind::Tip, r, r.pointer("/message/runs"), simple_text(r, "/purchaseAmountText"))
        } else if let Some(r) = item.get("liveChatPaidStickerRenderer") {
            let amount = simple_text(r, "/purchaseAmountText").unwrap_or_default();
            (Kind::Tip, r, None, Some(format!("{amount} Super Sticker").trim().to_string()))
        } else if let Some(r) = item.get("liveChatMembershipItemRenderer") {
            // "Welcome to …!" for a new member, "Member for 6 months" for a milestone.
            (
                Kind::Sub,
                r,
                r.pointer("/message/runs"),
                runs_text(r, "/headerSubtext").or_else(|| runs_text(r, "/headerPrimaryText")),
            )
        } else if let Some(r) = item
            .pointer("/liveChatSponsorshipsGiftPurchaseAnnouncementRenderer/header/liveChatSponsorshipsHeaderRenderer")
        {
            (Kind::Gift, r, None, runs_text(r, "/primaryText"))
        } else {
            continue;
        };
        let mut parts = Vec::new();
        if let Some(lead) = lead.filter(|l| !l.is_empty()) {
            push_text(&mut parts, &lead);
            if runs.is_some() {
                push_text(&mut parts, " ");
            }
        }
        for run in runs.and_then(Value::as_array).into_iter().flatten() {
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
        let mut msg = ChatMessage {
            time: ms / 1000.0,
            author: simple_text(r, "/authorName").unwrap_or_else(|| "?".into()),
            color: None,
            parts,
            kind,
            amount: None,
            event_time: None,
        };
        // "Sent 5 … gift memberships": how many.
        if kind == Kind::Gift {
            msg.amount = msg.text().split_whitespace().find_map(leading_number).or(Some(1.0));
        }
        out.push(msg);
    }
}

/// YouTube's `{ "simpleText": … }` at `path` in `r`.
fn simple_text(r: &Value, path: &str) -> Option<String> {
    r.pointer(path)?.get("simpleText")?.as_str().map(str::to_string)
}

/// YouTube's `{ "runs": [{ "text": … }, …] }` (or a simpleText) at `path` in `r`, joined.
fn runs_text(r: &Value, path: &str) -> Option<String> {
    let v = r.pointer(path)?;
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return Some(s.to_string());
    }
    let runs = v.get("runs")?.as_array()?;
    Some(runs.iter().filter_map(|r| r["text"].as_str()).collect())
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
        let (at, id, m, _) = &messages[0];
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
    fn recognizes_twitch_events() {
        let event = |t: &str| twitch_event(t.split(' ').next().unwrap(), t);
        assert_eq!(event("mairimyarel subscribed at Tier 1. They've subscribed for 8 months!"), (Kind::Sub, Some(1.0)));
        assert_eq!(event("big subscribed at Tier 3."), (Kind::Sub, Some(3.0)), "the tier");
        assert_eq!(
            event("neezydagod subscribed with Prime. They've subscribed for 44 months!"),
            (Kind::Sub, Some(1.0))
        );
        assert_eq!(event("someone is gifting 50 Tier 1 Subs to KaiCenat's community!"), (Kind::Gift, Some(50.0)));
        assert_eq!(event("someone gifted a Tier 1 sub to friend!"), (Kind::Gift, Some(1.0)));
        assert_eq!(
            twitch_event("Mairimyarel", "mairimyarel subscribed at Tier 1."),
            (Kind::Sub, Some(1.0)),
            "any case"
        );
        assert_eq!(
            twitch_event("viewer", "I subscribed at Tier 1 yesterday"),
            (Kind::Message, None),
            "only the subscriber's own notice"
        );
        assert_eq!(twitch_event("kai", "Cheer100 Hi, Kai! How’re you? cheer50"), (Kind::Tip, Some(150.0)));
        assert_eq!(
            twitch_event("v", "Squid1 Squid2 Squid3 Squid4 MercyWing1"),
            (Kind::Message, None),
            "emotes, not cheers"
        );
        assert_eq!(twitch_event("v", "cheer up everyone"), (Kind::Message, None));
    }

    #[test]
    fn counts_a_twitch_gift_bomb_once() {
        let msg = |time: f64, author: &str, text: &str| {
            let (kind, amount) = twitch_event(author, text);
            ChatMessage {
                time,
                author: author.into(),
                color: None,
                parts: vec![ChatPart::Text { text: text.into() }],
                kind,
                amount,
                event_time: None,
            }
        };
        let mut bombs = GiftBombs::default();
        let mut chat = vec![
            msg(10.0, "big", "big is gifting 2 Tier 1 Subs to Kai's community!"),
            msg(11.0, "big", "big gifted a Tier 1 sub to a!"),
            msg(11.0, "big", "big gifted a Tier 1 sub to b!"),
            // A third: a gift of its own, not part of the bomb.
            msg(12.0, "big", "big gifted a Tier 1 sub to c!"),
            msg(13.0, "other", "other gifted a Tier 1 sub to d!"),
        ];
        for m in &mut chat {
            bombs.count_once(m);
        }
        let gifts: Vec<_> = chat.iter().filter(|m| m.kind == Kind::Gift).map(|m| (m.time, m.amount)).collect();
        assert_eq!(gifts, [(10.0, Some(2.0)), (12.0, Some(1.0)), (13.0, Some(1.0))]);
        assert_eq!(chat.len(), 5, "the lines stay in chat");
    }

    #[test]
    fn recognizes_kick_events() {
        assert_eq!(kick_event("KickBot", "@Ari_letric has subbed for 15 months!", None), (Kind::Sub, None));
        assert_eq!(kick_event("KickBot", "@mazzahh just gifted 1,000 KICKs!", None), (Kind::Tip, Some(1000.0)));
        assert_eq!(
            kick_event("KickBot", "@someone just gifted 5 subs to the community!", None),
            (Kind::Gift, Some(5.0))
        );
        assert_eq!(
            kick_event("KickBot", "If you enjoy the content and would like to offer further support", None),
            (Kind::Message, None)
        );
        assert_eq!(kick_event("viewer", "has subbed lol", None), (Kind::Message, None), "only KickBot announces");
        assert_eq!(kick_event("Ari_letric", "oh wow", Some("subscription_renewed")), (Kind::Sub, None));

        // A resub as both the celebration and KickBot's line counts once.
        let msg = |time: f64, author: &str, text: &str, kind: Kind| ChatMessage {
            time,
            author: author.into(),
            color: None,
            parts: vec![ChatPart::Text { text: text.into() }],
            kind,
            amount: None,
            event_time: None,
        };
        let celebrated = |time, author, text, subbed_at| ChatMessage {
            event_time: Some(subbed_at),
            ..msg(time, author, text, Kind::Sub)
        };
        let subs = vec![
            (1, msg(7194.0, "KickBot", "@Ari_letric has subbed for 15 months!", Kind::Sub)),
            (2, msg(7211.0, "KickBot", "@blixtyyy has subbed for 15 months!", Kind::Sub)),
            // Shared an hour after the sub KickBot announced.
            (3, celebrated(11017.0, "Ari_letric", "oh wow", 7194.0)),
            // Never announced (the channel has KickBot's announcements off, say).
            (4, celebrated(23814.0, "tetraspis", "been at it for ages", 23000.0)),
        ];
        assert_eq!(resubs_announced(&subs), [3]);
    }

    /// A message `n` of a fake chat, `per_second` to a second.
    fn fake(n: usize, per_second: usize) -> (String, ChatMessage) {
        let text = format!("message {n}");
        let msg = ChatMessage {
            time: (n / per_second) as f64,
            author: "v".into(),
            color: None,
            parts: vec![ChatPart::Text { text }],
            kind: Kind::Message,
            amount: None,
            event_time: None,
        };
        (n.to_string(), msg)
    }

    /// The store's messages, by their text, in time order.
    fn saved(store: &MemoryChat) -> Vec<String> {
        let mut m = store.messages.clone();
        m.sort_by(|a, b| {
            a.time.total_cmp(&b.time).then(a.text().len().cmp(&b.text().len())).then(a.text().cmp(&b.text()))
        });
        m.iter().map(|m| m.text()).collect()
    }

    #[test]
    fn a_twitch_fetch_carries_on_where_it_stopped() {
        // 3,000 messages, 7 a second; pages of 100 from an offset, as Twitch serves them.
        let all: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
        let page = |offset: i64| {
            let from: Vec<_> = all.iter().filter(|(_, m)| m.time as i64 >= offset).cloned().collect();
            let more = from.len() > 100;
            Ok((from.into_iter().take(100).collect::<Vec<_>>(), more))
        };
        let expected = saved(&MemoryChat { messages: all.iter().map(|(_, m)| m.clone()).collect(), position: None });

        // Fails partway (the network drops), after some batches were saved.
        let mut store = MemoryChat::default();
        let mut calls = 0;
        let failing = |offset: i64| {
            calls += 1;
            if calls > 20 { anyhow::bail!("network down") } else { page(offset) }
        };
        assert!(twitch_into(&mut store, Some(430.0), failing, &mut |_| {}).is_err());
        let partial = store.messages.len();
        assert!((1000..3000).contains(&partial), "saved {partial} before failing");
        assert!(store.position.is_some());

        // Carrying on ends with every message, once.
        twitch_into(&mut store, Some(430.0), page, &mut |_| {}).unwrap();
        assert_eq!(saved(&store), expected);
    }

    #[test]
    fn a_kick_fetch_carries_on_where_it_stopped() {
        // 3,000 messages over 500 s from Unix time 1000, paged back 25 at a time by cursor, as Kick serves them.
        let start = 1000.0;
        // Kick's cursors are to the microsecond, so no two messages share one.
        let all: Vec<_> = (0..3000)
            .map(|n| {
                let (id, m) = fake(n, 1);
                (id, ChatMessage { time: n as f64 / 6.0, ..m })
            })
            .collect();
        let page = |cursor: i64| {
            let mut older: Vec<_> =
                all.iter().filter(|(_, m)| ((start + m.time) * 1e6) < cursor as f64).cloned().collect();
            older.sort_by(|a, b| {
                b.1.time.total_cmp(&a.1.time).then(b.0.parse::<usize>().unwrap().cmp(&a.0.parse().unwrap()))
            });
            let page: Vec<_> = older.into_iter().take(25).collect();
            let next = page.last().map(|(_, m)| ((start + m.time) * 1e6) as i64);
            let rows =
                page.into_iter().map(|(id, m)| (start + m.time, id, ChatMessage { time: 0.0, ..m }, None)).collect();
            Ok((rows, next))
        };
        let expected = saved(&MemoryChat { messages: all.iter().map(|(_, m)| m.clone()).collect(), position: None });

        let mut store = MemoryChat::default();
        let mut calls = 0;
        let failing = |cursor: i64| {
            calls += 1;
            if calls > 60 { anyhow::bail!("network down") } else { page(cursor) }
        };
        assert!(kick_into(&mut store, start, 500.0, failing, &mut |_| {}).is_err());
        assert!(store.position.is_some() && !store.messages.is_empty() && store.messages.len() < 3000);

        kick_into(&mut store, start, 500.0, page, &mut |_| {}).unwrap();
        assert_eq!(saved(&store), expected);
    }

    #[test]
    fn reads_youtube_events() {
        let line = |offset: &str, item: Value| {
            json!({ "replayChatItemAction": { "videoOffsetTimeMsec": offset,
                "actions": [{ "addChatItemAction": { "item": item } }] } })
            .to_string()
        };
        let text = [
            line("1000", json!({ "liveChatPaidMessageRenderer": {
                "authorName": { "simpleText": "@fan" }, "purchaseAmountText": { "simpleText": "$5.00" },
                "message": { "runs": [{ "text": "love the stream" }] } } })),
            line("2000", json!({ "liveChatPaidStickerRenderer": {
                "authorName": { "simpleText": "@fan" }, "purchaseAmountText": { "simpleText": "¥500" } } })),
            line("3000", json!({ "liveChatMembershipItemRenderer": {
                "authorName": { "simpleText": "@new" }, "headerSubtext": { "runs": [{ "text": "Welcome to " }, { "text": "Channel" }, { "text": "!" }] } } })),
            line("4000", json!({ "liveChatMembershipItemRenderer": {
                "authorName": { "simpleText": "@old" }, "headerPrimaryText": { "runs": [{ "text": "Member for " }, { "text": "6" }, { "text": " months" }] },
                "message": { "runs": [{ "text": "half a year!" }] } } })),
            line("5000", json!({ "liveChatSponsorshipsGiftPurchaseAnnouncementRenderer": { "header": { "liveChatSponsorshipsHeaderRenderer": {
                "authorName": { "simpleText": "@generous" }, "primaryText": { "runs": [{ "text": "Sent " }, { "text": "5" }, { "text": " Channel gift memberships" }] } } } } })),
        ]
        .join("\n");
        let messages = parse_youtube(&text);
        let seen: Vec<_> = messages.iter().map(|m| (m.author.as_str(), m.kind, m.amount, m.text())).collect();
        assert_eq!(
            seen,
            [
                ("@fan", Kind::Tip, None, "$5.00 love the stream".to_string()),
                ("@fan", Kind::Tip, None, "¥500 Super Sticker".to_string()),
                ("@new", Kind::Sub, None, "Welcome to Channel!".to_string()),
                ("@old", Kind::Sub, None, "Member for 6 months half a year!".to_string()),
                ("@generous", Kind::Gift, Some(5.0), "Sent 5 Channel gift memberships".to_string()),
            ]
        );
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
