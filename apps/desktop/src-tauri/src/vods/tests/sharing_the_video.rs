//! A VOD from a site without a player of its own (core pipeline.rs
//! `keeps_video`): Play and Keep video download its video into the kept
//! video's file, and transcribing all of it downloads that same file
//! (`pipeline::audio_for`). Decided for 0.7.4: whichever download starts
//! second waits for the first and uses its file; Play works, and the video
//! comes down once (videos.rs `try_turn`).

use serde_json::json;
use vodscribe_core::testkit::{Gate, Media, eventually};

use super::*;
use crate::testkit::TestApp;

/// A VOD from a site without its own player, a minute long.
fn site_vod(app: &TestApp, id: &str) -> i64 {
    app.add_site_vod("rumble", id, "Day 3", 60.0).id
}

/// Request `id`'s status.
fn status_of(app: &TestApp, id: i64) -> Status {
    app.db().stretch(id).unwrap().unwrap().status
}

/// Play pressed (or Keep video), as the page asks (videos.svelte.ts `keep`).
fn play(app: &TestApp, vod: i64) {
    app.invoke("keep_video", json!({ "id": vod })).unwrap();
}

/// What the page last heard of VOD `vod`'s video download.
fn last_heard(app: &TestApp, vod: i64) -> Value {
    app.heard("video-progress").into_iter().rev().find(|e| e["id"] == vod).expect("heard of its video")
}

/// Request `request` has its sound from VOD `vod`'s kept video, a whole one.
fn transcribed_from_the_kept_video(app: &TestApp, vod: i64, request: i64) {
    eventually("its sound taken", || status_of(app, request) == Status::Ready);
    let kept = app.db().media_path(vod).unwrap().expect("its video kept");
    assert_eq!(app.db().audio_path(vod).unwrap(), Some(kept.clone()), "its sound from the video kept");
    assert!(Media::read(std::path::Path::new(&kept)).is_some(), "the kept video is whole");
}

/// Play while transcribing all of it waits its turn: Play downloads it, and
/// the transcription, its turn come, waits for that download, then takes its
/// sound from the video kept.
#[test]
fn play_while_a_whole_transcription_waits_its_turn_downloads_the_video_once() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    let request = {
        let state = app.state();
        let (_held, _) = state.worker.hold_starts();
        let request = app.asked(vod, 0.0, None);
        assert_eq!(status_of(&app, request), Status::Queued);
        play(&app, vod);
        // Play's download begun before the transcription's turn comes.
        begun.reached();
        request
    };
    waits.reached();
    assert_eq!(status_of(&app, request), Status::Queued, "it downloads beside Play's download");
    waits.open();
    begun.open();
    transcribed_from_the_kept_video(&app, vod, request);
    assert!(last_heard(&app, vod)["error"].is_null(), "Play's download failed");
}

/// Play while transcribing all of it waits its turn, Play's download done
/// before that turn: the transcription takes its sound from the video kept.
#[test]
fn a_whole_transcription_after_plays_download_uses_its_video() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let state = app.state();
    let (held, _) = state.worker.hold_starts();
    let request = app.asked(vod, 0.0, None);
    play(&app, vod);
    eventually("its video kept", || app.vod(vod).media);
    drop(held);
    transcribed_from_the_kept_video(&app, vod, request);
}

/// Transcribing all of it asked while Play's download runs: it waits for
/// that download, then takes its sound from the video kept.
#[test]
fn a_whole_transcription_asked_while_play_downloads_waits_and_uses_its_video() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    play(&app, vod);
    begun.reached();
    let request = app.asked(vod, 0.0, None);
    waits.reached();
    assert_eq!(status_of(&app, request), Status::Queued, "it downloads beside Play's download");
    waits.open();
    begun.open();
    transcribed_from_the_kept_video(&app, vod, request);
}

/// Play's download stopped while the transcription waits for it: what it
/// had goes, and the transcription downloads the video itself, whole, as it
/// would have with no Play.
#[test]
fn a_whole_transcription_waiting_for_a_stopped_play_downloads_the_video_itself() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    play(&app, vod);
    begun.reached();
    let request = app.asked(vod, 0.0, None);
    waits.reached();
    waits.open();
    app.invoke("stop_video", json!({ "id": vod })).unwrap();
    app.yt_dlp.holds(&Gate::opened());
    begun.open();
    transcribed_from_the_kept_video(&app, vod, request);
    assert!(last_heard(&app, vod)["error"].is_null(), "a stop isn't a failure");
}

/// Play's download failing while the transcription waits for it: the
/// transcription downloads the video itself, and fails as the site refuses it.
#[test]
fn a_whole_transcription_waiting_for_a_failed_play_downloads_itself_and_fails_as_it_did() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    app.yt_dlp.refuses(&app.vod(vod).info.url);
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    // Play's download held as it asks the site for the video's details.
    let looked = Gate::new();
    app.yt_dlp.holds_details(&looked);
    play(&app, vod);
    looked.reached();
    let request = app.asked(vod, 0.0, None);
    waits.reached();
    assert_eq!(status_of(&app, request), Status::Queued, "it downloads beside Play's download");
    waits.open();
    looked.open();
    eventually("the transcription failed", || status_of(&app, request) == Status::Error);
    let refused = "The site refused the download.";
    let played = last_heard(&app, vod)["error"].as_str().unwrap_or_default().to_string();
    assert!(played.contains(refused), "Play's download: {played}");
    let error = app.db().stretch(request).unwrap().unwrap().error.unwrap_or_default();
    assert!(error.contains(refused), "the transcription: {error}");
}

/// Removing the VOD while the transcription waits for Play's download: both
/// end, and nothing of the video is left.
#[test]
fn removing_a_vod_whose_transcription_waits_for_play_ends_both() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    play(&app, vod);
    begun.reached();
    app.asked(vod, 0.0, None);
    waits.reached();
    waits.open();
    app.invoke("delete_vod", json!({ "id": vod })).unwrap();
    eventually("nothing of it runs", || {
        let state = app.state();
        let (_held, running) = state.worker.hold_starts();
        let videos = app.invoke("video_downloads", json!({})).unwrap();
        running.is_empty() && videos.as_array().unwrap().is_empty()
    });
    assert!(app.requests(vod).is_empty(), "its transcription goes on");
    let media = vodscribe_core::pipeline::file_owners(&app.paths().media());
    assert!(!media.contains(&vod), "its video's files stay");
    begun.open();
}

/// Play while transcribing all of it downloads the video: Play waits for
/// that download, said to be downloading (to play once it's in), then plays
/// the video kept. The video comes down once.
#[test]
fn play_while_a_whole_transcription_downloads_waits_and_plays_its_video() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    let request = app.asked(vod, 0.0, None);
    begun.reached();
    assert_eq!(status_of(&app, request), Status::Downloading);
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    play(&app, vod);
    waits.reached();
    let told = last_heard(&app, vod);
    let shown = (told["progress"].as_f64(), told["error"].is_null(), told["streamed"].as_bool());
    assert_eq!(shown, (Some(0.0), true, Some(false)), "{told}");
    begun.open();
    transcribed_from_the_kept_video(&app, vod, request);
    let kept = app.db().media_path(vod).unwrap().unwrap();
    let written = std::fs::metadata(&kept).unwrap().modified().unwrap();
    waits.open();
    eventually("Play's download over", || last_heard(&app, vod)["progress"].is_null());
    assert!(last_heard(&app, vod)["error"].is_null(), "{}", last_heard(&app, vod));
    assert_eq!(app.db().media_path(vod).unwrap(), Some(kept.clone()));
    assert_eq!(std::fs::metadata(&kept).unwrap().modified().unwrap(), written, "it came down again");
}

/// Transcribing all of it stopped while Play waits for its download: Play
/// downloads the video itself, whole.
#[test]
fn play_waiting_for_a_stopped_whole_transcription_downloads_the_video_itself() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.asked(vod, 0.0, None);
    begun.reached();
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    play(&app, vod);
    waits.reached();
    waits.open();
    app.on("cancel_vod", vod);
    app.yt_dlp.holds(&Gate::opened());
    begun.open();
    eventually("Play's download over", || last_heard(&app, vod)["progress"].is_null());
    assert!(last_heard(&app, vod)["error"].is_null(), "{}", last_heard(&app, vod));
    let kept = app.db().media_path(vod).unwrap().expect("its video kept");
    assert!(Media::read(std::path::Path::new(&kept)).is_some(), "the kept video is whole");
    assert!(app.requests(vod).is_empty(), "the transcription goes on");
}

/// Transcribing all of it stopped while it waits for Play's download, that
/// download just written and being recorded: Play's video stays, kept
/// (worker.rs `cancel`: "a video kept whole, or downloaded on request
/// (videos.rs), stays").
#[test]
fn stopping_a_whole_transcription_that_waits_for_play_keeps_plays_video() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    let remux = Gate::new();
    app.ffmpeg.holds_at(0.5, &remux);
    play(&app, vod);
    remux.reached();
    app.asked(vod, 0.0, None);
    waits.reached();
    waits.open();
    let file = app.paths().source_video(vod, "mp4");
    let state = app.state();
    // Play's download held as it records its file.
    let files = state.videos.files();
    remux.open();
    eventually("Play's video written", || file.is_file());
    app.on("cancel_vod", vod);
    eventually("the transcription ended", || {
        let (_held, running) = state.worker.hold_starts();
        running.is_empty() && app.requests(vod).is_empty()
    });
    drop(files);
    eventually("Play's download over", || last_heard(&app, vod)["progress"].is_null());
    assert!(last_heard(&app, vod)["error"].is_null(), "{}", last_heard(&app, vod));
    assert!(file.is_file(), "Play's video was deleted");
    assert_eq!(app.db().media_path(vod).unwrap(), Some(file.to_string_lossy().into_owned()));
}

/// Runs `check`, then `then`, then fails as `check` did, if it did: a gate
/// left closed by a failed check is opened first, so the app can end.
fn checked_then(check: impl FnOnce(), then: impl FnOnce()) {
    let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(check));
    then();
    if let Err(failed) = checked {
        std::panic::resume_unwind(failed);
    }
}

/// Play of the VOD, queued behind another VOD's video, stopped while
/// transcribing all of it downloads the video: the transcription's
/// download stays (videos.rs `stop`).
#[test]
fn stopping_a_queued_play_keeps_the_transcriptions_download() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let other = app.add_site_vod("youtube", "y", "Other", 60.0).id;
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    let request = app.asked(vod, 0.0, None);
    begun.reached();
    eventually("the transcription downloading", || status_of(&app, request) == Status::Downloading);
    let media = app.paths().media();
    eventually("its part written", || vodscribe_core::pipeline::file_owners(&media).contains(&vod));
    play(&app, other);
    play(&app, vod);
    let listed = app.invoke("video_downloads", json!({})).unwrap();
    assert_eq!(listed, json!([other, vod]), "Play of the VOD queued behind the other's");
    app.invoke("stop_video", json!({ "id": vod })).unwrap();
    let left = vodscribe_core::pipeline::file_owners(&media).contains(&vod);
    begun.open();
    assert!(left, "the transcription's download was deleted under it");
    eventually("the transcription's download over", || status_of(&app, request) == Status::Ready);
}

/// Quitting while Play waits for transcribing all of it to download the
/// video: the app ends.
#[test]
fn quitting_while_play_waits_for_a_transcription_ends() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.asked(vod, 0.0, None);
    begun.reached();
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    play(&app, vod);
    waits.reached();
    waits.open();
    let _folder = app.quit();
}

/// Quitting while transcribing all of it waits for Play's download: Play's
/// part files stay for the next start to carry on from, as a real exit
/// leaves them (videos.rs `run`: "left as after a crash").
#[test]
fn quitting_while_a_transcription_waits_for_play_leaves_plays_parts() {
    let app = TestApp::new();
    // No length known: Play downloads it whole, into the kept video's file.
    let vod = app.add_site_vod("rumble", "r", "Day 3", 0.0).id;
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    play(&app, vod);
    begun.reached();
    app.asked(vod, 0.0, None);
    waits.reached();
    waits.open();
    let media = app.paths().media();
    eventually("Play's part written", || vodscribe_core::pipeline::file_owners(&media).contains(&vod));
    let _folder = app.quit();
    assert!(vodscribe_core::pipeline::file_owners(&media).contains(&vod), "Play's part deleted by the quit");
}

/// Removing the VOD while Play waits for transcribing all of it: both end,
/// and nothing of the video is left.
#[test]
fn removing_a_vod_whose_play_waits_for_a_transcription_ends_both() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.asked(vod, 0.0, None);
    begun.reached();
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    play(&app, vod);
    waits.reached();
    waits.open();
    app.invoke("delete_vod", json!({ "id": vod })).unwrap();
    eventually("nothing of it runs", || {
        let state = app.state();
        let (_held, running) = state.worker.hold_starts();
        let videos = app.invoke("video_downloads", json!({})).unwrap();
        running.is_empty() && videos.as_array().unwrap().is_empty()
    });
    assert!(app.requests(vod).is_empty(), "its transcription goes on");
    let media = vodscribe_core::pipeline::file_owners(&app.paths().media());
    assert!(!media.contains(&vod), "its video's files stay");
    begun.open();
}

/// Delete all videos while Play waits for transcribing all of it: Play
/// ends, and the transcription keeps the video it downloads.
#[test]
fn delete_all_videos_while_play_waits_for_a_transcription_ends_play() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    let request = app.asked(vod, 0.0, None);
    begun.reached();
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    play(&app, vod);
    waits.reached();
    waits.open();
    app.invoke("delete_all_videos", json!({})).unwrap();
    eventually("Play's download over", || last_heard(&app, vod)["progress"].is_null());
    begun.open();
    transcribed_from_the_kept_video(&app, vod, request);
}

/// Play of another VOD while Play of this one waits for transcribing all of
/// this one: the other's download begins, held back by nothing of this VOD.
#[test]
fn play_of_another_vod_begins_while_one_waits_for_a_transcription() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let other = app.add_site_vod("youtube", "y", "Other", 60.0).id;
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.asked(vod, 0.0, None);
    begun.reached();
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    play(&app, vod);
    waits.reached();
    waits.open();
    play(&app, other);
    let media = app.paths().media();
    checked_then(
        || eventually("the other's download begun", || vodscribe_core::pipeline::file_owners(&media).contains(&other)),
        || begun.open(),
    );
}

/// Transcribing another VOD while transcribing all of this one waits for
/// Play's download: the other's download begins, held back by nothing of
/// this VOD.
#[test]
fn another_vods_transcription_begins_while_one_waits_for_play() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let other = app.add_site_vod("youtube", "y", "Other", 60.0).id;
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    play(&app, vod);
    begun.reached();
    app.asked(vod, 0.0, None);
    waits.reached();
    waits.open();
    let request = app.asked(other, 0.0, None);
    checked_then(
        || eventually("the other's download begun", || status_of(&app, request) == Status::Downloading),
        || begun.open(),
    );
}

/// Play waiting for transcribing all of it to download the video shows how
/// far that download is, to play once it's in.
#[test]
fn play_waiting_for_a_transcription_shows_its_download_progress() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    let request = app.asked(vod, 0.0, None);
    begun.reached();
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    play(&app, vod);
    waits.reached();
    waits.open();
    begun.open();
    transcribed_from_the_kept_video(&app, vod, request);
    let heard = app.heard("video-progress");
    let partway = heard.iter().filter(|e| e["id"] == vod).any(|e| {
        e["progress"].as_f64().is_some_and(|p| p > 0.0 && p < 1.0) && e["streamed"] == false && e["error"].is_null()
    });
    assert!(partway, "no progress shown: {heard:?}");
}

/// Play passed over behind a whole transcription, then a quit, and the app
/// started on the same library: no Play is left waiting there, as in a new
/// process (videos.rs `forget_turns`: "an app started on its folder again
/// starts with none, as a new process does").
#[test]
fn a_play_waiting_for_a_transcription_is_not_left_waiting_after_a_quit_and_a_start() {
    let app = TestApp::new();
    let vod = site_vod(&app, "r");
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.asked(vod, 0.0, None);
    begun.reached();
    let waits = Gate::new();
    app.holds_video_waits(&waits);
    play(&app, vod);
    waits.reached();
    waits.open();
    assert!(crate::videos::play_waits(&app.paths(), vod), "Play passed over");
    let folder = app.quit();
    let app = TestApp::on(folder);
    assert!(!crate::videos::play_waits(&app.paths(), vod), "Play still waiting in the app started again");
}
