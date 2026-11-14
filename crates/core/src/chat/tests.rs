//! The specs of `chat.rs`, in the files in `tests/`, and the helpers they
//! share: a VOD of each site of a test's own, and its chat fetched as
//! `fetch` fetches it, from the kit's network and yt-dlp.

use super::*;
use crate::testkit::sites::KickVod;
use std::sync::atomic::AtomicUsize;

mod fetch;
mod parse;

/// A number no other test here has, for a VOD or channel of a test's own:
/// tests run at once and see each other's answers on a site's host.
fn own() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A Twitch VOD's number of this test's own.
fn own_twitch_number() -> String {
    format!("{}", 3_000_000_000 + own())
}

/// A Kick VOD of this test's own (a UUIDv7, as Kick's are), `duration`
/// seconds long, on a channel of its own.
fn own_kick_vod(duration: f64) -> KickVod {
    let n = own();
    KickVod::new(&format!("01a0e9a8-a9e0-797e-9646-{n:012x}"), 5_000_000 + n as u64, duration)
}

/// Twitch VOD `number`, `duration` seconds long if Twitch says, as yt-dlp
/// describes it.
fn twitch_vod(number: &str, duration: Option<f64>) -> VodInfo {
    VodInfo {
        platform: "twitch".into(),
        platform_id: format!("v{number}"),
        url: format!("https://www.twitch.tv/videos/{number}"),
        title: "A stream".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration,
        thumbnail: None,
        published: None,
        started_at: None,
    }
}

/// Kick VOD `vod` as `kick::resolve` describes it, `duration` seconds long.
fn kick_vod(vod: &KickVod, duration: f64) -> VodInfo {
    let (_, id) = kick::parse_vod_url(&vod.url()).unwrap();
    VodInfo {
        platform: "kick".into(),
        platform_id: id,
        url: vod.url(),
        title: "A stream".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(duration),
        thumbnail: None,
        published: None,
        started_at: None,
    }
}

/// `fetch` of a Twitch or Kick VOD's chat into `store`, from the site's
/// API (the test's network), as the chat queue fetches it.
fn fetch_from_site(info: &VodInfo, store: &mut MemoryChat, cancel: &AtomicBool) -> Result<bool> {
    fetch(info, None, Path::new("no-yt-dlp-chat"), store, cancel, |_| {})
}

/// The messages in time order.
fn in_order(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    messages.sort_by(|a, b| a.time.total_cmp(&b.time));
    messages
}
