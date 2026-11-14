//! The specs of `kick.rs`: its parsers of Kick's links and answers (a
//! parser, so the function is the spec), what's refused before Kick is
//! asked, and `resolve`, `variants` and `chat_origin` against Kick's API as
//! the kit's network answers it (`KickVod`).

use super::*;
use crate::testkit::sites::KickVod;
use crate::testkit::{Net, Reply};

/// When the VODs here started, in Unix seconds: 2026-09-28T21:30:00Z.
const STARTED: f64 = 1790631000.0;

/// Kick VOD `n` of these tests', an hour long, on channel `6_000_000 + n`.
fn a_kick_vod(n: u64) -> KickVod {
    KickVod::new(&format!("01a0e9a8-a9e0-797e-9647-{n:012x}"), 6_000_000 + n, 3600.0)
}

/// Its playback answer for a VOD of these tests' is `status` with `body`.
fn playback_answers(net: &Net, vod: u64, status: u16, body: Value) {
    let url = format!("https://web.kick.com/api/v1/stream/01a0e9a8-a9e0-797e-9647-{vod:012x}/playback");
    net.replies(&url, move |_| Some(Reply { status, ..Reply::json(&body) }));
}

/// A `kick.com/<channel>/videos/<id>` link gives its channel's slug and
/// the video's id, and nothing else is one (kick.rs `parse_vod_url`: "Returns (channel
/// slug, video id) for a `kick.com/<channel>/videos/<id>` URL").
#[test]
fn a_kick_vod_link_gives_its_channel_and_video_id() {
    let id = "01a0b0d6-75c0-75a7-8050-a72ab4dc9e0f";
    let want = Some(("barky".to_string(), id.to_string()));
    assert_eq!(parse_vod_url(&format!("https://kick.com/barky/videos/{id}")), want);
    assert_eq!(parse_vod_url(&format!("https://www.kick.com/barky/videos/{id}?t=120")), want);
    assert_eq!(parse_vod_url(&format!("kick.com/barky/videos/{id}/")), want);
    assert_eq!(parse_vod_url("https://kick.com/barky/videos"), None);
    assert_eq!(parse_vod_url(&format!("https://kick.com/barky/clips/{id}")), None);
    assert_eq!(parse_vod_url(&format!("https://notkick.com/barky/videos/{id}")), None);
    assert_eq!(parse_vod_url(&format!("https://kick.com/barky/videos/{id}/more")), None);
    assert_eq!(parse_vod_url("https://kick.com/barky/videos/123"), None, "not a Kick VOD's id");
}

/// A link that isn't a Kick VOD's is neither resolved nor listed, so Kick
/// isn't asked about it (kick.rs `resolve`: "Resolves a Kick VOD"; kick.rs `variants`:
/// "Lists a VOD's quality levels").
#[test]
fn a_link_that_is_not_a_kick_vods_is_not_looked_up() {
    for url in ["https://kick.com/barky", "https://www.twitch.tv/videos/2901646294", "kick.com/barky/clips/x"] {
        assert!(resolve(url).is_err(), "{url}");
        assert!(variants(url).is_err(), "{url}");
    }
}

/// A Kick VOD's details are as Kick's playback answer gives them: its
/// title, its series as the channel, its slug as the channel's login and
/// no page, its length, its link, and its poster (kick.rs `resolve`: "Resolves a
/// Kick VOD to its metadata and a (short-lived, signed) HLS manifest URL";
/// pipeline.rs `channel_by_api`: "Kick (`kick::resolve`): the login, its slug, and no
/// page").
#[test]
fn a_kick_vods_details_are_as_kick_gives_them() {
    let json = json!({
        "playback_url": {
            "vod": "https://web.kick.com/api/v1/stream/manifest.m3u8?init=x",
            "thumbnail": "https://stream.kick.com/3c81249a5ce0/ivs/v1/196233775518/SvN6unb8Sp4d/2026/9/17/19/27/UiUoaT51vf1v/media/thumbnails/thumbnail-sheet-{index}.webp"
        },
        "video_session": { "video_title": "Day 9", "video_series": "Barky", "video_duration": 36602 }
    });
    let (info, manifest) = parse_playback(&json, "barky", "abc").unwrap();
    assert_eq!((info.platform.as_str(), info.platform_id.as_str()), ("kick", "abc"));
    assert_eq!(info.title, "Day 9");
    assert_eq!(info.channel.as_deref(), Some("Barky"));
    assert_eq!((info.channel_login.as_deref(), info.channel_url), (Some("barky"), None));
    assert_eq!(info.duration, Some(36602.0));
    assert_eq!(info.url, "https://kick.com/barky/videos/abc");
    assert_eq!(
        info.thumbnail.as_deref(),
        Some("https://images.kick.com/video_thumbnails/SvN6unb8Sp4d/UiUoaT51vf1v/360.webp")
    );
    assert_eq!(manifest, "https://web.kick.com/api/v1/stream/manifest.m3u8?init=x");
}

/// A VOD Kick gives no https manifest for (a subscriber-only one) isn't
/// resolved (kick.rs `resolve`: "and a (short-lived, signed) HLS manifest URL";
/// kick.rs `parse_playback`: "Subscriber-only VODs can't be added.").
#[test]
fn a_kick_vod_kick_wont_play_is_not_resolved() {
    for vod in [json!(""), json!("http://web.kick.com/api/v1/stream/manifest.m3u8"), json!(null)] {
        let json = json!({ "playback_url": { "vod": vod }, "video_session": { "video_title": "Day 9" } });
        assert!(parse_playback(&json, "barky", "abc").is_err(), "{vod}");
    }
}

/// A VOD's poster is derived from the thumbnail sheet the playback API
/// gives, and nothing else is taken for one (kick.rs `poster_url`: "The VOD's poster
/// image, derived from the thumbnail sprite-sheet URL the playback API
/// returns").
#[test]
fn a_kick_vods_poster_is_derived_from_its_thumbnail_sheet() {
    let sheet = "https://stream.kick.com/3c81249a5ce0/ivs/v1/196233775518/SvN6unb8Sp4d/2026/9/17/19/27/UiUoaT51vf1v/media/thumbnails/thumbnail-sheet-{index}.webp";
    assert_eq!(
        poster_url(sheet).as_deref(),
        Some("https://images.kick.com/video_thumbnails/SvN6unb8Sp4d/UiUoaT51vf1v/360.webp")
    );
    assert_eq!(poster_url("https://stream.kick.com/x/other/path.webp"), None);
    assert_eq!(poster_url("https://stream.kick.com/x/ivs/v1/1/ch/nomedia.webp"), None);
    assert_eq!(poster_url("https://stream.kick.com/x/ivs/v1/1//2026/key/media/t.webp"), None, "no channel key");
    assert_eq!(poster_url("https://stream.kick.com/x/ivs/v1/1/ch/media/t.webp"), None, "no video key");
    assert_eq!(poster_url("https://stream.kick.com/x/ivs/v1/1/ch/2026//media/t.webp"), None, "an empty video key");
}

/// A master playlist lists each quality with its height and its own
/// playlist, which the page plays (kick.rs `Variant`: "One quality level of a
/// VOD's HLS stream"; kick.rs `variants`: "the per-quality playlists and segments
/// do, so the UI can play those directly").
#[test]
fn a_master_playlist_lists_each_quality_with_its_height_and_playlist() {
    let text = "#EXTM3U\n#EXT-X-STREAM-INF:CODECS=\"avc1.4D401F,mp4a.40.2\",VIDEO=\"720p60\",RESOLUTION=1280x720,FRAME-RATE=60.0,BANDWIDTH=3487142\nhttps://cdn/1.m3u8\n#EXT-X-STREAM-INF:VIDEO=\"160p30\",RESOLUTION=284x160,BANDWIDTH=230000\nhttps://cdn/4.m3u8\n";
    let v = parse_master(text);
    assert_eq!(v.len(), 2);
    assert_eq!((v[0].name.as_str(), v[0].height, v[0].url.as_str()), ("720p60", 720, "https://cdn/1.m3u8"));
    assert_eq!((v[1].name.as_str(), v[1].height, v[1].url.as_str()), ("160p30", 160, "https://cdn/4.m3u8"));
}

/// A Kick VOD is resolved, as Kick's playback answer gives it, to its
/// details and its manifest (kick.rs `resolve`: "Resolves a Kick VOD to its
/// metadata and a (short-lived, signed) HLS manifest URL").
#[test]
fn a_kick_vod_resolves_to_its_details_and_manifest() {
    let net = Net::new();
    let vod = a_kick_vod(1);
    net.kick_vod(&vod);
    let (info, manifest) = resolve(&vod.url()).unwrap();
    assert_eq!((info.platform.as_str(), info.url.as_str()), ("kick", vod.url().as_str()));
    assert_eq!((info.title.as_str(), info.channel.as_deref()), ("A stream", Some("Barky")));
    assert_eq!((info.channel_login.as_deref(), info.duration), (Some("barky"), Some(3600.0)));
    assert_eq!(manifest, vod.master());
}

/// A VOD Kick says doesn't exist, and one Kick fails to answer for, aren't
/// resolved, saying which (kick.rs `resolve`: "Kick says this VOD doesn't
/// exist. It may have been deleted or made private.", "Kick isn't
/// responding right now ({status} {kind}). Try again later.").
#[test]
fn a_kick_vod_kick_does_not_have_or_does_not_answer_for_is_not_resolved() {
    let net = Net::new();
    playback_answers(&net, 2, 404, json!({}));
    playback_answers(&net, 3, 503, json!({ "data": { "type": "unavailable" } }));
    let gone = resolve(&a_kick_vod(2).url()).unwrap_err();
    assert_eq!(format!("{gone:#}"), "Kick says this VOD doesn't exist. It may have been deleted or made private.");
    let down = resolve(&a_kick_vod(3).url()).unwrap_err();
    assert_eq!(format!("{down:#}"), "Kick isn't responding right now (503 unavailable). Try again later.");
}

/// A Kick VOD's qualities are those its master playlist lists, each with
/// its own playlist, and a VOD whose playlist lists none can't be played
/// (kick.rs `variants`: "Lists a VOD's quality levels, for playback",
/// "Kick didn't offer a way to play this VOD.").
#[test]
fn a_kick_vods_qualities_are_those_its_master_playlist_lists() {
    let net = Net::new();
    let vod = a_kick_vod(4);
    net.kick_vod(&vod);
    let listed: Vec<_> = variants(&vod.url()).unwrap().into_iter().map(|v| (v.name, v.height, v.url)).collect();
    assert_eq!(
        listed,
        [("1080p60".into(), 1080, vod.playlist("1080p60")), ("360p30".into(), 360, vod.playlist("360p30"))]
    );
    let none = a_kick_vod(5);
    net.serves(&none.master(), "#EXTM3U\n");
    net.kick_vod(&none);
    assert_eq!(format!("{:#}", variants(&none.url()).unwrap_err()), "Kick didn't offer a way to play this VOD.");
}

/// A VOD's chat is in its channel's history, from when its playlist's first
/// segment was, or failing that the time in its id (kick.rs `chat_origin`:
/// "Finds VOD `id`'s channel and start time. The start is the wall-clock
/// time the media playlist gives its first segment; failing that, the time
/// in the VOD's id (a UUIDv7, made when the stream started)").
#[test]
fn a_kick_vods_chat_is_its_channels_from_when_it_started() {
    let net = Net::new();
    let vod = a_kick_vod(6).started(STARTED);
    net.kick_vod(&vod);
    let (_, id) = parse_vod_url(&vod.url()).unwrap();
    assert_eq!(chat_origin(&id).unwrap(), ChatOrigin { channel: 6_000_006, start: STARTED });
    let undated = a_kick_vod(7);
    net.kick_vod(&undated);
    let (_, id) = parse_vod_url(&undated.url()).unwrap();
    // 01a0e9a8-a9e0 is 2026-09-28T20:15:40Z.
    assert_eq!(chat_origin(&id).unwrap(), ChatOrigin { channel: 6_000_007, start: 1790626540.0 });
    playback_answers(&net, 8, 500, json!({}));
    let (_, id) = parse_vod_url(&a_kick_vod(8).url()).unwrap();
    assert_eq!(
        format!("{:#}", chat_origin(&id).unwrap_err()),
        "Kick isn't responding right now (500). Try again later."
    );
}

#[test]
fn finds_when_a_vod_starts() {
    // 01a0e9a8-a9e0 is 2026-09-28T20:15:40Z; the playlist's first segment is 3 s earlier.
    assert_eq!(uuid_v7_time("01a0e9a8-a9e0-797e-9646-df39aa9989ee"), Some(1790626540.0));
    assert_eq!(uuid_v7_time("01a0e9a8-a9e0-497e-9646-df39aa9989ee"), None, "not a UUIDv7");
    let playlist = "#EXTM3U\n#EXT-X-TWITCH-TOTAL-SECS:40415.302\n#EXT-X-PROGRAM-DATE-TIME:2026-09-28T20:15:37.046Z\n\
                    #EXTINF:10.000,\n0.ts\n#EXT-X-PROGRAM-DATE-TIME:2026-09-28T20:15:47.046Z\n";
    assert_eq!(first_program_date(playlist), Some(1790626537.046));
    assert_eq!(first_program_date("#EXTM3U\n"), None);
}
