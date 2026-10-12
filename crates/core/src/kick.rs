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
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status == 404 {
        bail!("Kick says this VOD doesn't exist. It may have been deleted or made private.");
    }
    if status != 200 {
        let kind = json.pointer("/data/type").and_then(Value::as_str).unwrap_or("unknown error");
        bail!("Kick returned {status} ({kind})");
    }
    parse_playback(&json, &slug, &id)
}

fn parse_playback(json: &Value, slug: &str, id: &str) -> Result<(VodInfo, String)> {
    let session = |k: &str| json.pointer(&format!("/video_session/{k}"));
    let text = |k: &str| session(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let manifest = json
        .pointer("/playback_url/vod")
        .and_then(Value::as_str)
        .filter(|s| s.starts_with("https://"))
        .context("Kick didn't return a playable stream. Subscriber-only VODs aren't supported.")?;
    let info = VodInfo {
        platform: "kick".into(),
        platform_id: id.to_string(),
        url: format!("https://kick.com/{slug}/videos/{id}"),
        title: text("video_title").unwrap_or_else(|| "Untitled".into()),
        channel: text("video_series").or_else(|| Some(slug.to_string())),
        duration: session("video_duration").and_then(Value::as_f64).filter(|d| *d > 0.0),
        thumbnail: json.pointer("/playback_url/thumbnail").and_then(Value::as_str).and_then(poster_url),
        published: None,
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
    let (mut body, _) = crate::net::get(&manifest, &never)?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut body, &mut text)?;
    let variants = parse_master(&text);
    if variants.is_empty() {
        bail!("Kick's stream has no playable qualities");
    }
    Ok(variants)
}

fn parse_master(text: &str) -> Vec<Variant> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") else { continue };
        let Some(url) = lines.next().filter(|u| u.starts_with("http")) else { continue };
        let attr = |key: &str| {
            attrs.split(',').find_map(|kv| kv.strip_prefix(key)).map(|v| v.trim_matches('"').to_string())
        };
        let height = attr("RESOLUTION=").and_then(|r| r.split('x').nth(1)?.parse().ok()).unwrap_or(0);
        let name = attr("VIDEO=").unwrap_or_else(|| format!("{height}p"));
        out.push(Variant { name, height, url: url.to_string() });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vod_urls() {
        let id = "01a0b0d6-75c0-75a7-8050-a72ab4dc9e0f";
        let want = Some(("barky".to_string(), id.to_string()));
        assert_eq!(parse_vod_url(&format!("https://kick.com/barky/videos/{id}")), want);
        assert_eq!(parse_vod_url(&format!("https://www.kick.com/barky/videos/{id}?t=120")), want);
        assert_eq!(parse_vod_url(&format!("kick.com/barky/videos/{id}/")), want);
        assert_eq!(parse_vod_url("https://kick.com/barky/videos"), None);
        assert_eq!(parse_vod_url(&format!("https://kick.com/barky/clips/{id}")), None);
        assert_eq!(parse_vod_url(&format!("https://notkick.com/barky/videos/{id}")), None);
    }

    #[test]
    fn parses_master_playlist() {
        let text = "#EXTM3U\n#EXT-X-STREAM-INF:CODECS=\"avc1.4D401F,mp4a.40.2\",VIDEO=\"720p60\",RESOLUTION=1280x720,FRAME-RATE=60.0,BANDWIDTH=3487142\nhttps://cdn/1.m3u8\n#EXT-X-STREAM-INF:VIDEO=\"160p30\",RESOLUTION=284x160,BANDWIDTH=230000\nhttps://cdn/4.m3u8\n";
        let v = parse_master(text);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].name.as_str(), v[0].height, v[0].url.as_str()), ("720p60", 720, "https://cdn/1.m3u8"));
        assert_eq!(v[1].height, 160);
    }

    #[test]
    fn parses_playback_response() {
        let json = json!({
            "playback_url": { "vod": "https://web.kick.com/api/v1/stream/manifest.m3u8?init=x" },
            "video_session": { "video_title": "Day 9", "video_series": "Barky", "video_duration": 36602 }
        });
        let (info, manifest) = parse_playback(&json, "barky", "abc").unwrap();
        assert_eq!(info.title, "Day 9");
        assert_eq!(info.channel.as_deref(), Some("Barky"));
        assert_eq!(info.duration, Some(36602.0));
        assert_eq!(info.url, "https://kick.com/barky/videos/abc");
        assert!(manifest.contains("manifest.m3u8"));

        let empty = json!({ "playback_url": { "vod": "" }, "video_session": {} });
        assert!(parse_playback(&empty, "barky", "abc").is_err());
    }

    #[test]
    fn derives_the_poster_from_the_sprite_sheet() {
        let sheet = "https://stream.kick.com/3c81249a5ce0/ivs/v1/196233775518/SvN6unb8Sp4d/2026/9/17/19/27/UiUoaT51vf1v/media/thumbnails/thumbnail-sheet-{index}.webp";
        assert_eq!(
            poster_url(sheet).as_deref(),
            Some("https://images.kick.com/video_thumbnails/SvN6unb8Sp4d/UiUoaT51vf1v/360.webp")
        );
        assert_eq!(poster_url("https://stream.kick.com/x/other/path.webp"), None);
        assert_eq!(poster_url("https://stream.kick.com/x/ivs/v1/1/ch/nomedia.webp"), None);
    }
}
