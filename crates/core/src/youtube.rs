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
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn knows_a_video_id() {
        assert!(is_video_id("noa29qsTc4g") && is_video_id("a-b_c123456"));
        assert!(!is_video_id("e2eolder") && !is_video_id("noa29qsTc4g?") && !is_video_id("noa29qsTc4gx"));
    }

    #[test]
    fn reads_the_channel() {
        let answer = |page: &str, embed: &str| {
            json!({ "author_name": "Quill", "author_url": page,
                "html": format!("<iframe src=\"https://www.youtube.com/embed/{embed}?feature=oembed\"></iframe>") })
        };
        assert_eq!(
            parse_channel(&answer("https://www.youtube.com/@quill.tv", "aaaaaaaaaaa"), "aaaaaaaaaaa").unwrap(),
            ("@quill.tv".into(), "https://www.youtube.com/@quill.tv".into())
        );
        // Another video's answer, or a channel named otherwise, isn't taken.
        assert!(parse_channel(&answer("https://www.youtube.com/@quill", "bbbbbbbbbbb"), "aaaaaaaaaaa").is_err());
        for page in [
            "https://www.youtube.com/channel/UC0123456789",
            "https://www.youtube.com/@",
            "https://www.youtube.com/@%E5%90%8D",
            "https://www.youtube.com/@quill/videos",
            "https://evil.example/@quill",
            "http://www.youtube.com/@quill",
        ] {
            assert!(parse_channel(&answer(page, "aaaaaaaaaaa"), "aaaaaaaaaaa").is_err(), "{page}");
        }
        assert!(parse_channel(&json!({ "html": "youtube.com/embed/aaaaaaaaaaa?" }), "aaaaaaaaaaa").is_err());
    }

    /// Asks YouTube for real: `cargo test -p vodscribe-core -- --ignored youtube_channel_live`.
    #[test]
    #[ignore = "network"]
    fn youtube_channel_live() {
        let id = std::env::var("VIDEO").unwrap_or_else(|_| "noa29qsTc4g".into());
        eprintln!("{:?}", channel(&id, &AtomicBool::new(false)).unwrap());
    }
}
