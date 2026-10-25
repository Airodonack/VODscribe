//! Twitch clips. A clip is a stretch of a broadcast, kept as a video of its
//! own: its id is its slug (`CoweringRudeDiamond…`), where a VOD's is a number.
//! Its chat is the broadcast's over that stretch, while Twitch still has the
//! broadcast; its video needs a token Twitch signs for each viewing.

use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::chat::{TWITCH_CLIENT_ID, TWITCH_GQL, api};

/// Whether `platform_id` (a Twitch item's id in the library) is a clip's slug
/// rather than a VOD's number (which yt-dlp may give as `v123`).
pub fn is_clip(platform_id: &str) -> bool {
    let id = platform_id.trim_start_matches('v');
    !id.is_empty() && !id.bytes().all(|b| b.is_ascii_digit())
}

/// Where a clip came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// The broadcast's VOD and where in it the clip starts, while Twitch keeps it.
    pub source: Option<(String, f64)>,
    pub duration: f64,
    /// The channel's id.
    pub broadcaster: Option<String>,
}

fn ask(query: &str, slug: &str) -> Result<Value> {
    let body = json!({ "query": query, "variables": { "slug": slug } });
    let json = api(TWITCH_GQL, Some(&body), &[("Client-ID", TWITCH_CLIENT_ID)], &AtomicBool::new(false))?;
    json.pointer("/data/clip").filter(|c| !c.is_null()).cloned().context("Twitch doesn't know this clip.")
}

/// What Twitch says of clip `slug`.
pub fn clip(slug: &str) -> Result<Clip> {
    let c = ask(
        "query($slug: ID!) { clip(slug: $slug) { durationSeconds videoOffsetSeconds video { id } broadcaster { id } } }",
        slug,
    )?;
    Ok(parse_clip(&c))
}

fn parse_clip(c: &Value) -> Clip {
    let video = c.pointer("/video/id").and_then(Value::as_str).map(str::to_string);
    let offset = c.get("videoOffsetSeconds").and_then(Value::as_f64);
    Clip {
        source: video.zip(offset),
        duration: c.get("durationSeconds").and_then(Value::as_f64).unwrap_or(0.0),
        broadcaster: c.pointer("/broadcaster/id").and_then(Value::as_str).map(str::to_string),
    }
}

/// A URL that plays clip `slug`'s video (720p at most when offered, plenty for
/// a preview pane), signed by Twitch for a while.
pub fn clip_video(slug: &str) -> Result<String> {
    let c = ask(
        "query($slug: ID!) { clip(slug: $slug) {
            playbackAccessToken(params: { platform: \"web\", playerType: \"site\", playerBackend: \"mediaplayer\" }) {
                signature value }
            videoQualities { quality sourceURL } } }",
        slug,
    )?;
    let token = c.get("playbackAccessToken").context("Twitch didn't let this clip play.")?;
    let (Some(sig), Some(value)) = (token["signature"].as_str(), token["value"].as_str()) else {
        anyhow::bail!("Twitch didn't let this clip play.");
    };
    let qualities = c.get("videoQualities").and_then(Value::as_array).cloned().unwrap_or_default();
    let height = |q: &Value| q["quality"].as_str().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let pick = qualities
        .iter()
        .filter(|q| height(q) <= 720)
        .max_by_key(|q| height(q))
        .or_else(|| qualities.first())
        .and_then(|q| q["sourceURL"].as_str())
        .filter(|u| u.starts_with("https://"))
        .context("Twitch didn't return a playable video for this clip.")?;
    let encode = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    Ok(format!("{pick}?sig={}&token={}", encode(sig), encode(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_clips_from_vods() {
        assert!(!is_clip("2899842479") && !is_clip("v2899842479"));
        assert!(is_clip("CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga"));
        assert!(!is_clip(""));
    }

    #[test]
    fn reads_where_a_clip_came_from() {
        let c = json!({ "durationSeconds": 32, "videoOffsetSeconds": 1805,
            "video": { "id": "2896314708" }, "broadcaster": { "id": "719949561" } });
        assert_eq!(
            parse_clip(&c),
            Clip { source: Some(("2896314708".into(), 1805.0)), duration: 32.0, broadcaster: Some("719949561".into()) }
        );
        // The broadcast is gone: no chat.
        let gone = json!({ "durationSeconds": 34, "videoOffsetSeconds": null, "video": null });
        assert_eq!(parse_clip(&gone).source, None);
    }

    /// Asks Twitch for real: `cargo test -p vodscribe-core -- --ignored twitch_clip`.
    #[test]
    #[ignore = "network"]
    fn twitch_clip_live() {
        let slug = std::env::var("CLIP").unwrap_or_else(|_| "CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga".into());
        eprintln!("{:?}", clip(&slug).unwrap());
        let url = clip_video(&slug).unwrap();
        let resp = crate::net::agent().head(&url).call().unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    /// A recent clip's chat, from its broadcast: `CLIP=<slug> cargo test -p vodscribe-core -- --ignored twitch_clip_chat`.
    #[test]
    #[ignore = "network"]
    fn twitch_clip_chat_live() {
        let slug = std::env::var("CLIP").expect("CLIP: a recent clip's slug");
        let info = crate::fetch::VodInfo {
            platform: "twitch".into(),
            platform_id: slug,
            url: String::new(),
            title: String::new(),
            channel: None,
            duration: None,
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let mut store = crate::chat::MemoryChat::default();
        let found = crate::chat::fetch(
            &info,
            None,
            std::path::Path::new("/nowhere"),
            0,
            &mut store,
            &AtomicBool::new(false),
            |_| {},
        );
        let times: Vec<f64> = store.messages.iter().map(|m| m.time).collect();
        eprintln!(
            "chat: {:?}, {} messages, from {:?} to {:?}",
            found.as_ref().ok(),
            times.len(),
            times.first(),
            times.last()
        );
        assert!(found.unwrap());
        assert!(times.iter().all(|t| (0.0..=60.0).contains(t)), "at the clip's own times");
    }
}
