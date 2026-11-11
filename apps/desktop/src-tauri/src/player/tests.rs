//! The player commands' specs (player.rs): a VOD's file or an edit opened,
//! played, sought and stepped on the screen, what the page is told, and the
//! frames it's given, and its loudness. The fake ffmpeg decodes (its frames say their number,
//! `media::numbered_frame`), and the window's screen is the test's
//! (`TestApp::with_screen`).

use super::*;
use vodscribe_core::fetch::VodInfo;

/// The tests' account of the player: none open, then one, as JSON the dump reads.
#[test]
fn the_player_seen_says_whether_one_is_open_and_what_its_threads_are_at() {
    let native = Native::default();
    let none = serde_json::to_value(native.seen()).unwrap();
    assert_eq!(none["open"], false);
    assert!(none["player"].is_null());
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::default()).unwrap();
    *native.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(7), embed: None });
    let seen = serde_json::to_value(native.seen()).unwrap();
    assert_eq!(seen["open"], true);
    assert_eq!(seen["doc"], serde_json::json!({ "kind": "vod", "id": 7 }));
    assert_eq!(seen["player"]["thread_alive"], true);
    assert_eq!(seen["player"]["decoding"], false);
    assert_eq!(seen["player"]["state"]["playing"], false);
    // Held by a stuck thread: said so, not waited on.
    let held = native.open.lock().unwrap();
    let begun = std::time::Instant::now();
    let stuck = std::thread::scope(|s| s.spawn(|| serde_json::to_value(native.seen()).unwrap()).join().unwrap());
    assert!(stuck["open"].is_null());
    assert!(begun.elapsed() < std::time::Duration::from_secs(2));
    drop(held);
}

/// A VOD from a site without its own player, its video downloading
/// (videos.rs `stream_then_keep`), opened while the download ends: the
/// open reads the library before the video is recorded, and the
/// recording's `refile` comes before the open is in place.
#[test]
fn a_vod_opened_as_its_video_is_kept_plays_the_video_kept() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let info = VodInfo {
        platform: "vimeo".into(),
        platform_id: "x".into(),
        url: "https://vimeo.com/x".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let id = db.add_vod(&info).unwrap().0.id;
    let db = Mutex::new(db);
    let native = Native::default();
    // player_open's `streamed`: what's in of the section (none, here), waiting for the rest.
    assert!(!db.lock().unwrap().vod(id).unwrap().unwrap().media, "no video kept yet");
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::covering(60.0, Vec::new())).unwrap();
    player.set_waits_for_footage(true);
    // Meanwhile (the probe), the download ends: recorded, then refiled, with no player open yet.
    let kept = db.lock().unwrap().files().source_video(id, "mp4");
    db.lock().unwrap().set_media_path(id, Some(&kept.to_string_lossy())).unwrap();
    refile_open(&native, id, &kept, 60.0);
    // The open is in place (open_with), and catches up.
    *native.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(id), embed: None });
    catch_up(&native, &db, id);
    let open = native.open.lock().unwrap();
    let player = &open.as_ref().unwrap().player;
    assert_eq!(player.timeline(), Timeline::file(&kept, 60.0), "the video kept, not the section it replaced");
    assert!(!player.waits_for_footage());
}

/// An edit playing holds back the storyboards of other VODs only: those
/// of its own pieces' VODs are made meanwhile, as the edit's timeline
/// shows them.
#[test]
fn an_edit_playing_holds_back_only_other_vods_work() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let vod = |id: &str| {
        let info = VodInfo {
            platform: "vimeo".into(),
            platform_id: id.into(),
            url: format!("https://vimeo.com/{id}"),
            title: "t".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        db.add_vod(&info).unwrap().0.id
    };
    let (own, other, third) = (vod("a"), vod("b"), vod("c"));
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, own, (10.0, 20.0), "a", None).unwrap();
    let native = Native::default();
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::covering(60.0, Vec::new())).unwrap();
    player.play();
    assert!(player.playing());
    *native.open.lock().unwrap() = Some(Open { player, doc: Doc::Edit(edit), embed: None });
    let db = Mutex::new(db);
    assert!(!native.playing_other(&db, own), "the edit's own piece's VOD isn't held back");
    assert!(native.playing_other(&db, other), "another VOD's is");
    assert!(native.playing_other(&db, third), "and a third's");
    // A piece of the other added while it plays: its work goes on too.
    db.lock().unwrap().add_piece(edit, other, (0.0, 5.0), "b", None).unwrap();
    assert!(!native.playing_other(&db, other), "a piece added since counts");
    // Paused: nothing waits.
    native.open.lock().unwrap().as_ref().unwrap().player.pause();
    assert!(!native.playing_other(&db, third), "nothing waits while it's paused");
}

#[test]
fn frames_scale_down_by_mixing_or_by_area() {
    // 4 by 2: a white column, then black ones, over two rows.
    let row = [255u8, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let rgb = [row, row].concat();
    // Halved: each pixel the mean of the two under it.
    let mut out = Vec::new();
    scale_rgba(&rgb, 4, 2, 2, 1, &mut out);
    assert_eq!(out, [128, 128, 128, 255, 0, 0, 0, 255]);
    // To one pixel, a quarter: the mean of all four columns, not two of them.
    let mut out = Vec::new();
    scale_rgba(&rgb, 4, 2, 1, 1, &mut out);
    assert_eq!(out, [64, 64, 64, 255]);
    // A thin line in a 12x reduction still counts, as bilinear's taps would miss it.
    let mut wide = vec![0u8; 24 * 3];
    wide[13 * 3..14 * 3].copy_from_slice(&[240, 240, 240]);
    let mut out = Vec::new();
    scale_rgba(&wide, 24, 1, 2, 1, &mut out);
    assert_eq!(out, [0, 0, 0, 255, 20, 20, 20, 255]);
}

/// The loudness set is between silent and full, whatever the page sends
/// (`player_set_volume`: "The loudness, 0 to 1."; plans/sections-074.md
/// "Decided", unstated behaviour 7, owner 2026-11-11: "the volume is clamped
/// to 0..1: spec it"). A site's player can send more: the embed's stand-in
/// for its video (embed_shim.js, `volume`) takes any number it's given.
#[test]
fn the_volume_set_is_between_silent_and_full() {
    let app = crate::testkit::TestApp::new();
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::default()).unwrap();
    *app.state().player.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(1), embed: None });
    let set = |volume: f64| {
        app.invoke("player_set_volume", serde_json::json!({ "volume": volume })).unwrap();
        app.state().player.open.lock().unwrap().as_ref().unwrap().player.volume()
    };
    assert_eq!(set(1.5), 1.0);
    assert_eq!(set(-0.5), 0.0);
    assert_eq!(set(0.25), 0.25);
    assert_eq!(set(100.0), 1.0, "a site's 0 to 100");
}

use serde_json::{Value, json};
use vodscribe_core::db::Vod;
use vodscribe_core::testkit::{Media, eventually, media};

use crate::testkit::TestApp;

/// A file of `duration` seconds whose picture is 64 by 36 at 30 fps, added:
/// small, so its frames decode in a moment.
fn small_video(app: &TestApp, duration: f64) -> Vod {
    let mut source = Media::video(duration);
    let video = source.video.as_mut().unwrap();
    (video.width, video.height) = (64, 36);
    app.add_file("Day 3.mp4", &source)
}

/// The page's open of VOD `id` at `at`, playing if `play`: its answer.
fn open(app: &TestApp, id: i64, at: f64, play: bool) -> Value {
    app.invoke("player_open", json!({ "id": id, "at": at, "play": play })).unwrap()
}

/// The number of the fake frame in `bytes` (RGB, 3 a pixel, or RGBA, 4),
/// `width` by `height`.
fn number_in(bytes: &[u8], width: u32, height: u32) -> u64 {
    let channels = bytes.len() / (width * height) as usize;
    media::frame_number(bytes, width, height, channels).expect("a fake video's frame")
}

/// A little-endian u32 at `at` in `bytes`.
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The window without a screen (Linux under X11 without one, a failed
/// attach) has no player, and the page plays files itself (player.rs:154:
/// "Whether there's a player here: a screen to draw on"; lib.rs: "Without
/// a screen, the page plays files itself as before").
#[test]
fn there_is_a_player_only_with_a_screen_to_draw_on() {
    let app = TestApp::new();
    assert_eq!(app.invoke("player_available", json!({})), Ok(json!(false)));
    app.with_screen();
    assert_eq!(app.invoke("player_available", json!({})), Ok(json!(true)));
}

/// A VOD's file opens at the time asked, paused or playing as asked, and
/// the frame on screen is the one there (player.rs:160: "Opens VOD `id`'s
/// file at `at` seconds, playing if `play`").
#[test]
fn a_vod_opened_plays_from_where_asked_playing_or_paused_as_asked() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    let paused = open(&app, vod.id, 10.0, false);
    assert_eq!(paused["duration"], 60.0);
    assert_eq!(paused["fps"], 30.0);
    assert_eq!(paused["status"]["time"], 10.0);
    assert_eq!(paused["status"]["playing"], false);
    assert_eq!(app.screen().frame_shown(), 300, "the frame at 0:10");
    let playing = open(&app, vod.id, 20.0, true);
    assert_eq!(playing["status"]["time"], 20.0);
    assert_eq!(playing["status"]["playing"], true);
    eventually("frames from 0:20 on", || app.screen().showing().is_some_and(|f| f.pts >= 20.0));
    assert!(app.screen().frame_shown() >= 600, "played on from 0:20");
}

/// The still the page shows while something covers the screen is the frame
/// showing, its width and height, then its RGB (player.rs:457: "its width
/// and height (each 4 bytes, little-endian), then its pixels as RGB. Empty
/// if none shows").
#[test]
fn the_still_is_the_frame_showing_as_its_size_then_rgb() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    assert_eq!(app.invoke_bytes("player_still", json!({})), Ok(Vec::new()), "none shows: empty");
    open(&app, vod.id, 10.0, false);
    let on_screen = app.screen().frame_shown();
    let still = app.invoke_bytes("player_still", json!({})).unwrap();
    let (width, height) = (u32_at(&still, 0), u32_at(&still, 4));
    assert_eq!((width, height), (64, 36));
    assert_eq!(still.len(), 8 + 64 * 36 * 3, "RGB, three bytes a pixel");
    assert_eq!(number_in(&still[8..], width, height), on_screen, "the frame on screen");
}

/// The timeline's picture of a moment is the frame a seek there would show,
/// and the player stays where it is (player.rs:512: "The frame a seek to
/// `time` of `doc` would show … The player isn't moved"; 0.6.0: "The
/// timeline shows a picture of each moment.").
#[test]
fn a_preview_is_the_frame_a_seek_would_show_and_the_player_stays_where_it_is() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    open(&app, vod.id, 10.0, false);
    assert_eq!(app.screen().frame_shown(), 300);
    let doc = json!({ "kind": "vod", "id": vod.id });
    let args = json!({ "doc": doc, "time": 30.0, "width": 32, "height": 18 });
    let preview = app.invoke_bytes("player_preview", args).unwrap();
    let (width, height) = (u32_at(&preview, 0), u32_at(&preview, 4));
    assert_eq!((width, height), (32, 18), "scaled down to fit");
    assert_eq!(f64::from_le_bytes(preview[8..16].try_into().unwrap()), 30.0, "where it shows");
    assert_eq!(u32_at(&preview, 16), 1, "exactly the frame there");
    assert_eq!(preview.len(), 20 + 32 * 18 * 4, "RGBA");
    assert_eq!(number_in(&preview[20..], width, height), 900, "the frame at 0:30");
    let status = app.invoke("player_pause", json!({})).unwrap();
    assert_eq!(status["time"], 10.0, "the player is where it was");
    assert_eq!(app.screen().frame_shown(), 300, "and so is the screen");
}

/// Closing the player clears the screen, but not when a site's player is
/// playing in it: only that session closes it (player.rs:609: "Closes the
/// player, clearing the screen; not one an embed session has open (only
/// that session closes it: `close_embed`), so a page that closes late
/// leaves the next VOD's be").
#[test]
fn closing_clears_the_screen_but_never_an_embed_sessions_player() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    open(&app, vod.id, 10.0, false);
    assert_eq!(app.screen().frame_shown(), 300);
    app.invoke("player_close", json!({})).unwrap();
    assert!(app.screen().showing().is_none(), "the screen cleared");
    assert!(app.invoke("player_play", json!({})).is_err(), "no player open");

    let path = std::path::PathBuf::from(vod.source_path.as_deref().unwrap());
    let timeline = Timeline::file(&path, 60.0);
    app.run(crate::player::open_embed(app.handle().clone(), vod.id, 7, timeline, 5.0)).unwrap();
    assert_eq!(app.screen().frame_shown(), 150, "the session's player shows 0:05");
    app.invoke("player_close", json!({})).unwrap();
    assert_eq!(app.screen().frame_shown(), 150, "still showing");
    assert!(app.invoke("player_play", json!({})).is_ok(), "the session's player is still open");
    crate::player::close_embed(app.handle(), 7);
    assert!(app.screen().showing().is_none(), "the session closed it");
    assert!(app.invoke("player_pause", json!({})).is_err());
}
