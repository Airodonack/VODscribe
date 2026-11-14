//! Removing VODs (`delete_vod`), which ends their transcription and their
//! chat's download, erasing those nothing uses (`Sweeper`), and deleting a
//! VOD's kept video (`delete_video`, Settings' `delete_all_videos`).
//! Transcriptions run on the kit's faked worker (`TestApp::worker`), chats
//! on its chat queue (`TestApp::fetches_chats`). Work slow to stop runs till
//! the test lets it go (`TestApp::busy_till`), and a wait for it runs out
//! when the test moves the app's clock on (`wait_runs_out`).
//!
//! Stated elsewhere, cited here and not repeated: removing a VOD while its
//! clips wait to be made makes none of them (CHANGELOG.md, 0.4.1 Fixes:
//! "Removing a VOD while its clips waited to be made could still download
//! them, leaving their files behind."), clips/tests/queue.rs
//! `removing_a_vod_stops_its_clip_being_made_and_makes_none_of_its_others`.

use super::*;

use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::{Value, json};
use vodscribe_core::db::ChatStatus;
use vodscribe_core::db::ClipStatus;
use vodscribe_core::settings::ClipCodec;
use vodscribe_core::testkit::{Gate, Media, eventually, yt_dlp};
use vodscribe_core::transcript::Segment;

use crate::testkit::{TestApp, near};

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

/// Remove on VOD `id`'s row with its video deleted anyway, whatever edits
/// use of it, on a thread of its own as the page awaits it while it goes on:
/// its answer, once it's done. The app's end waits for it.
fn removing_anyway(app: &TestApp, id: i64) -> mpsc::Receiver<CmdResult<Option<String>>> {
    let (answer, answered) = mpsc::channel();
    let handle = app.handle().clone();
    let thread = std::thread::spawn(move || {
        let state = handle.state::<AppState>();
        let _ =
            answer.send(tauri::async_runtime::block_on(delete_vod(handle.clone(), state, id, Some(true), Some(true))));
    });
    crate::testkit::started(app.handle(), thread);
    answered
}

/// Has the wait for a VOD's work held at `waits` (`TestApp::holds_work_waits`)
/// run out: once it has come to the gate, the app's clock is moved past
/// `WORK_STOPPING` and the gate opened. The waits after it are held at
/// `next` (`Gate::opened()`: none are).
fn wait_runs_out(app: &TestApp, waits: &Gate, next: &Gate) {
    waits.reached();
    app.holds_work_waits(next);
    app.clock().advance(WORK_STOPPING + Duration::from_millis(1));
    waits.open();
}

/// The ids of the VODs in the library's list, in its order.
fn listed(app: &TestApp) -> Vec<i64> {
    let vods = app.invoke("list_vods", json!({})).unwrap();
    vods.as_array().unwrap().iter().map(|v| v["id"].as_i64().unwrap()).collect()
}

/// An edit holding `start`..`end` seconds of VOD `vod_id`
/// (`TestApp::edit_of`): the edit's id and its piece's.
fn an_edit(app: &TestApp, vod_id: i64, start: f64, end: f64) -> (i64, i64) {
    let edit = app.edit_of(&[(vod_id, start, end)]);
    (edit, pieces(app, edit)[0]["id"].as_i64().unwrap())
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

/// The lines of VOD `id`'s transcript, as the page reads them.
fn transcript(app: &TestApp, id: i64) -> Vec<String> {
    let lines = app.invoke("transcript", json!({ "vodId": id })).unwrap();
    lines.as_array().unwrap().iter().map(|l| l["text"].as_str().unwrap().to_string()).collect()
}

/// Asks for VOD `id`'s whole video to be kept and waits till it is.
fn keep_video(app: &TestApp, id: i64) {
    app.invoke("keep_video", json!({ "id": id })).unwrap();
    eventually("its video kept", || app.db().vod(id).unwrap().is_some_and(|v| v.media));
}

/// A removed VOD leaves the library's list, and the page hears it went
/// (vods.rs `delete_vod`: "Takes VOD `id` out of the library's list";
/// vods.rs the `vod-removed` the page is sent).
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
/// took (vods.rs `delete_vod`: "its own clip goes, with its marks and
/// its clip bar (the clips' files too, unless dragged out of the app)").
/// clips/tests/handed_out.rs states the same from the clip's side, on a
/// disk without hard links (`a_vod_removed_keeps_the_clip_file_another_app_was_given`).
#[test]
fn a_removed_vods_clips_go_with_their_files_unless_another_app_took_them() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let taken = app.make_clip(vod.id, "0:10", "0:20");
    let left = app.make_clip(vod.id, "0:30", "0:40");
    let given = app.dragged_out(&taken);
    remove(&app, vod.id);
    assert_eq!((app.clip(taken.id), app.clip(left.id)), (None, None), "its clips are still there");
    assert!(!Path::new(left.file_path.as_deref().unwrap()).exists(), "a clip's file stayed");
    assert!(given.is_file(), "the file another app took was deleted");
}

/// A removed VOD's download stops where it is: its erasure, which waits for
/// that, finishes with the download never let go on (vods.rs
/// `delete_vod`: "its download stops where it is. Its transcription,
/// started from it, ends";
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
    app.erasures_done();
    assert!(app.db().stretches(vod.id).unwrap().is_empty(), "its request is still there");
    assert!(!app.paths().source_audio(vod.id, "m4a").exists(), "its download went on");
}

/// A removed VOD's download stopped partway leaves no file behind: what
/// was written of it, in its part file, goes with the VOD (CHANGELOG.md,
/// 0.3.0 Fixes: "Stopping or removing a download could leave partial files
/// behind.").
#[test]
fn a_removed_vods_partial_download_leaves_no_files() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let halfway = Gate::new();
    app.yt_dlp.holds_at(0.5, &halfway);
    app.invoke("transcribe_stretch", json!({ "vodId": vod.id, "start": 0.0, "end": null })).unwrap();
    halfway.reached();
    let audio = app.paths().source_audio(vod.id, "m4a");
    let part = PathBuf::from(format!("{}.part", audio.display()));
    assert!(part.is_file(), "half of it written");
    remove(&app, vod.id);
    app.erasures_done();
    assert!(!part.exists(), "its part file stayed");
    assert!(!audio.exists(), "its download went on");
}

/// A removed VOD nothing uses is erased at once, each as it's removed,
/// while a clip of another is still being made (vods.rs `delete_vod`:
/// "if nothing does now, it's erased at once, a job beside the encodes";
/// vods.rs `Sweeper`: "Each is queued as it's removed"; CHANGELOG.md,
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
    app.erasures_done();
    for vod in &removed {
        assert!(app.db().lines(vod.id).unwrap().is_empty(), "VOD {}'s transcript is still there", vod.id);
        assert!(app.db().is_gone(vod.id).unwrap(), "VOD {} not erased", vod.id);
    }
    assert_eq!(app.clip(clip.id).unwrap().status, ClipStatus::Cutting, "the clip was made first");
    halfway.open();
    assert_eq!(app.made(clip.id).status, ClipStatus::Ready);
}

/// An erasure erases the VOD's files, footage and pictures, by their names,
/// and no other VOD's (vods.rs `erase_source`: "Erases VOD `id`'s files,
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
    app.erasures_done();
    let left: Vec<PathBuf> = files_of(removed.id).into_iter().filter(|f| f.exists()).collect();
    assert_eq!(left, Vec::<PathBuf>::new(), "its files stayed");
    assert!(files_of(other.id).iter().all(|f| f.is_file()), "another VOD's files went");
}

/// A removed VOD an edit uses is kept as a source, its transcript and all,
/// till nothing uses it; then it goes, at the next start (vods.rs
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
    app.erasures_done();
    assert_eq!(listed(&app), Vec::<i64>::new());
    assert!(!app.db().is_gone(vod.id).unwrap(), "erased while the edit uses it");
    assert_eq!(transcript(&app, vod.id), ["The Eagle has landed"]);
    assert_eq!(pieces(&app, edit)[0]["hole"], false);
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    let app = TestApp::on(app.quit());
    app.erasures_done();
    assert!(app.db().vod(vod.id).unwrap().is_none(), "kept once nothing used it");
}

/// A VOD just removed keeps its row as a gone source, so an undo in an edit
/// that puts back a piece of it finds a hole (vods.rs `Sweep`: "one
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
    app.erasures_done();
    assert!(app.db().is_gone(vod.id).unwrap(), "its row isn't a gone source");
    let undone = app.invoke("undo", json!({ "view": { "kind": "edit", "id": edit } })).unwrap();
    assert!(!undone.is_null(), "nothing to undo");
    let pieces = pieces(&app, edit);
    assert_eq!(pieces.len(), 1, "the piece isn't back");
    assert_eq!((&pieces[0]["vod_id"], &pieces[0]["hole"]), (&json!(vod.id), &json!(true)));
}

/// Removed with its video, a VOD keeps only the stretch an edit uses, as a
/// video of its own holding exactly that stretch, and the edit's piece plays
/// it; the rest is erased at once (vods.rs `delete_vod`: "With
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
/// uses is now kept in your chosen format."; clips.rs
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
/// the answer says why (vods.rs `delete_vod`: "if one can't be
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
/// leaves holes where the edit's pieces of it were (vods.rs
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
/// changed meanwhile, is made again as it is now (vods.rs `realize`:
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
/// (vods.rs `delete_video`: "Deletes the video kept for VOD `id`,
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

/// Deleting a VOD's video while its transcription waits in the queue takes
/// the transcription out, keeping the lines it wrote as Stop does, and
/// deletes the video (decided for 0.7.4: deleting a VOD's video while it's
/// transcribed cancels the transcription;
/// vods.rs `cancel_vod`: "Ends a VOD's requests, keeping the lines they
/// wrote").
#[test]
fn deleting_a_vods_video_while_its_transcription_waits_cancels_it_keeping_its_lines() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    keep_video(&app, vod.id);
    let file = app.db().playable_path(vod.id).unwrap().unwrap();
    let request = app.asked(vod.id, 0.0, None);
    // Its audio is the kept video: it's ready at once, and waits for the
    // transcriber, which no test installs.
    eventually("the request ready", || app.db().stretch(request).unwrap().unwrap().status == Status::Ready);
    // What its transcription wrote before it was paused and resumed: the
    // worker's first batch, through to 0:30.
    let line = Segment { start: 12.0, end: 14.0, text: "The Eagle has landed".into(), words: Vec::new() };
    app.db().replace_segments(vod.id, request, (0.0, Some(30.0)), &[line], 30.0).unwrap();
    assert_eq!(app.invoke("delete_video", json!({ "id": vod.id })), Ok(Value::Null));
    assert!(!Path::new(&file).exists(), "its video is still there");
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media, "it says its video is kept");
    assert!(app.db().requests(vod.id).unwrap().is_empty(), "its transcription still waits");
    let stretches = app.db().stretches(vod.id).unwrap();
    assert_eq!(stretches.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(), [(0.0, Some(30.0))]);
    assert_eq!(transcript(&app, vod.id), ["The Eagle has landed"]);
}

/// Deleting a VOD's video while it's being transcribed stops the
/// transcription where it is and deletes the video, with nothing of it left
/// running (decided for 0.7.4: deleting a VOD's video while it's
/// transcribed cancels the transcription;
/// CHANGELOG.md, 0.4.0 Fixes: "Removing a VOD, or deleting its downloaded
/// video, didn't stop what was still using it."). The transcription reads
/// the kept video, and is held after its first minute.
#[test]
fn deleting_a_vods_video_while_it_is_transcribed_stops_the_transcription() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed."), (200.0, "Later on.")]);
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 600.0);
    keep_video(&app, vod.id);
    let file = app.db().playable_path(vod.id).unwrap().unwrap();
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.asked(vod.id, 0.0, None);
    app.held(vod.id, &gate);
    assert_eq!(app.invoke("delete_video", json!({ "id": vod.id })), Ok(Value::Null));
    assert!(!Path::new(&file).exists(), "its video is still there");
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media, "it says its video is kept");
    // The gate stays shut: the transcription was stopped, not let go on.
    assert!(app.db().requests(vod.id).unwrap().is_empty(), "its transcription is still there");
    assert!(!app.transcribing(vod.id), "its transcription is still running");
    assert_eq!(app.lines(vod.id), lines(&[(10.0, "The Eagle has landed.")]));
}

/// A removed VOD's chat download stops where it is (CHANGELOG.md, 0.3.0:
/// "YouTube chat shows how far it has got and appears as it downloads, and
/// removing the VOD stops it."; `vods.rs` `delete_vod`'s `chat.stop`).
/// The site is held once the chat's first line is in: the download, stopped,
/// leaves nothing while it's still held, and the chat queue goes on to the
/// next VOD's chat.
#[test]
fn a_removed_vods_chat_fetch_stops() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    let lines = [10.0, 20.0, 30.0].iter().map(|&at| yt_dlp::chat_line(at, "@ada", "hi")).collect();
    app.yt_dlp.serves_chat(&vod.info.url, lines);
    let downloading = Gate::new();
    app.yt_dlp.holds_chat(&downloading);
    app.fetches_chats();
    downloading.reached();
    eventually("how far it has got shows", || !app.heard("chat-progress").is_empty());
    let part = PathBuf::from(format!("{}.live_chat.json.part", app.paths().temp_chat(vod.id).display()));
    assert!(part.exists(), "it's downloading");
    remove(&app, vod.id);
    eventually("its download stops, leaving nothing", || !part.exists());
    let next = app.add_site_vod("twitch", "1000000002", "A stream", 60.0);
    app.net.twitch_chat("1000000002", &[(5.0, "bo", "o7")]);
    app.invoke("retry_chat", json!({ "id": next.id })).unwrap();
    eventually("the next VOD's chat is fetched", || app.db().vod(next.id).unwrap().unwrap().chat == ChatStatus::Done);
}

/// A VOD from a site without a player of its own, whose video was deleted,
/// downloads it again when it's played: the play asks for it to be kept
/// (Player.svelte `startDownload`), and it is (vods.rs `delete_video`:
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
/// put back (vods.rs `delete_vod`'s `history.forget`; history.rs
/// `forget`: "Forgets `doc`'s history (it's deleted)"; history.rs, its module doc:
/// "deleting a VOD or an edit aren't undone"). Going back and forward is
/// the page's; this is what the app does for it.
#[test]
fn a_removed_vods_undo_history_goes_with_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.make_clip(vod.id, "0:10", "0:20");
    remove(&app, vod.id);
    let undone = app.invoke("undo", json!({ "view": { "kind": "vod", "id": vod.id } }));
    assert_eq!(undone, Ok(Value::Null));
}

/// A removed VOD nothing uses has its transcription canceled, the request
/// running and the one waiting, and is erased (decided for 0.7.4: a VOD
/// nothing uses has all its transcription canceled, then is erased).
#[test]
fn a_removed_vod_nothing_uses_has_its_transcription_canceled_then_is_erased() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.asked(vod, 0.0, Some(180.0));
    app.held(vod, &gate);
    app.waiting(vod, 200.0, 260.0);
    remove(&app, vod);
    app.erasures_done();
    assert!(app.db().is_gone(vod).unwrap(), "not erased");
    assert_eq!(app.db().stretches(vod).unwrap(), [], "its transcription is still there");
    assert_eq!(app.lines(vod), []);
}

/// Settings › Delete all videos deletes the video of a VOD whose
/// transcription waits too, canceling it first and keeping the lines it
/// wrote, as the row's Delete video does (decided for 0.7.4: Settings ›
/// Delete all videos cancels transcriptions too, as the row's Delete video
/// does).
#[test]
fn delete_all_videos_cancels_the_transcriptions_reading_them_keeping_their_lines() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    keep_video(&app, vod.id);
    let file = app.db().playable_path(vod.id).unwrap().unwrap();
    let request = app.asked(vod.id, 0.0, None);
    // Its audio is the kept video: it's ready at once, and waits for the
    // transcriber, which no test installs.
    eventually("the request ready", || app.db().stretch(request).unwrap().unwrap().status == Status::Ready);
    let line = Segment { start: 12.0, end: 14.0, text: "The Eagle has landed".into(), words: Vec::new() };
    app.db().replace_segments(vod.id, request, (0.0, Some(30.0)), &[line], 30.0).unwrap();
    assert_eq!(app.invoke("delete_all_videos", json!({})), Ok(json!(0)), "a video kept");
    assert!(!Path::new(&file).exists(), "its video is still there");
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media, "it says its video is kept");
    assert!(app.db().requests(vod.id).unwrap().is_empty(), "its transcription still waits");
    let stretches = app.db().stretches(vod.id).unwrap();
    assert_eq!(stretches.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(), [(0.0, Some(30.0))]);
    assert_eq!(transcript(&app, vod.id), ["The Eagle has landed"]);
}

/// Removed when its requests can't be ended (the library refuses the
/// change), a VOD is still out of the list, the page hears it went, and
/// it's erased: its requests are left for the next start
/// (`end_unlisted_requests`) (vods.rs the `vod-removed` the page is
/// sent; vods.rs `delete_vod`: "if nothing does now, it's erased at
/// once").
#[test]
fn a_vod_whose_requests_cant_be_ended_is_still_removed_and_erased() {
    let app = TestApp::new();
    let vod = app.file("Ten minutes.mp4", 10.0);
    let request = app.asked(vod, 0.0, None);
    // Its audio is the file: it's ready at once, and waits for the
    // transcriber, which no test installs, with no stage holding it.
    app.ready(request);
    let line = Segment { start: 12.0, end: 14.0, text: "The Eagle has landed".into(), words: Vec::new() };
    app.db().replace_segments(vod, request, (0.0, Some(30.0)), &[line], 30.0).unwrap();
    // Ending it keeps it done as far as it reached: a change the library
    // now refuses. An erasure deletes rows, and isn't refused.
    let refuses = vodscribe_core::testkit::library::beside(&app.paths().db());
    refuses
        .execute_batch("CREATE TRIGGER refused BEFORE UPDATE ON stretches BEGIN SELECT RAISE(ABORT, 'refused'); END;")
        .unwrap();
    assert_eq!(app.invoke("delete_vod", json!({ "id": vod })), Ok(Value::Null));
    assert_eq!(listed(&app), Vec::<i64>::new());
    assert_eq!(app.heard("vod-removed"), [json!(vod)]);
    app.erasures_done();
    assert!(app.db().is_gone(vod).unwrap(), "not erased");
}

/// Remove takes the library before the queue as it ends a VOD's requests,
/// as an erasure does, so removing one VOD while another's erasure ends
/// can't wait for it for good: taking them the other way round, removing
/// one VOD deadlocked with another's erasure (worker.rs
/// `control`: "the library's lock before the queue's, everywhere").
#[test]
fn remove_takes_the_library_before_the_queue() {
    let app = TestApp::new();
    let vod = app.file("Ten minutes.mp4", 10.0);
    an_edit(&app, vod, 300.0, 360.0);
    app.ready(app.asked(vod, 0.0, Some(100.0)));
    let remove =
        move |app: AppHandle| tauri::async_runtime::block_on(delete_vod(app.clone(), app.state(), vod, None, None));
    let (first, removed) = took_the_library_first(&app, remove);
    assert_eq!(removed, Ok(None));
    assert!(first, "the queue taken before the library");
}

/// Delete video takes the library before the queue as it cancels the
/// transcription, as an erasure does (worker.rs `control`: "the
/// library's lock before the queue's, everywhere").
#[test]
fn delete_video_takes_the_library_before_the_queue() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    keep_video(&app, vod.id);
    let id = vod.id;
    let delete = move |app: AppHandle| tauri::async_runtime::block_on(delete_video(app.clone(), app.state(), id));
    let (first, deleted) = took_the_library_first(&app, delete);
    assert_eq!(deleted, Ok(()));
    assert!(first, "the queue taken before the library");
}

/// Settings › Delete all videos takes the library before the queue as it
/// cancels a transcription reading a video, as an erasure does (worker.rs
/// `control`: "the library's lock before the queue's, everywhere").
#[test]
fn delete_all_videos_takes_the_library_before_the_queue() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    keep_video(&app, vod.id);
    let request = app.asked(vod.id, 0.0, None);
    eventually("the request ready", || app.db().stretch(request).unwrap().unwrap().status == Status::Ready);
    let delete = |app: AppHandle| {
        let state = app.state::<AppState>();
        tauri::async_runtime::block_on(crate::videos::delete_all_videos(app.clone(), state))
    };
    let (first, deleted) = took_the_library_first(&app, delete);
    assert_eq!(deleted, Ok(0));
    assert!(first, "the queue taken before the library");
}

/// The rule for a removed VOD's transcription (decided for 0.7.4: Remove
/// ends a VOD's transcriptions).
const ENDS: &str = "a deleted source VOD should end a transcription that was started from that VOD";

/// Removing a VOD an edit uses ends its transcription, the request running
/// and the one waiting, though the edit plays what one asks for: each
/// keeps its lines so far, as Stop does (decided for 0.7.4: `ENDS`;
/// vods.rs `delete_vod`: "Its transcription, started
/// from it, ends, whatever uses it").
#[test]
fn removing_a_vod_an_edit_uses_ends_its_transcription_keeping_its_lines() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed."), (330.0, "Said in the edit."), (500.0, "Said after it.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    an_edit(&app, vod, 300.0, 360.0);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.asked(vod, 0.0, Some(180.0));
    app.held(vod, &gate);
    app.waiting(vod, 300.0, 360.0);
    remove(&app, vod);
    app.settled(vod);
    assert_eq!(app.requests(vod), [], "{ENDS}");
    assert_eq!(app.lines(vod), lines(&[(10.0, "The Eagle has landed.")]));
    let [(start, end, done)] = app.stretches(vod).try_into().unwrap();
    assert_eq!((start, done), (0.0, Status::Done), "done as far as it reached");
    assert!(end.is_some_and(|end| end < 100.0), "done to {end:?}");
}

/// A VOD added back after it was removed while an edit used it shows what
/// its transcription had written, done, with nothing paused or waiting
/// (decided for 0.7.4: `ENDS`; core db.rs `add_vod`: "Taken out of the list but kept for other clips: listed
/// again").
#[test]
fn a_vod_added_back_shows_what_its_transcription_had_written_and_nothing_asked() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed."), (330.0, "Said in the edit.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    an_edit(&app, vod, 300.0, 360.0);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.asked(vod, 0.0, None);
    app.held(vod, &gate);
    remove(&app, vod);
    app.settled(vod);
    let kept = app.stretches(vod);
    let file = app.db().vod(vod).unwrap().unwrap().source_path.unwrap();
    let added = app.invoke("add_files", json!({ "paths": [file] })).unwrap();
    assert_eq!(added["vods"][0]["id"], json!(vod), "not the VOD removed: {added}");
    assert_eq!(listed(&app), [vod]);
    let shown = app.db().vod(vod).unwrap().unwrap();
    assert_eq!((shown.status, shown.transcribed), (Status::None, vodscribe_core::stretches::Transcribed::Some));
    assert_eq!(app.stretches(vod), kept);
    assert!(kept.iter().all(|s| s.2 == Status::Done), "{kept:?}");
    assert_eq!(app.lines(vod), lines(&[(10.0, "The Eagle has landed.")]));
}

/// The requests an older build left on a VOD it took out of the list, one
/// paused part way and one waiting, are ended at the next start, the lines
/// so far kept (decided for 0.7.4: at start, the requests an older build
/// left on removed VODs are ended; vods.rs `end_unlisted_requests`).
#[test]
fn requests_an_older_build_left_on_a_removed_vod_are_ended_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.file("Ten minutes.mp4", 10.0);
    an_edit(&app, vod, 300.0, 360.0);
    {
        // As an older build left it: out of the list, a request paused
        // after its first half minute and another waiting.
        let state = app.state();
        let (_held, _) = state.worker.hold_starts();
        let paused = app.asked(vod, 0.0, Some(120.0));
        let line = Segment { start: 12.0, end: 14.0, text: "The Eagle has landed".into(), words: Vec::new() };
        app.db().replace_segments(vod, paused, (0.0, Some(30.0)), &[line], 30.0).unwrap();
        app.db().set_stretch_status(paused, Status::Paused, 0.25, None).unwrap();
        app.asked(vod, 300.0, Some(360.0));
        app.db().remove_entry(vod).unwrap();
    }
    let app = TestApp::on(app.quit());
    assert_eq!(app.requests(vod), [], "left asked for");
    assert_eq!(app.stretches(vod), [(0.0, Some(30.0), Status::Done)]);
    assert_eq!(transcript(&app, vod), ["The Eagle has landed"]);
}

/// What the Remove dialog says of Remove and delete the video, when an edit
/// uses the VOD (decided for 0.7.4, with Remove ending all of a VOD's
/// transcriptions).
const KEEPS_ONLY: &str = "Remove and delete the video keeps only what the edit uses.";

/// Removed with its video anyway while it's being transcribed, a VOD an
/// edit uses is erased all the same: its transcription ends, and it's a
/// gone source, no lines or stretch done left of it, now and after a
/// restart (`KEEPS_ONLY`; vods.rs `delete_vod`: "With `anyway` too, it's
/// erased without that: the pieces other clips have of it are holes"). A
/// file on this computer is the user's and stays, so the edit's piece
/// plays from it (edits.rs `pieces`).
#[test]
fn removed_with_its_video_anyway_while_transcribed_a_vod_an_edit_uses_is_erased() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    an_edit(&app, vod, 300.0, 360.0);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.asked(vod, 0.0, Some(180.0));
    app.held(vod, &gate);
    let answer = app.invoke("delete_vod", json!({ "id": vod, "deleteVideo": true, "anyway": true })).unwrap();
    assert_eq!(answer, Value::Null);
    gate.open();
    app.erasures_done();
    let erased = |app: &TestApp, when: &str| {
        assert!(app.db().is_gone(vod).unwrap(), "{when}: kept as a source, its video with it ({KEEPS_ONLY})");
        assert_eq!(app.stretches(vod), [], "{when}: a stretch done with no lines");
        assert_eq!(app.lines(vod), [], "{when}");
    };
    erased(&app, "removed");
    let app = TestApp::on(app.quit());
    app.erasures_done();
    erased(&app, "after a restart");
}

/// Removed with its video anyway while its work is slow to stop (longer
/// than Remove waits for it, `WORK_STOPPING`), a site's VOD an edit uses is
/// erased once that work has stopped, the edit's piece of it a hole, the
/// erasure beside the encodes
/// (`KEEPS_ONLY`; vods.rs `Sweeper`: "Each is queued as it's removed").
#[test]
fn removed_with_its_video_anyway_a_vod_whose_work_is_slow_to_stop_is_erased_once_it_has() {
    let app = TestApp::new();
    let vod = a_site_vod(&app).id;
    let (edit, _) = an_edit(&app, vod, 10.0, 20.0);
    let (work, waits) = (Gate::new(), Gate::new());
    app.busy_till(vod, &work);
    app.holds_work_waits(&waits);
    let removing = removing_anyway(&app, vod);
    wait_runs_out(&app, &waits, &Gate::opened());
    assert_eq!(removing.recv().unwrap(), Ok(None));
    assert_eq!(listed(&app), Vec::<i64>::new());
    assert_ne!(app.state().sweeper.listed(), (None, vec![]), "its erasure done before its work stopped");
    work.open();
    app.erasures_done();
    assert!(app.db().is_gone(vod).unwrap(), "kept as a source, its video with it ({KEEPS_ONLY})");
    assert_eq!(pieces(&app, edit)[0]["hole"], json!(true), "the edit's piece still plays");
}

/// Removed with its video anyway, its work slow to stop, then added back by
/// its link before its erasure has begun (another erasure ahead of it), a
/// VOD is left as it is now: not gone, and what it's transcribed since is
/// kept (vods.rs `erase_unused`: "Added back meanwhile, it's left as it is").
#[test]
fn removed_with_its_video_anyway_and_added_back_before_its_erasure_a_vod_is_left_as_it_is() {
    let app = TestApp::new();
    let ahead = app.add_site_vod("twitch", "2901646294", "Another talk", 60.0);
    app.says(ahead.id, 12.0, "First");
    let gate = Gate::new();
    app.holds_erasures(&gate);
    remove(&app, ahead.id);
    gate.reached();
    let vod = a_site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    an_edit(&app, vod.id, 10.0, 20.0);
    let (work, waits) = (Gate::new(), Gate::new());
    app.busy_till(vod.id, &work);
    app.holds_work_waits(&waits);
    let removing = removing_anyway(&app, vod.id);
    wait_runs_out(&app, &waits, &Gate::opened());
    assert_eq!(removing.recv().unwrap(), Ok(None));
    app.invoke("add_vod", json!({ "url": vod.info.url })).unwrap();
    assert_eq!(listed(&app), [vod.id]);
    app.says(vod.id, 30.0, "Said since");
    gate.open();
    app.erasures_done();
    assert_eq!(listed(&app), [vod.id]);
    assert!(!app.db().is_gone(vod.id).unwrap(), "added back, then erased by the Remove before it");
    assert_eq!(transcript(&app, vod.id), ["Said since"]);
}

/// Removed with its video anyway, its work slower to stop than Remove and
/// then its erasure wait for it (`WORK_STOPPING` each), a VOD an edit uses
/// is erased at the next start, its files too (`KEEPS_ONLY`; vods.rs
/// `Sweeper::run`: "those removed with their videos whose erasure didn't
/// finish").
#[test]
fn removed_with_its_video_anyway_a_vod_whose_work_outlasts_both_waits_is_erased_at_the_next_start() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    an_edit(&app, vod.id, 10.0, 20.0);
    let paths = app.paths();
    let files = [paths.source_audio(vod.id, "m4a"), paths.source_video(vod.id, "mp4"), paths.source_info(vod.id)];
    for file in &files {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"made").unwrap();
    }
    let (work, waits, erasure_waits) = (Gate::new(), Gate::new(), Gate::new());
    app.busy_till(vod.id, &work);
    app.holds_work_waits(&waits);
    let removing = removing_anyway(&app, vod.id);
    wait_runs_out(&app, &waits, &erasure_waits);
    assert_eq!(removing.recv().unwrap(), Ok(None));
    wait_runs_out(&app, &erasure_waits, &Gate::opened());
    app.erasures_done();
    let left = || files.iter().filter(|f| f.exists()).cloned().collect::<Vec<PathBuf>>();
    let app = TestApp::on(app.quit());
    app.erasures_done();
    assert!(app.db().is_gone(vod.id).unwrap());
    assert_eq!(left(), Vec::<PathBuf>::new(), "the video it was removed with stays");
}

/// Removed with its video anyway, its work slow to stop, and VODscribe quit
/// before its erasure has begun (another ahead of it), a VOD an edit uses
/// is erased at the next start (`KEEPS_ONLY`; vods.rs `delete_vod`: "it's
/// still erased if the app quits first").
#[test]
fn removed_with_its_video_anyway_then_quit_before_its_erasure_a_vod_is_erased_at_the_next_start() {
    let app = TestApp::new();
    let ahead = app.add_site_vod("twitch", "2901646294", "Another talk", 60.0);
    app.says(ahead.id, 12.0, "First");
    let gate = Gate::new();
    app.holds_erasures(&gate);
    remove(&app, ahead.id);
    gate.reached();
    let vod = a_site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    an_edit(&app, vod.id, 10.0, 20.0);
    let (work, waits) = (Gate::new(), Gate::new());
    app.busy_till(vod.id, &work);
    app.holds_work_waits(&waits);
    let removing = removing_anyway(&app, vod.id);
    wait_runs_out(&app, &waits, &Gate::opened());
    assert_eq!(removing.recv().unwrap(), Ok(None));
    let app = TestApp::on(app.quit());
    app.erasures_done();
    assert!(app.db().is_gone(vod.id).unwrap(), "removed with its video, kept whole");
    assert_eq!(transcript(&app, vod.id), Vec::<String>::new());
}

/// A VOD removed with its video anyway while an edit used it, added back,
/// transcribed anew and then removed again keeping its video, while
/// transcribed, keeps its new transcript for the edit, as Remove does
/// (`a_vod_used_by_an_edit_is_kept_as_a_source_until_nothing_uses_it`;
/// worker.rs `forget_if_erased`: "the VOD not gone as it began").
#[test]
fn a_vod_added_back_after_its_video_was_deleted_keeps_its_new_transcript_when_removed_again() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed."), (200.0, "Later on.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    an_edit(&app, vod, 300.0, 360.0);
    let answer = app.invoke("delete_vod", json!({ "id": vod, "deleteVideo": true, "anyway": true })).unwrap();
    assert_eq!(answer, Value::Null);
    app.erasures_done();
    assert!(app.db().is_gone(vod).unwrap());
    let info = app.db().vod(vod).unwrap().unwrap().info;
    app.db().add_vod(&info).unwrap();
    assert_eq!(listed(&app), [vod]);
    app.asked(vod, 0.0, Some(60.0));
    app.settled(vod);
    let before = app.lines(vod);
    assert!(!before.is_empty(), "transcribed anew");
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.asked(vod, 180.0, Some(240.0));
    gate.reached();
    remove(&app, vod);
    gate.open();
    app.settled(vod);
    app.erasures_done();
    let after = app.lines(vod);
    assert!(after.starts_with(&before), "a plain Remove keeps the transcript of a VOD an edit uses");
}

/// Removed with its video anyway while an edit uses it, then added back by
/// its link, a VOD is one like any other again: not gone, so the edit's
/// piece of it plays again (its footage downloaded again as needed) and its
/// video can be kept, with nothing left of the transcript it was removed
/// with (as one nothing used is added anew: vods.rs `add_to_library`).
#[test]
fn a_vod_removed_with_its_video_and_added_back_is_a_vod_like_any_other_again() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let (edit, _) = an_edit(&app, vod.id, 10.0, 20.0);
    let answer = app.invoke("delete_vod", json!({ "id": vod.id, "deleteVideo": true, "anyway": true })).unwrap();
    assert_eq!(answer, Value::Null);
    app.erasures_done();
    app.invoke("add_vod", json!({ "url": vod.info.url })).unwrap();
    assert_eq!(listed(&app), [vod.id]);
    assert_eq!(pieces(&app, edit)[0]["hole"], json!(false), "added back, the edit's piece of it a hole still");
    assert!(!app.db().is_gone(vod.id).unwrap(), "added back, still gone");
    assert_eq!(transcript(&app, vod.id), Vec::<String>::new());
    keep_video(&app, vod.id);
}

/// The site serves VOD `vod`'s chat replay: three messages.
fn serves_its_chat(app: &TestApp, vod: &Vod) {
    let lines = [10.0, 20.0, 30.0].iter().map(|&at| yt_dlp::chat_line(at, "@ada", "hi")).collect();
    app.yt_dlp.serves_chat(&vod.info.url, lines);
}

/// Whether VOD `id`'s chat is fetched: all three messages of it.
fn has_its_chat(app: &TestApp, id: i64) -> bool {
    app.db().vod(id).unwrap().is_some_and(|v| v.chat == ChatStatus::Done) && app.db().chat(id).unwrap().len() == 3
}

/// Remove on VOD `id`'s row with its video deleted anyway, whatever edits
/// use of it, waited out.
fn remove_with_video_anyway(app: &TestApp, id: i64) {
    let answer = app.invoke("delete_vod", json!({ "id": id, "deleteVideo": true, "anyway": true })).unwrap();
    assert_eq!(answer, Value::Null);
    app.erasures_done();
}

/// Removed with its video anyway while an edit uses it, then added back by
/// its link, a VOD has its chat fetched again, as one added anew does
/// (`a_vod_removed_with_its_video_and_added_back_is_a_vod_like_any_other_again`;
/// vods.rs `add_vod`).
#[test]
fn a_vod_removed_with_its_video_and_added_back_has_its_chat_fetched_again() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    serves_its_chat(&app, &vod);
    app.fetches_chats();
    eventually("its chat fetched", || has_its_chat(&app, vod.id));
    an_edit(&app, vod.id, 10.0, 20.0);
    remove_with_video_anyway(&app, vod.id);
    app.invoke("add_vod", json!({ "url": vod.info.url })).unwrap();
    eventually("its chat fetched again", || has_its_chat(&app, vod.id));
}

/// Removed with its video anyway while an edit uses it, then added back by
/// its link, a VOD has its picture again, fetched from its site as when it
/// was added (`a_vod_removed_with_its_video_and_added_back_is_a_vod_like_any_other_again`;
/// thumbnails.rs `cache_thumbnail`), not the name of the one its erasure
/// deleted (vods.rs `erase_source`: "Erases VOD `id`'s files, footage and
/// pictures").
#[test]
fn a_vod_removed_with_its_video_and_added_back_has_its_picture_again() {
    let app = TestApp::new();
    app.net.public("pictures.example");
    app.net.serves("https://pictures.example/talk.png", b"\x89PNG\r\n\x1a\n".to_vec());
    let link = "https://www.youtube.com/watch?v=noa29qsTc4g";
    let mut details = yt_dlp::details("youtube", "noa29qsTc4g", link, "A talk", 60.0);
    details["thumbnail"] = "https://pictures.example/talk.png".into();
    app.yt_dlp.serves(details);
    let id = app.invoke("add_vod", json!({ "url": link })).unwrap()["vod"]["id"].as_i64().unwrap();
    app.threads_done();
    let picture = |app: &TestApp| app.db().vod(id).unwrap().unwrap().thumb.filter(|name| !name.is_empty());
    let first = picture(&app).expect("its picture fetched");
    an_edit(&app, id, 10.0, 20.0);
    remove_with_video_anyway(&app, id);
    assert!(app.invoke("thumbnail", json!({ "name": first })).is_err(), "its picture kept by its erasure");
    app.invoke("add_vod", json!({ "url": link })).unwrap();
    app.threads_done();
    let name = picture(&app).expect("no picture named");
    assert!(app.invoke("thumbnail", json!({ "name": name })).is_ok(), "the picture named is gone");
}

/// Removed with its video anyway while an edit uses it, then added back by
/// its link, a VOD is offered its VODscription again once the file host is
/// found to have one, as one added anew is
/// (`a_vod_removed_with_its_video_and_added_back_is_a_vod_like_any_other_again`;
/// vods.rs `add_vod`, `look_up_added`).
#[test]
fn a_vod_removed_with_its_video_and_added_back_is_offered_its_vodscription_again() {
    let app = TestApp::new();
    // The file host is asked about one VOD at a time in the whole process,
    // by its id (annotations.rs `ASKING`): this test's is numbered apart.
    for i in 0..4 {
        app.add_video_file(&format!("Before {i}.mp4"), 60.0);
    }
    let link = "https://www.youtube.com/watch?v=readded0001";
    app.yt_dlp.serves(yt_dlp::details("youtube", "readded0001", link, "A stream", 600.0));
    app.net.replies("https://dl.vodscribe.com/annotations/youtube/readded0001.json", |asked| {
        (asked.method == "HEAD").then(|| vodscribe_core::testkit::Reply::status(200).with_header("ETag", "\"v1\""))
    });
    let id = app.invoke("add_vod", json!({ "url": link })).unwrap()["vod"]["id"].as_i64().unwrap();
    app.threads_done();
    assert_eq!(app.heard("annotations-found"), [json!(id)]);
    an_edit(&app, id, 10.0, 20.0);
    remove_with_video_anyway(&app, id);
    app.invoke("add_vod", json!({ "url": link })).unwrap();
    app.threads_done();
    assert_eq!(app.heard("annotations-found"), [json!(id), json!(id)], "not offered again once added back");
}

/// A VOD an older version added back after it was removed with its video
/// while an edit used it is listed and still gone, as 0.7.3 left it (its
/// `add_vod` listed it again, leaving it gone): from the next start it's a
/// VOD like any other, the edit's piece of it playing again and its chat
/// fetched again (lib.rs `tidy_at_start`; core sources.rs
/// `clear_listed_gone`).
#[test]
fn a_vod_an_older_version_added_back_still_gone_plays_in_edits_from_the_next_start() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    serves_its_chat(&app, &vod);
    app.fetches_chats();
    eventually("its chat fetched", || has_its_chat(&app, vod.id));
    let (edit, _) = an_edit(&app, vod.id, 10.0, 20.0);
    // What 0.7.3's erasure left of it, listed again.
    app.db().clear_chat(vod.id).unwrap();
    app.db().set_gone(vod.id).unwrap();
    assert_eq!(pieces(&app, edit)[0]["hole"], json!(true));
    let app = TestApp::on(app.quit());
    serves_its_chat(&app, &vod);
    app.fetches_chats();
    assert_eq!(listed(&app), [vod.id]);
    assert_eq!(pieces(&app, edit)[0]["hole"], json!(false), "a gap in the edit still");
    eventually("its chat fetched again", || has_its_chat(&app, vod.id));
}

/// Removed with its video anyway, its work slow to stop, then added back
/// while its erasure waits for that work, a VOD's new transcription isn't
/// paused by the erasure: it's left as it is (vods.rs `erase_unused`:
/// "Added back meanwhile, work begun on it since isn't paused").
#[test]
fn a_vod_added_back_while_its_erasure_waits_is_transcribed_unpaused() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    an_edit(&app, vod, 300.0, 360.0);
    let file = app.db().vod(vod).unwrap().unwrap().source_path.unwrap();
    let (work, waits, erasure_waits) = (Gate::new(), Gate::new(), Gate::new());
    app.busy_till(vod, &work);
    app.holds_work_waits(&waits);
    let removing = removing_anyway(&app, vod);
    wait_runs_out(&app, &waits, &erasure_waits);
    assert_eq!(removing.recv().unwrap(), Ok(None));
    // Its chat and transcript erased: the erasure waits for the work.
    erasure_waits.reached();
    app.invoke("add_files", json!({ "paths": [file] })).unwrap();
    assert_eq!(listed(&app), [vod]);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.asked(vod, 0.0, Some(180.0));
    app.held(vod, &gate);
    // The erasure looks again, and ends.
    erasure_waits.open();
    app.erasures_done();
    let statuses: Vec<Status> = app.requests(vod).iter().map(|s| s.status).collect();
    assert_eq!(statuses, [Status::Transcribing], "paused by the erasure of the VOD before it was added back");
    gate.open();
    app.settled(vod);
    app.erasures_done();
    assert!(!app.db().is_gone(vod).unwrap());
    assert_eq!(app.lines(vod), lines(&[(10.0, "The Eagle has landed.")]));
}

/// A site VOD whose video is kept, removed with its video anyway (its work
/// slow to stop) and added back before its erasure has begun, leaves no
/// video behind that it doesn't know of, now or after a restart: what its
/// erasure would have deleted goes as it's added back (vods.rs
/// `add_to_library`).
#[test]
fn a_vod_added_back_before_its_erasure_keeps_no_video_it_does_not_know_of() {
    let app = TestApp::new();
    let ahead = app.add_site_vod("twitch", "2901646294", "Another talk", 60.0);
    app.says(ahead.id, 12.0, "First");
    let vod = a_site_vod(&app);
    an_edit(&app, vod.id, 10.0, 20.0);
    keep_video(&app, vod.id);
    let video = PathBuf::from(app.db().playable_path(vod.id).unwrap().unwrap());
    let gate = Gate::new();
    app.holds_erasures(&gate);
    remove(&app, ahead.id);
    gate.reached();
    let (work, waits) = (Gate::new(), Gate::new());
    app.busy_till(vod.id, &work);
    app.holds_work_waits(&waits);
    let removing = removing_anyway(&app, vod.id);
    wait_runs_out(&app, &waits, &Gate::opened());
    assert_eq!(removing.recv().unwrap(), Ok(None));
    app.invoke("add_vod", json!({ "url": vod.info.url })).unwrap();
    gate.open();
    app.erasures_done();
    let app = TestApp::on(app.quit());
    app.erasures_done();
    let row = app.db().vod(vod.id).unwrap().unwrap();
    assert!(row.listed && !app.db().is_gone(vod.id).unwrap());
    assert!(row.media || !video.exists(), "its video file stays, the VOD not knowing it");
}

/// Removed with its video anyway while an edit uses it, its work slower to
/// stop than Remove waits for it, and added back by its link while Remove
/// waits, a VOD is one like any other again, now and after a restart:
/// listed, not gone, the edit's piece of it playing (vods.rs
/// `add_to_library`: "One removed with its video while clips use it has its
/// files deleted first, and is listed again, not gone"; vods.rs
/// `erase_unused`: "Added back meanwhile, it's left as it is").
#[test]
fn a_vod_added_back_while_remove_waits_for_work_that_outlasts_the_wait_is_a_vod_like_any_other() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let (edit, _) = an_edit(&app, vod.id, 10.0, 20.0);
    let (work, waits) = (Gate::new(), Gate::new());
    app.busy_till(vod.id, &work);
    app.holds_work_waits(&waits);
    let removing = removing_anyway(&app, vod.id);
    waits.reached();
    app.invoke("add_vod", json!({ "url": vod.info.url })).unwrap();
    assert_eq!(listed(&app), [vod.id]);
    wait_runs_out(&app, &waits, &Gate::opened());
    assert_eq!(removing.recv().unwrap(), Ok(None));
    work.open();
    app.erasures_done();
    let like_any_other = |app: &TestApp, when: &str| {
        assert_eq!(listed(app), [vod.id], "{when}");
        assert!(!app.db().is_gone(vod.id).unwrap(), "{when}: listed, and gone for good");
        assert_eq!(pieces(app, edit)[0]["hole"], json!(false), "{when}: the edit's piece of it a hole");
    };
    like_any_other(&app, "added back");
    let app = TestApp::on(app.quit());
    app.erasures_done();
    like_any_other(&app, "after a restart");
}

/// Removed with its video anyway while an edit uses it, and added back by
/// its link while Remove waits for its work, which then stops in time, a VOD
/// is left as it is by the Remove: not gone, and what it's transcribed
/// since is kept (vods.rs `add_to_library`: "is listed again, not gone, new
/// to the library as one added anew is"; vods.rs `erase_unused`: "Added back
/// meanwhile, it's left as it is").
#[test]
fn a_vod_added_back_while_remove_waits_for_work_that_stops_in_time_is_left_as_it_is() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let (edit, _) = an_edit(&app, vod.id, 10.0, 20.0);
    let (work, waits) = (Gate::new(), Gate::new());
    app.busy_till(vod.id, &work);
    app.holds_work_waits(&waits);
    let removing = removing_anyway(&app, vod.id);
    waits.reached();
    app.invoke("add_vod", json!({ "url": vod.info.url })).unwrap();
    assert_eq!(listed(&app), [vod.id]);
    app.says(vod.id, 30.0, "Said since");
    work.open();
    waits.open();
    assert_eq!(removing.recv().unwrap(), Ok(None));
    app.erasures_done();
    assert_eq!(listed(&app), [vod.id]);
    assert!(!app.db().is_gone(vod.id).unwrap(), "added back, then erased by the Remove before it");
    assert_eq!(pieces(&app, edit)[0]["hole"], json!(false));
    assert_eq!(transcript(&app, vod.id), ["Said since"]);
}

/// Removed with its video anyway while an edit uses it, and VODscribe quit
/// while Remove waits for its work, a VOD is erased at the next start, its
/// files too (`KEEPS_ONLY`; vods.rs `delete_vod`: "so it's still erased if
/// the app quits first"; vods.rs `Sweeper::run`: "those removed with their
/// videos whose erasure didn't finish").
#[test]
fn quit_while_remove_waits_for_its_work_a_vod_removed_with_its_video_is_erased_at_the_next_start() {
    let app = TestApp::new();
    let vod = a_site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    an_edit(&app, vod.id, 10.0, 20.0);
    let paths = app.paths();
    let files = [paths.source_audio(vod.id, "m4a"), paths.source_video(vod.id, "mp4"), paths.source_info(vod.id)];
    for file in &files {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"made").unwrap();
    }
    let (work, waits) = (Gate::new(), Gate::new());
    app.busy_till(vod.id, &work);
    app.holds_work_waits(&waits);
    let _removing = removing_anyway(&app, vod.id);
    waits.reached();
    // Its work outlasts the wait, the app quitting meanwhile.
    app.clock().advance(WORK_STOPPING + Duration::from_millis(1));
    let app = TestApp::on(app.quit());
    app.erasures_done();
    assert!(app.db().is_gone(vod.id).unwrap(), "removed with its video, kept whole");
    assert_eq!(transcript(&app, vod.id), Vec::<String>::new());
    let left: Vec<PathBuf> = files.iter().filter(|f| f.exists()).cloned().collect();
    assert_eq!(left, Vec::<PathBuf>::new(), "the video it was removed with stays");
}

/// Removed with its video anyway while an edit uses it, and added back by
/// its link while Remove waits for its work, a VOD stays in the page's list:
/// the page doesn't hear it was removed, and hears it as it is now, listed,
/// whether its work outlasts Remove's wait or stops in time (vods.rs
/// `delete_vod`: "Added back meanwhile, it's left as it is"; App.svelte, the
/// `vodRemoved` it's told drops the row).
#[test]
fn a_vod_added_back_while_remove_waits_for_its_work_stays_in_the_pages_list() {
    for outlasts in [true, false] {
        let app = TestApp::new();
        let vod = a_site_vod(&app);
        an_edit(&app, vod.id, 10.0, 20.0);
        let (work, waits) = (Gate::new(), Gate::new());
        app.busy_till(vod.id, &work);
        app.holds_work_waits(&waits);
        let removing = removing_anyway(&app, vod.id);
        waits.reached();
        app.invoke("add_vod", json!({ "url": vod.info.url })).unwrap();
        let before = app.heard("vod").len();
        if outlasts {
            wait_runs_out(&app, &waits, &Gate::opened());
        } else {
            work.open();
            waits.open();
        }
        assert_eq!(removing.recv().unwrap(), Ok(None));
        let case = if outlasts { "its work outlasting the wait" } else { "its work stopping in time" };
        assert_eq!(app.heard("vod-removed"), Vec::<Value>::new(), "{case}: the page drops its row");
        let since = app.heard("vod").split_off(before);
        let shown = since.iter().rfind(|v| v["id"] == json!(vod.id));
        assert_eq!(shown.map(|v| &v["listed"]), Some(&json!(true)), "{case}: the page isn't shown it again");
    }
}
