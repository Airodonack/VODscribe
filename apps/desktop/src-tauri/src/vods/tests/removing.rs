//! Removing VODs (`delete_vod`), erasing those nothing uses (`Sweeper`),
//! and deleting a VOD's kept video (`delete_video`).
//!
//! Stated elsewhere, cited here and not repeated: removing a VOD while its
//! clips wait to be made makes none of them (CHANGELOG.md, 0.4.1 Fixes:
//! "Removing a VOD while its clips waited to be made could still download
//! them, leaving their files behind."), the clips pilot's
//! `removing_a_vod_stops_its_clip_being_made_and_makes_none_of_its_others`
//! in clips/tests/queue.rs.

use super::*;

use std::path::PathBuf;

use serde_json::{Value, json};
use vodscribe_core::db::{Clip, ClipStatus};
use vodscribe_core::settings::ClipCodec;
use vodscribe_core::testkit::{Gate, Media, eventually};

use crate::testkit::TestApp;

/// A minute of YouTube VOD, "A talk"; the fake site serves it.
fn a_site_vod(app: &TestApp) -> Vod {
    app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 60.0)
}

/// Remove on VOD `id`'s row (its video kept for edits that use it): the
/// command's answer, null when it's removed.
fn remove(app: &TestApp, id: i64) -> Value {
    app.invoke("delete_vod", json!({ "id": id })).unwrap()
}

/// Remove on VOD `id`'s row with its video deleted too.
fn remove_with_video(app: &TestApp, id: i64) -> Value {
    app.invoke("delete_vod", json!({ "id": id, "deleteVideo": true })).unwrap()
}

/// `remove_with_video` on a thread of its own, as the page awaits it while
/// it goes on: its answer once it's done.
fn removing_with_video(app: &TestApp, id: i64) -> std::thread::JoinHandle<CmdResult<Option<String>>> {
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        let state = handle.state::<AppState>();
        tauri::async_runtime::block_on(delete_vod(handle.clone(), state, id, Some(true), None))
    })
}

/// Waits till the list of jobs shows no VOD being erased, or waiting to be.
fn erasures_done(app: &TestApp) {
    eventually("the erasures done", || {
        let jobs = app.invoke("list_jobs", json!({})).unwrap();
        jobs.as_array().unwrap().iter().all(|job| job["kind"] != "erase")
    });
}

/// The ids of the VODs in the library's list, in its order.
fn listed(app: &TestApp) -> Vec<i64> {
    let vods = app.invoke("list_vods", json!({})).unwrap();
    vods.as_array().unwrap().iter().map(|v| v["id"].as_i64().unwrap()).collect()
}

/// An edit holding `start`..`end` seconds of VOD `vod_id`, made as the page
/// makes one: the edit's id and its piece's.
fn an_edit(app: &TestApp, vod_id: i64, start: f64, end: f64) -> (i64, i64) {
    let edit = app.invoke("create_edit", json!({})).unwrap()["id"].as_i64().unwrap();
    let piece = json!({ "editId": edit, "vodId": vod_id, "start": start, "end": end });
    let view = app.invoke("add_range_to_edit", piece).unwrap();
    (edit, view["pieces"][0]["id"].as_i64().unwrap())
}

/// Edit `edit`'s pieces as its view shows them.
fn pieces(app: &TestApp, edit: i64) -> Vec<Value> {
    app.invoke("get_edit", json!({ "id": edit })).unwrap()["pieces"].as_array().unwrap().clone()
}

/// What the video VOD `id` plays from holds; panics if it has none.
fn video_of(app: &TestApp, id: i64) -> Media {
    let file = app.db().playable_path(id).unwrap().unwrap_or_else(|| panic!("VOD {id} has no video"));
    Media::read(Path::new(&file)).unwrap_or_else(|| panic!("no media at {file}"))
}

/// Whether `media` is a cut of `link` whose 0 is second `at` of it, to the
/// millisecond (a cut seeks half a millisecond early: core clips.rs `cut`).
fn cut_of(media: &Media, link: &str, at: f64) -> bool {
    media.cut_from.as_ref().is_some_and(|(from, its)| from == link && near(*its, at))
}

/// Whether two lengths in seconds are the same to the millisecond.
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.001
}

/// The lines of VOD `id`'s transcript, as the page reads them.
fn transcript(app: &TestApp, id: i64) -> Vec<String> {
    let lines = app.invoke("transcript", json!({ "vodId": id })).unwrap();
    lines.as_array().unwrap().iter().map(|l| l["text"].as_str().unwrap().to_string()).collect()
}

/// Clip `clip` dragged out of the window and let go on an editor that takes
/// the file itself: the file it was given.
fn dragged_out(app: &TestApp, clip: &Clip) -> PathBuf {
    use crate::testkit::{LetGo, Receiver};
    app.drag_out(&[clip.id]).unwrap();
    let drag = app.drags().last();
    drag.let_go(LetGo::On(Receiver::taking_files("Kdenlive")));
    app.drag_ended();
    let given = drag.given();
    assert_eq!(given.len(), 1, "clip {} not handed out", clip.id);
    given[0].clone()
}

/// Asks for VOD `id`'s whole video to be kept and waits till it is.
fn keep_video(app: &TestApp, id: i64) {
    app.invoke("keep_video", json!({ "id": id })).unwrap();
    eventually("its video kept", || app.db().vod(id).unwrap().is_some_and(|v| v.media));
}

/// A removed VOD leaves the library's list, and the page hears it went
/// (vods.rs:375, `delete_vod`: "Takes VOD `id` out of the library's list";
/// vods.rs:435, the `vod-removed` the page is sent).
#[test]
fn a_removed_vod_leaves_the_list_and_the_page_hears_it() {
    let app = TestApp::new();
    let removed = app.add_video_file("Day 3.mp4", 60.0);
    let kept = app.add_video_file("Day 4.mp4", 60.0);
    assert_eq!(remove(&app, removed.id), Value::Null);
    assert_eq!(listed(&app), [kept.id]);
    assert_eq!(app.heard("vod-removed"), [json!(removed.id)]);
}

/// A removed VOD's clips go with their files, except a file another app
/// took (vods.rs:375, `delete_vod`: "its own clip goes, with its marks and
/// its clip bar (the clips' files too, unless dragged out of the app)").
/// The clips pilot's E30 states the same from the clip's side, on a disk
/// without hard links (`a_vod_removed_keeps_the_clip_file_another_app_was_given`).
#[test]
fn a_removed_vods_clips_go_with_their_files_unless_another_app_took_them() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let taken = app.make_clip(vod.id, "0:10", "0:20");
    let left = app.make_clip(vod.id, "0:30", "0:40");
    let given = dragged_out(&app, &taken);
    remove(&app, vod.id);
    assert_eq!((app.clip(taken.id), app.clip(left.id)), (None, None), "its clips are still there");
    assert!(!Path::new(left.file_path.as_deref().unwrap()).exists(), "a clip's file stayed");
    assert!(given.is_file(), "the file another app took was deleted");
}

/// A removed VOD's download stops where it is: its erasure, which waits for
/// that, finishes with the download never let go on (vods.rs:375,
/// `delete_vod`: "its download or transcription stops where it is";
/// CHANGELOG.md, 0.4.0 Fixes: "Removing a VOD, or deleting its downloaded
/// video, didn't stop what was still using it.").
#[test]
fn a_removed_vods_download_or_transcription_stops_where_it_is() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.invoke("transcribe_stretch", json!({ "vodId": vod.id, "start": 0.0, "end": null })).unwrap();
    begun.reached();
    remove(&app, vod.id);
    // The gate stays shut: only a download that stopped lets the erasure
    // past waiting for it (`WORK_STOPPING`, longer than `eventually` waits).
    erasures_done(&app);
    assert!(app.db().stretches(vod.id).unwrap().is_empty(), "its request is still there");
    assert!(!app.paths().source_audio(vod.id, "m4a").exists(), "its download went on");
}

/// A removed VOD nothing uses is erased at once, each as it's removed,
/// while a clip of another is still being made (vods.rs:378, `delete_vod`:
/// "if nothing does now, it's erased at once, a job beside the encodes";
/// vods.rs:542, `Sweeper`: "Each is queued as it's removed"; CHANGELOG.md,
/// 0.7.1: "Removed VODs are erased at once." "It's erased beside whatever is
/// being made rather than as VODscribe quits").
#[test]
fn a_removed_vod_nothing_uses_is_erased_at_once_beside_whats_being_made() {
    let app = TestApp::new();
    let making = app.add_video_file("Day 3.mp4", 60.0);
    let removed = [app.add_video_file("Day 4.mp4", 60.0), app.add_video_file("Day 5.mp4", 60.0)];
    for vod in &removed {
        app.says(vod.id, 12.0, "The Eagle has landed");
    }
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(making.id, "0:10", "0:20").unwrap();
    halfway.reached();
    for vod in &removed {
        remove(&app, vod.id);
    }
    erasures_done(&app);
    for vod in &removed {
        assert!(app.db().lines(vod.id).unwrap().is_empty(), "VOD {}'s transcript is still there", vod.id);
        assert!(app.db().is_gone(vod.id).unwrap(), "VOD {} not erased", vod.id);
    }
    assert_eq!(app.clip(clip.id).unwrap().status, ClipStatus::Cutting, "the clip was made first");
    halfway.open();
    assert_eq!(app.made(clip.id).status, ClipStatus::Ready);
}

/// An erasure erases the VOD's files, footage and pictures, by their names,
/// and no other VOD's (vods.rs:439, `erase_source`: "Erases VOD `id`'s files,
/// footage and pictures"; tests/e2e/scenarios/readd-erasing.mjs: "its work
/// files, kept video, thumbnail, peaks, storyboard and footage", "what's
/// deleted is decided by name, not by what's in them").
#[test]
fn an_erasure_erases_the_vods_files_footage_and_pictures() {
    let app = TestApp::new();
    let removed = a_site_vod(&app);
    let other = app.add_site_vod("twitch", "2901646294", "Another talk", 60.0);
    let paths = app.paths();
    let files_of = |id: i64| {
        let video = paths.source_video(id, "mp4");
        let section = format!("{}.mp4", vodscribe_core::footage::section_stem(id, 10.0, 20.0));
        vec![
            paths.source_audio(id, "m4a"),
            paths.source_info(id),
            video.clone(),
            paths.source_thumb(&format!("{id}-1.jpg")),
            vodscribe_core::peaks::path(&paths.peaks(), id),
            paths.source_storyboard(id, &video),
            paths.source_footage().join(section),
        ]
    };
    for file in files_of(removed.id).into_iter().chain(files_of(other.id)) {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"made").unwrap();
    }
    remove(&app, removed.id);
    erasures_done(&app);
    let left: Vec<PathBuf> = files_of(removed.id).into_iter().filter(|f| f.exists()).collect();
    assert_eq!(left, Vec::<PathBuf>::new(), "its files stayed");
    assert!(files_of(other.id).iter().all(|f| f.is_file()), "another VOD's files went");
}

/// A removed VOD an edit uses is kept as a source, its transcript and all,
/// till nothing uses it; then it goes, at the next start (vods.rs:378,
/// `delete_vod`: "What other clips use of it stays, a source only, until
/// nothing does"; CHANGELOG.md, 0.5.0: "Removing a VOD that an edit uses
/// leaves the edit's pieces of it where they are").
#[test]
fn a_vod_used_by_an_edit_is_kept_as_a_source_until_nothing_uses_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let (edit, _) = an_edit(&app, vod.id, 10.0, 20.0);
    remove(&app, vod.id);
    erasures_done(&app);
    assert_eq!(listed(&app), Vec::<i64>::new());
    assert!(!app.db().is_gone(vod.id).unwrap(), "erased while the edit uses it");
    assert_eq!(transcript(&app, vod.id), ["The Eagle has landed"]);
    assert_eq!(pieces(&app, edit)[0]["hole"], false);
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    let app = TestApp::on(app.quit());
    erasures_done(&app);
    assert!(app.db().vod(vod.id).unwrap().is_none(), "kept once nothing used it");
}

/// A VOD just removed keeps its row as a gone source, so an undo in an edit
/// that puts back a piece of it finds a hole (vods.rs:528, `Sweep`: "one
/// just removed keeps its row as a gone source till the next start
/// (`kept`), so an undo in an edit that puts back a piece of it finds it, a
/// hole").
#[test]
fn a_vod_just_removed_keeps_its_row_as_a_gone_source_so_undo_in_an_edit_finds_a_hole() {
    let app = TestApp::new();
    // A site's: a file's own file stays where it is, and plays.
    let vod = a_site_vod(&app);
    let (edit, piece) = an_edit(&app, vod.id, 10.0, 20.0);
    app.invoke("remove_piece", json!({ "id": piece })).unwrap();
    remove(&app, vod.id);
    erasures_done(&app);
    assert!(app.db().is_gone(vod.id).unwrap(), "its row isn't a gone source");
    let undone = app.invoke("undo", json!({ "view": { "kind": "edit", "id": edit } })).unwrap();
    assert!(!undone.is_null(), "nothing to undo");
    let pieces = pieces(&app, edit);
    assert_eq!(pieces.len(), 1, "the piece isn't back");
    assert_eq!((&pieces[0]["vod_id"], &pieces[0]["hole"]), (&json!(vod.id), &json!(true)));
}

/// Removed with its video, a VOD keeps only the stretch an edit uses, as a
/// video of its own holding exactly that stretch, and the edit's piece plays
/// it; the rest is erased at once (vods.rs:381, `delete_vod`: "With
/// `delete_video`, it's erased now: first each stretch other clips use is
/// realized"; CHANGELOG.md, 0.5.0: "Removed with its video, only the
/// stretches the edits use are kept, as videos of their own, and the rest is
/// erased at once").
#[test]
fn removing_with_its_video_keeps_only_the_stretches_edits_use_as_videos_of_their_own() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    // Past the first half minute: its footage is fetched from half a minute
    // before (core footage.rs `MARGIN`), so where the stretch is in it counts.
    let (edit, piece) = an_edit(&app, vod.id, 40.0, 50.0);
    assert_eq!(remove_with_video(&app, vod.id), Value::Null);
    let pieces = pieces(&app, edit);
    let kept = pieces[0]["vod_id"].as_i64().unwrap();
    assert_eq!((&pieces[0]["id"], &pieces[0]["hole"]), (&json!(piece), &json!(false)));
    assert_ne!(kept, vod.id, "the piece is still of the VOD removed");
    let video = video_of(&app, kept);
    assert!(cut_of(&video, &vod.info.url, 40.0), "kept from {:?}", video.cut_from);
    assert!(near(video.duration, 10.0), "kept {} s", video.duration);
    assert!(app.db().vod(vod.id).unwrap().is_none(), "the rest of it is still there");
}

/// A stretch kept from a VOD removed with its video is made in the format
/// chosen in Settings › Encoding when ffmpeg can't make H.264 (CHANGELOG.md,
/// 0.7.3 Fixes: "Removing a VOD works without H.264. The stretch an edit
/// uses is now kept in your chosen format."; clips.rs:597,
/// `encoding_to_keep`: "only an ffmpeg without that makes it in the codec
/// Settings › Encoding names").
#[test]
fn a_stretch_kept_from_a_removed_vod_is_made_in_the_chosen_format() {
    let app = TestApp::new();
    app.ffmpeg.lacks("libx264");
    let mut settings = vodscribe_core::settings::Settings::load(&app.paths().settings());
    settings.clip_codec = Some(ClipCodec::H265);
    settings.normalized().save(&app.paths().settings()).unwrap();
    let vod = a_site_vod(&app);
    let (edit, _) = an_edit(&app, vod.id, 40.0, 50.0);
    assert_eq!(remove_with_video(&app, vod.id), Value::Null);
    let kept = pieces(&app, edit)[0]["vod_id"].as_i64().unwrap();
    assert_eq!(video_of(&app, kept).video.map(|v| v.codec), Some("hevc".into()));
}

/// A stretch that can't be kept (the site refuses it) removes nothing, and
/// the answer says why (vods.rs:382, `delete_vod`: "if one can't be
/// (offline, a full disk), nothing is removed and the answer is why";
/// CHANGELOG.md, 0.5.0: "if those stretches can't be kept (offline, say), it
/// asks whether to try again").
#[test]
fn a_stretch_that_cannot_be_kept_removes_nothing_and_says_why() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let (edit, piece) = an_edit(&app, vod.id, 10.0, 20.0);
    app.yt_dlp.refuses(&vod.info.url);
    let why = remove_with_video(&app, vod.id);
    assert!(why.as_str().is_some_and(|why| !why.is_empty()), "no reason given: {why}");
    assert_eq!(listed(&app), [vod.id]);
    assert_eq!(app.heard("vod-removed"), Vec::<Value>::new());
    let pieces = pieces(&app, edit);
    assert_eq!((&pieces[0]["id"], &pieces[0]["vod_id"]), (&json!(piece), &json!(vod.id)));
    assert_eq!(transcript(&app, vod.id), ["The Eagle has landed"]);
}

/// Removed with its video anyway, without keeping what an edit uses, a VOD
/// leaves holes where the edit's pieces of it were (vods.rs:384,
/// `delete_vod`: "With `anyway` too, it's erased without that: the pieces
/// other clips have of it are holes"; CHANGELOG.md, 0.5.0: "or delete it
/// anyway, leaving gaps in the edits").
#[test]
fn removed_anyway_the_pieces_other_clips_have_of_it_are_holes() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let (edit, _) = an_edit(&app, vod.id, 10.0, 20.0);
    let answer = app.invoke("delete_vod", json!({ "id": vod.id, "deleteVideo": true, "anyway": true })).unwrap();
    assert_eq!(answer, Value::Null);
    let pieces = pieces(&app, edit);
    assert_eq!((&pieces[0]["vod_id"], &pieces[0]["hole"]), (&json!(vod.id), &json!(true)));
}

/// A stretch being kept from a VOD removed with its video, whose piece is
/// changed meanwhile, is made again as it is now (vods.rs:478, `realize`:
/// "Made from the clip at one revision and moved against it: if the clip
/// changed meanwhile, made again").
#[test]
fn a_stretch_kept_from_a_clip_changed_meanwhile_is_made_again() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let (edit, piece) = an_edit(&app, vod.id, 40.0, 50.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let removing = removing_with_video(&app, vod.id);
    halfway.reached();
    // Moved, as long as it was: only its clip's revision tells it changed.
    app.invoke("trim_piece", json!({ "id": piece, "start": 41.0, "end": 51.0 })).unwrap();
    halfway.open();
    assert_eq!(removing.join().unwrap(), Ok(None));
    let kept = pieces(&app, edit)[0]["vod_id"].as_i64().unwrap();
    let video = video_of(&app, kept);
    assert!(cut_of(&video, &vod.info.url, 41.0), "kept as it was: {:?}", video.cut_from);
    assert!(near(video.duration, 10.0), "kept {} s", video.duration);
}

/// Deleting a VOD's kept video deletes its file and keeps its transcript
/// (vods.rs:676, `delete_video`: "Deletes the video kept for VOD `id`,
/// keeping its transcript"; CHANGELOG.md, 0.3.0: "the crossed-out camera
/// in its row deletes the video and keeps the transcript").
#[test]
fn deleting_a_vods_video_keeps_its_transcript() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    keep_video(&app, vod.id);
    let file = app.db().playable_path(vod.id).unwrap().unwrap();
    app.invoke("delete_video", json!({ "id": vod.id })).unwrap();
    assert!(!Path::new(&file).exists(), "its video is still there");
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media, "it says its video is kept");
    assert_eq!(transcript(&app, vod.id), ["The Eagle has landed"]);
}

/// A VOD from a site without a player of its own, whose video was deleted,
/// downloads it again when it's played: the play asks for it to be kept
/// (Player.svelte `startDownload`), and it is (vods.rs:676, `delete_video`:
/// "elsewhere is downloaded again when played (`pipeline::keeps_video`)").
#[test]
fn a_vod_whose_video_was_deleted_downloads_it_again_when_played_where_its_site_has_no_player() {
    let app = TestApp::new();
    let vod = app.add_site_vod("rumble", "v1", "A talk", 60.0);
    keep_video(&app, vod.id);
    app.invoke("delete_video", json!({ "id": vod.id })).unwrap();
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media);
    keep_video(&app, vod.id);
    assert!(near(video_of(&app, vod.id).duration, 60.0));
}

/// A removed VOD's history goes with it: undo in its view finds nothing to
/// put back (vods.rs:403, `delete_vod`'s `history.forget`; history.rs:216,
/// `forget`: "Forgets `doc`'s history (it's deleted)"; history.rs:7:
/// "deleting a VOD or an edit aren't undone"). The plan's 2A3 23 (back and
/// forward) is the page's; this is what the line it cites does.
#[test]
fn a_removed_vods_undo_history_goes_with_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.make_clip(vod.id, "0:10", "0:20");
    remove(&app, vod.id);
    let undone = app.invoke("undo", json!({ "view": { "kind": "vod", "id": vod.id } }));
    assert_eq!(undone, Ok(Value::Null));
}
