//! The player commands' specs (player.rs): a VOD's file or an edit opened,
//! played, sought and stepped on the screen, what the page is told, the
//! frames it's given, and its loudness. The fake ffmpeg decodes (its frames
//! say their number, `media::numbered_frame`), and the window's screen is
//! the test's (`TestApp::with_screen`).

use super::*;

use serde_json::{Value, json};
use vodscribe_core::db::Vod;
use vodscribe_core::fetch::VodInfo;
use vodscribe_core::testkit::{Gate, Media, eventually, media};

use crate::testkit::{TestApp, frame_number};

/// The tests' account of the player: none open, then one, as JSON the dump
/// reads (player.rs:64: "What the app's player is at, for the end-to-end
/// tests' page dump … whether one is open, for what, and what its threads
/// are at"). A test hook's own spec.
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
/// recording's `refile` comes before the open is in place (player.rs:183:
/// "The player just opened for VOD `id`, if it plays the VOD as its video
/// downloads (`streamed`), plays the video kept, if it was kept meanwhile";
/// 0.7.0 Fixes: "Playback survives the download finishing."). Below the
/// commands: the order needs a hold while the file is probed (K9).
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

/// Frames are scaled down for the page by mixing the pixels under each,
/// or, more than halved, by their mean (player.rs:555: "each pixel the
/// bilinear mix at its centre (at half size, the mean of four); more than
/// halved, the mean of all the pixels it covers").
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
    // Three greys to two, across or down: each the mix at its centre.
    let greys: Vec<u8> = [0u8, 90, 180].iter().flat_map(|&g| [g; 3]).collect();
    for (w, h, to_w, to_h) in [(3, 1, 2, 1), (1, 3, 1, 2)] {
        let mut out = Vec::new();
        scale_rgba(&greys, w, h, to_w, to_h, &mut out);
        assert_eq!(out, [23, 23, 23, 255, 158, 158, 158, 255], "{w} by {h} to {to_w} by {to_h}");
    }
    // A third the height: each the mean of the three rows under it.
    let column: Vec<u8> = [0u8, 0, 90, 0, 0, 0].iter().flat_map(|&g| [g; 3]).collect();
    let mut out = Vec::new();
    scale_rgba(&column, 1, 6, 1, 2, &mut out);
    assert_eq!(out, [30, 30, 30, 255, 0, 0, 0, 255]);
    // By area, every row counts: a white pixel on the first of two.
    let mut two_rows = vec![0u8; 8 * 3];
    two_rows[..3].copy_from_slice(&[255, 255, 255]);
    let mut out = Vec::new();
    scale_rgba(&two_rows, 4, 2, 1, 1, &mut out);
    assert_eq!(out, [32, 32, 32, 255]);
}

/// The loudness set is between silent and full, whatever the page sends
/// (`player_set_volume`: "The loudness, 0 to 1."; plans/sections-074.md
/// "Decided", unstated behaviour 7, owner 2026-11-11: "the volume is clamped
/// to 0..1: spec it"). A site's player can send more: the embed's stand-in
/// for its video (embed_shim.js, `volume`) takes any number it's given.
#[test]
fn the_volume_set_is_between_silent_and_full() {
    let app = TestApp::new();
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::default()).unwrap();
    *app.state().player.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(1), embed: None });
    let set = |volume: f64| {
        app.invoke("player_set_volume", json!({ "volume": volume })).unwrap();
        app.state().player.open.lock().unwrap().as_ref().unwrap().player.volume()
    };
    assert_eq!(set(1.5), 1.0);
    assert_eq!(set(-0.5), 0.0);
    assert_eq!(set(0.25), 0.25);
    assert_eq!(set(100.0), 1.0, "a site's 0 to 100");
}

/// A file of `duration` seconds whose picture is `width` by `height` at 30
/// fps, added as `name`: small, so its frames decode in a moment.
fn video(app: &TestApp, name: &str, duration: f64, (width, height): (u32, u32)) -> Vod {
    let mut source = Media::video(duration);
    let video = source.video.as_mut().unwrap();
    (video.width, video.height) = (width, height);
    app.add_file(name, &source)
}

/// A file of `duration` seconds, 64 by 36, added.
fn small_video(app: &TestApp, duration: f64) -> Vod {
    video(app, "Day 3.mp4", duration, (64, 36))
}

/// A VOD a minute long from a site without its own player, none of its
/// video here but `from`..`to`, a section downloaded before (64 by 36), as
/// the footage queue leaves one (footage.rs `ensure_as`).
fn site_vod_with(app: &TestApp, (from, to): (f64, f64)) -> Vod {
    let vod = app.add_site_vod("vimeo", "x", "Day 3", 60.0);
    let mut section = Media::video(to - from);
    let video = section.video.as_mut().unwrap();
    (video.width, video.height) = (64, 36);
    section.container = "matroska".into();
    let name = vodscribe_core::footage::section_stem(vod.id, from, to);
    section.write(&app.paths().source_footage().join(format!("{name}.mkv")));
    vod
}

/// An edit titled "Best of" made of `pieces`, each a VOD's id and the
/// stretch of it, in order.
fn edit_of(app: &TestApp, pieces: &[(i64, (f64, f64))]) -> i64 {
    let db = app.db();
    let edit = db.add_edit("Best of").unwrap().id;
    for &(vod, stretch) in pieces {
        db.add_piece(edit, vod, stretch, "piece", None).unwrap();
    }
    edit
}

/// The page's open of VOD `id` at `at`, playing if `play`: its answer.
fn open(app: &TestApp, id: i64, at: f64, play: bool) -> Value {
    app.invoke("player_open", json!({ "id": id, "at": at, "play": play })).unwrap()
}

/// The page's open of edit `id` at `at`, playing if `play`: its answer.
fn open_edit(app: &TestApp, id: i64, at: f64, play: bool) -> Value {
    app.invoke("player_open_edit", json!({ "id": id, "at": at, "play": play })).unwrap()
}

/// The page's seek to `time`, playing from there if `play`: its status.
fn seek(app: &TestApp, time: f64, play: bool) -> Value {
    app.invoke("player_seek", json!({ "time": time, "play": play })).unwrap()
}

/// The status of a player that's playing, as a command answers it (a play
/// while playing changes nothing).
fn playing_status(app: &TestApp) -> Value {
    let status = app.invoke("player_play", json!({})).unwrap();
    assert_eq!(status["playing"], true, "it was playing");
    status
}

/// Waits till the screen shows frame `n` of a fake video `width` wide.
fn shows(app: &TestApp, n: u64, width: u32) {
    eventually(&format!("frame {n}, {width} wide, on the screen"), || {
        app.screen().showing().is_some_and(|f| f.width == width && frame_number(&f) == n)
    });
}

/// Whether a player playing holds back VOD `vod`'s background work (the
/// storyboards' queue asks: storyboards.rs).
fn holds_back(app: &TestApp, vod: i64) -> bool {
    let state = app.state();
    state.player.playing_other(&state.db, vod)
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

/// A VOD opened while another plays takes its place: the screen shows the
/// new one's frames, the commands move it, and nothing else plays
/// (player.rs:1: "The app's own player … on the screen (screen.rs) … one
/// player at a time").
#[test]
fn one_player_plays_at_a_time() {
    let app = TestApp::new();
    app.with_screen();
    let first = video(&app, "Day 3.mp4", 60.0, (64, 36));
    let second = video(&app, "Day 4.mp4", 60.0, (32, 18));
    open(&app, first.id, 10.0, true);
    eventually("the first playing", || app.screen().showing().is_some_and(|f| f.width == 64 && f.pts > 10.0));
    let opened = open(&app, second.id, 40.0, false);
    assert_eq!(opened["status"]["playing"], false);
    shows(&app, 1200, 32);
    assert!(!holds_back(&app, first.id) && !holds_back(&app, second.id), "nothing plays: the first is gone");
    let stepped = app.invoke("player_step", json!({ "frames": 1 })).unwrap();
    assert_eq!(stepped["playing"], false);
    shows(&app, 1201, 32);
}

/// A VOD opened at the time asked, paused or playing as asked, and the
/// frame on screen is the one there (player.rs:160: "Opens VOD `id`'s file
/// at `at` seconds, playing if `play`").
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

/// The VOD open already, asked for again, is the same player moved there:
/// it keeps the speed it was set to, where another VOD's opening starts a
/// new one (player.rs:160: "The one open for it already is kept, moved
/// there"; :38: "Opened for this VOD or edit; a page that asks for the same
/// one keeps it").
#[test]
fn the_vod_open_already_is_kept_and_moved_there() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    let other = video(&app, "Day 4.mp4", 60.0, (32, 18));
    open(&app, vod.id, 10.0, false);
    app.invoke("player_set_speed", json!({ "speed": 2.0 })).unwrap();
    let again = open(&app, vod.id, 30.0, true);
    assert_eq!(again["duration"], 60.0);
    assert_eq!(again["status"]["speed"], 2.0, "the same player");
    assert_eq!(again["status"]["time"], 30.0, "moved there");
    assert_eq!(again["status"]["playing"], true);
    eventually("frames from 0:30 on", || app.screen().showing().is_some_and(|f| frame_number(&f) >= 900));
    // Another VOD, then this one again: a player of its own each time.
    open(&app, other.id, 0.0, false);
    let anew = open(&app, vod.id, 30.0, false);
    assert_eq!(anew["status"]["speed"], 1.0, "a new player");
}

/// A VOD from a site without its own player, its video still coming in,
/// plays as far as it's in, then the time stands there, the picture holding
/// its last frame, and the page is told it's waiting; a seek past what's in
/// waits there too (player.rs:164: "A VOD from a site without its own
/// player whose video is still downloading (videos.rs) plays as far as it's
/// in"; :201: "where the download hasn't reached, the time stands (the
/// picture holding its last frame) and the page says it's waiting … A seek
/// past what's in waits the same way"; 0.6.0: "Other sites' VODs play by
/// downloading." ("it plays in VODscribe as it comes in, waiting where it
/// hasn't yet"); e2e download-play.mjs). As more comes in, it plays on
/// (:229: "More of VOD `vod_id`'s footage is in … it plays on into what came").
#[test]
fn a_vod_still_downloading_plays_as_far_as_it_is_in_and_waits_there() {
    let app = TestApp::new();
    app.with_screen();
    let vod = site_vod_with(&app, (0.0, 30.0));
    let opened = open(&app, vod.id, 28.0, true);
    assert_eq!(opened["duration"], 60.0, "all of it, as its site says");
    assert_eq!(opened["status"]["waiting"], false, "0:28 is in");
    eventually("waiting at 0:30, where what's in ends", || {
        let now = playing_status(&app);
        now["waiting"] == true && now["time"] == 30.0
    });
    let last = app.screen().frame_shown();
    assert!((840..900).contains(&last), "the picture holds the last frame in, not {last}");
    let sought = seek(&app, 45.0, true);
    assert_eq!(sought["time"], 45.0);
    assert_eq!(sought["waiting"], true, "past what's in: waiting there");
    // All of it comes in (Keep video), held before it's kept: it plays on.
    let keeping = Gate::new();
    app.ffmpeg.holds_at(0.5, &keeping);
    app.invoke("keep_video", json!({ "id": vod.id })).unwrap();
    keeping.reached();
    assert_eq!(playing_status(&app)["waiting"], false, "0:45 is in now");
    eventually("playing on from 0:45", || app.screen().showing().is_some_and(|f| frame_number(&f) > 1350));
    keeping.open();
}

/// A VOD playing as its video downloads, once its video is kept, plays the
/// video kept from where it is, waiting for nothing: the section it came as
/// is gone, and the kept video plays anywhere (player.rs:252: "VOD
/// `vod_id`'s video is kept now … the player open playing it as it
/// downloaded plays that from here on, where it is, waiting for nothing";
/// 0.7.0 Fixes: "Playback survives the download finishing." ("it carries on
/// from the video kept, where it was")).
#[test]
fn a_vod_playing_as_it_downloads_plays_the_kept_video_from_where_it_is() {
    let app = TestApp::new();
    app.with_screen();
    let vod = site_vod_with(&app, (0.0, 30.0));
    open(&app, vod.id, 28.0, true);
    eventually("waiting at 0:30", || playing_status(&app)["waiting"] == true);
    // Keep video: all of it comes as a section, then is kept (videos.rs).
    app.invoke("keep_video", json!({ "id": vod.id })).unwrap();
    eventually("its video kept", || {
        app.heard("video-progress").iter().any(|p| p["id"] == vod.id && p["progress"].is_null())
    });
    assert!(app.db().vod(vod.id).unwrap().unwrap().media, "kept: {:?}", app.heard("video-progress"));
    let now = playing_status(&app);
    assert_eq!(now["waiting"], false, "waiting for nothing");
    assert!(now["time"].as_f64().unwrap() >= 30.0, "from where it was, not {}", now["time"]);
    eventually("played on past 0:30", || app.screen().showing().is_some_and(|f| frame_number(&f) > 900));
    // The kept video, not the section it came as (deleted once it's kept).
    seek(&app, 45.0, false);
    shows(&app, 1350, 1920);
}

/// An edit plays its pieces one after the other, each from its own VOD's
/// footage (player.rs:270: "Opens edit `id`, its pieces end to end from
/// their footage, at `at` seconds, playing if `play`").
#[test]
fn an_edit_opens_with_its_pieces_end_to_end() {
    let app = TestApp::new();
    app.with_screen();
    let first = video(&app, "Day 3.mp4", 60.0, (64, 36));
    let second = video(&app, "Day 4.mp4", 60.0, (32, 18));
    let edit = edit_of(&app, &[(first.id, (10.0, 20.0)), (second.id, (40.0, 45.0))]);
    let opened = open_edit(&app, edit, 0.0, false);
    assert_eq!(opened["duration"], 15.0, "ten seconds, then five");
    assert_eq!(opened["status"]["playing"], false);
    shows(&app, 300, 64);
    seek(&app, 9.5, false);
    shows(&app, 585, 64);
    seek(&app, 10.0, false);
    shows(&app, 1200, 32);
    let played = open_edit(&app, edit, 14.0, true);
    assert_eq!(played["status"]["playing"], true);
    eventually("the second piece plays", || {
        app.screen().showing().is_some_and(|f| f.width == 32 && frame_number(&f) > 1320)
    });
}

/// An edit's footage that isn't here is fetched as it opens; once it's in,
/// the page asks for the plan again, and that plays it (player.rs:270:
/// "Footage not here yet is fetched, and the page asks for a plan again
/// (`player_plan`) as it comes in").
#[test]
fn an_edits_footage_not_here_is_fetched_and_its_plan_asked_again_as_it_comes() {
    let app = TestApp::new();
    app.with_screen();
    let vod = app.add_site_vod("vimeo", "x", "Day 3", 60.0);
    let edit = edit_of(&app, &[(vod.id, (10.0, 20.0))]);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    let opened = open_edit(&app, edit, 0.0, false);
    assert_eq!(opened["duration"], 10.0);
    begun.reached();
    let plan = app.invoke("player_plan", json!({ "id": edit })).unwrap();
    assert_eq!(plan[0]["footage"], false, "not here yet: {plan}");
    begun.open();
    eventually("its footage fetched", || app.heard("footage").iter().any(|f| f["vod_id"] == vod.id));
    assert!(app.heard("footage").iter().all(|f| f["error"].is_null()), "{:?}", app.heard("footage"));
    let plan = app.invoke("player_plan", json!({ "id": edit })).unwrap();
    assert_eq!(plan[0]["footage"], true, "here now: {plan}");
    assert_eq!(app.screen().frame_shown(), 300, "the piece's start, 0:10");
}

/// A plan asked for while an edit plays, changed after the playhead, plays
/// on without a break (the time keeps running), and plays the new plan from
/// then on (player.rs:323: "the player open for it plays them from here on
/// (without a break where the playhead's stretch is unchanged)").
#[test]
fn a_new_plan_plays_on_without_a_break_where_the_playheads_stretch_is_unchanged() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    let edit = edit_of(&app, &[(vod.id, (0.0, 20.0)), (vod.id, (40.0, 50.0))]);
    open_edit(&app, edit, 5.0, true);
    eventually("the time running", || playing_status(&app)["running"] == true);
    app.db().add_piece(edit, vod.id, (50.0, 55.0), "piece", None).unwrap();
    let plan = app.invoke("player_plan", json!({ "id": edit })).unwrap();
    assert_eq!(plan.as_array().unwrap().len(), 3, "{plan}");
    let now = playing_status(&app);
    assert_eq!(now["running"], true, "no break: the time runs on");
    assert!(now["time"].as_f64().unwrap() >= 5.0);
    // The piece added plays: 0:31 of the edit is 0:51 of the VOD.
    seek(&app, 31.0, false);
    shows(&app, 1530, 64);
}

/// Each status made counts up, whichever of a command's reply or the
/// `player-time` events carries it, so the newest by its count is the
/// player as it is (player.rs:109: "Counts up with each status made: a
/// command's reply and the `player-time` events travel apart, so the page
/// keeps the newest").
#[test]
fn the_page_keeps_the_newest_status_by_its_count() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    let opened = open(&app, vod.id, 10.0, true);
    eventually("frames playing", || app.screen().showing().is_some_and(|f| frame_number(&f) > 305));
    let before = app.heard("player-time");
    let paused = app.invoke("player_pause", json!({})).unwrap();
    let count = |s: &Value| s["seq"].as_u64().unwrap();
    assert!(count(&paused) > count(&opened["status"]));
    assert!(before.iter().all(|s| count(s) < count(&paused)), "every status before counts less");
    let heard = app.heard("player-time");
    let newest = heard.iter().chain([&paused]).max_by_key(|s| count(s)).unwrap();
    assert_eq!(newest["playing"], false, "the newest is paused: {newest}");
    assert_eq!(newest["time"], paused["time"]);
}

/// The time the page shows is where the frame on screen is, and the frame
/// rate it counts frames by: after a seek it's the frame's, not the time
/// asked (player.rs:105: "`shown`, where the frame on screen is"; 0.5.0:
/// "The time under the player, as a clock or by frame, is the frame on
/// screen").
#[test]
fn the_time_shown_is_the_frame_on_screen() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    let opened = open(&app, vod.id, 10.25, false);
    assert_eq!(opened["status"]["fps"], 30.0);
    assert_eq!(app.screen().frame_shown(), 307, "the frame showing at 0:10.25");
    let on_screen = |app: &TestApp| {
        let frame = app.screen().showing().unwrap();
        let told = app.heard("player-time").last().cloned().unwrap();
        told["shown"].as_f64() == Some(frame.pts)
    };
    eventually("the page told the frame's time", || on_screen(&app));
    app.invoke("player_step", json!({ "frames": 1 })).unwrap();
    shows(&app, 308, 64);
    eventually("the page told the next frame's time", || on_screen(&app));
}

/// The frame the page draws itself is empty when it's the one it has, and
/// else its size, number and RGBA, scaled down to fit the box asked, never
/// up (player.rs:481: "empty if it's frame `since` (or none shows), else
/// its width and height (4 bytes each, little-endian) and number (8 bytes),
/// then RGBA pixels, scaled down to fit in `width` by `height` (never
/// up)").
#[test]
fn a_frame_asked_for_is_empty_if_unchanged_and_else_scaled_down_to_fit_never_up() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    let ask = |since: Option<u64>, width: u32, height: u32| {
        app.invoke_bytes("player_frame", json!({ "since": since, "width": width, "height": height })).unwrap()
    };
    assert_eq!(ask(None, 640, 360), Vec::<u8>::new(), "none shows: empty");
    open(&app, vod.id, 10.0, false);
    assert_eq!(app.screen().frame_shown(), 300);
    let big = ask(None, 640, 360);
    assert_eq!((u32_at(&big, 0), u32_at(&big, 4)), (64, 36), "never scaled up");
    assert_eq!(big.len(), 16 + 64 * 36 * 4, "RGBA");
    assert_eq!(number_in(&big[16..], 64, 36), 300);
    let number = u64::from_le_bytes(big[8..16].try_into().unwrap());
    assert_eq!(ask(Some(number), 640, 360), Vec::<u8>::new(), "unchanged: empty");
    let small = ask(Some(number + 1), 32, 100);
    assert_eq!((u32_at(&small, 0), u32_at(&small, 4)), (32, 18), "scaled down to fit, its shape kept");
    assert_eq!(small.len(), 16 + 32 * 18 * 4);
    assert_eq!(number_in(&small[16..], 32, 18), 300);
    let flat = ask(Some(number + 1), 640, 18);
    assert_eq!((u32_at(&flat, 0), u32_at(&flat, 4)), (32, 18), "to fit the height");
}

/// The still the page shows while something covers the screen is the frame
/// showing, its width and height, then its RGB (player.rs:461: "its width
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
/// and the player stays where it is (player.rs:516: "The frame a seek to
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
    let other = json!({ "doc": { "kind": "vod", "id": vod.id + 1 }, "time": 30.0, "width": 32, "height": 18 });
    assert_eq!(app.invoke_bytes("player_preview", other), Ok(Vec::new()), "not the one open: none");
    let status = app.invoke("player_pause", json!({})).unwrap();
    assert_eq!(status["time"], 10.0, "the player is where it was");
    assert_eq!(app.screen().frame_shown(), 300, "and so is the screen");
}

/// Closing the player clears the screen, but not when a site's player is
/// playing in it: only that session closes it (player.rs:613: "Closes the
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

/// A site's player's session is the only one that changes what its player
/// plays or closes it: another session's late change or close leaves it be
/// (player.rs:40: "its session's token (embed.rs), so only that session
/// changes or closes it"; :443: "a late close of an earlier VOD's leaves
/// the next one's be"). Its VOD's footage coming in, or its video kept,
/// changes nothing either (:229, :252: those are for "not a site's
/// player"), and the timeline has no picture of it (:516: "a site's VOD
/// played from its player has none").
#[test]
fn only_an_embed_session_changes_or_closes_its_player() {
    let app = TestApp::new();
    app.with_screen();
    let first = video(&app, "Day 3.mp4", 60.0, (64, 36));
    let second = video(&app, "Day 4.mp4", 60.0, (32, 18));
    let site = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 60.0);
    let file = |vod: &Vod| Timeline::file(std::path::Path::new(vod.source_path.as_deref().unwrap()), 60.0);
    app.run(crate::player::open_embed(app.handle().clone(), site.id, 7, file(&first), 5.0)).unwrap();
    shows(&app, 150, 64);
    let doc = json!({ "doc": { "kind": "vod", "id": site.id }, "time": 30.0, "width": 32, "height": 18 });
    assert_eq!(app.invoke_bytes("player_preview", doc), Ok(Vec::new()), "no picture of a site's player's");
    // Another session's, or its VOD's footage and video: nothing changes.
    crate::player::set_embed_timeline(app.handle(), 8, file(&second));
    crate::player::close_embed(app.handle(), 8);
    crate::player::footage_grew(app.handle(), site.id);
    crate::player::refile(app.handle(), site.id, std::path::Path::new(second.source_path.as_deref().unwrap()), 60.0);
    seek(&app, 20.0, false);
    shows(&app, 600, 64);
    // Its own: what it appended plays.
    crate::player::set_embed_timeline(app.handle(), 7, file(&second));
    seek(&app, 30.0, false);
    shows(&app, 900, 32);
    // The next VOD's session, then the earlier one's late close: the next one's plays on.
    app.run(crate::player::open_embed(app.handle().clone(), second.id, 9, file(&second), 5.0)).unwrap();
    shows(&app, 150, 32);
    crate::player::close_embed(app.handle(), 7);
    assert!(app.invoke("player_play", json!({})).is_ok(), "the next one's still open");
    crate::player::close_embed(app.handle(), 9);
    assert!(app.invoke("player_pause", json!({})).is_err(), "its own session closed it");
}

/// A seek goes to the frame at the time asked, and plays from there if
/// asked (player.rs:657: "Goes to `time`, playing from there if `play`").
#[test]
fn a_seek_goes_there_playing_if_asked() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    open(&app, vod.id, 0.0, false);
    let paused = seek(&app, 30.0, false);
    assert_eq!(paused["time"], 30.0);
    assert_eq!(paused["playing"], false);
    shows(&app, 900, 64);
    let playing = seek(&app, 40.0, true);
    assert_eq!(playing["time"], 40.0);
    assert_eq!(playing["playing"], true);
    eventually("playing from 0:40", || app.screen().showing().is_some_and(|f| frame_number(&f) > 1200));
}

/// A step moves a frame (or several, or back) and pauses there
/// (player.rs:668: "Moves `frames` frames (back, if negative), paused";
/// 0.4.0: "VODscribe's own player plays your files" (", and . step a
/// frame")).
#[test]
fn a_step_moves_frames_paused() {
    let app = TestApp::new();
    app.with_screen();
    let vod = small_video(&app, 60.0);
    open(&app, vod.id, 10.0, false);
    shows(&app, 300, 64);
    let step = |frames: i64| app.invoke("player_step", json!({ "frames": frames })).unwrap();
    assert_eq!(step(1)["playing"], false);
    shows(&app, 301, 64);
    step(-3);
    shows(&app, 298, 64);
    step(5);
    shows(&app, 303, 64);
    // Playing, a step pauses, a frame on from the one showing.
    app.invoke("player_play", json!({})).unwrap();
    eventually("playing on", || app.screen().showing().is_some_and(|f| frame_number(&f) > 310));
    let stepped = step(1);
    assert_eq!(stepped["playing"], false, "paused");
    let at = (stepped["time"].as_f64().unwrap() * 30.0).round() as u64;
    shows(&app, at, 64);
}

/// A VOD playing holds back other VODs' work in the background (their
/// storyboards), not its own; paused, it holds back none (player.rs:46:
/// "Whether a player is playing footage other than VOD `vod`'s: work in
/// the background for `vod` waits meanwhile, so playing stays smooth").
#[test]
fn a_vod_playing_holds_back_other_vods_background_work() {
    let app = TestApp::new();
    app.with_screen();
    let playing = small_video(&app, 60.0);
    let other = video(&app, "Day 4.mp4", 60.0, (32, 18));
    open(&app, playing.id, 10.0, true);
    assert!(holds_back(&app, other.id), "another VOD's work waits");
    assert!(!holds_back(&app, playing.id), "its own goes on");
    app.invoke("player_pause", json!({})).unwrap();
    assert!(!holds_back(&app, other.id), "paused, nothing waits");
}

/// An edit playing holds back the storyboards of other VODs only: those
/// of its own pieces' VODs are made meanwhile, as the edit's timeline
/// shows them, its pieces read as they are now (player.rs:48: "An edit
/// plays its pieces' VODs, so their work goes on: the edit's timeline
/// shows their storyboards. Its pieces are read as they are now, since
/// they change while it plays").
#[test]
fn an_edit_playing_holds_back_only_other_vods_work() {
    let app = TestApp::new();
    app.with_screen();
    let own = small_video(&app, 60.0);
    let other = video(&app, "Day 4.mp4", 60.0, (32, 18));
    let third = video(&app, "Day 5.mp4", 60.0, (32, 18));
    let edit = edit_of(&app, &[(own.id, (10.0, 20.0))]);
    open_edit(&app, edit, 0.0, true);
    assert!(!holds_back(&app, own.id), "the edit's own piece's VOD isn't held back");
    assert!(holds_back(&app, other.id), "another VOD's is");
    assert!(holds_back(&app, third.id), "and a third's");
    // A piece of the other added while it plays: its work goes on too.
    app.db().add_piece(edit, other.id, (0.0, 5.0), "b", None).unwrap();
    assert!(!holds_back(&app, other.id), "a piece added since counts");
    app.invoke("player_pause", json!({})).unwrap();
    assert!(!holds_back(&app, third.id), "nothing waits while it's paused");
}
