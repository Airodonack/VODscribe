use super::*;

use serde_json::{Value, json};
use vodscribe_core::db::Vod;
use vodscribe_core::testkit::{Gate, Media, eventually, yt_dlp};

use crate::testkit::{TestApp, at};

// The bar, its history and the files a clip leaves, as the page and the
// file manager see them.

/// The ranges of the pills on VOD `vod_id`'s clip bar, in its order.
fn bar(app: &TestApp, vod_id: i64) -> Vec<(f64, f64)> {
    app.clips(vod_id).into_iter().filter(|c| c.shown).map(|c| (c.start, c.end)).collect()
}

/// The ids of the pills on VOD `vod_id`'s clip bar.
fn pills(app: &TestApp, vod_id: i64) -> Vec<i64> {
    app.clips(vod_id).into_iter().filter(|c| c.shown).map(|c| c.id).collect()
}

/// Waits till every pill on VOD `vod_id`'s clip bar is made, and gives them.
fn all_made(app: &TestApp, vod_id: i64) -> Vec<Clip> {
    pills(app, vod_id).into_iter().map(|id| app.made(id)).collect()
}

/// Ctrl+Z (⌘Z) in VOD `vod_id`'s view: what was undone (`history::Done`), or
/// null for nothing.
fn undo(app: &TestApp, vod_id: i64) -> Value {
    app.invoke("undo", json!({ "view": { "kind": "vod", "id": vod_id } })).unwrap()
}

/// Ctrl+Shift+Z in VOD `vod_id`'s view.
fn redo(app: &TestApp, vod_id: i64) -> Value {
    app.invoke("redo", json!({ "view": { "kind": "vod", "id": vod_id } })).unwrap()
}

/// The × on clip `id`'s pill.
fn delete(app: &TestApp, id: i64) {
    app.invoke("delete_clip", json!({ "id": id })).unwrap();
}

/// Moves clip `id` to `from`..`to` and waits till it's made again.
fn resize(app: &TestApp, id: i64, from: &str, to: &str) -> Clip {
    app.invoke("set_clip_range", json!({ "id": id, "start": at(from), "end": at(to) })).unwrap();
    app.made(id)
}

/// Show in folder on clip `id`: where the file manager was shown it.
fn show(app: &TestApp, id: i64) -> PathBuf {
    app.invoke("show_clip", json!({ "id": id })).unwrap();
    app.desktop().shown().last().cloned().expect("nothing shown")
}

/// An edit, `e`, holding 0:00–0:30 of VOD `vod_id`.
fn an_edit(app: &TestApp, vod_id: i64) -> i64 {
    let db = app.db();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod_id, (0.0, 30.0), "a", None).unwrap();
    edit
}

/// C on `from`..`to` (seconds) of edit `edit`'s timeline: the clip's id, its
/// file not waited for.
fn add_edit_clip(app: &TestApp, edit: i64, from: f64, to: f64) -> i64 {
    let made = app.invoke("add_edit_clip", json!({ "editId": edit, "from": from, "to": to })).unwrap();
    made["id"].as_i64().unwrap()
}

/// A minute of YouTube VOD, "A talk", streamed on `published` (as yt-dlp
/// gives the date); the fake site serves it.
fn dated_vod(app: &TestApp, published: &str) -> Vod {
    let info = vodscribe_core::fetch::VodInfo { title: "A talk".into(), published: Some(published.into()), ..info() };
    let vod = app.db().add_vod(&info).unwrap().0;
    app.yt_dlp.serves(yt_dlp::details(&info.platform, &info.platform_id, &info.url, &info.title, 60.0));
    vod
}

/// Waits till the clips' queue has nothing running or waiting.
fn idle(app: &TestApp) {
    eventually("the clips' queue done", || app.state().clips.listed() == (None, Vec::new()));
}

/// What's in the library's clips folder (clips' files and their names'
/// folders), by name.
fn clips_folder(app: &TestApp) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(app.paths().clips())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Whether clip `clip`'s file is there.
fn has_file(clip: &Clip) -> bool {
    clip.file_path.as_deref().is_some_and(|f| Path::new(f).is_file())
}

/// Clip `clip`'s own file, made.
fn file_of(clip: &Clip) -> PathBuf {
    PathBuf::from(clip.file_path.as_deref().expect("a made clip's file"))
}

// Deleting a clip.

/// A deleted clip's file and picture go (apps/desktop/src-tauri/src/clips.rs:744,
/// `delete_clip`: "Deletes clip `id`, and its file unless that was dragged
/// out of the app"; :919, `after_delete`: "its files (`let_go_files`) and
/// its thumbnail deleted"; CHANGELOG.md, 0.4.0: "Files you made and no
/// longer use don't pile up").
#[test]
fn a_deleted_clip_s_file_and_picture_go() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let (file, picture) = (file_of(&clip), app.paths().clip_thumb(clip.id));
    assert!(file.is_file() && picture.is_file(), "made without its file or picture");
    delete(&app, clip.id);
    assert!(app.clip(clip.id).is_none(), "still in the library");
    assert!(!file.exists(), "its file left");
    assert!(!picture.exists(), "its picture left");
}

/// The page hears a deleted clip has gone (clips.rs:919, `after_delete`:
/// "and the page told").
#[test]
fn the_page_hears_a_deleted_clip_has_gone() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    delete(&app, clip.id);
    assert_eq!(app.heard("clip-removed"), [json!(clip.id)]);
}

/// A clip deleted and undone comes back as it was, and is made again
/// (tests/e2e/scenarios/clip.mjs: "then deleting it, and undoing that"; in
/// its run: "the clip comes back as it was, 1 to 4 s with its title, and is
/// made again").
#[test]
fn a_clip_deleted_and_undone_comes_back_as_it_was_and_is_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.says(vod.id, 1.5, "The Eagle has landed");
    let clip = app.make_clip(vod.id, "0:01", "0:04");
    let media = app.clip_media(&clip);
    delete(&app, clip.id);
    undo(&app, vod.id);
    let [back] = &all_made(&app, vod.id)[..] else { panic!("not back alone: {:?}", bar(&app, vod.id)) };
    assert_eq!((back.start, back.end, back.title.as_str()), (1.0, 4.0, "The Eagle has landed"));
    assert_eq!(app.clip_media(back), media, "not made again as it was");
}

/// A clip deleted and undone comes back under its own id, so what still
/// holds that id finds it: the page's selection and Back, which copy, rename
/// or open the clip by it, and the clip's own history in the editor
/// (owner, 2026-11-11, plans/clips-pilot.md "Main red after cc7506f":
/// "undo of a delete brings the clip back under its own old id ... That
/// keeps every holder's id valid (the page, Back)").
#[test]
fn a_clip_deleted_and_undone_comes_back_under_its_own_id() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    // Opened in the editor, a stretch of it taken out: a step in its own history.
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 2.0, "to": 3.0 })).unwrap();
    delete(&app, clip.id);
    undo(&app, vod.id);
    assert_eq!(pills(&app, vod.id), [clip.id], "not back under its own id");
    app.made(clip.id);
    let in_editor = app.invoke("undo", json!({ "view": { "kind": "edit", "id": clip.id } })).unwrap();
    assert_ne!(in_editor, Value::Null, "the clip's own history lost");
    app.invoke("copy_items", json!({ "items": [{ "kind": "clip", "id": clip.id }] })).unwrap();
    app.invoke("rename_clip", json!({ "id": clip.id, "title": "Landed" })).unwrap();
    assert_eq!(app.clip(clip.id).unwrap().title, "Landed");
    all_made(&app, vod.id);
}

/// Undone, redone and undone again, a deleted clip keeps its id each time,
/// and a clip made after gets one never given before (core db.rs
/// `MIGRATE_V42`: "a deleted clip's id is never given to a new one"): the
/// clip brought back doesn't move the counter back past one made after it.
#[test]
fn a_clip_deleted_undone_redone_and_undone_again_keeps_its_id() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let older = app.make_clip(vod.id, "0:01", "0:03");
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    // A clip made after it, of another VOD, and deleted for good.
    let other = app.add_video_file("Day 4.mp4", 60.0);
    let later = app.make_clip(other.id, "0:10", "0:20");
    delete(&app, later.id);
    delete(&app, clip.id);
    undo(&app, vod.id);
    assert_eq!(pills(&app, vod.id), [older.id, clip.id], "undone");
    redo(&app, vod.id);
    assert_eq!(pills(&app, vod.id), [older.id], "redone");
    undo(&app, vod.id);
    assert_eq!(pills(&app, vod.id), [older.id, clip.id], "undone again");
    let new = app.make_clip(vod.id, "0:30", "0:40");
    assert!(new.id > later.id, "a new clip given id {}, not past {}", new.id, later.id);
    all_made(&app, vod.id);
}

/// A clip whose file was deleted outside the app, deleted and undone while
/// the window looks for gone files, is made again at a path of its own, so
/// the look's take, which goes by the path it saw missing, leaves it
/// (`forget_gone_clips`: "Only those as they were when looked at (not made
/// again meanwhile)"; plans/clips-pilot.md "Decided" 4b: each make a new
/// file of its own; review-undo-old-id F1).
#[test]
fn a_clip_brought_back_and_made_again_while_gone_files_are_looked_for_stays() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let path = file_of(&clip);
    std::fs::remove_file(&path).unwrap();
    let looked = Gate::new();
    app.holds_gone_looks(&looked);
    let handle = app.handle().clone();
    let vod_id = vod.id;
    let looking = std::thread::spawn(move || {
        let state = handle.state::<AppState>();
        tauri::async_runtime::block_on(forget_gone_clips(handle.clone(), state, Some(vod_id), None))
    });
    looked.reached();
    delete(&app, clip.id);
    undo(&app, vod.id);
    let back = app.made(clip.id);
    assert_ne!(file_of(&back), path, "made again at the path of a make before");
    looked.open();
    looking.join().unwrap().unwrap();
    assert_eq!(pills(&app, vod.id), [clip.id], "the clip brought back and made again was taken for gone");
    assert!(has_file(&app.clip(clip.id).unwrap()), "its new file deleted");
}

/// A clip cut from the middle of the clip bar and undone comes back in the
/// middle, not last (the 0.7.2 bug: CHANGELOG.md, 0.7.2 Fixes: "Undo keeps
/// the clip bar's order." "… and so did one deleted from there when
/// undone"; plans/walkthrough-0.7.2.md step 20: "Pick the middle pill,
/// Ctrl+X (⌘X), Ctrl+Z (⌘Z): it comes back in the middle, not last").
#[test]
fn a_clip_cut_from_the_middle_of_the_bar_and_undone_comes_back_in_the_middle() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (from, to) in [("0:10", "0:20"), ("0:30", "0:40"), ("0:50", "0:55")] {
        app.make_clip(vod.id, from, to);
    }
    let [_, middle, _] = pills(&app, vod.id)[..] else { unreachable!() };
    ctrl_x(&app, &[middle]);
    assert_eq!(bar(&app, vod.id), [(10.0, 20.0), (50.0, 55.0)]);
    undo(&app, vod.id);
    assert_eq!(bar(&app, vod.id), [(10.0, 20.0), (30.0, 40.0), (50.0, 55.0)]);
}

/// A clip deleted from the middle of the clip bar stays in the middle
/// through undo, redo and undo again (CHANGELOG.md, 0.7.2 Fixes: "Undo keeps
/// the clip bar's order."; plans/walkthrough-0.7.2.md step 20: "it comes back
/// in the middle, not last").
#[test]
fn a_clip_deleted_from_the_middle_and_undone_then_redone_and_undone_stays_in_the_middle() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (from, to) in [("0:10", "0:20"), ("0:30", "0:40"), ("0:50", "0:55")] {
        app.make_clip(vod.id, from, to);
    }
    let all = [(10.0, 20.0), (30.0, 40.0), (50.0, 55.0)];
    delete(&app, pills(&app, vod.id)[1]);
    undo(&app, vod.id);
    assert_eq!(bar(&app, vod.id), all, "undone");
    redo(&app, vod.id);
    assert_eq!(bar(&app, vod.id), [all[0], all[2]], "redone");
    undo(&app, vod.id);
    assert_eq!(bar(&app, vod.id), all, "undone again");
    all_made(&app, vod.id);
}

/// Several clips deleted at once are one step, which undo brings back
/// whole, in their places (clips.rs:754, `delete_clips`: "as one change in
/// each clip bar's history: undone, they all come back"; CHANGELOG.md,
/// 0.6.0: "Clip and delete several at once."; tests/e2e/scenarios/multi-select.mjs:
/// "Delete takes both off the clip bar, and one undo brings both back").
#[test]
fn several_clips_deleted_at_once_are_one_step_that_undo_brings_back_whole() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (from, to) in [("0:10", "0:20"), ("0:30", "0:40"), ("0:50", "0:55")] {
        app.make_clip(vod.id, from, to);
    }
    let [first, _, last] = pills(&app, vod.id)[..] else { unreachable!() };
    app.invoke("delete_clips", json!({ "ids": [first, last] })).unwrap();
    assert_eq!(bar(&app, vod.id), [(30.0, 40.0)]);
    assert_eq!(undo(&app, vod.id)["label"], "Clips deleted");
    let mut back = bar(&app, vod.id);
    back.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(back, [(10.0, 20.0), (30.0, 40.0), (50.0, 55.0)], "one undo");
    assert!(all_made(&app, vod.id).iter().all(has_file), "not made again");
}

/// Several clips deleted at once and undone come back in their places on
/// the clip bar, not after the one left (CHANGELOG.md, 0.7.2 Fixes: "Undo
/// keeps the clip bar's order." "… and so did one deleted from there when
/// undone"; plans/walkthrough-0.7.2.md step 20: "it comes back in the
/// middle, not last").
#[test]
fn several_clips_deleted_at_once_and_undone_come_back_in_their_places() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (from, to) in [("0:10", "0:20"), ("0:30", "0:40"), ("0:50", "0:55")] {
        app.make_clip(vod.id, from, to);
    }
    let [first, _, last] = pills(&app, vod.id)[..] else { unreachable!() };
    app.invoke("delete_clips", json!({ "ids": [first, last] })).unwrap();
    undo(&app, vod.id);
    assert_eq!(bar(&app, vod.id), [(10.0, 20.0), (30.0, 40.0), (50.0, 55.0)], "out of place");
    all_made(&app, vod.id);
}

/// Several clips deleted at once go all or none: one that can't go leaves
/// every one as it was, made or being made, with no step to undo
/// (clips.rs:754, `delete_clips`: "All of them go, or none do").
#[test]
fn several_clips_deleted_at_once_go_all_or_none() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let made = app.make_clip(vod.id, "0:10", "0:20");
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let making = app.add_clip(vod.id, "0:30", "0:40").unwrap();
    halfway.reached();
    assert!(app.invoke("delete_clips", json!({ "ids": [made.id, 9999, making.id] })).is_err());
    halfway.open();
    assert_eq!(bar(&app, vod.id), [(10.0, 20.0), (30.0, 40.0)], "not every clip in place");
    assert!(has_file(&app.made(made.id)), "a file deleted");
    assert_eq!(app.made(making.id).status, ClipStatus::Ready, "its making stopped");
    assert!(app.heard("clip-removed").is_empty(), "the page told one went");
    assert_eq!(undo(&app, vod.id)["label"], "Clip added", "a step recorded");
}

/// A clip only dragged out (made for a drag, never on the clip bar) leaves
/// no step to undo when it's deleted (clips.rs:744, `delete_clip`: "One only
/// dragged out, not in the clip bar, leaves no step to undo: the bar is as
/// it was").
#[test]
fn a_clip_only_dragged_out_deleted_leaves_no_step_to_undo() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let dragged = app.run(crate::clips::add_clip(app.handle().clone(), vod.id, 10.0, 20.0, Some(false))).unwrap();
    app.made(dragged.id);
    delete(&app, dragged.id);
    assert_eq!(undo(&app, vod.id), Value::Null);
}

// Deleting a clip still to be made.

/// A clip deleted while its file is being made leaves no file (clips.rs:906,
/// `remove`: "taking it out of the queue, or stopping its cut"; :530, in
/// `cut`: "Deleted while it was being cut."; CHANGELOG.md, 0.5.0 Fixes: "A
/// stretch pulled out of the window with Clip and let go the moment it left
/// could stay as a clip, with its file").
#[test]
fn a_clip_deleted_while_it_is_made_leaves_no_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    delete(&app, clip.id);
    halfway.open();
    idle(&app);
    assert!(app.clip(clip.id).is_none(), "still in the library");
    assert_eq!(clips_folder(&app), Vec::<String>::new(), "left in the clips folder");
    assert!(!app.paths().clip_thumb(clip.id).exists(), "its picture left");
}

/// A clip deleted while it waits its turn is never made (clips.rs:906,
/// `remove`: "taking it out of the queue").
#[test]
fn a_waiting_clip_deleted_is_never_made() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let first = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    let waiting = app.add_clip(vod.id, "0:30", "0:40").unwrap();
    delete(&app, waiting.id);
    halfway.open();
    let first = app.made(first.id);
    idle(&app);
    assert_eq!(clips_folder(&app), [file_of(&first).file_name().unwrap().to_string_lossy()], "another file made");
    let cut = |c: &Value| c["id"] == waiting.id && c["status"] != "waiting";
    assert!(!app.heard("clip").iter().any(cut), "the deleted clip was cut");
}

/// Several waiting clips deleted at once leave the queue, as one deleted
/// alone does: what waits its turn after is only the clips there are
/// (clips.rs:906, `remove`: "taking it out of the queue"; :761, in
/// `delete_clips`: "as `remove` does for one"; CHANGELOG.md, 0.7.1: "A list
/// shows what's being made."). Seen in the queue the list of jobs is made
/// from (`Cutter::listed`), which leaves out clips that are gone.
#[test]
fn several_waiting_clips_deleted_at_once_leave_the_queue() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let first = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    let second = app.add_clip(vod.id, "0:30", "0:40").unwrap();
    let third = app.add_clip(vod.id, "0:50", "0:55").unwrap();
    app.invoke("delete_clips", json!({ "ids": [second.id, third.id] })).unwrap();
    let next = app.add_clip(vod.id, "0:05", "0:08").unwrap();
    assert_eq!(app.state().clips.listed(), (Some(first.id), vec![next.id]), "deleted clips still waiting");
    halfway.open();
    all_made(&app, vod.id);
}

/// Several waiting clips cut at once (Ctrl+X) are deleted, there being no
/// file to keep, and leave the queue as deleted ones do (clips.rs
/// `cut_clips`: "one not made yet is deleted, as there's nothing to keep";
/// the spec above).
#[test]
fn several_waiting_clips_cut_at_once_leave_the_queue() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let first = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    let second = app.add_clip(vod.id, "0:30", "0:40").unwrap();
    let third = app.add_clip(vod.id, "0:50", "0:55").unwrap();
    ctrl_x(&app, &[second.id, third.id]);
    assert!(app.clip(second.id).is_none() && app.clip(third.id).is_none(), "kept, with nothing made");
    assert_eq!(app.state().clips.listed(), (Some(first.id), vec![]), "cut clips still waiting");
    halfway.open();
    all_made(&app, vod.id);
}

// Show in folder, and the names a clip is handed out by.

/// A clip shown in its folder goes by its VOD's date, title and range
/// (clips.rs:1322, `drag_name`: "a clip its VOD's date, title and range";
/// :928, `show_clip`: "under the name it's handed out by"; CHANGELOG.md,
/// 0.7.2: "Clips now live in the library." "A clip or edit dragged out or
/// shown in its folder still goes by its title").
#[test]
fn a_clip_shown_in_its_folder_goes_by_its_vod_s_date_title_and_range() {
    let app = TestApp::new();
    let vod = dated_vod(&app, "2026-10-15");
    let clip = app.make_clip(vod.id, "0:10", "0:20.5");
    let shown = show(&app, clip.id);
    assert_eq!(shown.file_name().unwrap(), "2026-10-15 A talk 00h00m10.000s-00h00m20.500s.mp4");
    assert_eq!(Media::read(&shown), Some(app.clip_media(&clip)), "not the clip's file");
}

/// A clip shown in its folder goes by its own file's extension, made as it
/// was, after the format clips are made in has changed (clips.rs:1322,
/// `drag_name`: "With its file's extension if it's made, else the one it
/// will have, made in `format`").
#[test]
fn a_made_clip_is_shown_with_its_own_file_s_extension() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let mut settings = vodscribe_core::settings::Settings::load(&app.paths().settings());
    settings.clip_format = vodscribe_core::settings::ClipFormat::Mov;
    settings.clip_codec = None;
    app.run(crate::setup::set_settings(app.state(), settings)).unwrap();
    let shown = show(&app, clip.id);
    assert_eq!(shown.extension().unwrap(), "mp4", "{shown:?}");
}

/// A clip changed in the editor (lines struck) is shown by its own title
/// (clips.rs:1322, `drag_name`: "an edit's (or a snippet changed in the
/// editor) its title").
#[test]
fn a_clip_changed_in_the_editor_is_shown_by_its_own_title() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.says(vod.id, 11.0, "The Eagle has landed");
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 4.0, "to": 6.0 })).unwrap();
    app.made(clip.id);
    assert_eq!(show(&app, clip.id).file_name().unwrap(), "The Eagle has landed.mp4");
}

/// A clip shown twice is shown under the same name while it's the same file
/// (clips.rs:325, `handed_out`: "One handed out before under that name is
/// given again while it's still this file").
#[test]
fn a_clip_shown_twice_is_shown_under_the_same_name_while_it_is_the_same_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let first = show(&app, clip.id);
    assert_eq!(show(&app, clip.id), first);
    let names = std::fs::read_dir(app.paths().clip_names(clip.id)).unwrap().count();
    assert_eq!(names, 1, "a second name made");
}

/// A clip made again keeps the name it was handed out by, with the file it
/// had then, for the app that took it; its next hand out gets a name of its
/// own (clips.rs:325, `handed_out`: "once the clip is made again (a new file)
/// or retitled, the old name stays as it was, for the editor whose project
/// uses it, and the next hand out gets a name of its own"; :475, in `cut`:
/// "those it was handed out by keep the one they had").
#[test]
fn a_clip_made_again_keeps_its_old_name_and_picture_for_the_app_that_took_it_and_gets_a_new_name() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    let taken = show(&app, clip.id);
    let longer = resize(&app, clip.id, "0:10", "0:25");
    assert_eq!(Media::read(&taken), Some(media.clone()), "the name taken holds the clip as it's made now");
    assert_eq!(Media::read(&show(&app, clip.id)), Some(app.clip_media(&longer)));
    // Back to 0:10–0:20, made again: a new file, under a name of its own.
    undo(&app, vod.id);
    app.made(clip.id);
    let again = show(&app, clip.id);
    let name = taken.file_name().unwrap().to_string_lossy().replace(".mp4", " (2).mp4");
    assert_eq!(again.file_name().unwrap().to_string_lossy(), name);
    assert_eq!(Media::read(&taken), Some(media), "the name taken changed");
}

/// A clip handed out takes no more space where the disk allows: a hard
/// link, not a copy or a symbolic link (clips.rs:325, `handed_out`: "A hard
/// link, the same file under a second name, where the disk allows (no space
/// taken); a copy where it doesn't; never a symbolic link").
#[cfg(unix)]
#[test]
fn a_clip_handed_out_takes_no_more_space_where_the_disk_allows() {
    use std::os::unix::fs::MetadataExt;
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let shown = show(&app, clip.id);
    let link = std::fs::symlink_metadata(&shown).unwrap();
    assert!(link.file_type().is_file(), "a symbolic link");
    assert_eq!(link.ino(), std::fs::metadata(file_of(&clip)).unwrap().ino(), "a copy");
}

/// A clip not yet made can't be shown in its folder (clips.rs:936,
/// `show_clip`: "That clip's file isn't made yet.").
#[test]
fn a_clip_not_yet_made_cannot_be_shown_in_its_folder() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    let refused = app.invoke("show_clip", json!({ "id": clip.id }));
    halfway.open();
    assert_eq!(refused, Err(json!("That clip's file isn't made yet.")));
    assert!(app.desktop().shown().is_empty(), "shown anyway");
    app.made(clip.id);
}

/// A clip shown in its folder keeps that name, and its file, when it goes
/// (clips.rs:928, `show_clip`: "from there it's the user's to take"; :415,
/// `let_go_files`: "the names it was handed out by kept if another app took
/// them").
#[test]
fn a_clip_shown_in_its_folder_keeps_that_name_when_it_goes() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    let shown = show(&app, clip.id);
    delete(&app, clip.id);
    assert_eq!(Media::read(&shown), Some(media), "the name shown went");
}

/// A deleted clip keeps every name another app took, of each file it had
/// (clips.rs:415, `let_go_files`: "the names it was handed out by kept if
/// another app took them (`handed_out`), as its project may use them";
/// CHANGELOG.md, 0.7.0 Fixes: "Deleting an edit deletes its clips' files."
/// "unless you dragged one out of VODscribe").
#[test]
fn a_deleted_clip_keeps_the_names_another_app_took() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let before = (show(&app, clip.id), app.clip_media(&clip));
    let longer = resize(&app, clip.id, "0:10", "0:25");
    let after = (show(&app, clip.id), app.clip_media(&longer));
    delete(&app, clip.id);
    for (name, media) in [before, after] {
        assert_eq!(Media::read(&name), Some(media), "{name:?} went");
    }
}

/// A deleted clip nobody took leaves nothing in the clips folder: no name,
/// no names' folder (clips.rs:415, `let_go_files`: "else deleted with it";
/// crates/core/src/lib.rs:314, `Paths::clip_names`: "gone with it").
#[test]
fn a_deleted_clip_nobody_took_leaves_no_names_and_no_names_folder() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    delete(&app, clip.id);
    assert!(!app.paths().clip_names(clip.id).exists(), "its names' folder left");
    assert_eq!(clips_folder(&app), Vec::<String>::new());
}

/// A clip whose file is still in 0.7.1's clip folder (it couldn't be linked
/// into the library) leaves that file there when it goes; its own goes
/// (clips.rs:392, `own_file`: "not one read from 0.7.1's clip folder
/// (`Paths::old_clips`), which stays there"; CHANGELOG.md, 0.7.2: "Clips now
/// live in the library." "the ones already in Videos/VODscribe stay there").
/// At `let_go_files`, below `delete_clip`: a library as 0.7.1 left it can't
/// be made at the command seam yet.
#[test]
fn a_clip_going_leaves_files_outside_the_library() {
    let root = vodscribe_core::testkit::TempDir::new("let-go");
    let paths = vodscribe_core::Paths::new(root.join("lib"));
    let old = root.join("Videos/VODscribe/A talk.mp4");
    let own = paths.clip_file(4, "mp4");
    for file in [&old, &own] {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"clip").unwrap();
    }
    let_go_files(&paths, 3, Some(&old.to_string_lossy()), false);
    assert!(old.is_file(), "0.7.1's file deleted");
    let_go_files(&paths, 4, Some(&own.to_string_lossy()), false);
    assert!(!own.exists(), "its own file left");
}

/// A clip deleted as a drag's receiver asks for its file leaves no name
/// behind: the ask comes wholly before the delete, which then lets go of the
/// name it made, or wholly after, and finds the clip gone (clips.rs:1148,
/// `hand_out_made`: "Named under the library's lock: a delete lets go of the
/// clip's names under it too (`remove`), so it comes wholly before or
/// after. Between them, a name linked after the delete had looked for them
/// stayed, a full-size file that no clip named and nothing removed").
#[test]
fn a_clip_deleted_as_a_drag_hands_it_out_leaves_no_name_behind() {
    for asked_first in [true, false] {
        let app = TestApp::new();
        let vod = app.add_video_file("Day 3.mp4", 60.0);
        let clip = app.make_clip(vod.id, "0:10", "0:20");
        // As the receiver asks, on the main thread (`drag_clip`'s `files`),
        // under a name as `drag_name` makes it. Not one with a ':' in it:
        // Windows can't make that name, so the clip's own file is handed
        // out instead, noted as given, and the delete rightly keeps it.
        let name = "Day 3 00h00m10.000s-00h00m20.000s.mp4";
        let ask = || hand_out_made(&app.state().db, &app.paths(), clip.id, name);
        let given = if asked_first {
            let given = ask();
            delete(&app, clip.id);
            given
        } else {
            delete(&app, clip.id);
            ask()
        };
        assert_eq!(clips_folder(&app), Vec::<String>::new(), "asked first: {asked_first}");
        match given {
            Made::Ready(path) => assert!(asked_first && !path.exists(), "handed out {path:?}, still there"),
            Made::Failed(why) => assert!(!asked_first, "{why}"),
            Made::Making => panic!("still being made"),
        }
    }
}

// A clip whose file was deleted outside the app.

/// Forget the clips whose files went: what the page asks when the window
/// comes back (clip.mjs: "back in the window, its pill goes").
fn back_in_the_window(app: &TestApp, bar: Value) {
    app.invoke("forget_gone_clips", bar).unwrap();
}

/// A clip whose file was deleted outside the app leaves its clip bar
/// (clips.rs:852, `forget_gone_clips`: "Takes off a clip bar … the clips
/// whose file was deleted outside the app (in a file manager)"; CHANGELOG.md,
/// 0.4.1: "A clip whose file you delete outside VODscribe (in a file
/// manager, say) leaves the clip bar too."; clip.mjs: "back in the window,
/// its pill goes").
#[test]
fn a_clip_whose_file_was_deleted_outside_leaves_its_bar() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let gone = app.make_clip(vod.id, "0:10", "0:20");
    let kept = app.make_clip(vod.id, "0:30", "0:40");
    std::fs::remove_file(file_of(&gone)).unwrap();
    back_in_the_window(&app, json!({ "vodId": vod.id }));
    assert_eq!(pills(&app, vod.id), [kept.id]);
    assert_eq!(app.heard("clip-removed"), [json!(gone.id)]);
}

/// Clips on a drive that's away stay (clips.rs:852, `forget_gone_clips`:
/// "Only where the file's folder is still there: a drive that's away
/// (unmounted, unplugged) takes none of them").
#[test]
fn clips_on_a_drive_that_is_away_stay() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    // The clips folder's drive unplugged, and plugged back in after.
    let (folder, away) = (app.paths().clips(), app.paths().clips().with_file_name("clips away"));
    std::fs::rename(&folder, &away).unwrap();
    back_in_the_window(&app, json!({ "vodId": vod.id }));
    std::fs::rename(&away, &folder).unwrap();
    assert_eq!(pills(&app, vod.id), [clip.id]);
    assert!(has_file(&app.clip(clip.id).unwrap()), "its file deleted");
}

/// A clip whose file went outside the app is a step of its own: undone, it
/// comes back made again, and the next undo takes back what came before it
/// (clips.rs:893, in `forget_gone_clips`: "A change of its own in the bar's
/// history: undone, the clips come back (made again), apart from what was
/// done before or after"; CHANGELOG.md, 0.5.0 Fixes: "The clip going is now
/// its own step: undo brings it back (made again), and the next undo takes
/// back the mark."; clip.mjs: "undone, the clip is made again, and the
/// chapter added before that goes only with the next undo").
#[test]
fn a_clip_whose_file_went_is_its_own_step_and_undone_is_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    let chapter = json!({ "vodId": vod.id, "time": 30.0, "kind": "chapter", "text": "Landing" });
    app.invoke("add_mark", chapter).unwrap();
    let chapters = || app.invoke("list_marks", json!({ "vodId": vod.id })).unwrap().as_array().unwrap().len();
    std::fs::remove_file(file_of(&clip)).unwrap();
    back_in_the_window(&app, json!({ "vodId": vod.id }));
    assert_eq!(undo(&app, vod.id)["label"], "Clip's file deleted", "not a step of its own");
    let [back] = &all_made(&app, vod.id)[..] else { panic!("not back: {:?}", bar(&app, vod.id)) };
    assert_eq!(app.clip_media(back), media, "not made again");
    assert_eq!(chapters(), 1, "the chapter undone with it");
    undo(&app, vod.id);
    assert_eq!(chapters(), 0, "the chapter not undone next");
    assert_eq!(bar(&app, vod.id), [(10.0, 20.0)], "the clip undone with the chapter");
}

/// A clip being made again isn't taken for gone, while it's made or waits
/// its turn: its file not there is no file deleted outside the app
/// (clips.rs:895, in `forget_gone_clips`: "Only those as they were when
/// looked at (not made again meanwhile)").
#[test]
fn a_clip_being_made_again_is_not_taken_for_gone() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let making = app.make_clip(vod.id, "0:10", "0:20");
    let waiting = app.make_clip(vod.id, "0:30", "0:40");
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    app.invoke("set_clip_range", json!({ "id": making.id, "start": 10.0, "end": 25.0 })).unwrap();
    halfway.reached();
    app.invoke("set_clip_range", json!({ "id": waiting.id, "start": 30.0, "end": 45.0 })).unwrap();
    // Its file as it was, gone before it's made again in its place.
    std::fs::remove_file(file_of(&waiting)).unwrap();
    back_in_the_window(&app, json!({ "vodId": vod.id }));
    halfway.open();
    assert_eq!(pills(&app, vod.id), [making.id, waiting.id], "taken for gone");
    assert!(all_made(&app, vod.id).iter().all(has_file), "not made again");
}

/// An edit's clip bar lets go of clips whose files went outside the app, as
/// a VOD's does (CHANGELOG.md, 0.5.0: "An edit has a clip bar of its own"
/// "… and leave the bar when their file is deleted outside VODscribe (Ctrl+Z
/// brings them back), as a VOD's do"; clips.rs:852, `forget_gone_clips`:
/// "VOD `vod_id`'s, or edit `edit_id`'s").
#[test]
fn an_edit_s_clip_bar_lets_go_of_clips_whose_files_went_too() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = an_edit(&app, vod.id);
    let clip = app.made(add_edit_clip(&app, edit, 5.0, 9.0));
    std::fs::remove_file(file_of(&clip)).unwrap();
    back_in_the_window(&app, json!({ "editId": edit }));
    assert!(app.db().bar_clips(edit).unwrap().is_empty(), "still on the edit's bar");
    assert_eq!(app.heard("clip-removed"), [json!(clip.id)]);
}

/// Asking to forget the gone clips of neither bar, or of both, is refused
/// (clips.rs:867, `forget_gone_clips`: "Say whose clip bar: a VOD's or an
/// edit's.").
#[test]
fn asking_to_forget_gone_clips_of_neither_or_both_bars_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = an_edit(&app, vod.id);
    let refused = Err(json!("Say whose clip bar: a VOD's or an edit's."));
    assert_eq!(app.invoke("forget_gone_clips", json!({})), refused);
    assert_eq!(app.invoke("forget_gone_clips", json!({ "vodId": vod.id, "editId": edit })), refused);
}

// Clips going with their edit or VOD.

/// Deleting an edit lets go of the clips on its clip bar: out of the
/// library, their making stopped, their files deleted unless another app
/// took them, and the page told so they close in the editor (CHANGELOG.md,
/// 0.7.0 Fixes: "Deleting an edit deletes its clips' files." "The clips on
/// its clip bar go with their files (unless you dragged one out of
/// VODscribe), as with a VOD's clips"; 0.7.0 Fixes: "A deleted clip closes
/// in the editor."; edits.rs:303, `delete_edit`: "the clips on its clip bar
/// go with it, as a VOD's do").
#[test]
fn deleting_an_edit_lets_go_of_its_bar_clips_and_their_files() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let of_vod = app.make_clip(vod.id, "0:40", "0:50");
    let edit = an_edit(&app, vod.id);
    let shown = app.made(add_edit_clip(&app, edit, 1.0, 2.0));
    let taken = show(&app, shown.id);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let making = add_edit_clip(&app, edit, 3.0, 4.0);
    halfway.reached();

    // Not an edit (a VOD's clip): nothing goes.
    assert!(app.invoke("delete_edit", json!({ "id": of_vod.id })).is_err());
    assert_eq!(app.db().bar_clips(edit).unwrap().len(), 2, "its clips went");

    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    halfway.open();
    idle(&app);
    assert!(app.clip(shown.id).is_none() && app.clip(making).is_none(), "still in the library");
    assert!(!file_of(&shown).exists(), "its clip's file left");
    assert!(Media::read(&taken).is_some(), "the name another app took went");
    let left: Vec<_> =
        clips_folder(&app).into_iter().filter(|f| f.split('.').next() == Some(&*making.to_string())).collect();
    assert!(left.is_empty(), "the clip being made left {left:?}");
    let removed = app.heard("clip-removed");
    assert!(removed.contains(&json!(shown.id)) && removed.contains(&json!(making)), "the page not told");
    assert_eq!(pills(&app, vod.id), [of_vod.id], "the VOD's own clip went");
}

/// Removing a VOD deletes its clips' files, unless another app took them
/// (vods.rs:376, `delete_vod`: "its own clip goes, with its marks and its
/// clip bar (the clips' files too, unless dragged out of the app)").
#[test]
fn removing_a_vod_deletes_its_clips_files_unless_another_app_took_them() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let kept = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&kept);
    let taken = show(&app, kept.id);
    let clip = app.make_clip(vod.id, "0:30", "0:40");
    app.invoke("delete_vod", json!({ "id": vod.id })).unwrap();
    assert!(app.clips(vod.id).is_empty(), "its clips stayed");
    assert!(!file_of(&clip).exists(), "a clip's file left");
    assert_eq!(Media::read(&taken), Some(media), "the name another app took went");
}

// Ctrl+X on a clip bar: cut, held for a paste (CHANGELOG.md, 0.7.3: "Cut
// clips wait for a paste. Ctrl+X (⌘X) keeps the clip and its file until you
// paste it, copy or cut again, or quit, so Ctrl+V brings it back at once";
// the owner, walkthrough 2026-11-07: "Shouldn't Ctrl-X be cut? As in it's
// intended to be pasted somewhere? So the video should not actually be
// deleted until the clipboard is erased (through quit or overwritten or
// etc)").

/// What the page writes to the system's clipboard for a copy, in these tests.
const COPIED: &str = "copied";

/// Ctrl+C on pills `ids`, as the page does it (copyPaste.ts `copyPicked`):
/// copied, then the copy's text written.
fn ctrl_c(app: &TestApp, ids: &[i64]) {
    let items: Vec<_> = ids.iter().map(|id| json!({ "kind": "clip", "id": id })).collect();
    app.invoke("copy_items", json!({ "items": items })).unwrap();
    app.invoke("copied_text", json!({ "text": COPIED })).unwrap();
}

/// Ctrl+X on pills `ids` of a VOD's clip bar, as the page does it: copied,
/// then taken off the bar (copyPaste.ts `cutOut`).
fn ctrl_x(app: &TestApp, ids: &[i64]) {
    ctrl_c(app, ids);
    app.invoke("cut_clips", json!({ "ids": ids })).unwrap();
}

/// Ctrl+V on `target`'s clip bar (a VOD's or an edit's, as `Doc` is written):
/// the ids of the clips it made.
fn ctrl_v(app: &TestApp, target: Value) -> Vec<i64> {
    let pasted = app
        .invoke("paste_items", json!({ "target": target, "bar": true, "at": null, "after": null, "text": COPIED }))
        .unwrap();
    pasted["clips"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect()
}

/// A clip cut leaves the clip bar but keeps its clip and file (0.7.3: "Ctrl+X
/// (⌘X) keeps the clip and its file until you paste it").
#[test]
fn a_clip_cut_leaves_the_bar_but_keeps_its_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    ctrl_x(&app, &[clip.id]);
    assert!(pills(&app, vod.id).is_empty(), "still on the bar");
    let held = app.clip(clip.id).expect("the clip deleted");
    assert!(has_file(&held), "its file deleted");
}

/// Clips cut leave their bar at once, as one step that undo brings back
/// whole (plans/0.7.3.md "Changes users see": "Ctrl+X on a pill takes it off
/// the bar"; clips.rs:767, `cut_clips`: "as one change in each clip bar's
/// history, as `delete_clips` does").
#[test]
fn a_clip_cut_leaves_its_bar_at_once_as_one_step() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let a = app.make_clip(vod.id, "0:10", "0:20");
    let b = app.make_clip(vod.id, "0:30", "0:40");
    ctrl_x(&app, &[a.id, b.id]);
    assert!(pills(&app, vod.id).is_empty(), "still on the bar");
    assert_eq!(undo(&app, vod.id)["label"], "Clips cut");
    assert_eq!(pills(&app, vod.id), [a.id, b.id], "one undo");
}

/// A cut clip pasted comes back with its file: what was made, not made
/// again; pasted again, the same (0.7.3: "so Ctrl+V brings it back at
/// once").
#[test]
fn a_cut_clip_pasted_comes_back_with_its_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    ctrl_x(&app, &[clip.id]);
    // Made again, it would fail.
    app.ffmpeg.fails_at(0.5, "the source broke off");
    for _ in 0..2 {
        let pasted = ctrl_v(&app, json!({ "kind": "vod", "id": vod.id }));
        let [id] = pasted[..] else { panic!("pasted {pasted:?}") };
        assert_eq!(app.clip_media(&app.made(id)), media);
    }
}

/// A cut clip pasted gets a file of its own, which stays when the cut clip
/// goes with its file at the next copy (0.7.3: "so Ctrl+V brings it back at
/// once"; clipboard.rs: "pasted, as it was, its copy gets a copy of the file
/// rather than being made again").
#[test]
fn a_cut_clip_pasted_gets_its_own_file_without_being_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let other = app.make_clip(vod.id, "0:30", "0:40");
    let media = app.clip_media(&clip);
    ctrl_x(&app, &[clip.id]);
    app.ffmpeg.fails_at(0.5, "the source broke off");
    let [id] = ctrl_v(&app, json!({ "kind": "vod", "id": vod.id }))[..] else { panic!("pasted not one") };
    let pasted = app.made(id);
    assert_ne!(file_of(&pasted), file_of(&clip), "the cut clip's file shared");
    ctrl_c(&app, &[other.id]);
    assert!(!file_of(&clip).exists(), "the cut clip's file left");
    assert_eq!(app.clip_media(&app.made(id)), media, "the pasted clip's file went with it");
}

/// A piece added to a pasted clip in the editor while its file is copied is
/// in its file: the copy is recorded as of what was copied, not of what the
/// clip holds by then, so the change makes it again (clipboard.rs
/// `copy_file`, as `edits::render_edit` does; the 0.7.3 races review,
/// finding 1). The copy is held once written, before the library is taken,
/// as a large clip on a slow disk would hold it (`TestApp::holds_copies`).
#[test]
fn a_piece_added_as_a_pasted_clip_s_file_is_copied_is_in_its_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    ctrl_c(&app, &[clip.id]);
    let copying = Gate::new();
    app.holds_copies(&copying);
    let [id] = ctrl_v(&app, json!({ "kind": "vod", "id": vod.id }))[..] else { panic!("pasted not one") };
    copying.reached();
    let changed = app.ffmpeg.runs().len();
    app.invoke("add_range_to_edit", json!({ "editId": id, "vodId": vod.id, "start": 30.0, "end": 40.0 })).unwrap();
    copying.open();
    let (made, again) = made_again_since(&app, id, changed);
    assert_eq!(made.status, ClipStatus::Ready, "{:?}", made.error);
    assert!(again, "made and current with its first piece alone: {:?}", made.file_path);
}

/// A cut clip pasted on an edit's clip bar comes back there with its file
/// (0.7.3: "so Ctrl+V brings it back at once").
#[test]
fn a_cut_clip_pasted_on_an_edits_bar_comes_back_with_its_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = an_edit(&app, vod.id);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    ctrl_x(&app, &[clip.id]);
    app.ffmpeg.fails_at(0.5, "the source broke off");
    let pasted = ctrl_v(&app, json!({ "kind": "edit", "id": edit }));
    let [id] = pasted[..] else { panic!("pasted {pasted:?}") };
    let made = app.made(id);
    assert_eq!(made.bar, Some(vodscribe_core::db::ClipBar::Edit { id: edit }));
    assert_eq!(app.clip_media(&made), media);
}

/// A cut clip not pasted goes at the next copy or cut, its file with it, as
/// Delete takes a clip; undo still brings it back, made again (0.7.3: "keeps
/// the clip and its file until you paste it, copy or cut again").
#[test]
fn a_cut_clip_not_pasted_goes_at_the_next_copy() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let other = app.make_clip(vod.id, "0:30", "0:40");
    ctrl_x(&app, &[clip.id]);
    let file = file_of(&app.clip(clip.id).expect("held with its file"));
    ctrl_c(&app, &[other.id]);
    assert!(app.clip(clip.id).is_none(), "still in the library");
    assert!(!file.exists(), "its file left");
    assert!(app.heard("clip-removed").contains(&json!(clip.id)), "the page not told");

    undo(&app, vod.id);
    let back = app.clips(vod.id).into_iter().find(|c| c.shown && c.start == 10.0).expect("undone, back on the bar");
    assert_eq!(app.made(back.id).status, ClipStatus::Ready);
}

/// A cut clip not pasted goes when the app quits, its file with it (0.7.3:
/// "until you paste it, copy or cut again, or quit").
#[test]
fn a_cut_clip_not_pasted_goes_when_the_app_quits() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    ctrl_x(&app, &[clip.id]);
    let file = file_of(&app.clip(clip.id).expect("held with its file"));
    crate::clipboard::quit(app.handle(), &app.db());
    assert!(app.clip(clip.id).is_none(), "still in the library");
    assert!(!file.exists(), "its file left");
}

/// A cut undone brings the pill back with its clip and file, and it stays
/// at the next copy: it's on the bar again, not held (clipboard.rs: "Undo of
/// the cut puts the held clip itself back on its bar (`unhold`)").
#[test]
fn a_cut_undone_brings_the_pill_back_with_its_clip() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let other = app.make_clip(vod.id, "0:30", "0:40");
    let media = app.clip_media(&clip);
    ctrl_x(&app, &[clip.id]);
    app.ffmpeg.fails_at(0.5, "the source broke off");
    undo(&app, vod.id);
    assert_eq!(pills(&app, vod.id), [clip.id, other.id], "the same clip, where it was");
    assert_eq!(app.clip_media(&app.made(clip.id)), media, "its file, not made again");
    ctrl_c(&app, &[other.id]);
    assert_eq!(app.clip_media(&app.made(clip.id)), media, "taken at the next copy");
}

/// A cut redone holds the clip again: off the bar, its file kept until the
/// next copy (clipboard.rs: "so a redone cut is held again").
#[test]
fn a_cut_redone_holds_the_clip_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let other = app.make_clip(vod.id, "0:30", "0:40");
    ctrl_x(&app, &[clip.id]);
    undo(&app, vod.id);
    redo(&app, vod.id);
    assert_eq!(pills(&app, vod.id), [other.id], "redone, off the bar");
    let held = app.clip(clip.id).expect("the clip deleted");
    assert!(has_file(&held), "its file deleted");
    ctrl_c(&app, &[other.id]);
    assert!(app.clip(clip.id).is_none(), "kept past the next copy");
}

/// A clip cut from an edit's clip bar is held too, and pasted comes back
/// with its file (0.7.3: "Cut clips wait for a paste.").
#[test]
fn a_clip_cut_from_an_edits_bar_is_held_too() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = an_edit(&app, vod.id);
    let clip = app.made(add_edit_clip(&app, edit, 5.0, 9.0));
    let media = app.clip_media(&clip);
    ctrl_c(&app, &[clip.id]);
    app.invoke(
        "remove_from_edit",
        json!({ "id": edit, "pieces": [], "stretches": [], "clips": [clip.id], "cut": true }),
    )
    .unwrap();
    let on_bar = || app.db().bar_clips(edit).unwrap().into_iter().filter(|c| c.shown).count();
    assert_eq!(on_bar(), 0, "still on the edit's bar");
    assert!(app.clip(clip.id).is_some_and(|c| has_file(&c)), "its file deleted");
    app.ffmpeg.fails_at(0.5, "the source broke off");
    let pasted = ctrl_v(&app, json!({ "kind": "edit", "id": edit }));
    let [id] = pasted[..] else { panic!("pasted {pasted:?}") };
    assert_eq!(app.clip_media(&app.made(id)), media);
}

/// Undo of a delete brings back the clip deleted, not a cut clip made
/// since, held off another VOD's bar (review of the cut, 2026-11-07).
#[test]
fn undo_of_a_delete_brings_back_that_clip_not_a_cut_one_made_since() {
    let app = TestApp::new();
    let a = app.add_video_file("A.mp4", 60.0);
    let b = app.add_video_file("B.mp4", 60.0);
    let deleted = app.make_clip(a.id, "0:10", "0:20");
    delete(&app, deleted.id);
    let cut = app.make_clip(b.id, "0:30", "0:40");
    ctrl_x(&app, &[cut.id]);
    undo(&app, a.id);
    assert_eq!(bar(&app, a.id), [(10.0, 20.0)], "the deleted clip not back");
    assert!(bar(&app, b.id).is_empty(), "the cut clip came back on its bar");
}

/// Undo past a cut on one bar brings back the clip deleted before it, not
/// the cut one made after: Delete a pill, C a new one, Ctrl+X it, then
/// undo three times (review of the cut, 2026-11-07).
#[test]
fn undo_past_a_cut_brings_back_the_clip_deleted_before_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("A.mp4", 60.0);
    let deleted = app.make_clip(vod.id, "0:10", "0:20");
    delete(&app, deleted.id);
    let cut = app.make_clip(vod.id, "0:30", "0:40");
    ctrl_x(&app, &[cut.id]);
    for _ in 0..3 {
        undo(&app, vod.id);
    }
    assert_eq!(bar(&app, vod.id), [(10.0, 20.0)], "undo past the cut brought back the wrong clip");
}

/// A cut redone after its clip was copied again holds it, as the cut did:
/// the clipboard has its file, so a paste gets that file, and the next copy
/// lets it go (review of the cut, 2026-11-07; the owner's words above).
#[test]
fn a_cut_redone_after_the_clip_was_copied_again_keeps_its_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let other = app.make_clip(vod.id, "0:30", "0:40");
    let media = app.clip_media(&clip);
    ctrl_x(&app, &[clip.id]);
    undo(&app, vod.id);
    ctrl_c(&app, &[clip.id]);
    redo(&app, vod.id);
    assert_eq!(pills(&app, vod.id), [other.id], "redone, off the bar");
    assert!(app.clip(clip.id).is_some_and(|c| has_file(&c)), "the clip the clipboard holds deleted");
    app.ffmpeg.fails_at(0.5, "the source broke off");
    let pasted = ctrl_v(&app, json!({ "kind": "vod", "id": vod.id }));
    let [id] = pasted[..] else { panic!("pasted {pasted:?}") };
    assert_eq!(app.clip_media(&app.made(id)), media);
    ctrl_c(&app, &[other.id]);
    assert!(app.clip(clip.id).is_none(), "kept past the next copy");
}

/// Quitting lets a cut not pasted go while the library still takes writes
/// (lib.rs `quit`, through `quit_library`): its clip and file go.
#[test]
fn quitting_lets_a_cut_go_before_the_library_stops_writing() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    ctrl_x(&app, &[clip.id]);
    let file = file_of(&app.clip(clip.id).expect("held with its file"));
    crate::quit_library(app.handle(), &app.db());
    assert!(!file.exists(), "its file left");
    // As the next start reads it: what's written after the writes stop is never kept.
    let kept = app.state().reads.lock().unwrap().clip(clip.id).unwrap();
    assert!(kept.is_none(), "still in the library");
}

/// A library set aside (recovery.rs: starting a new library "sets the old
/// one aside, its videos and clips with it, never deleting it") keeps its
/// clips' files: they go to the set-aside folder, where the next start, on a
/// new library, deletes none of them (review of the cut, round 2,
/// 2026-11-07).
#[test]
fn a_library_set_aside_keeps_its_clip_files_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let file = PathBuf::from(clip.file_path.clone().expect("made"));
    // Set aside as the app does it, with the library closed: Windows moves
    // no file a program has open.
    let folder = app.quit();
    let paths = folder.paths();
    let aside = vodscribe_core::Paths::new(vodscribe_core::db::set_aside(&paths).unwrap());
    let fresh = Db::open_in(&paths).unwrap();
    tidy_at_start(&paths, &fresh).unwrap();
    let kept = aside.local.join(file.strip_prefix(&paths.local).unwrap());
    assert!(kept.is_file(), "the set-aside library's clip file wasn't kept");
    assert!(!file.exists(), "left where the new library's clip {} goes", clip.id);
}

/// A file the app couldn't delete (on Windows, a cut clip's file a paste's
/// copy still had open as the app quit) goes at the next start; one written
/// anew at its path since stays (files.rs, `Leftovers`).
#[cfg(unix)]
#[test]
fn a_file_that_could_not_be_deleted_goes_at_the_next_start() {
    use std::os::unix::fs::PermissionsExt;
    let dir = vodscribe_core::testkit::TempDir::new("leftovers");
    let folder = dir.join("clips");
    std::fs::create_dir_all(&folder).unwrap();
    let (stuck, rewritten) = (folder.join("5.mp4"), folder.join("6.mp4"));
    std::fs::write(&stuck, b"clip").unwrap();
    std::fs::write(&rewritten, b"clip").unwrap();
    let store = dir.join("leftovers.json");
    let mut running = crate::files::Leftovers::default();
    running.keep_in(store.clone(), &folder);
    // A folder that can't be written to holds its files, as Windows holds one open.
    let mode = |m| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    for file in [&stuck, &rewritten] {
        assert!(std::fs::remove_file(file).is_err(), "deleted after all");
        running.add(file);
    }
    running.retry(); // as the app quits
    mode(0o755);
    assert!(store.is_file(), "nothing kept for the next start");
    drop(running);
    std::fs::write(&rewritten, b"another clip").unwrap();

    let mut next = crate::files::Leftovers::default();
    next.keep_in(store.clone(), &folder);
    assert!(!stuck.exists(), "left after the next start");
    assert!(rewritten.is_file(), "a file written anew at its path deleted");
    assert!(next.listed().is_empty() && !store.exists(), "still listed");
}

/// The list of files left to delete is read from the library's folder, so a
/// library from elsewhere (restored, shared) can carry one of its own: only
/// what it names inside the library's clips folder, where the app deletes,
/// goes at start; a file elsewhere stays, even with its size and time listed
/// (files.rs, `Leftovers::keep_in`).
#[test]
fn a_library_s_list_of_leftovers_deletes_nothing_outside_its_clips() {
    let dir = vodscribe_core::testkit::TempDir::new("leftovers-outside");
    let paths = vodscribe_core::Paths::new(dir.join("library"));
    std::fs::create_dir_all(paths.clips()).unwrap();
    let outside = dir.join("outside").join("notes.txt");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, b"the user's own file").unwrap();
    let meta = std::fs::metadata(&outside).unwrap();
    // As the app writes the list (`Leftover`: path, size, modified).
    let listed = json!([{ "path": outside, "size": meta.len(), "modified": meta.modified().unwrap() }]);
    std::fs::write(paths.leftovers(), serde_json::to_vec(&listed).unwrap()).unwrap();
    let mut list = crate::files::Leftovers::default();
    list.keep_in(paths.leftovers(), &paths.clips());
    assert!(outside.is_file(), "a file outside the library, at {}, was deleted at start", outside.display());
}

/// A clip's delete (or the start's tidy of a hidden one) deletes the names it
/// was handed out by in `clips/<id>/`: never through a link there, so a
/// library from elsewhere whose `clips/<id>` and clip file are links to
/// another folder and a file in it deletes nothing there (`let_go_files`).
#[cfg(unix)]
#[test]
fn a_clip_s_delete_follows_no_link_out_of_the_clips_folder() {
    use std::os::unix::fs::symlink;
    let dir = vodscribe_core::testkit::TempDir::new("names-linked");
    let paths = vodscribe_core::Paths::new(dir.join("library"));
    std::fs::create_dir_all(paths.clips()).unwrap();
    let outside = dir.join("outside").join("id_ed25519");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, b"secret").unwrap();
    // Relative links, as a tar or zip of a library carries them.
    symlink(Path::new("../../outside"), paths.clip_names(5)).unwrap();
    let file = paths.clip_file(5, "mp4");
    symlink(Path::new("../../outside/id_ed25519"), &file).unwrap();
    let_go_files(&paths, 5, Some(&file.to_string_lossy()), false);
    assert!(outside.is_file(), "a file outside the library, at {}, was deleted", outside.display());
}
