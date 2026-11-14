//! YouTube's oEmbed answer: a video's channel, its handle and page, in a
//! fraction of a second, where yt-dlp takes seconds to start and read the
//! video's page. It names nothing else the library keeps (no length, no
//! date), so adding a VOD still goes through yt-dlp; Refresh details, which
//! wants only the channel, asks here first (`pipeline::refresh_details`).

use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use serde_json::Value;

/// oEmbed's answers are well under a kilobyte.
const MAX_BYTES: u64 = 64 << 10;

/// Whether `id` is shaped as a YouTube video's id: eleven of its URL-safe
/// characters. Anything else isn't asked about.
pub fn is_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Video `id`'s channel, as oEmbed names it: its handle (`@name`) and its
/// page (`https://www.youtube.com/@name`), as yt-dlp's `uploader_id` and
/// `uploader_url` give them. An error when YouTube won't say (the video is
/// gone, private or not embeddable), answers for another video, or names
/// the channel by anything but a plain handle.
pub fn channel(id: &str, cancel: &AtomicBool) -> Result<(String, String)> {
    anyhow::ensure!(is_video_id(id), "not a YouTube video id");
    // The id needs no escaping; the address it's in does.
    let url =
        format!("https://www.youtube.com/oembed?format=json&url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D{id}");
    let body = crate::net::get_bytes(&url, MAX_BYTES, cancel)?;
    parse_channel(&serde_json::from_slice(&body).context("unreadable oEmbed answer")?, id)
}

fn parse_channel(json: &Value, id: &str) -> Result<(String, String)> {
    let html = json.get("html").and_then(Value::as_str).unwrap_or_default();
    anyhow::ensure!(html.contains(&format!("youtube.com/embed/{id}?")), "oEmbed answered for another video");
    let page = json.get("author_url").and_then(Value::as_str).context("oEmbed named no channel")?;
    // A handle as it's written, nothing escaped: yt-dlp's could be written
    // otherwise, and the card would go by another name.
    let handle = page
        .strip_prefix("https://www.youtube.com/")
        .filter(|h| h.starts_with('@') && h.len() > 1)
        .filter(|h| h.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'-' | b'_' | b'.')))
        .context("oEmbed named the channel by no handle")?;
    Ok((handle.to_string(), page.to_string()))
}

#[cfg(test)]
mod tests;
