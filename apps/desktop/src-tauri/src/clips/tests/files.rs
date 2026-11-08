use super::*;

use serde_json::{Value, json};

use crate::testkit::TestApp;

/// A clip whose file is read from 0.7.1's clip folder (it couldn't be
/// linked in) leaves that file there when it goes; its own goes.
#[test]
fn a_clip_going_leaves_files_outside_the_library() {
    let root = std::env::temp_dir().join(format!("vodscribe-let-go-{}", std::process::id()));
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
    assert!(!own.exists());
    std::fs::remove_dir_all(&root).unwrap();
}

/// A drag's receiver asked for a clip's file (its name made beside it,
/// `handed_out`) as the clip was deleted: the name was linked after the
/// delete had looked for names to remove, and a full-size copy stayed in
/// the clips folder that nothing named or removed.
#[test]
fn a_clip_deleted_as_a_drag_hands_it_out_leaves_no_name_behind() {
    use std::sync::atomic::AtomicU64;
    static N: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "vodscribe-hand-out-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let paths = vodscribe_core::Paths::new(&root);
    std::fs::create_dir_all(&root).unwrap();
    let db = Db::open(&root.join("library.sqlite")).unwrap();
    let vod = db.add_vod(&info()).unwrap().0.id;
    let clip = db.add_clip(vod, 1.0, 2.0, "c", true).unwrap();
    let file = db.files().clip_file(clip.id, "mp4");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"clip").unwrap();
    db.set_clip_file(clip.id, 1.0, 2.0, ClipStatus::Ready, Some(&file.to_string_lossy()), None).unwrap();
    let db = Arc::new(Mutex::new(db));
    let asking = {
        let (db, paths) = (db.clone(), paths.clone());
        std::thread::spawn(move || hand_out_made(&db, &paths, clip.id, "c 0:01.mp4"))
    };
    // Deleted while the receiver's ask is under way, as `remove` does.
    std::thread::sleep(Duration::from_millis(100));
    {
        let db = db.lock().unwrap();
        let path = db.clip(clip.id).unwrap().and_then(|c| c.file_path);
        db.delete_clip(clip.id).unwrap();
        let_go_files(&paths, clip.id, path.as_deref(), false);
    }
    let given = asking.join().unwrap();
    let left: Vec<_> = std::fs::read_dir(paths.clip_names(clip.id)).into_iter().flatten().flatten().collect();
    assert!(left.is_empty(), "left in the clips folder: {left:?}");
    assert!(!paths.clip_names(clip.id).exists(), "its names' folder is left");
    // Given only if the ask came first; then it was deleted with the clip.
    if let Made::Ready(path) = given {
        assert!(!path.exists(), "handed out {path:?}, still there");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Deleting an edit took the clips on its clip bar out of the library
/// (the database's cascade) but left their files in the clips folder,
/// their cuts running and the page not told: they go as a VOD's do.
#[test]
fn deleting_an_edit_releases_its_bar_clips() {
    let (db, vod) = library();
    let of_vod = db.add_clip(vod, 1.0, 2.0, "v", true).unwrap().id;
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
    let a = db.add_edit_clip(edit, 1.0, 2.0, "a", true).unwrap().id;
    let b = db.add_edit_clip(edit, 3.0, 4.0, "b", true).unwrap();
    let file = db.files().clip_file(b.id, "mp4").to_string_lossy().into_owned();
    assert!(db.set_clip_file(b.id, b.start, b.end, ClipStatus::Ready, Some(&file), None).unwrap());
    let b = b.id;
    db.hand_out_clip(b).unwrap();
    let stopped = std::cell::RefCell::new(Vec::new());
    let stop = |id| stopped.borrow_mut().push(id);

    // Not an edit (a VOD's own clip): nothing goes, none stopped.
    assert!(crate::edits::delete_edit_in(&db, of_vod, stop).is_err());
    assert_eq!(db.bar_clips(edit).unwrap().len(), 2);
    assert!(stopped.borrow().is_empty());

    let gone = crate::edits::delete_edit_in(&db, edit, stop).unwrap();
    let clips: Vec<_> = gone.clips.iter().map(|(c, out)| (c.id, c.file_path.as_deref(), *out)).collect();
    assert_eq!(clips, [(a, None, false), (b, Some(&*file), true)], "its clips, b dragged out");
    assert_eq!(*stopped.borrow(), [a, b], "their cuts stopped");
    assert!(db.revision(edit).unwrap().is_none() && db.clip(a).unwrap().is_none() && db.clip(b).unwrap().is_none());
    assert_eq!(db.clips(vod).unwrap().len(), 1, "the VOD's own clip stays");
}

/// Several clips deleted at once went one by one, a clip that was gone
/// skipped: one that couldn't go, in the middle, left those before it
/// deleted with no step to undo them.
#[test]
fn deleting_several_clips_is_all_or_none() {
    let (db, vod) = library();
    let a = db.add_clip(vod, 1.0, 2.0, "a", true).unwrap().id;
    let b = db.add_clip(vod, 3.0, 4.0, "b", true).unwrap().id;
    let history = history::History::default();
    let stopped = std::cell::RefCell::new(Vec::new());
    let stop = |id| stopped.borrow_mut().push(id);
    assert!(delete_clips_in(&db, &history, &[a, 9999, b], stop).is_err());
    assert_eq!(db.clips(vod).unwrap().len(), 2, "every clip in place");
    assert_eq!(history.steps(Doc::Vod(vod)), 0, "no step recorded");
    assert!(stopped.borrow().is_empty(), "none stopped");

    let gone = delete_clips_in(&db, &history, &[b, a], stop).unwrap();
    assert_eq!(gone.iter().map(|(c, _)| c.id).collect::<Vec<_>>(), [b, a]);
    assert!(db.clips(vod).unwrap().is_empty());
    assert_eq!(history.steps(Doc::Vod(vod)), 1, "one step for both");
}

// Ctrl+X on a clip bar: cut, held for a paste (owner, walkthrough
// 2026-11-07: "Shouldn't Ctrl-X be cut? As in it's intended to be pasted
// somewhere? So the video should not actually be deleted until the
// clipboard is erased (through quit or overwritten or etc)").

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

/// The ids of the pills on VOD `vod_id`'s clip bar.
fn pills(app: &TestApp, vod_id: i64) -> Vec<i64> {
    app.clips(vod_id).into_iter().filter(|c| c.shown).map(|c| c.id).collect()
}

/// Whether clip `clip`'s file is there.
fn has_file(clip: &Clip) -> bool {
    clip.file_path.as_deref().is_some_and(|f| Path::new(f).is_file())
}

/// A clip cut leaves the clip bar but keeps its clip and file.
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

/// A cut clip pasted comes back with its file: what was made, not made
/// again; pasted again, the same.
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

/// A cut clip pasted on an edit's clip bar comes back there with its file.
#[test]
fn a_cut_clip_pasted_on_an_edits_bar_comes_back_with_its_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = {
        let db = app.db();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod.id, (0.0, 30.0), "a", None).unwrap();
        edit
    };
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
/// Delete takes a clip; undo still brings it back, made again.
#[test]
fn a_cut_clip_not_pasted_goes_at_the_next_copy() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let other = app.make_clip(vod.id, "0:30", "0:40");
    ctrl_x(&app, &[clip.id]);
    let file = PathBuf::from(app.clip(clip.id).and_then(|c| c.file_path).expect("held with its file"));
    ctrl_c(&app, &[other.id]);
    assert!(app.clip(clip.id).is_none(), "still in the library");
    assert!(!file.exists(), "its file left");
    assert!(app.heard("clip-removed").contains(&json!(clip.id)), "the page not told");

    app.invoke("undo", json!({ "view": { "kind": "vod", "id": vod.id } })).unwrap();
    let back = app.clips(vod.id).into_iter().find(|c| c.shown && c.start == 10.0).expect("undone, back on the bar");
    assert_eq!(app.made(back.id).status, ClipStatus::Ready);
}

/// A cut clip not pasted goes when the app quits, its file with it.
#[test]
fn a_cut_clip_not_pasted_goes_when_the_app_quits() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    ctrl_x(&app, &[clip.id]);
    let file = PathBuf::from(app.clip(clip.id).and_then(|c| c.file_path).expect("held with its file"));
    crate::clipboard::quit(app.handle(), &app.db());
    assert!(app.clip(clip.id).is_none(), "still in the library");
    assert!(!file.exists(), "its file left");
}

/// A cut undone brings the pill back with its clip and file, and it stays
/// at the next copy: it's on the bar again, not held.
#[test]
fn a_cut_undone_brings_the_pill_back_with_its_clip() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let other = app.make_clip(vod.id, "0:30", "0:40");
    let media = app.clip_media(&clip);
    ctrl_x(&app, &[clip.id]);
    app.ffmpeg.fails_at(0.5, "the source broke off");
    app.invoke("undo", json!({ "view": { "kind": "vod", "id": vod.id } })).unwrap();
    assert_eq!(pills(&app, vod.id), [clip.id, other.id], "the same clip, where it was");
    assert_eq!(app.clip_media(&app.made(clip.id)), media, "its file, not made again");
    ctrl_c(&app, &[other.id]);
    assert_eq!(app.clip_media(&app.made(clip.id)), media, "taken at the next copy");
}

/// A cut redone holds the clip again: off the bar, its file kept until the
/// next copy.
#[test]
fn a_cut_redone_holds_the_clip_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let other = app.make_clip(vod.id, "0:30", "0:40");
    ctrl_x(&app, &[clip.id]);
    let view = json!({ "view": { "kind": "vod", "id": vod.id } });
    app.invoke("undo", view.clone()).unwrap();
    app.invoke("redo", view).unwrap();
    assert_eq!(pills(&app, vod.id), [other.id], "redone, off the bar");
    let held = app.clip(clip.id).expect("the clip deleted");
    assert!(has_file(&held), "its file deleted");
    ctrl_c(&app, &[other.id]);
    assert!(app.clip(clip.id).is_none(), "kept past the next copy");
}

/// A clip cut from an edit's clip bar is held too, and pasted comes back
/// with its file.
#[test]
fn a_clip_cut_from_an_edits_bar_is_held_too() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = {
        let db = app.db();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod.id, (0.0, 30.0), "a", None).unwrap();
        edit
    };
    let made = app.invoke("add_edit_clip", json!({ "editId": edit, "from": 5.0, "to": 9.0 })).unwrap();
    let clip = app.made(made["id"].as_i64().unwrap());
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
    app.invoke("delete_clip", json!({ "id": deleted.id })).unwrap();
    let cut = app.make_clip(b.id, "0:30", "0:40");
    ctrl_x(&app, &[cut.id]);
    app.invoke("undo", json!({ "view": { "kind": "vod", "id": a.id } })).unwrap();
    let on =
        |vod: i64| -> Vec<_> { app.clips(vod).into_iter().filter(|c| c.shown).map(|c| (c.start, c.end)).collect() };
    assert_eq!(on(a.id), [(10.0, 20.0)], "the deleted clip not back");
    assert!(on(b.id).is_empty(), "the cut clip came back on its bar");
}

/// Undo past a cut on one bar brings back the clip deleted before it, not
/// the cut one made after: Delete a pill, C a new one, Ctrl+X it, then
/// undo three times (review of the cut, 2026-11-07).
#[test]
fn undo_past_a_cut_brings_back_the_clip_deleted_before_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("A.mp4", 60.0);
    let deleted = app.make_clip(vod.id, "0:10", "0:20");
    app.invoke("delete_clip", json!({ "id": deleted.id })).unwrap();
    let cut = app.make_clip(vod.id, "0:30", "0:40");
    ctrl_x(&app, &[cut.id]);
    let view = json!({ "view": { "kind": "vod", "id": vod.id } });
    for _ in 0..3 {
        app.invoke("undo", view.clone()).unwrap();
    }
    let on: Vec<_> = app.clips(vod.id).into_iter().filter(|c| c.shown).map(|c| (c.start, c.end)).collect();
    assert_eq!(on, [(10.0, 20.0)], "undo past the cut brought back the wrong clip");
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
    let view = json!({ "view": { "kind": "vod", "id": vod.id } });
    app.invoke("undo", view.clone()).unwrap();
    ctrl_c(&app, &[clip.id]);
    app.invoke("redo", view).unwrap();
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
    let file = PathBuf::from(app.clip(clip.id).and_then(|c| c.file_path).expect("held with its file"));
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
/// anew at its path since stays.
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
    running.keep_in(store.clone());
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
    next.keep_in(store.clone());
    assert!(!stuck.exists(), "left after the next start");
    assert!(rewritten.is_file(), "a file written anew at its path deleted");
    assert!(next.listed().is_empty() && !store.exists(), "still listed");
}
