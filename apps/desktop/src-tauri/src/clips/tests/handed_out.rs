//! A file handed out to another app stays when its clip goes: where no
//! second name could be made for it (a disk that takes no hard links, such
//! as exFAT, on a drag; a names folder that can't be made, on Show in
//! folder), the clip's own file is what went, and deleting the clip deleted
//! it from under the app whose project used it (clips pilot plan,
//! disagreement 4).
//!
//! Sources, each test citing the one it states:
//! - `Db::hand_out_clip`: "it's someone else's now, so it stays when the clip goes";
//! - `delete_clip`: "and its file unless that was dragged out of the app";
//! - `handed_out`: "The clip's own file if none can be made";
//! - `show_clip`: "from there it's the user's to take".
//!
//! Each make of a clip (or render of an edit) writes a file of its own
//! (`Paths::clip_make`; owner, 2026-11-07, plans/clips-pilot.md "Decided"
//! 4b): one made again at the path another app was given wrote over it.

use super::*;

use serde_json::json;
use vodscribe_core::settings::ClipCodec;

use vodscribe_core::testkit::Media;

use crate::testkit::{TestApp, at};

fn file_of(clip: &Clip) -> PathBuf {
    PathBuf::from(clip.file_path.clone().expect("a made clip's file"))
}

/// Clips and edits are made as `codec` from now (Settings › Encoding).
fn codec(app: &TestApp, codec: ClipCodec) {
    let mut settings = vodscribe_core::settings::Settings::load(&app.paths().settings());
    settings.clip_codec = Some(codec);
    settings.normalized().save(&app.paths().settings()).unwrap();
}

/// Clip `clip` moved to `from`..`to`, as a resize does, and made again.
fn resized(app: &TestApp, clip: &Clip, from: &str, to: &str) -> Clip {
    app.invoke("set_clip_range", json!({ "id": clip.id, "start": at(from), "end": at(to) })).unwrap();
    app.made(clip.id)
}

/// The file at `path` as another app sees it: its bytes, and on unix which
/// file it is (its inode), so one written anew in its place shows.
fn as_given(path: &Path) -> (Vec<u8>, Option<u64>) {
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| m.ino())
    };
    #[cfg(not(unix))]
    let inode = None;
    (std::fs::read(path).unwrap_or_default(), inode)
}

/// Edit `edit`'s video once it's rendered anew (another file than `before`).
fn rendered(app: &TestApp, edit: i64, before: Option<&Path>) -> PathBuf {
    let ready = || {
        let now = crate::edits::made_now(app.handle());
        let edit = app.db().edit(edit, now).unwrap().filter(|e| e.status == ClipStatus::Ready)?;
        edit.file_path.map(PathBuf::from).filter(|f| Some(f.as_path()) != before && f.is_file())
    };
    vodscribe_core::testkit::eventually("the edit rendered", || ready().is_some());
    ready().unwrap()
}

/// An edit of `vod`'s first 30 s, rendered, and its video shown where no
/// name can be made (a file where its names' folder goes): that video
/// itself is what went.
fn edit_shown_itself(app: &TestApp, vod: i64) -> (i64, PathBuf) {
    let edit = app.db().add_edit("Best of").unwrap().id;
    app.db().add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let video = rendered(app, edit, None);
    std::fs::write(app.paths().clip_names(edit), b"not a folder").unwrap();
    app.invoke("show_edit", json!({ "id": edit })).unwrap();
    assert_eq!(app.desktop().shown().pop().as_ref(), Some(&video), "no name could be made");
    (edit, video)
}

/// On a disk that takes no hard links a drag hands out the clip's own file,
/// not a copy (`linked_out`: a copy on the main thread froze the window).
/// Deleted, the clip leaves that file for the app that took it
/// (`Db::hand_out_clip`; `delete_clip`).
#[test]
fn a_clip_dragged_out_where_the_disk_takes_no_links_keeps_that_file_when_deleted() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    assert_eq!(given, file_of(&clip), "a drag here hands out the clip's own file");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(app.clip(clip.id).is_none());
    assert!(given.is_file(), "the file another app took was deleted with its clip");
}

/// As above, then the delete undone: the clip comes back under its own id
/// and is made again as a file of its own, never written over the one the
/// other app took (`Paths::clip_make`: "every make writes a file of its
/// own, so none rewrites the one before, which another app may have been
/// given").
#[test]
fn a_clip_deleted_and_undone_is_made_again_beside_the_file_another_app_took() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    let was = as_given(&given);
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    app.invoke("undo", json!({ "view": { "kind": "vod", "id": vod.id } })).unwrap();
    let back = app.made(clip.id);
    assert_ne!(file_of(&back), given, "made again at the path another app took");
    assert!(as_given(&given) == was, "the file another app took was written over");
}

/// The same drag where the disk takes links hands out a name of its own,
/// which stays; the clip's own file goes with it (`handed_out`).
#[test]
fn a_clip_dragged_out_where_the_disk_takes_links_keeps_only_its_name() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    assert!(given.starts_with(app.paths().clip_names(clip.id)), "handed out as {given:?}");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(given.is_file(), "the name another app took");
    assert!(!file_of(&clip).exists(), "the clip's own file stayed");
}

/// Show in folder where no name can be made beside the clip's file (its
/// names' folder can't be made: a file is in its place) shows the clip's own
/// file; from there it's the user's to take, so it stays when the clip goes
/// (`show_clip`).
#[test]
fn a_clip_shown_where_no_name_can_be_made_keeps_that_file_when_deleted() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    std::fs::write(app.paths().clip_names(clip.id), b"not a folder").unwrap();
    app.invoke("show_clip", json!({ "id": clip.id })).unwrap();
    let shown = app.desktop().shown().pop().expect("shown in the file manager");
    assert_eq!(shown, file_of(&clip), "no name could be made");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(shown.is_file(), "the file shown was deleted with its clip");
}

/// Show in folder on a disk that takes no hard links shows a copy under the
/// clip's name ("a copy where it doesn't"), which stays; the clip's own file
/// goes with it (`handed_out`).
#[test]
fn a_clip_shown_where_the_disk_takes_no_links_is_a_copy_that_stays() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.invoke("show_clip", json!({ "id": clip.id })).unwrap();
    let shown = app.desktop().shown().pop().expect("shown in the file manager");
    assert!(shown.starts_with(app.paths().clip_names(clip.id)), "shown as {shown:?}");
    assert_eq!(Media::read(&shown), Media::read(&file_of(&clip)), "a copy of the clip");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(shown.is_file(), "the copy shown");
    assert!(!file_of(&clip).exists(), "the clip's own file stayed");
}

/// A stretch dragged straight out of the window is a clip made only for the
/// drag, forgotten at the next start (`tidy_at_start`): its own file,
/// handed out where the disk takes no links, stays.
#[test]
fn a_clip_made_only_for_a_drag_keeps_the_file_it_handed_out_at_the_next_start() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.run(crate::clips::add_clip(app.handle().clone(), vod.id, at("0:10"), at("0:20"), Some(false)));
    let clip = app.made(clip.unwrap().id);
    let given = app.dragged_out(&clip);
    let app = TestApp::on(app.quit());
    assert!(app.clip(clip.id).is_none(), "forgotten at the start");
    assert!(given.is_file(), "the file another app took was deleted at the next start");
}

/// Several clips deleted at once leave the file another app was given, as
/// one does (`delete_clips`, `delete_clip`).
#[test]
fn clips_deleted_together_keep_the_file_another_app_was_given() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let a = app.make_clip(vod.id, "0:10", "0:20");
    let b = app.make_clip(vod.id, "0:30", "0:40");
    let given = app.dragged_out(&a);
    app.invoke("delete_clips", json!({ "ids": [a.id, b.id] })).unwrap();
    assert!(app.clips(vod.id).is_empty());
    assert!(given.is_file(), "the file another app took was deleted with its clip");
    assert!(!file_of(&b).exists(), "the clip nobody took kept its file");
}

/// Removing a VOD deletes its clips' files unless another app took them
/// (vods.rs `delete_vod`).
#[test]
fn a_vod_removed_keeps_the_clip_file_another_app_was_given() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    app.invoke("delete_vod", json!({ "id": vod.id, "anyway": true })).unwrap();
    assert!(app.clip(clip.id).is_none(), "its clip is still there");
    assert!(given.is_file(), "the file another app took was deleted with its VOD");
}

/// A clip made again in another format (codec changed in Settings, made
/// again as the page asks by its range) deletes its file in the format
/// before (`cut`), but not one another app was given: the old name stays for
/// the editor whose project uses it (`handed_out`). The new one is the
/// app's own, and goes with the clip.
#[test]
fn a_clip_made_again_in_another_format_keeps_the_file_another_app_was_given() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    let mut settings = vodscribe_core::settings::Settings::load(&app.paths().settings());
    settings.clip_codec = Some(ClipCodec::Prores);
    settings.normalized().save(&app.paths().settings()).unwrap();
    app.invoke("set_clip_range", json!({ "id": clip.id, "start": clip.start, "end": clip.end })).unwrap();
    let again = app.made(clip.id);
    let new = file_of(&again);
    assert_ne!(new, given, "made again as a MOV");
    assert!(given.is_file(), "the file another app took was deleted as its clip was made again");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(given.is_file(), "the file another app took was deleted with its clip");
    assert!(!new.exists(), "the clip's new file, nobody's but the app's, stayed");
}

/// A file the app couldn't delete is tried again later (files.rs
/// `Leftovers`): one another app was given is never on that list, so it
/// isn't deleted once that app lets go of it either (on Windows, a file in
/// use can't be deleted, and goes once it's let go).
#[cfg(unix)]
#[test]
fn a_file_another_app_was_given_is_never_left_to_delete_later() {
    use std::os::unix::fs::PermissionsExt;
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    // A folder that can't be written to holds its files, as Windows holds one open.
    let clips = app.paths().clips();
    let mode = |m| std::fs::set_permissions(&clips, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    let deleted = app.invoke("delete_clip", json!({ "id": clip.id }));
    mode(0o755);
    deleted.unwrap();
    assert!(!crate::files::left_to_delete().contains(&given), "listed to delete later");
    crate::files::delete_leftovers();
    assert!(given.is_file(), "the file another app took was deleted");
}

/// An edit's video is handed out as a clip's is (edits.rs `show_edit`,
/// `clips::handed_out`): shown where no name can be made, it's the edit's own
/// file, which stays when the edit goes (`delete_edit`).
#[test]
fn an_edit_shown_where_no_name_can_be_made_keeps_that_video_when_deleted() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = app.db().add_edit("Best of").unwrap().id;
    app.db().add_piece(edit, vod.id, (0.0, 30.0), "a", None).unwrap();
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let rendered = || app.db().edit(edit, Default::default()).unwrap().filter(|e| e.status == ClipStatus::Ready);
    vodscribe_core::testkit::eventually("the edit rendered", || rendered().is_some());
    let video = PathBuf::from(rendered().and_then(|e| e.file_path).expect("its video"));
    std::fs::write(app.paths().clip_names(edit), b"not a folder").unwrap();
    app.invoke("show_edit", json!({ "id": edit })).unwrap();
    let shown = app.desktop().shown().pop().expect("shown in the file manager");
    assert_eq!(shown, video, "no name could be made");
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    assert!(shown.is_file(), "the video shown was deleted with its edit");
}

/// A clip made again in the format it was (resized) is a new file: the one
/// another app was given stays as it was, the same file byte for byte, and
/// the clip plays its new one (`cut`, `Db::next_clip_file`; plans/clips-pilot.md
/// "Decided" 4b).
#[test]
fn a_clip_made_again_in_the_same_format_keeps_the_file_another_app_was_given() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    let before = as_given(&given);
    let again = resized(&app, &clip, "0:30", "0:35");
    assert_eq!(as_given(&given), before, "the file another app took was written over");
    let new = file_of(&again);
    assert_ne!(new, given, "made again at the path another app has");
    let media = app.clip_media(&again);
    let new_range =
        media.cut_from.as_ref().is_some_and(|c| (c.1 - 30.0).abs() < 0.01) && (media.duration - 5.0).abs() < 0.01;
    assert!(new_range, "the clip plays {media:?}, not 0:30 to 0:35");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert_eq!(as_given(&given), before, "the file another app took was deleted with its clip");
    assert!(!new.exists(), "the clip's new file, nobody's but the app's, stayed");
}

/// Made again as a MOV and then as an MP4 again, the clip never comes back
/// to the path another app was given: that file stays as it was through both,
/// and after the clip is deleted (the review's R2: the second remake wrote
/// over it and the delete took it).
#[test]
fn a_clip_made_again_as_a_mov_and_back_keeps_the_file_another_app_was_given() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    let before = as_given(&given);
    codec(&app, ClipCodec::Prores);
    let mov = resized(&app, &clip, "0:10", "0:20");
    codec(&app, ClipCodec::H264);
    let mp4 = resized(&app, &mov, "0:10", "0:20");
    assert_ne!(file_of(&mp4), given, "made again at the path another app has");
    assert!(file_of(&mp4).is_file() && !file_of(&mov).exists(), "plays its newest file, the MOV gone");
    assert_eq!(as_given(&given), before, "the file another app took was written over");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert_eq!(as_given(&given), before, "the file another app took was deleted with its clip");
}

/// An edit rendered again as it was is a new video: the one another app was
/// given stays as it was, and the edit plays its new one (render_edit,
/// `Db::next_clip_file`; plans/clips-pilot.md "Decided" 4b: "Edits' renders
/// the same").
#[test]
fn an_edit_rendered_again_in_the_same_format_keeps_the_video_another_app_was_given() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (edit, given) = edit_shown_itself(&app, vod.id);
    let before = as_given(&given);
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let new = rendered(&app, edit, Some(&given));
    assert_eq!(as_given(&given), before, "the video another app took was written over");
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    assert_eq!(as_given(&given), before, "the video another app took was deleted with its edit");
    assert!(!new.exists(), "the edit's new video, nobody's but the app's, stayed");
}

/// An edit rendered as a MOV and then as an MP4 again never comes back to
/// the path another app was given (the review's R2, for an edit).
#[test]
fn an_edit_rendered_as_a_mov_and_back_keeps_the_video_another_app_was_given() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (edit, given) = edit_shown_itself(&app, vod.id);
    let before = as_given(&given);
    codec(&app, ClipCodec::Prores);
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let mov = rendered(&app, edit, Some(&given));
    codec(&app, ClipCodec::H264);
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let mp4 = rendered(&app, edit, Some(&mov));
    assert_ne!(mp4, given, "rendered again at the path another app has");
    assert!(!mov.exists(), "the MOV between them stayed");
    assert_eq!(as_given(&given), before, "the video another app took was written over");
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    assert_eq!(as_given(&given), before, "the video another app took was deleted with its edit");
}

/// An edit deleted as its render in another format ends: the delete keeps
/// the video another app was given, and the render, finding the edit gone,
/// leaves it too (the review's R1). The render is held at a `Gate` and
/// `delete_edit`'s steps before it stops the render run while it's held.
#[test]
fn an_edit_deleted_as_its_render_ends_keeps_the_video_another_app_was_given() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (edit, given) = edit_shown_itself(&app, vod.id);
    codec(&app, ClipCodec::Prores);
    let held = vodscribe_core::testkit::Gate::new();
    app.ffmpeg.holds_at(0.5, &held);
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    held.reached();
    // `delete_edit` up to its `state.clips.cancel(id)`.
    let gone = crate::edits::delete_edit_in(&app.db(), edit, |_| {}).unwrap();
    crate::clips::let_go_files(&app.paths(), edit, gone.file.as_deref(), gone.handed_out);
    held.open();
    vodscribe_core::testkit::eventually("the render ended", || app.state().clips.listed().0.is_none());
    assert!(given.is_file(), "the video another app was given was deleted by the render that ended");
}

/// A clip made again while a drag of its own file is under way: once the
/// drop is over, only the file the drag gave is another app's; the clip's
/// new one is the app's, and goes with it (the review's R3: the file the drag
/// gave is noted as it's given, `hand_out_made`, and the remake takes the
/// clip back, `cut`).
#[test]
fn a_clip_made_again_during_its_drag_lets_its_new_file_go_with_it() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let name = drag_name(&app.db(), vodscribe_core::settings::ClipFormat::Mp4, clip.id);
    let drag = DragOut::new(app.handle().clone(), vec![(clip.id, name)]);
    let Some(Made::Ready(given)) = drag.files().into_iter().next() else { panic!("not handed out") };
    assert_eq!(given, file_of(&clip));
    let again = resized(&app, &clip, "0:30", "0:35");
    drag.taken();
    let new = file_of(&again);
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(!new.exists(), "the clip's new file, nobody else's, stayed as if another app had it");
}

/// A clip whose own file went, shown in its folder later (a copy under its
/// name, `hand_out_clip`), still keeps that file: the name handed out after
/// doesn't take back the file before it (`Db::hand_out_clip`'s
/// `max(handed_out, 1)`; the review's R5a).
#[test]
fn a_name_handed_out_after_the_clip_s_own_file_keeps_that_file() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    app.invoke("show_clip", json!({ "id": clip.id })).unwrap();
    let shown = app.desktop().shown().pop().unwrap();
    assert!(shown.starts_with(app.paths().clip_names(clip.id)), "a copy: {shown:?}");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(given.is_file(), "the file another app took was deleted after a later Show in folder");
}

/// The same for an edit: its video itself went (Show in folder, no name
/// could be made), then shown under its name once one can be: the video
/// stays when the edit goes (`Db::hand_out_edit`'s `max(handed_out, 1)`).
#[test]
fn a_name_handed_out_after_the_edit_s_own_video_keeps_that_video() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (edit, given) = edit_shown_itself(&app, vod.id);
    std::fs::remove_file(app.paths().clip_names(edit)).unwrap();
    app.invoke("show_edit", json!({ "id": edit })).unwrap();
    let shown = app.desktop().shown().pop().unwrap();
    assert!(shown.starts_with(app.paths().clip_names(edit)), "under its name: {shown:?}");
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    assert!(given.is_file(), "the video another app took was deleted after a later Show in folder");
}

/// The files (not folders) in `dir`, by name.
fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A make the library didn't record (the clip was resized as its ffmpeg
/// ended, so it's made again) is nobody's: it goes, and doesn't stay in the
/// clips folder once the clip is deleted (`cut`; the re-review's RR1). The
/// window, ffmpeg done and the library not yet taken, is held open by
/// holding the library as ffmpeg ends. `set_clip_range`'s stop lands on that
/// make, whose cut queues the clip again; the remake begins before the
/// command's own queue, so the clip is both being made and waiting, and its
/// delete takes it out of the queue but leaves the remake to end (CI run
/// 43055187212, where this order came by chance).
#[test]
fn a_make_the_library_did_not_record_goes() {
    use vodscribe_core::testkit::{Gate, eventually};
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let held = Gate::new();
    app.ffmpeg.holds_at(0.5, &held);
    app.invoke("set_clip_range", json!({ "id": clip.id, "start": at("0:30"), "end": at("0:35") })).unwrap();
    held.reached();
    let second = app.paths().clip_make(clip.id, 2, "mp4");
    let next = Gate::new();
    {
        let db = app.db();
        held.open();
        eventually("the make's file written", || second.is_file());
        // `set_clip_range` as ffmpeg ended: the range in the library, and
        // its stop while the cut waits for the library.
        db.set_clip_range(clip.id, at("0:40"), at("0:45")).unwrap();
        app.ffmpeg.holds_at(0.5, &next);
        app.state().clips.stop(clip.id);
    }
    // The cut, queuing the clip again, has ended, and the remake has begun
    // (its file, the next unrecorded make, is the same path) ...
    next.reached();
    // ... before the command's queue.
    app.state().clips.queue_once(clip.id);
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    next.open();
    eventually("the clips' queue idle", || {
        let (running, waiting) = app.state().clips.listed();
        running.is_none() && waiting.is_empty()
    });
    let left = files_in(&app.paths().clips());
    assert!(!second.exists(), "a make never recorded, nobody's, stays after its clip is deleted: {left:?}");
}

/// A clip deleted as its make ends, its encode over and the library not yet
/// told, leaves no file: that make is nobody's (`cut`: "Deleted while it was
/// being cut, or moved as ffmpeg ended (it's queued again): this make is
/// nobody's, and none will record it."). The delete's stop comes too late to
/// stop ffmpeg, so it's the cut's end that finds the clip gone.
#[test]
fn a_clip_deleted_as_its_make_ends_leaves_no_file() {
    use vodscribe_core::testkit::{Gate, eventually};
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let ended = Gate::new();
    app.holds_cut_ends(&ended);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    ended.reached();
    let made = app.paths().clip_make(clip.id, 1, "mp4");
    assert!(made.is_file(), "its encode over");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    ended.open();
    eventually("the clips' queue idle", || app.state().clips.listed() == (None, Vec::new()));
    let left = files_in(&app.paths().clips());
    assert!(!made.exists(), "the make of a clip deleted as it ended stays: {left:?}");
}

/// Clip `clip`, whose own file `given` another app took, made again by
/// `remake`, then deleted as that make ends and the delete undone before the
/// make is recorded: the make is recorded for the clip brought back (it's
/// of what that clip is), and the file the other app took stays
/// (`Db::hand_out_clip`: "it's someone else's now, so it stays when the clip
/// goes"). The make let go of the file the clip had as it began, which the
/// clip brought back never had.
fn deleted_as_its_remake_ends_and_undone(app: &TestApp, vod: i64, clip: i64, given: &Path, remake: impl Fn()) {
    use vodscribe_core::testkit::Gate;
    let ended = Gate::new();
    app.holds_cut_ends(&ended);
    remake();
    ended.reached();
    app.invoke("delete_clip", json!({ "id": clip })).unwrap();
    app.invoke("undo", json!({ "view": { "kind": "vod", "id": vod } })).unwrap();
    ended.open();
    app.made(clip);
    assert!(given.is_file(), "the file another app took was deleted");
}

/// As above, for a clip cut (one piece as it came: `clips::cut`).
#[test]
fn a_clip_deleted_as_its_cut_ends_and_undone_keeps_the_file_another_app_took() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    deleted_as_its_remake_ends_and_undone(&app, vod.id, clip.id, &given, || {
        app.invoke("set_clip_range", json!({ "id": clip.id, "start": at("0:10"), "end": at("0:25") })).unwrap();
    });
}

/// As above, for a clip rendered (a stretch taken out in the editor:
/// `edits::render_edit`).
#[test]
fn a_clip_deleted_as_its_render_ends_and_undone_keeps_the_file_another_app_took() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 2.0, "to": 3.0 })).unwrap();
    let clip = app.made(clip.id);
    let given = app.dragged_out(&clip);
    deleted_as_its_remake_ends_and_undone(&app, vod.id, clip.id, &given, || {
        app.invoke("remove_stretch", json!({ "id": clip.id, "from": 5.0, "to": 6.0 })).unwrap();
    });
}

/// On a disk without hard links a drag hands out the clip's own file as the
/// app it's let go on asks; a remake that ends before the drop is over
/// keeps that file, as it's noted handed out when it's given, not at the
/// drop (`hand_out_made`; the re-review's RR2).
#[test]
fn a_clip_made_again_before_the_drop_keeps_the_own_file_the_drag_gave() {
    let app = TestApp::new();
    app.takes_no_hard_links();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let name = drag_name(&app.db(), vodscribe_core::settings::ClipFormat::Mp4, clip.id);
    let drag = DragOut::new(app.handle().clone(), vec![(clip.id, name)]);
    let Some(Made::Ready(given)) = drag.files().into_iter().next() else { panic!("not handed out") };
    assert_eq!(given, file_of(&clip), "the own file");
    let before = as_given(&given);
    resized(&app, &clip, "0:30", "0:35");
    assert_eq!(as_given(&given), before, "the file the drag handed out went with a remake before the drop");
    drag.taken();
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert_eq!(as_given(&given), before, "the file the drag handed out was deleted with its clip");
}

/// The same where the disk takes links: the drag hands out a name, which a
/// remake never touches (the re-review's RR2b).
#[test]
fn a_clip_made_again_before_the_drop_keeps_the_name_the_drag_gave() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let name = drag_name(&app.db(), vodscribe_core::settings::ClipFormat::Mp4, clip.id);
    let drag = DragOut::new(app.handle().clone(), vec![(clip.id, name)]);
    let Some(Made::Ready(given)) = drag.files().into_iter().next() else { panic!("not handed out") };
    assert!(given.starts_with(app.paths().clip_names(clip.id)), "a name: {given:?}");
    resized(&app, &clip, "0:30", "0:35");
    drag.taken();
    assert!(given.is_file(), "the name the drag handed out went with the remake");
}

/// Clip `clip` dragged out of the window and let go on an app that takes
/// copies written for it (Finder, Explorer), through the kit's drag: the
/// name its copy was written from, which that app never took.
fn copied_out(app: &TestApp, clip: &Clip) -> PathBuf {
    use crate::testkit::{LetGo, Receiver};
    app.drag_out(&[clip.id]).unwrap();
    let drag = app.drags().last();
    drag.let_go(LetGo::On(Receiver::taking_copies("Finder")));
    assert_eq!(app.drag_ended()["delivered"], json!(true), "clip {} not copied out", clip.id);
    let given = drag.given();
    assert_eq!(given.len(), 1, "clip {} not handed out", clip.id);
    given[0].clone()
}

/// Every file left in the library's clips folder, at any depth.
fn left_in_clips(app: &TestApp) -> Vec<PathBuf> {
    let mut left = Vec::new();
    let mut dirs = vec![app.paths().clips()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                dirs.push(entry.path());
            } else {
                left.push(entry.path());
            }
        }
    }
    left
}

/// A clip copied out to an app that takes copies, then made again and
/// deleted, leaves nothing in the library: the name its copy was written
/// from was never another app's, so it goes with the make it held (`cut`;
/// `delete_clip`: "its file unless that was dragged out"; the 0.7.3 races
/// review, finding 2).
#[test]
fn a_clip_copied_out_then_made_again_and_deleted_leaves_nothing() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let name = copied_out(&app, &clip);
    assert!(name.starts_with(app.paths().clip_names(clip.id)), "a name: {name:?}");
    resized(&app, &clip, "0:30", "0:35");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    let left = left_in_clips(&app);
    assert!(left.is_empty(), "files left in the clips folder after the clip went: {left:?}");
}

/// The same made again by a change in the editor, which renders it
/// (`edits::render_edit`).
#[test]
fn a_clip_copied_out_then_changed_in_the_editor_and_deleted_leaves_nothing() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    copied_out(&app, &clip);
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 1.0, "to": 10.0 })).unwrap();
    app.made(clip.id);
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    let left = left_in_clips(&app);
    assert!(left.is_empty(), "files left in the clips folder after the clip went: {left:?}");
}

/// A name another app took stays through a remake and the clip's delete,
/// though the make it held is gone (plans/clips-pilot.md "Decided" 4b: "the
/// one before goes as now unless it was handed out, then it stays").
#[test]
fn a_name_another_app_took_stays_when_its_clip_is_made_again_and_deleted() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    let before = as_given(&given);
    resized(&app, &clip, "0:30", "0:35");
    assert_eq!(as_given(&given), before, "the name another app took went with the remake");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert_eq!(as_given(&given), before, "the name another app took went with its clip");
}

/// An edit's video rendered again while a drag of it is under way: the name
/// the drag gave stays, as the app it's let go on may yet take it, and that
/// app gets it (`edits::drag_edit`, `let_go_make`).
#[test]
fn an_edit_rendered_again_during_its_drag_keeps_the_name_the_drag_gave() {
    use crate::testkit::{LetGo, Receiver};
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = app.db().add_edit("Best of").unwrap().id;
    app.db().add_piece(edit, vod.id, (0.0, 30.0), "a", None).unwrap();
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let video = rendered(&app, edit, None);
    app.invoke("drag_edit", json!({ "id": edit, "drag": 1 })).unwrap();
    let drag = app.drags().last();
    let mut names: Vec<_> =
        left_in_clips(&app).into_iter().filter(|f| f.starts_with(app.paths().clip_names(edit))).collect();
    assert_eq!(names.len(), 1, "handed out as {names:?}");
    let name = names.remove(0);
    let before = as_given(&name);
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    rendered(&app, edit, Some(&video));
    assert_eq!(as_given(&name), before, "the name the drag gave went with the render");
    drag.let_go(LetGo::On(Receiver::taking_files("Kdenlive")));
    app.drag_ended();
    assert_eq!(drag.given(), std::slice::from_ref(&name));
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    assert_eq!(as_given(&name), before, "the name another app took went with its edit");
}
