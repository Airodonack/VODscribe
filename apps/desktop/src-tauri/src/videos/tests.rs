//! The kept videos' specs (videos.rs): a VOD's whole video downloaded on
//! request ("Keep video", Play on a site without its own player), one at a
//! time, stopped, listed for the page, deleted all at once from Settings ›
//! Storage, and their folder opened. Downloads run on the kit's fake yt-dlp,
//! held at a `Gate` where a test needs one under way.
//!
//! Stated elsewhere, cited here and not repeated:
//! - A download a quit stopped carries on at the next start, at the height
//!   asked; one that failed, one stopped by removing its VOD or by Delete
//!   all videos, and one a whole transcription downloads aren't
//!   (CHANGELOG.md, 0.7.4: "Unfinished work resumes after a restart."):
//!   tests/start.rs `a_video_download_a_quit_stopped_carries_on_at_the_next_start`,
//!   `a_video_download_that_failed_is_not_started_again_at_the_next_start`,
//!   `a_video_a_whole_transcription_downloads_is_left_to_it_at_the_next_start`,
//!   `a_video_asked_beside_a_failed_whole_transcription_is_carried_on_at_the_next_start`,
//!   `a_video_download_stopped_by_removing_its_vod_is_not_asked_after_a_quit`,
//!   `a_video_download_stopped_by_delete_all_videos_is_not_asked_after_a_quit`,
//!   `a_removed_vods_video_download_is_not_carried_on_at_the_next_start`.
//! - Delete all videos cancels the transcriptions reading them, keeping
//!   their lines, and takes the library before the queue (CHANGELOG.md,
//!   0.7.4: "Delete video works during a transcription."):
//!   vods/tests/removing.rs
//!   `delete_all_videos_cancels_the_transcriptions_reading_them_keeping_their_lines`,
//!   `delete_all_videos_takes_the_library_before_the_queue`.
//! - A VOD playing as its video downloads, or opened as it's kept, plays the
//!   video kept (CHANGELOG.md, 0.7.0 Fixes: "Playback survives the download
//!   finishing."): player/tests.rs
//!   `a_vod_playing_as_it_downloads_plays_the_kept_video_from_where_it_is`,
//!   `a_vod_opened_as_its_video_is_kept_plays_the_video_kept`.
//! - A video kept on request stays when its transcription is canceled:
//!   vods/tests/transcribing.rs `a_cancel_keeps_a_video_downloaded_on_request`.
//! - A refresh of a VOD's details runs beside its video downloading:
//!   vods/tests/adding.rs
//!   `refresh_details_runs_beside_a_download_or_transcription_without_touching_either`.

use super::*;

use std::sync::Arc;
use std::sync::mpsc;

use serde_json::{Value, json};
use vodscribe_core::db::Status;
use vodscribe_core::fetch::VodInfo;
use vodscribe_core::testkit::{Gate, Media, eventually, library::said};

use crate::testkit::{TestApp, near};

const WAIT: Duration = Duration::from_secs(5);

/// A minute of YouTube VOD, "A talk"; the fake site serves it.
fn a_youtube_vod(app: &TestApp) -> Vod {
    app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 60.0)
}

/// A minute of VOD from a site without its own player (`pipeline::keeps_video`).
fn a_rumble_vod(app: &TestApp, id: &str) -> Vod {
    app.add_site_vod("rumble", id, "A talk", 60.0)
}

/// "Keep video" on VOD `id`'s row, at the Storage quality: the command's answer.
fn keep(app: &TestApp, id: i64) -> Result<Value, Value> {
    app.invoke("keep_video", json!({ "id": id }))
}

/// The video-progress events the page has heard of VOD `id`, in order.
fn progress_of(app: &TestApp, id: i64) -> Vec<Value> {
    app.heard("video-progress").into_iter().filter(|p| p["id"] == id).collect()
}

/// Waits till the page hears VOD `id`'s video download is over, and gives
/// what it heard then.
fn over(app: &TestApp, id: i64) -> Value {
    eventually("its download over", || progress_of(app, id).iter().any(|p| p["progress"].is_null()));
    progress_of(app, id).into_iter().rfind(|p| p["progress"].is_null()).unwrap()
}

/// Waits till VOD `id` says its video is kept.
fn kept(app: &TestApp, id: i64) {
    eventually("its video kept", || app.db().vod(id).unwrap().unwrap().media);
}

/// The VODs whose video is downloading or waiting to, as the page asks.
fn downloads(app: &TestApp) -> Vec<i64> {
    serde_json::from_value(app.invoke("video_downloads", json!({})).unwrap()).unwrap()
}

/// The part files VOD `id`'s video download has written in the kept videos' folder.
fn parts(app: &TestApp, id: i64) -> Vec<std::path::PathBuf> {
    vodscribe_core::fetch::part_files(&app.paths().source_video_stem(id)).into_iter().map(|(p, _)| p).collect()
}

/// Whether VOD `id` has a file of its own in the kept videos' folder.
fn has_files(app: &TestApp, id: i64) -> bool {
    let stem = id.to_string();
    let Ok(entries) = std::fs::read_dir(app.paths().media()) else { return false };
    entries.flatten().any(|e| e.file_name().to_string_lossy().split('.').next() == Some(stem.as_str()))
}

// Keep video.

/// "Keep video" downloads a site's VOD's whole video and keeps it in the
/// kept videos' folder, the VOD then saying it has it; the page hears the
/// download start, then end with no error (videos.rs, its module doc:
/// "Downloading a VOD's whole video to this computer, on request … Kept
/// like other sites' videos, in `Paths::media`"; videos.rs `keep_video`:
/// "Queues the download of VOD `id`'s whole video"; videos.rs
/// `VideoProgress`: "`progress` is None once it's over").
#[test]
fn keep_video_downloads_a_site_vods_whole_video_and_keeps_it() {
    let app = TestApp::new();
    let vod = a_youtube_vod(&app);
    assert_eq!(keep(&app, vod.id), Ok(Value::Null));
    assert_eq!(progress_of(&app, vod.id)[0]["progress"], 0.0, "the page wasn't told it started");
    let ended = over(&app, vod.id);
    assert_eq!(ended["error"], Value::Null, "{ended}");
    let file = app.db().media_path(vod.id).unwrap().expect("its video recorded");
    assert!(Path::new(&file).starts_with(app.paths().media()), "kept outside the videos' folder: {file}");
    let media = Media::read(Path::new(&file)).expect("its video");
    assert!(near(media.duration, 60.0), "not all of it: {}", media.duration);
    assert_eq!(media.cut_from, Some((vod.info.url.clone(), 0.0)));
    assert!(app.db().vod(vod.id).unwrap().unwrap().media);
    assert!(parts(&app, vod.id).is_empty(), "a part file left");
}

/// The page hears the VOD has its video before it hears the download is
/// over, so a VOD from a site without its own player isn't taken for
/// unplayable meanwhile (videos.rs `Videos::run`: "The VOD first: told the
/// download's over while the VOD doesn't say it has its video yet, the
/// page of one from a site without a player would take it for unplayable,
/// and take down its player").
#[test]
fn the_page_hears_the_vod_has_its_video_before_it_hears_the_download_is_over() {
    let app = TestApp::new();
    let vod = a_rumble_vod(&app, "r1");
    keep(&app, vod.id).unwrap();
    over(&app, vod.id);
    let told = app.heard("vod");
    assert!(told.iter().any(|v| v["id"] == vod.id && v["media"] == true), "not told it has its video: {told:?}");
}

/// A quality that Settings › Storage doesn't offer is refused, and nothing
/// is downloaded (videos.rs `HEIGHTS`: "The heights a download can be
/// capped at (as Settings → Storage offers)").
#[test]
fn keep_video_refuses_a_quality_storage_does_not_offer() {
    let app = TestApp::new();
    let vod = a_youtube_vod(&app);
    let refused = app.invoke("keep_video", json!({ "id": vod.id, "height": 360 }));
    assert_eq!(refused, Err(json!("That isn't one of the video qualities.")));
    assert_eq!(downloads(&app), Vec::<i64>::new());
    assert_eq!(progress_of(&app, vod.id), Vec::<Value>::new());
    assert_eq!(app.db().wanted_videos().unwrap(), []);
}

/// "Keep video" with no quality asks for the one Settings › Storage
/// chooses, 1080p unless chosen otherwise, and one given is asked as given
/// (videos.rs `keep_video`: "at most `height` tall, or as Settings →
/// Storage says"; core settings.rs `DEFAULT_VIDEO_HEIGHT`: "1080p"; core
/// db.rs `set_video_wanted`: "VOD `id`'s whole video is asked to download
/// at most `height` tall"). Each is held once begun.
#[test]
fn keep_video_with_no_quality_asks_for_the_one_storage_chooses() {
    let app = TestApp::new();
    let first = a_youtube_vod(&app);
    let second = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    let third = app.add_site_vod("kick", "k1", "Day 5", 60.0);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    keep(&app, first.id).unwrap();
    begun.reached();
    let settings = Settings { video_height: Some(480), ..Settings::default() };
    app.invoke("set_settings", json!({ "settings": settings })).unwrap();
    keep(&app, second.id).unwrap();
    app.invoke("keep_video", json!({ "id": third.id, "height": 720 })).unwrap();
    let mut asked = app.db().wanted_videos().unwrap();
    asked.sort();
    assert_eq!(asked, [(first.id, 1080), (second.id, 480), (third.id, 720)]);
}

/// "Keep video" on a VOD whose video is kept already downloads nothing
/// again: its video stays as it is (videos.rs `begin`: "None if it's gone,
/// or has its video already (so a video recorded is never one a download
/// is still writing)").
#[test]
fn keep_video_on_a_vod_whose_video_is_kept_downloads_nothing_again() {
    let app = TestApp::new();
    let vod = a_youtube_vod(&app);
    keep(&app, vod.id).unwrap();
    over(&app, vod.id);
    let file = app.db().media_path(vod.id).unwrap().unwrap();
    let had = std::fs::read(&file).unwrap();
    let heard = progress_of(&app, vod.id).len();
    assert_eq!(keep(&app, vod.id), Ok(Value::Null));
    assert_eq!(downloads(&app), Vec::<i64>::new(), "downloading it again");
    assert_eq!(progress_of(&app, vod.id).len(), heard, "the page told of another download");
    assert_eq!(app.db().media_path(vod.id).unwrap(), Some(file.clone()));
    assert_eq!(std::fs::read(&file).unwrap(), had);
}

/// On a site without its own player, "Keep video" (and Play, which asks for
/// it) isn't refused while transcribing all of the VOD downloads its video:
/// it's told to play once that's in, and then has the video kept; nor while
/// only a stretch's sound downloads, which streams as ever (videos.rs
/// `keep_video`: "Unless transcribing all of it has the file: then it
/// waits for that (`try_turn`), and plays once it's in"; CHANGELOG.md,
/// 0.7.4 Fixes: "Play and transcription share one download."). Was
/// `keep_video_is_refused_only_while_transcribing_all_of_it_downloads_the_video`,
/// before 0.7.4's sharing (vods/tests/sharing_the_video.rs has the rest).
#[test]
fn keep_video_while_transcribing_all_of_it_downloads_the_video_waits_for_that() {
    let app = TestApp::new();
    let part = a_rumble_vod(&app, "r2");
    app.db()
        .atomically(|db| {
            // At its stage in one go, so the transcription's queue never sees it waiting.
            let stretch = db.request(part.id, 100.0, Some(130.0))?.id;
            db.set_stretch_status(stretch, Status::Downloading, 0.1, None)
        })
        .unwrap();
    assert_eq!(keep(&app, part.id), Ok(Value::Null), "refused while a stretch's sound alone downloads");
    assert_eq!(progress_of(&app, part.id)[0]["streamed"], true);
    kept(&app, part.id);
    let whole = a_rumble_vod(&app, "r1");
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.invoke("transcribe_stretch", json!({ "vodId": whole.id, "start": 0.0, "end": null })).unwrap();
    begun.reached();
    assert_eq!(keep(&app, whole.id), Ok(Value::Null));
    let told = progress_of(&app, whole.id);
    assert_eq!((told[0]["progress"].as_f64(), told[0]["streamed"].as_bool()), (Some(0.0), Some(false)), "{told:?}");
    begun.open();
    kept(&app, whole.id);
    assert_eq!(over(&app, whole.id)["error"], Value::Null);
}

/// A VOD from a site without its own player, which says how long it is,
/// comes as a section that plays as it's in, and the page is told so from
/// the start; a VOD from YouTube, Twitch or Kick, or one that doesn't say
/// how long it is, plays only once it's all in (videos.rs `keep_video`: "a
/// site without its own player streams it, if it says how long it is";
/// videos.rs `VideoProgress`: "`streamed`: it comes in as a section of all
/// of it, which plays as far as it's in … else it plays only once it's all
/// in, so the page gives the percent"; CHANGELOG.md, 0.7.2: "Download notes
/// say less." ("the percent shows only for a download that plays once it's
/// all in")).
#[test]
fn a_vod_from_a_site_without_its_own_player_comes_as_a_section_that_plays_as_it_is_in() {
    let app = TestApp::new();
    let streamed = a_rumble_vod(&app, "r1");
    let youtube = a_youtube_vod(&app);
    let unsaid = app.add_site_vod("rumble", "r2", "No length", 0.0);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    for vod in [&streamed, &youtube, &unsaid] {
        keep(&app, vod.id).unwrap();
    }
    assert_eq!(progress_of(&app, streamed.id)[0]["streamed"], true);
    assert_eq!(progress_of(&app, youtube.id)[0]["streamed"], false);
    assert_eq!(progress_of(&app, unsaid.id)[0]["streamed"], false);
}

/// A VOD's video that came as a section is kept as an MP4 of all of it,
/// and the section it came as goes (videos.rs `stream_then_keep`: "copies
/// it, as it is, into an MP4 kept as its video (`clips::remux`)"; "It's the
/// kept video now").
#[test]
fn a_video_that_came_as_a_section_is_kept_as_an_mp4_and_the_section_goes() {
    let app = TestApp::new();
    let vod = a_rumble_vod(&app, "r1");
    keep(&app, vod.id).unwrap();
    assert_eq!(over(&app, vod.id)["error"], Value::Null);
    let file = app.db().media_path(vod.id).unwrap().expect("its video recorded");
    assert_eq!(Path::new(&file), app.paths().source_video(vod.id, "mp4"));
    let media = Media::read(Path::new(&file)).expect("its video");
    assert_eq!(media.container, "mp4");
    assert!(near(media.duration, 60.0), "not all of it: {}", media.duration);
    let section = vodscribe_core::footage::section_stem(vod.id, 0.0, 60.0);
    let footage: Vec<_> = std::fs::read_dir(app.paths().source_footage())
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    assert!(!footage.iter().any(|name| name.starts_with(&section)), "its section stays: {footage:?}");
}

/// A video that arrives in a format this computer doesn't play isn't kept,
/// and the page is told why (videos.rs `download`: "Sites only claim their
/// codecs: kept only if what arrived plays here"; CHANGELOG.md, 0.3.0:
/// "VODs from other sites" ("in a format your computer plays")). This
/// computer plays VP9 and Opus alone (setup.rs `set_playable`); the site
/// sends H.264 and AAC.
#[test]
fn a_video_in_a_format_this_computer_does_not_play_is_not_kept_and_says_why() {
    let app = TestApp::new();
    let vod = a_youtube_vod(&app);
    let playable = json!({ "video": [["vp9", "opus"]], "audio": ["opus"] });
    app.invoke("set_playable", json!({ "playable": playable })).unwrap();
    keep(&app, vod.id).unwrap();
    let ended = over(&app, vod.id);
    let error = ended["error"].as_str().expect("no error told");
    assert!(error.contains("doesn't play here") && error.contains("h264"), "{error}");
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media);
    assert!(!has_files(&app, vod.id), "its download left behind");
}

/// A download the site refuses keeps nothing, and the page is told why
/// (videos.rs `VideoProgress`: "`error` is set if it failed").
#[test]
fn a_video_download_the_site_refuses_keeps_nothing_and_says_why() {
    let app = TestApp::new();
    let vod = a_youtube_vod(&app);
    app.yt_dlp.refuses(&vod.info.url);
    keep(&app, vod.id).unwrap();
    let ended = over(&app, vod.id);
    let error = ended["error"].as_str().expect("no error told");
    assert!(error.contains("The site refused the download."), "{error}");
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media);
    assert_eq!(app.db().media_path(vod.id).unwrap(), None);
    assert_eq!(downloads(&app), Vec::<i64>::new());
}

/// A video download that failed while another waited behind it is no
/// longer asked for: the next start doesn't try it again unasked (core
/// db.rs `MIGRATE_V45`: "set when it's asked for, cleared when it's done,
/// stopped or failed"). The site refuses the first, held as it's looked up
/// so the next is asked for meanwhile. tests/start.rs
/// `a_video_download_that_failed_is_not_started_again_at_the_next_start`
/// states it for one alone.
#[test]
fn a_video_download_that_failed_while_another_waited_is_no_longer_asked_for() {
    let app = TestApp::new();
    let first = a_youtube_vod(&app);
    let next = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    app.yt_dlp.refuses(&first.info.url);
    let looking = Gate::new();
    app.yt_dlp.holds_details(&looking);
    keep(&app, first.id).unwrap();
    looking.reached();
    keep(&app, next.id).unwrap();
    looking.open();
    assert!(over(&app, first.id)["error"].is_string(), "the first didn't fail");
    kept(&app, next.id);
    assert_eq!(app.db().wanted_videos().unwrap(), [], "asked for though it failed");
    let app = TestApp::on(app.quit());
    assert_eq!(downloads(&app), Vec::<i64>::new(), "tried again, unasked");
}

// The queue.

/// Videos download one at a time, in the order asked: the next waits,
/// writing nothing, till the one under way is in (videos.rs, its module
/// doc: "One download at a time, in a queue of its own"). The first is
/// held once begun.
#[test]
fn videos_download_one_at_a_time_in_the_order_asked() {
    let app = TestApp::new();
    let first = a_youtube_vod(&app);
    let next = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    keep(&app, first.id).unwrap();
    begun.reached();
    keep(&app, next.id).unwrap();
    assert!(!parts(&app, first.id).is_empty(), "the first isn't under way");
    assert!(parts(&app, next.id).is_empty(), "the next began beside the first");
    begun.open();
    kept(&app, first.id);
    kept(&app, next.id);
}

/// The page is told which VODs' videos are downloading or waiting to, the
/// one under way first, then those waiting in their order; none once
/// they're in (videos.rs `video_downloads`: "The VODs whose video is
/// downloading or waiting to (the interface asks at start)"; videos.rs
/// `Videos::listed`: "the running one first").
#[test]
fn video_downloads_lists_the_one_under_way_then_those_waiting() {
    let app = TestApp::new();
    let first = a_youtube_vod(&app);
    let second = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    let third = app.add_site_vod("kick", "k1", "Day 5", 60.0);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    keep(&app, first.id).unwrap();
    begun.reached();
    keep(&app, third.id).unwrap();
    keep(&app, second.id).unwrap();
    assert_eq!(downloads(&app), [first.id, third.id, second.id]);
    begun.open();
    for vod in [&first, &second, &third] {
        kept(&app, vod.id);
    }
    eventually("none listed", || downloads(&app).is_empty());
}

/// A video downloading doesn't hold up a transcription: its queue is its
/// own (videos.rs, its module doc: "in a queue of its own, beside
/// transcription's"). The download is held once begun while a file on this
/// computer is transcribed.
#[test]
fn a_video_downloading_does_not_hold_up_a_transcription() {
    let app = TestApp::new();
    app.worker().hears(said(10.0, "The Eagle has landed"));
    let vod = a_youtube_vod(&app);
    let file = app.add_video_file("Day 3.mp4", 60.0);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    keep(&app, vod.id).unwrap();
    begun.reached();
    app.invoke("transcribe_stretch", json!({ "vodId": file.id, "start": 0.0, "end": null })).unwrap();
    eventually("the file transcribed", || !app.db().segments(file.id).unwrap().is_empty());
    assert_eq!(downloads(&app), [vod.id], "the download isn't still under way");
}

// Stop.

/// Stopping a video download under way discards what it had, the VOD
/// keeps no video and the page hears it's over, with no error (videos.rs
/// `stop_video`: "Stops VOD `id`'s video download, discarding what's
/// downloaded so far"; CHANGELOG.md, 0.3.0 Fixes: "Stopping or removing a
/// download could leave partial files behind."). Held halfway in.
#[test]
fn stopping_a_video_download_discards_what_it_had() {
    let app = TestApp::new();
    let vod = a_youtube_vod(&app);
    let halfway = Gate::new();
    app.yt_dlp.holds_at(0.5, &halfway);
    keep(&app, vod.id).unwrap();
    halfway.reached();
    assert!(!parts(&app, vod.id).is_empty(), "half of it written");
    assert_eq!(app.invoke("stop_video", json!({ "id": vod.id })), Ok(Value::Null));
    assert_eq!(over(&app, vod.id)["error"], Value::Null);
    eventually("what it had discarded", || !has_files(&app, vod.id));
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media);
    assert_eq!(downloads(&app), Vec::<i64>::new());
    assert_eq!(app.db().wanted_videos().unwrap(), [], "still asked for");
}

/// Stopping a video download that waits its turn takes it out of the
/// queue: the page hears it's over, and it never downloads, while the one
/// under way goes on (videos.rs `stop_video`: "or takes it out of the
/// queue"). The one under way is held once begun.
#[test]
fn stopping_a_video_download_waiting_its_turn_takes_it_out_of_the_queue() {
    let app = TestApp::new();
    let first = a_youtube_vod(&app);
    let next = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    keep(&app, first.id).unwrap();
    begun.reached();
    keep(&app, next.id).unwrap();
    app.invoke("stop_video", json!({ "id": next.id })).unwrap();
    assert_eq!(over(&app, next.id)["error"], Value::Null);
    assert_eq!(downloads(&app), [first.id]);
    begun.open();
    kept(&app, first.id);
    eventually("none listed", || downloads(&app).is_empty());
    assert!(!app.db().vod(next.id).unwrap().unwrap().media, "downloaded though stopped");
    assert!(!has_files(&app, next.id), "downloaded though stopped");
}

/// A video download stopped while it waits its turn takes its VOD's files
/// that no kept video records with it (a video Delete all videos left of a
/// VOD queued again meanwhile, or a download's parts a quit left), rather
/// than leave them untracked (videos.rs `stop`: "Waiting, it has nothing of
/// its own yet, but the VOD's files may be a video forgotten while it was
/// queued … or a download's parts a quit left: without a video kept,
/// they'd stay untracked"). Moved up from videos.rs's
/// `a_download_stopped_while_waiting_takes_its_vods_untracked_files`. The
/// one under way is held once begun.
#[test]
fn a_download_stopped_while_waiting_takes_its_vods_untracked_files() {
    let app = TestApp::new();
    let first = a_youtube_vod(&app);
    let next = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    let left = app.paths().source_video(next.id, "mp4");
    std::fs::create_dir_all(app.paths().media()).unwrap();
    std::fs::write(&left, b"video").unwrap();
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    keep(&app, first.id).unwrap();
    begun.reached();
    keep(&app, next.id).unwrap();
    app.invoke("stop_video", json!({ "id": next.id })).unwrap();
    assert!(!left.exists(), "left untracked");
}

// Delete all videos.

/// Settings › Delete all videos deletes every kept video, keeping the
/// transcripts, and the page hears each VOD no longer has its video
/// (videos.rs `delete_all_videos`: "Deletes every downloaded video, keeping
/// the transcripts: they then play on their sites"; "returns how many"
/// are kept).
#[test]
fn delete_all_videos_deletes_every_kept_video_keeping_the_transcripts() {
    let app = TestApp::new();
    let vods = [a_youtube_vod(&app), a_rumble_vod(&app, "r1")];
    let mut files = Vec::new();
    for vod in &vods {
        keep(&app, vod.id).unwrap();
        kept(&app, vod.id);
        files.push(app.db().media_path(vod.id).unwrap().unwrap());
        app.says(vod.id, 12.0, "The Eagle has landed");
    }
    assert_eq!(app.invoke("delete_all_videos", json!({})), Ok(json!(0)), "a video kept");
    for (vod, file) in vods.iter().zip(&files) {
        assert!(!Path::new(file).exists(), "its video is still there");
        assert!(!app.db().vod(vod.id).unwrap().unwrap().media, "it says its video is kept");
        let last = app.heard("vod").into_iter().rfind(|v| v["id"] == vod.id).expect("the page wasn't told");
        assert_eq!(last["media"], false, "{last}");
        let lines = app.invoke("transcript", json!({ "vodId": vod.id })).unwrap();
        assert_eq!(lines[0]["text"], "The Eagle has landed");
    }
}

/// Delete all videos stops the download under way and empties the queue,
/// deleting what each had; the page hears each is over, and neither is kept
/// (videos.rs `delete_all_videos`: "Downloads under way stop"; videos.rs
/// `Videos::stop_all`: "Stops every download and empties the queue
/// (deleting what each had)"). The one under way is held halfway in.
#[test]
fn delete_all_videos_stops_the_downloads_under_way_and_those_waiting() {
    let app = TestApp::new();
    let first = a_youtube_vod(&app);
    let next = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    let halfway = Gate::new();
    app.yt_dlp.holds_at(0.5, &halfway);
    keep(&app, first.id).unwrap();
    halfway.reached();
    keep(&app, next.id).unwrap();
    assert_eq!(app.invoke("delete_all_videos", json!({})), Ok(json!(0)));
    for vod in [&first, &next] {
        assert_eq!(over(&app, vod.id)["error"], Value::Null);
    }
    eventually("none listed", || downloads(&app).is_empty());
    eventually("what it had deleted", || !has_files(&app, first.id));
    for vod in [&first, &next] {
        assert!(!app.db().vod(vod.id).unwrap().unwrap().media, "VOD {} kept", vod.id);
        assert!(!has_files(&app, vod.id), "VOD {} has files", vod.id);
    }
}

/// Delete all videos deletes the parts a download left that no one asks
/// for any more, with no kept video to record them (videos.rs
/// `delete_all_videos`: "Those of VODs without a kept video go too (a
/// download paused by quitting leaves its parts)").
#[test]
fn delete_all_videos_deletes_the_parts_a_download_left() {
    let app = TestApp::new();
    let vod = a_youtube_vod(&app);
    let part = std::path::PathBuf::from(format!("{}.part", app.paths().source_video(vod.id, "mp4").display()));
    std::fs::create_dir_all(app.paths().media()).unwrap();
    std::fs::write(&part, b"half").unwrap();
    assert_eq!(app.invoke("delete_all_videos", json!({})), Ok(json!(0)));
    assert!(!part.exists(), "its parts stay");
}

/// Delete all videos leaves a transcription that doesn't read a kept
/// video to go on (videos.rs `delete_all_videos`: "Downloads under way
/// stop, and so does transcribing from a video"). A site's VOD with no
/// video kept, ten minutes long, is transcribed from its sound, held after
/// its first minute.
#[test]
fn delete_all_videos_leaves_a_transcription_not_reading_a_video_to_go_on() {
    let app = TestApp::new();
    app.worker().hears([said(10.0, "The Eagle has landed"), said(200.0, "Later on")].concat());
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 600.0);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.invoke("transcribe_stretch", json!({ "vodId": vod.id, "start": 0.0, "end": null })).unwrap();
    gate.reached();
    assert_eq!(app.invoke("delete_all_videos", json!({})), Ok(json!(0)));
    assert!(!app.db().requests(vod.id).unwrap().is_empty(), "its transcription was canceled");
    gate.open();
    eventually("its transcription over", || app.db().requests(vod.id).unwrap().is_empty());
    let lines: Vec<String> = app.db().segments(vod.id).unwrap().into_iter().map(|line| line.text).collect();
    assert_eq!(lines, ["The Eagle has landed", "Later on"], "not transcribed on past its first minute");
}

// The kept videos' folder.

/// Settings › Storage opens the kept videos' folder in the file manager,
/// made first if no video has been kept yet (videos.rs `open_videos_dir`:
/// "Opens the folder of downloaded videos in the system file manager").
#[test]
fn open_videos_dir_opens_the_kept_videos_folder_made_if_missing() {
    let app = TestApp::new();
    let _ = std::fs::remove_dir(app.paths().media());
    assert_eq!(app.invoke("open_videos_dir", json!({})), Ok(Value::Null));
    assert_eq!(app.desktop().opened(), [app.paths().media()]);
    assert!(app.paths().media().is_dir());
}

// The races at the queue's own points: below the commands, since no gate
// holds a download between recording its file and its job's end, nor a
// delete between forgetting a video and deleting its files.

/// A library with one finished YouTube VOD, in a folder of its own
/// (with the media folder made), deleted when the test ends.
struct Setup {
    db: Arc<Mutex<Db>>,
    videos: Arc<Videos>,
    paths: Paths,
    id: i64,
    /// The test's own, not the queue thread's copy.
    owner: bool,
}

impl Drop for Setup {
    fn drop(&mut self) {
        if self.owner {
            let _ = std::fs::remove_dir_all(&self.paths.root);
        }
    }
}

impl Setup {
    fn new() -> Self {
        use std::sync::atomic::AtomicU64;
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("vodscribe-videos-{}-{n}", std::process::id()));
        let paths = Paths::new(&root);
        std::fs::create_dir_all(paths.media()).unwrap();
        let db = Db::open(&root.join("library.sqlite")).unwrap();
        let info = VodInfo {
            platform: "youtube".into(),
            platform_id: "x".into(),
            url: "https://youtu.be/x".into(),
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
        let whole = db.request(id, 0.0, None).unwrap();
        db.finish_stretch(whole.id).unwrap();
        Setup { db: Arc::new(Mutex::new(db)), videos: Arc::new(Videos::new()), paths, id, owner: true }
    }

    /// Where the VOD's video goes, with `ext`.
    fn file(&self, ext: &str) -> std::path::PathBuf {
        self.db.lock().unwrap().files().source_video(self.id, ext)
    }

    /// A video kept as a download leaves it: the file, recorded.
    fn kept(&self) -> std::path::PathBuf {
        let file = self.file("mp4");
        std::fs::write(&file, b"video").unwrap();
        self.db.lock().unwrap().set_media_path(self.id, Some(&file.to_string_lossy())).unwrap();
        file
    }

    fn media_path(&self) -> Option<String> {
        self.db.lock().unwrap().media_path(self.id).unwrap()
    }

    /// The queue's thread, running `download` as `download` in videos.rs
    /// does: begun, then whatever it does in between, its stop passed on.
    fn run(&self, download: impl Fn(&Setup, &AtomicBool) + Send + 'static) {
        let me = Setup {
            db: self.db.clone(),
            videos: self.videos.clone(),
            paths: self.paths.clone(),
            id: self.id,
            owner: false,
        };
        self.videos
            .0
            .run("videos-test", move |_, stop| {
                if begin(&me.db, &me.videos, me.id).unwrap().is_some() {
                    download(&me, stop);
                }
            })
            .unwrap();
    }

    /// What "Keep video" does to the queue.
    fn keep_video(&self) {
        let id = self.id;
        self.videos.0.push_new((id, 720), |(v, _)| *v == id);
    }
}

/// A download that has recorded its file, but whose job hasn't ended yet,
/// when "Delete video" comes: the file goes with the record (videos.rs
/// `Videos::files`: "a delete never takes a download's files, nor leaves a
/// recorded video behind untracked").
#[test]
fn a_video_deleted_as_its_download_ends_is_deleted() {
    let s = Setup::new();
    let (recorded, has_recorded) = mpsc::channel();
    let (end, ends) = mpsc::channel::<()>();
    let ends = Mutex::new(ends);
    s.run(move |s, stop| {
        let file = s.file("mp4");
        std::fs::write(&file, b"video").unwrap();
        record(&s.db, &s.videos, &s.paths, s.id, &file, stop).unwrap();
        recorded.send(()).unwrap();
        // Still the queue's running job (the pause).
        let _ = ends.lock().unwrap().recv_timeout(WAIT);
    });
    s.keep_video();
    has_recorded.recv_timeout(WAIT).unwrap();
    crate::vods::delete_kept_video(&s.db, &s.videos, &s.paths, s.id, pipeline::remove_media).unwrap();
    end.send(()).unwrap();
    assert_eq!(s.media_path(), None);
    assert!(!s.file("mp4").exists(), "deleted, not left untracked");
}

/// "Keep video" again while "Delete video" deletes the old one: the new
/// download's files aren't deleted with it (videos.rs `Videos::files`: "a
/// delete never takes a download's files").
#[test]
fn a_download_started_during_a_delete_keeps_its_files() {
    let s = Setup::new();
    let old = s.kept();
    let (wrote, has_written) = mpsc::channel();
    s.run(move |s, _| {
        std::fs::write(s.file("mp4.part"), b"new").unwrap();
        wrote.send(()).unwrap();
    });
    crate::vods::delete_kept_video(&s.db, &s.videos, &s.paths, s.id, |paths, id| {
        // Mid-delete (the pause): Keep video, and its download given the
        // time to start.
        s.keep_video();
        let _ = has_written.recv_timeout(Duration::from_millis(500));
        pipeline::remove_media(paths, id);
    })
    .unwrap();
    // The download writes now, if it couldn't during the delete.
    let deadline = Instant::now() + WAIT;
    while !s.file("mp4.part").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!old.exists(), "the old video goes");
    assert!(s.file("mp4.part").exists(), "the new download's part stays");
}

/// "Delete video" with a "Keep video" queued (it lands between the
/// forget and the check: keep_video checks the record, then queues),
/// then that download stopped before it began: no file is left behind
/// with no record of it (videos.rs `stop`: "the VOD's files may be a video
/// forgotten while it was queued").
#[test]
fn a_redownload_stopped_while_waiting_leaves_no_file() {
    let s = Setup::new();
    let old = s.kept();
    // No queue thread: the download waits.
    s.keep_video();
    crate::vods::delete_kept_video(&s.db, &s.videos, &s.paths, s.id, pipeline::remove_media).unwrap();
    assert!(!stop(&s.db, &s.videos, &s.paths, s.id), "it was waiting");
    assert_eq!(s.media_path(), None);
    assert!(!old.exists(), "not left untracked");
}
