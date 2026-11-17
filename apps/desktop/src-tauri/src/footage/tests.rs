//! The specs of `footage.rs`: the video fetched from sites for clips and
//! edits, kept only while an edit uses it, what Settings › Storage says it
//! takes and deletes at once (`footage_bytes`, `clear_footage`), the
//! queue's word to the page on each fetch, a fetch stopped by a removal, a
//! clip waiting behind one, what the start fetches again, and a VOD's frame
//! from its footage (`vod_frame`).
//!
//! A section's last use is the file's own date, which the app's tidying
//! reads against the system clock (core footage.rs `tidy`), so a test sets
//! it back (`footage::used_at`) rather than waits; the start's storyboards'
//! sweep, which reads the sections, is held meanwhile (`holds_sweeps`).
//! Waiting: the tidying every ten minutes with nothing fetched, timed on
//! the real clock (`TIDY_EVERY`), till the app's clock covers it.
//!
//! Stated elsewhere, cited here and not repeated: a clip of a site's VOD
//! is cut from a section downloaded for it, and one made a little longer
//! needs no new download (clips/tests/making.rs
//! `a_clip_of_a_site_s_vod_is_cut_from_a_section_downloaded_for_it`,
//! `a_clip_made_a_little_longer_needs_no_new_download`); footage a quit
//! left downloading goes at the start and is downloaded again
//! (tests/start.rs `at_start_footage_a_quit_left_downloading_goes`,
//! `footage_a_quit_stopped_downloading_is_downloaded_at_the_next_start`);
//! an edit's footage not here is fetched and its plan asked again
//! (player/tests.rs
//! `an_edits_footage_not_here_is_fetched_and_its_plan_asked_again_as_it_comes`);
//! a removed VOD's footage is erased with it (vods/tests/removing.rs
//! `an_erasure_erases_the_vods_files_footage_and_pictures`).

use super::*;

use std::time::SystemTime;

use serde_json::{Value, json};
use vodscribe_core::db::{ClipStatus, Vod};
use vodscribe_core::testkit::{Gate, Media, eventually};

use crate::testkit::{Folder, TestApp};

/// Ten minutes of a VOD from Twitch; the fake site serves it.
fn a_site_vod(app: &TestApp) -> Vod {
    app.add_site_vod("twitch", "2901646294", "Day 3", 600.0)
}

/// Section `from`..`to` of VOD `vod` in the footage folder of `paths`, as
/// the footage queue leaves one, last used `unused_for` ago.
fn section(paths: &vodscribe_core::Paths, vod: i64, (from, to): (f64, f64), unused_for: Duration) -> PathBuf {
    let path = paths.source_footage().join(format!("{}.mkv", footage::section_stem(vod, from, to)));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut media = Media::video(to - from);
    media.container = "matroska".into();
    media.write(&path);
    footage::used_at(&path, SystemTime::now() - unused_for);
    path
}

/// An hour and `minutes` more, or less if negative.
fn hour_and(minutes: i64) -> Duration {
    Duration::from_secs((3600 + minutes * 60) as u64)
}

/// Starts the app on `folder` with its storyboards' sweep held, so nothing
/// reads the sections (a read is a use) while the start tidies; the gate
/// lets the sweep go.
fn start_tidying(folder: Folder) -> (TestApp, Gate) {
    let sweep = Gate::new();
    folder.holds_sweeps(&sweep);
    (TestApp::on(folder), sweep)
}

/// What Settings › Storage says fetched video takes.
fn footage_bytes(app: &TestApp) -> u64 {
    app.invoke("footage_bytes", json!({})).unwrap().as_u64().unwrap()
}

/// Delete fetched video, in Settings › Storage.
fn clear_footage(app: &TestApp) {
    app.invoke("clear_footage", json!({})).unwrap();
}

/// The sections of VOD `vod` in the footage folder, from and to.
fn sections_of(app: &TestApp, vod: i64) -> Vec<(f64, f64)> {
    footage::sections(&app.paths().source_footage(), vod).into_iter().map(|(_, from, to)| (from, to)).collect()
}

/// The page's open of edit `id` at its start, paused.
fn open_edit(app: &TestApp, id: i64) {
    app.invoke("player_open_edit", json!({ "id": id, "at": 0.0, "play": false })).unwrap();
}

/// The `footage` events the page heard for VOD `vod`.
fn fetched(app: &TestApp, vod: i64) -> Vec<Value> {
    app.heard("footage").into_iter().filter(|f| f["vod_id"] == vod).collect()
}

/// The last `footage-progress` event the page heard for VOD `vod`.
fn last_progress(app: &TestApp, vod: i64) -> Value {
    app.heard("footage-progress").into_iter().rfind(|p| p["vod_id"] == vod).expect("no progress heard")
}

/// Waits till the footage queue has taken all that's asked of it so far: it
/// takes what's asked in turn, so once a fetch of VOD `other`'s footage
/// asked now is over, all before it is too.
fn queue_through(app: &TestApp, other: i64) {
    open_edit(app, app.edit_of(&[(other, 10.0, 20.0)]));
    eventually("the other VOD's footage fetched", || !fetched(app, other).is_empty());
}

/// The files of VOD `vod`'s footage downloads in the work folder.
fn work_files_of(app: &TestApp, vod: i64) -> Vec<std::ffi::OsString> {
    let prefix = format!("footage-{vod}-");
    let Ok(entries) = std::fs::read_dir(app.paths().work()) else { return Vec::new() };
    entries.flatten().map(|e| e.file_name()).filter(|n| n.to_string_lossy().starts_with(&prefix)).collect()
}

/// The frame the page asks for of VOD `vod` at `at`, 320 wide.
fn vod_frame(app: &TestApp, vod: i64, at: f64) -> Vec<u8> {
    app.invoke_bytes("vod_frame", json!({ "vodId": vod, "at": at, "width": 320 })).unwrap()
}

/// Whether `bytes` are a JPEG: its start and end markers.
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xff, 0xd8]) && bytes.ends_with(&[0xff, 0xd9])
}

/// Settings › Storage › Fetched video shows how much it takes: every file
/// in the footage folder, and nothing before any is fetched (CHANGELOG.md,
/// 0.5.0: "Settings → Storage → Fetched video shows how much it takes";
/// footage.rs `footage_bytes`: "How many bytes the footage kept takes").
#[test]
fn fetched_video_shows_how_much_it_takes() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    assert_eq!(footage_bytes(&app), 0, "nothing fetched yet");
    let dir = app.paths().source_footage();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{}.mkv", footage::section_stem(vod.id, 0.0, 60.0))), [0u8; 1000]).unwrap();
    std::fs::write(dir.join(format!("{}.mkv", footage::section_stem(vod.id, 300.0, 400.0))), [0u8; 2500]).unwrap();
    assert_eq!(footage_bytes(&app), 3500);
}

/// Delete fetched video deletes at once, however recently used, all the
/// sections no edit uses, and keeps those an edit's piece plays from; what
/// it takes then is what's kept (CHANGELOG.md, 0.5.0: "Settings → Storage →
/// Fetched video ... deletes what edits don't use at once"; footage.rs
/// `clear_footage`: "Deletes the footage kept, but what edits use").
#[test]
fn delete_fetched_video_deletes_at_once_what_edits_dont_use() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let other = app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 600.0);
    app.edit_of(&[(vod.id, 100.0, 130.0)]);
    let paths = app.paths();
    let used = section(&paths, vod.id, (70.0, 160.0), Duration::ZERO);
    section(&paths, vod.id, (300.0, 400.0), Duration::ZERO);
    // Ending where the piece starts, starting where it ends: none of it plays.
    section(&paths, vod.id, (40.0, 100.0), Duration::ZERO);
    section(&paths, vod.id, (130.0, 200.0), Duration::ZERO);
    section(&paths, other.id, (0.0, 60.0), Duration::ZERO);
    clear_footage(&app);
    assert_eq!(sections_of(&app, vod.id), [(70.0, 160.0)], "the edit's kept, the rest gone");
    assert_eq!(sections_of(&app, other.id), [], "another VOD's section was kept");
    assert_eq!(footage_bytes(&app), std::fs::metadata(&used).unwrap().len(), "takes what's kept");
}

/// Fetched video deleted is fetched again when a clip needs it (CHANGELOG.md,
/// 0.5.0: "and fetched again if a clip needs it later").
#[test]
fn fetched_video_deleted_is_fetched_again_when_a_clip_needs_it() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    app.make_clip(vod.id, "1:00", "1:10");
    assert!(!sections_of(&app, vod.id).is_empty(), "the clip's footage wasn't fetched");
    clear_footage(&app);
    assert_eq!(sections_of(&app, vod.id), [], "Delete fetched video kept it");
    let clip = app.make_clip(vod.id, "0:55", "1:15");
    let media = app.clip_media(&clip);
    assert_eq!(media.cut_from.as_ref().map(|(url, _)| url.as_str()), Some(vod.info.url.as_str()), "{media:?}");
    let sections = sections_of(&app, vod.id);
    assert!(sections.iter().any(|&(from, to)| from <= 55.0 && to >= 75.0), "not fetched again: {sections:?}");
}

/// A section no edit uses goes about an hour after its last use, and one
/// used within the hour stays (CHANGELOG.md, 0.5.0: "Video fetched from
/// Kick, Twitch and YouTube for clips and edits is kept only while an edit
/// uses it: the rest is deleted about an hour after its last use";
/// footage.rs's module doc: "Sections no edit uses go about an hour after
/// their last use"). Here at the start's tidying (`Footage::run`: "first,
/// tidies the folder").
#[test]
fn a_section_no_edit_uses_goes_an_hour_after_its_last_use() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let folder = app.quit();
    let paths = folder.paths();
    let over = section(&paths, vod.id, (0.0, 60.0), hour_and(5));
    let within = section(&paths, vod.id, (300.0, 360.0), hour_and(-5));
    let (_app, sweep) = start_tidying(folder);
    eventually("the section unused for over an hour gone", || !over.exists());
    sweep.open();
    assert!(within.exists(), "a section used within the hour went");
}

/// Fetched video an edit uses is kept however long ago it was used, and
/// once no edit uses it, it goes as the rest does (CHANGELOG.md, 0.5.0:
/// "is kept only while an edit uses it"; footage.rs `UNUSED_FOR`: "it's
/// kept only as long as a clip or an edit's work needs it").
#[test]
fn fetched_video_is_kept_only_while_an_edit_uses_it() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let edit = app.edit_of(&[(vod.id, 100.0, 130.0)]);
    let folder = app.quit();
    let paths = folder.paths();
    let used = section(&paths, vod.id, (70.0, 160.0), Duration::from_secs(30 * 86_400));
    let unused = section(&paths, vod.id, (300.0, 360.0), Duration::from_secs(30 * 86_400));
    let (app, sweep) = start_tidying(folder);
    eventually("the section no edit uses gone", || !unused.exists());
    sweep.open();
    assert!(used.exists(), "a section an edit uses went");

    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    let folder = app.quit();
    // An hour and more on: the sweep's look at it was its last use.
    footage::used_at(&used, SystemTime::now() - hour_and(5));
    let (_app, sweep) = start_tidying(folder);
    eventually("the section no edit uses any more gone", || !used.exists());
    sweep.open();
}

/// What's done with goes after each fetch, not only at the start: a section
/// unused for over an hour, put there after one fetch, is gone once the next
/// ends (footage.rs `Footage::run`: "What's done with goes, after each";
/// `TIDY_EVERY`: "besides after each fetch").
#[test]
fn what_is_done_with_goes_after_each_fetch() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let other = app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 600.0);
    let first = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let second = app.edit_of(&[(vod.id, 300.0, 310.0)]);
    let (app, sweep) = start_tidying(app.quit());
    app.with_screen();
    open_edit(&app, first);
    // The queue tidies first, then fetches: past this, its start's tidying is done.
    eventually("the first edit's footage fetched", || !fetched(&app, vod.id).is_empty());
    let old = section(&app.paths(), other.id, (0.0, 60.0), hour_and(5));
    open_edit(&app, second);
    eventually("the second edit's footage fetched", || fetched(&app, vod.id).len() >= 2);
    let gone = !old.exists();
    sweep.open();
    assert!(gone, "a section unused for over an hour is there after a fetch");
}

/// Once a fetch is in, the page's last word on it is that all of its
/// section plays (footage.rs `ensure_as`: "The page's last word on it: all
/// of it plays now, or none of it does"), and the queue tells the page it's
/// in, with no error (footage.rs's module doc: "telling the page how far
/// each is (`footage-progress`) and when it's in (`footage`)").
#[test]
fn a_fetch_in_tells_the_page_all_of_its_section_plays() {
    let app = TestApp::new();
    app.with_screen();
    let vod = a_site_vod(&app);
    // 0:10-0:20 of the VOD: its section is 0:00-0:50 (`footage::MARGIN`).
    open_edit(&app, app.edit_of(&[(vod.id, 10.0, 20.0)]));
    eventually("its footage fetched", || !fetched(&app, vod.id).is_empty());
    assert_eq!(fetched(&app, vod.id)[0]["error"], Value::Null);
    let last = last_progress(&app, vod.id);
    assert_eq!((last["progress"].as_f64(), last["usable"].as_f64()), (Some(1.0), Some(50.0)), "{last}");
}

/// A fetch that fails tells the page why, and its last word is that none of
/// it plays (footage.rs `Fetched`'s `error`: "Why it couldn't be fetched,
/// if it couldn't"; `ensure_as`: "all of it plays now, or none of it does").
#[test]
fn a_fetch_that_fails_tells_the_page_why_and_that_none_of_it_plays() {
    let app = TestApp::new();
    app.with_screen();
    let vod = a_site_vod(&app);
    app.yt_dlp.refuses(&vod.info.url);
    open_edit(&app, app.edit_of(&[(vod.id, 10.0, 20.0)]));
    eventually("its fetch ended", || !fetched(&app, vod.id).is_empty());
    let error = fetched(&app, vod.id)[0]["error"].clone();
    assert!(error.as_str().is_some_and(|e| !e.is_empty()), "no reason given: {error}");
    let last = last_progress(&app, vod.id);
    assert_eq!((last["progress"].as_f64(), last["usable"].clone()), (Some(0.0), Value::Null), "{last}");
}

/// A VOD's frame at a moment comes from its footage here, a JPEG; none of
/// that moment here (past a section's end, before its first keyframe) is
/// no frame (footage.rs `vod_frame`: "The frame of VOD `vod_id` at `at`
/// seconds, a JPEG `width` wide ..., from its footage here; empty if
/// there's none of that moment yet"; core footage.rs `KEYFRAME_GAP`).
#[test]
fn a_vods_frame_comes_from_its_footage_here_or_is_none() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    assert_eq!(vod_frame(&app, vod.id, 20.0), b"", "none fetched yet");
    let paths = app.paths();
    section(&paths, vod.id, (0.0, 60.0), Duration::ZERO);
    section(&paths, vod.id, (100.0, 160.0), Duration::ZERO);
    assert!(is_jpeg(&vod_frame(&app, vod.id, 20.0)), "no frame of 0:20 from its section");
    assert!(is_jpeg(&vod_frame(&app, vod.id, 130.0)), "no frame of 2:10 from its section");
    assert_eq!(vod_frame(&app, vod.id, 80.0), b"", "1:20 isn't here");
    assert_eq!(vod_frame(&app, vod.id, 105.0), b"", "before the section's first keyframe");
}

/// A file's frame comes from the file itself (footage.rs `vod_frame`: "from
/// its footage here"; core footage.rs's module doc: "First the VOD's own
/// file, where it plays from one here (a file on this computer ...)").
#[test]
fn a_files_frame_comes_from_the_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    assert!(is_jpeg(&vod_frame(&app, vod.id, 30.0)));
}

/// The page hears how far a fetch is while it downloads, not only once it's
/// over (footage.rs's module doc: "telling the page how far each is
/// (`footage-progress`)"; `Fetching`'s `progress`: "a guess from what's
/// downloaded").
#[test]
fn the_page_hears_how_far_a_fetch_is_as_it_downloads() {
    let app = TestApp::new();
    app.with_screen();
    let vod = a_site_vod(&app);
    let halfway = Gate::new();
    app.yt_dlp.holds_at(0.5, &halfway);
    open_edit(&app, app.edit_of(&[(vod.id, 10.0, 20.0)]));
    halfway.reached();
    let partway = |p: &Value| p["vod_id"] == vod.id && p["progress"].as_f64().is_some_and(|p| p > 0.0 && p < 1.0);
    eventually("how far it is heard", || app.heard("footage-progress").iter().any(partway));
    halfway.open();
    eventually("its footage fetched", || !fetched(&app, vod.id).is_empty());
}

/// Removing a VOD stops its footage downloading and what of it waits, and
/// that's no failure; what was downloaded of it goes (footage.rs
/// `Footage::stop_vod`: "Stops fetching VOD `vod_id`'s footage (it's being
/// removed): what's waiting, and the download under way"; `Footage::run`:
/// "Stopped (the VOD removed) is no failure"; `ensure_as`: "Whatever was
/// downloaded goes, done or not"; CHANGELOG.md, 0.4.0 Fixes: "Removing a
/// VOD, or deleting its downloaded video, didn't stop what was still using
/// it.").
#[test]
fn removing_a_vod_stops_its_footage_downloading_and_waiting() {
    let app = TestApp::new();
    app.with_screen();
    let vod = a_site_vod(&app);
    let other = app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 600.0);
    // Two stretches far apart: one downloading, one waiting its turn.
    let edit = app.edit_of(&[(vod.id, 10.0, 20.0), (vod.id, 300.0, 310.0)]);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    open_edit(&app, edit);
    begun.reached();
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    app.invoke("delete_vod", json!({ "id": vod.id })).unwrap();
    begun.open();
    queue_through(&app, other.id);
    let heard = fetched(&app, vod.id);
    assert_eq!(heard.len(), 1, "what waited was fetched too: {heard:?}");
    assert_eq!(heard[0]["error"], Value::Null, "stopped is no failure");
    let left = work_files_of(&app, vod.id);
    assert!(left.is_empty(), "what was downloaded stayed: {left:?}");
}

/// A clip whose footage another download is fetching waits behind it and
/// is made once it's in (footage.rs `ensure_as`: "Behind another download
/// (the queue's, for an edit playing), stoppable").
#[test]
fn a_clip_behind_the_queues_download_is_made_once_it_is_in() {
    let app = TestApp::new();
    app.with_screen();
    let vod = a_site_vod(&app);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    open_edit(&app, app.edit_of(&[(vod.id, 10.0, 20.0)]));
    begun.reached();
    let clip = app.add_clip(vod.id, "0:12", "0:18").unwrap();
    eventually("the clip being made", || app.clip(clip.id).is_some_and(|c| c.status == ClipStatus::Cutting));
    begun.open();
    let clip = app.made(clip.id);
    assert_eq!((clip.status, clip.error.as_deref()), (ClipStatus::Ready, None));
}

/// A clip waiting behind another download can be canceled meanwhile, and
/// says so without waiting for that download (footage.rs `ensure_as`:
/// "Behind another download ..., stoppable"; CHANGELOG.md, 0.7.1: "A
/// canceled clip or edit says so.").
#[test]
fn a_clip_behind_the_queues_download_can_be_canceled_meanwhile() {
    let app = TestApp::new();
    app.with_screen();
    let vod = a_site_vod(&app);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    open_edit(&app, app.edit_of(&[(vod.id, 10.0, 20.0)]));
    begun.reached();
    let clip = app.add_clip(vod.id, "0:12", "0:18").unwrap();
    eventually("the clip being made", || app.clip(clip.id).is_some_and(|c| c.status == ClipStatus::Cutting));
    app.invoke("cancel_job", json!({ "kind": "clip", "id": clip.id })).unwrap();
    // The queue's download still held.
    let clip = app.made(clip.id);
    begun.open();
    assert_eq!((clip.status, clip.error.as_deref()), (ClipStatus::Error, Some(crate::clips::CANCELED)));
}

/// A clip canceled while its footage downloads leaves nothing of that
/// download behind (footage.rs `ensure_as`: "Whatever was downloaded goes,
/// done or not; unless the app is quitting").
#[test]
fn a_download_stopped_partway_leaves_nothing_behind() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let halfway = Gate::new();
    app.yt_dlp.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "1:00", "1:10").unwrap();
    halfway.reached();
    assert!(!work_files_of(&app, vod.id).is_empty(), "nothing of it downloading");
    app.invoke("cancel_job", json!({ "kind": "clip", "id": clip.id })).unwrap();
    halfway.open();
    assert_eq!(app.made(clip.id).error.as_deref(), Some(crate::clips::CANCELED));
    assert_eq!(work_files_of(&app, vod.id), Vec::<std::ffi::OsString>::new(), "what was downloaded stayed");
}

/// At the start, only the edits with a piece in a section a quit left
/// downloading have their footage asked for again; an edit whose piece
/// only meets that section's end doesn't (footage.rs `left_unfinished`:
/// "the footage missing of each edit with a piece in a section that was
/// downloading"; CHANGELOG.md, 0.7.4: "edits being rendered and the
/// footage they need start again").
#[test]
fn at_the_start_only_edits_in_a_section_left_downloading_fetch_again() {
    let app = TestApp::new();
    app.with_screen();
    let vod = a_site_vod(&app);
    let other = app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 600.0);
    // 1:00-1:30: its section is 0:30-2:00 (`footage::MARGIN`).
    let left = app.edit_of(&[(vod.id, 60.0, 90.0)]);
    // From where that section ends.
    app.edit_of(&[(vod.id, 120.0, 150.0)]);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    open_edit(&app, left);
    begun.reached();
    let app = TestApp::on(app.quit());
    app.with_screen();
    eventually("the left section downloaded again", || !fetched(&app, vod.id).is_empty());
    queue_through(&app, other.id);
    let froms: Vec<f64> = app
        .heard("footage-progress")
        .iter()
        .filter(|p| p["vod_id"] == vod.id)
        .filter_map(|p| p["from"].as_f64())
        .collect();
    assert!(froms.iter().all(|&from| from == 30.0), "another section of it fetched: {froms:?}");
}

/// A section done with (its VOD's video kept from it) while the player
/// still reads it, as it does for a moment after `player::refile`, or the
/// preview's decoder longer: deleted, or else gone at the next tidying
/// (footage.rs `drop_section`: "Then it's dated as unused for long, so the
/// next tidying (within `TIDY_EVERY`) deletes it").
#[test]
fn a_section_still_read_goes_at_the_next_tidying_if_not_now() {
    let dir = std::env::temp_dir().join(format!("vodscribe-drop-section-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.mkv", footage::section_stem(7, 0.0, 60.0)));
    std::fs::write(&path, b"section").unwrap();
    let held = crate::open_as_ffmpeg(&path);
    drop_section(&path);
    let left = path.is_file();
    // The player lets go; then the next tidying, as `tidy` does it.
    drop(held);
    footage::tidy(&dir, UNUSED_FOR, u64::MAX, |_| false);
    let still = path.is_file();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!still, "left after the next tidying (deleted now: {})", !left);
}
