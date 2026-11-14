//! Fallback for Kick VODs. In mid-2026 Kick moved VODs to new ids and a new
//! API, which yt-dlp's Kick extractor doesn't support yet
//! (https://github.com/yt-dlp/yt-dlp/issues/17284). This resolves the
//! metadata and an HLS manifest ourselves; yt-dlp then downloads the manifest
//! as a plain HLS stream.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::fetch::VodInfo;

/// Returns (channel slug, video id) for a `kick.com/<channel>/videos/<id>` URL.
pub fn parse_vod_url(url: &str) -> Option<(String, String)> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let path = rest.strip_prefix("kick.com/")?;
    let path = path.split(['?', '#']).next()?;
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let (slug, videos, id) = (parts.next()?, parts.next()?, parts.next()?);
    let valid_id = id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    (videos == "videos" && valid_id && parts.next().is_none()).then(|| (slug.to_string(), id.to_lowercase()))
}

/// Resolves a Kick VOD to its metadata and a (short-lived, signed) HLS manifest URL.
pub fn resolve(url: &str) -> Result<(VodInfo, String)> {
    let (slug, id) = parse_vod_url(url).context("not a Kick VOD link")?;
    let (status, json) = playback(&id)?;
    if status == 404 {
        bail!("Kick says this VOD doesn't exist. It may have been deleted or made private.");
    }
    if status != 200 {
        let kind = json.pointer("/data/type").and_then(Value::as_str).unwrap_or("unknown error");
        bail!("Kick isn't responding right now ({status} {kind}). Try again later.");
    }
    parse_playback(&json, &slug, &id)
}

/// Kick's API responses and master playlists are a few KB.
const API_MAX_BYTES: u64 = 1 << 20;
/// A day-long VOD's media playlist is about 1 MB.
const PLAYLIST_MAX_BYTES: u64 = 16 << 20;

/// Where a VOD's chat is in its channel's history.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChatOrigin {
    /// The channel's id, which its chat history is under.
    pub channel: u64,
    /// When the VOD starts, in Unix seconds.
    pub start: f64,
}

/// Finds VOD `id`'s channel and start time. The start is the wall-clock time
/// the media playlist gives its first segment; failing that, the time in the
/// VOD's id (a UUIDv7, made when the stream started), a few seconds later.
pub fn chat_origin(id: &str) -> Result<ChatOrigin> {
    let (status, json) = playback(id)?;
    if status != 200 {
        bail!("Kick isn't responding right now ({status}). Try again later.");
    }
    let channel = json
        .pointer("/video_session/creator_id")
        .and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or(v.as_u64()))
        .context("Kick didn't say which channel this VOD is from.")?;
    let manifest = json.pointer("/playback_url/vod").and_then(Value::as_str).filter(|s| s.starts_with("https://"));
    let start = manifest
        .and_then(|m| playlist_start(m).inspect_err(|e| eprintln!("Kick VOD {id}'s start time: {e:#}")).ok().flatten())
        .or_else(|| uuid_v7_time(id))
        .context("Kick didn't say when this VOD started.")?;
    Ok(ChatOrigin { channel, start })
}

/// The time of the first segment in the first quality's media playlist.
fn playlist_start(master_url: &str) -> Result<Option<f64>> {
    let never = std::sync::atomic::AtomicBool::new(false);
    let master = crate::net::get_bytes(master_url, API_MAX_BYTES, &never)?;
    let Some(variant) = parse_master(&String::from_utf8_lossy(&master)).into_iter().next() else { return Ok(None) };
    let media = crate::net::get_bytes(&variant.url, PLAYLIST_MAX_BYTES, &never)?;
    Ok(first_program_date(&String::from_utf8_lossy(&media)))
}

fn first_program_date(playlist: &str) -> Option<f64> {
    playlist.lines().find_map(|l| l.strip_prefix("#EXT-X-PROGRAM-DATE-TIME:")).and_then(crate::chat::parse_time)
}

/// The Unix time in a UUIDv7's first 48 bits (milliseconds).
fn uuid_v7_time(id: &str) -> Option<f64> {
    let hex: String = id.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || hex.as_bytes()[12] != b'7' {
        return None;
    }
    Some(u64::from_str_radix(&hex[..12], 16).ok()? as f64 / 1000.0)
}

/// The playback endpoint's status and JSON body.
fn playback(id: &str) -> Result<(u16, Value)> {
    let body = json!({
        "video_player": { "player": {} },
        "video_session": {},
        "user_session": { "non_personalised_ads": true },
    });
    let mut resp = crate::net::agent()
        .post(format!("https://web.kick.com/api/v1/stream/{id}/playback"))
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .build()
        .send(body.to_string())
        .context("contact Kick")?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(API_MAX_BYTES).read_to_string().unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

fn parse_playback(json: &Value, slug: &str, id: &str) -> Result<(VodInfo, String)> {
    let session = |k: &str| json.pointer(&format!("/video_session/{k}"));
    let text = |k: &str| session(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let manifest = json
        .pointer("/playback_url/vod")
        .and_then(Value::as_str)
        .filter(|s| s.starts_with("https://"))
        .context("Kick won't let VODscribe play this VOD. Subscriber-only VODs can't be added.")?;
    let info = VodInfo {
        platform: "kick".into(),
        platform_id: id.to_string(),
        url: format!("https://kick.com/{slug}/videos/{id}"),
        title: text("video_title").unwrap_or_else(|| "Untitled".into()),
        channel: text("video_series").or_else(|| Some(slug.to_string())),
        channel_login: Some(slug.to_string()),
        channel_url: None,
        duration: session("video_duration").and_then(Value::as_f64).filter(|d| *d > 0.0),
        thumbnail: json.pointer("/playback_url/thumbnail").and_then(Value::as_str).and_then(poster_url),
        published: None,
        started_at: None,
    };
    Ok((info, manifest.to_string()))
}

/// The VOD's poster image, derived from the thumbnail sprite-sheet URL the
/// playback API returns. The sheet lives at
/// `.../ivs/v1/<account>/<channel key>/<date parts>/<video key>/media/thumbnails/...`
/// and Kick serves the poster at
/// `images.kick.com/video_thumbnails/<channel key>/<video key>/<height>.webp`.
pub fn poster_url(sheet: &str) -> Option<String> {
    let (_, rest) = sheet.split_once("/ivs/v1/")?;
    let mut parts = rest.split('/');
    let _account = parts.next()?;
    let channel_key = parts.next()?;
    let parts: Vec<&str> = parts.collect();
    let media = parts.iter().position(|p| *p == "media")?;
    let video_key = parts[..media].last()?;
    (!channel_key.is_empty() && !video_key.is_empty())
        .then(|| format!("https://images.kick.com/video_thumbnails/{channel_key}/{video_key}/360.webp"))
}

/// One quality level of a VOD's HLS stream.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Variant {
    pub name: String,
    pub height: u32,
    pub url: String,
}

/// Lists a VOD's quality levels, for playback. Kick's API and master playlist
/// don't allow cross-origin requests from the packaged app, but the
/// per-quality playlists and segments do, so the UI can play those directly.
pub fn variants(url: &str) -> Result<Vec<Variant>> {
    let (_, manifest) = resolve(url)?;
    let never = std::sync::atomic::AtomicBool::new(false);
    let text = crate::net::get_bytes(&manifest, API_MAX_BYTES, &never)?;
    let variants = parse_master(&String::from_utf8_lossy(&text));
    if variants.is_empty() {
        bail!("Kick didn't offer a way to play this VOD.");
    }
    Ok(variants)
}

fn parse_master(text: &str) -> Vec<Variant> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") else { continue };
        let Some(url) = lines.next().filter(|u| u.starts_with("https://")) else { continue };
        let attr =
            |key: &str| attrs.split(',').find_map(|kv| kv.strip_prefix(key)).map(|v| v.trim_matches('"').to_string());
        let height = attr("RESOLUTION=").and_then(|r| r.split('x').nth(1)?.parse().ok()).unwrap_or(0);
        let name = attr("VIDEO=").unwrap_or_else(|| format!("{height}p"));
        out.push(Variant { name, height, url: url.to_string() });
    }
    out
}

#[cfg(test)]
mod tests;
