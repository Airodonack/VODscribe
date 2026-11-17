//! The storyboards' specs (storyboards.rs): a video deleted while its
//! storyboard is made, the start's sweep beside the queue, the files'
//! names, and an edit's moments found in its pieces' footage.

use super::*;

use serde_json::{Value, json};
use vodscribe_core::testkit::{Gate, Media, eventually, media};

use crate::testkit::TestApp;

/// "Delete video" just after the video was kept, its storyboard being
/// made: the batch reading the file (ffmpeg holding it open) ends before
/// the file is deleted, so it is, rather than left behind untracked.
#[test]
fn a_video_deleted_while_its_storyboard_is_made_is_deleted() {
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, mpsc};
    static N: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "vodscribe-sb-delete-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let paths = vodscribe_core::Paths::new(&root);
    std::fs::create_dir_all(paths.media()).unwrap();
    let db = Db::open(&paths.db()).unwrap();
    let id = vodscribe_core::testkit::library::add_site_vod(&db, "youtube", "x", "t", 60.0).id;
    let kept = db.files().source_video(id, "mp4");
    std::fs::write(&kept, b"video").unwrap();
    db.set_media_path(id, Some(&kept.to_string_lossy())).unwrap();
    let db = std::sync::Mutex::new(db);
    let videos = crate::videos::Videos::new();

    // The storyboard's job: a batch reads the file till it's stopped,
    // and says whether the file was deleted under it meanwhile.
    let storyboards = Arc::new(Storyboards::new());
    let (reading, is_reading) = mpsc::channel();
    let (ended, has_ended) = mpsc::channel();
    let file = kept.clone();
    storyboards
        .jobs
        .run("storyboards-test", move |_, stop| {
            let held = crate::open_as_ffmpeg(&file);
            reading.send(()).unwrap();
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let deleted_under_it = !file.exists();
            drop(held);
            ended.send(deleted_under_it).unwrap();
        })
        .unwrap();
    storyboards.push(id);
    is_reading.recv_timeout(Duration::from_secs(5)).unwrap();

    let remove = |paths: &vodscribe_core::Paths, id| crate::vods::remove_kept(&storyboards, paths, id);
    crate::vods::delete_kept_video(&db, &videos, &paths, id, remove).unwrap();
    // Then `forget_gone`, as delete_video does.
    storyboards.stop(id);
    let deleted_under_it = has_ended.recv_timeout(Duration::from_secs(5)).unwrap();
    let left = kept.exists();
    let _ = std::fs::remove_dir_all(&root);
    assert!(!deleted_under_it, "deleted while its storyboard's ffmpeg read it");
    assert!(!left, "left behind, its record gone");
    assert_eq!(db.lock().unwrap().media_path(id).unwrap(), None);
}

/// VOD `vod`'s storyboard of its own file, once the storyboards' queue has
/// made it (on the fake ffmpeg, which decodes a storyboard's keyframes): the
/// VOD opened, as the page tells the app (`set_view`), its storyboards are
/// made next.
fn made_board(app: &TestApp, vod: &vodscribe_core::db::Vod) -> PathBuf {
    app.invoke("set_view", serde_json::json!({ "view": { "kind": "vod", "id": vod.id } })).unwrap();
    let source = PathBuf::from(vod.source_path.clone().unwrap());
    let board = app.paths().source_storyboard(vod.id, &source);
    eventually(&format!("VOD {}'s storyboard made", vod.id), || made(&board, &source));
    board
}

/// A VOD added while the startup sweep runs keeps its storyboard
/// (`sweep`: "deletes the storyboards of VODs and files no longer
/// here"; this VOD is here). The sweep is held once it has read the
/// library, before it goes through the storyboards.
#[test]
fn a_vod_added_during_the_startup_sweep_keeps_its_storyboard() {
    let app = TestApp::new();
    let first = app.add_video_file("Day 1.mp4", 60.0);
    let control = made_board(&app, &first);
    let folder = app.quit();
    let read = Gate::new();
    folder.holds_sweeps(&read);
    let app = TestApp::on(folder);
    read.reached();
    // Added, and its storyboard made, after the sweep read the library: the
    // sweep goes by the VOD its name says and what the library has now.
    let added = app.add_video_file("Day 2.mp4", 60.0);
    let board = made_board(&app, &added);
    read.open();
    app.state().storyboards.swept();

    assert!(control.exists(), "the storyboard of a VOD in the library when the sweep began was deleted");
    assert!(board.exists(), "the storyboard of VOD {}, added during the sweep, was deleted", added.id);
}

/// A storyboard's header being written aside (`.sb-new`, then renamed)
/// while the startup sweep runs is left to be written: the sweep runs
/// beside the storyboards' queue. One a quit or crash left is gone at
/// the next start.
#[test]
fn a_storyboard_begun_during_the_startup_sweep_is_left_to_be_made() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    // Made, so the sweep asks the queue for nothing of it.
    let aside = made_board(&app, &vod).with_extension("sb-new");
    let folder = app.quit();
    let read = Gate::new();
    folder.holds_sweeps(&read);
    let app = TestApp::on(folder);
    read.reached();
    // As the queue begins a header, beside the sweep: what it holds isn't read.
    std::fs::create_dir_all(aside.parent().unwrap()).unwrap();
    std::fs::write(&aside, b"half a header").unwrap();
    read.open();
    app.state().storyboards.swept();
    assert!(aside.exists(), "a header being written was deleted under the queue");

    let app = TestApp::on(app.quit());
    assert!(!aside.exists(), "a header a quit left was kept");
    drop(app);
}

#[test]
fn a_files_storyboard_is_named_by_its_vod_and_path() {
    let paths = vodscribe_core::Paths::new("/data");
    let file = |p: &str| paths.source_storyboard(7, Path::new(p));
    let a = file("/videos/a.mp4");
    assert_eq!(a, file("/videos/a.mp4"), "the same each time");
    assert_ne!(a, file("/videos/b.mp4"));
    let name = a.file_name().unwrap().to_str().unwrap();
    assert_eq!(vod_of(name), Some(7));
    // FNV-1a's own check value: the name mustn't change between versions.
    assert_eq!(paths.source_storyboard(7, Path::new("a")), paths.storyboards().join("7-af63dc4c8601ec8c.sb"));
    assert_eq!(vod_of("7-1.peaks"), None);
}

fn piece(id: i64, vod_id: i64, start: f64, end: f64) -> vodscribe_core::edits::EditPiece {
    vodscribe_core::edits::EditPiece {
        id,
        edit_id: 1,
        vod_id,
        start,
        end,
        title: String::new(),
        vod_title: String::new(),
        hole: false,
        group: None,
    }
}

#[test]
fn an_edits_moment_is_found_in_its_pieces_footage() {
    use vodscribe_core::footage::Source;
    // A clip of 1-3 s of VOD 7 (its own file), then 10-12 s of VOD 8
    // (a section from 9 s), then 0-2 s of VOD 9 (downloading, read
    // through the media server), then 5-6 s of VOD 10 (none here).
    let pieces = [piece(1, 7, 1.0, 3.0), piece(2, 8, 10.0, 12.0), piece(3, 9, 0.0, 2.0), piece(4, 10, 5.0, 6.0)];
    let timeline = Timeline::of(&pieces, |vod, a, b| match vod {
        7 => vec![(a, b, Source { path: "/v/7.mp4".into(), offset: 0.0, section: None })],
        8 => vec![(a, b, Source { path: "/f/8-9000-20000.mkv".into(), offset: 9.0, section: Some("s".into()) })],
        9 => vec![(a, b, Source { path: "http://127.0.0.1:5/footage/9".into(), offset: 0.0, section: None })],
        _ => vec![],
    });
    let vods: HashMap<i64, i64> = pieces.iter().map(|p| (p.id, p.vod_id)).collect();
    let own = locate(&timeline, &vods, 0.5).unwrap();
    assert_eq!((own.vod_id, own.path.as_path(), own.file_time), (7, Path::new("/v/7.mp4"), 1.5));
    // Its storyboard's frame at 1 s of the file shows at 0 s of the edit.
    assert_eq!(1.0 + own.offset, 0.0);
    let section = locate(&timeline, &vods, 2.5).unwrap();
    assert_eq!((section.vod_id, section.file_time), (8, 1.5), "VOD 10.5 s, 1.5 s into the section");
    assert_eq!(1.0 + section.offset, 2.0, "the section's 1 s (VOD 10 s) is the edit's 2 s");
    assert_eq!(locate(&timeline, &vods, 4.5), None, "downloading: none");
    assert_eq!(locate(&timeline, &vods, 6.5), None, "not here: none");
}

/// The storyboard of `source`, one of VOD `vod`'s files, once it's made.
fn board_of(app: &TestApp, vod: i64, source: &Path) -> PathBuf {
    let board = app.paths().source_storyboard(vod, source);
    eventually(&format!("the storyboard of {} made", source.display()), || made(&board, source));
    board
}

/// What `storyboard_frame` gives for `doc` at `time`: the time it shows at
/// and the picture's time in its file (a fake picture says it), or None for
/// nothing.
fn shown(app: &TestApp, doc: Value, time: f64, within: f64) -> Option<(f64, f64)> {
    let bytes = app.invoke_bytes("storyboard_frame", json!({ "doc": doc, "time": time, "within": within })).unwrap();
    if bytes.is_empty() {
        return None;
    }
    let at = f64::from_le_bytes(bytes[..8].try_into().unwrap());
    Some((at, media::storyboard_time(&bytes[8..]).expect("a storyboard's picture")))
}

/// How many slots the player crate's storyboard makes in a batch.
const FIRST_BATCH: usize = 8;

fn vod_doc(id: i64) -> Value {
    json!({ "kind": "vod", "id": id })
}

/// The timeline's picture at a time is the keyframe at or before it that
/// the storyboard holds: in a file with a keyframe each second and a slot each
/// two, 7.4 s shows 6 s (0.6.0: "The timeline
/// shows a picture of each moment"; storyboards.rs `storyboard_frame`: "the
/// keyframe at or before it"), told with the time it shows at.
#[test]
fn the_timelines_picture_is_the_keyframe_at_or_before_the_pointer() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    made_board(&app, &vod);
    assert_eq!(shown(&app, vod_doc(vod.id), 7.4, 0.0), Some((6.0, 6.0)));
    assert_eq!(shown(&app, vod_doc(vod.id), 0.2, 0.0), Some((0.0, 0.0)));
    assert_eq!(shown(&app, vod_doc(vod.id), 59.9, 0.0), Some((58.0, 58.0)), "the last slot");
}

/// A VOD with no storyboard yet shows nothing, and the storyboard is asked
/// for first, so the picture comes (storyboards.rs `frame_at`: "A storyboard
/// missing or unfinished is asked for first"; 0.6.0: "The pictures are made
/// in the background").
#[test]
fn a_vod_without_its_storyboard_shows_nothing_and_has_it_made() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    let source = PathBuf::from(vod.source_path.clone().unwrap());
    assert_eq!(shown(&app, vod_doc(vod.id), 7.4, 0.0), None);
    board_of(&app, vod.id, &source);
    assert_eq!(shown(&app, vod_doc(vod.id), 7.4, 0.0), Some((6.0, 6.0)));
}

/// Footage with no file here (a site's video not downloaded) shows nothing
/// (storyboards.rs `find`: "if that's a file here (not footage downloading,
/// read through the media server)").
#[test]
fn a_vod_with_no_file_here_shows_no_picture() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "x", "Their stream", 60.0);
    assert_eq!(shown(&app, vod_doc(vod.id), 7.4, 0.0), None);
}

/// A file changed since its storyboard was made has it made again, and shows
/// nothing meanwhile rather than the old file's pictures (storyboards.rs
/// `frame_at`: "Of the file as it was: made again").
#[test]
fn a_file_changed_since_has_its_storyboard_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    let source = PathBuf::from(vod.source_path.clone().unwrap());
    let board = made_board(&app, &vod);
    assert_eq!(Storyboard::open(&board).unwrap().layout.slots, 30);
    // Replaced by a longer video (a different size, whatever the clock's grain).
    Media::video(100.0).write(&source);
    assert_eq!(shown(&app, vod_doc(vod.id), 7.4, 0.0), None, "the old file's picture isn't shown");
    eventually("made again", || made(&board, &source));
    assert_eq!(Storyboard::open(&board).unwrap().layout.slots, 50);
    assert_eq!(shown(&app, vod_doc(vod.id), 80.2, 0.0), Some((80.0, 80.0)));
}

/// An edit's timeline shows each piece's footage at the edit's own time: a
/// piece of 10 to 20 s of a VOD shows the frame there at 0 to 10 s (0.6.0:
/// "Pointing at the timeline of a video on this computer (a file, a kept
/// video, an edit's pieces) … shows a small … picture of that moment";
/// storyboards.rs `storyboard_frame`: "in its own time").
#[test]
fn an_edits_timeline_shows_its_pieces_footage_at_the_edits_time() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    made_board(&app, &vod);
    let other = app.add_video_file("Day 2.mp4", 60.0);
    made_board(&app, &other);
    let edit = app.edit_of(&[(vod.id, 10.0, 20.0), (other.id, 30.0, 35.0)]);
    let doc = json!({ "kind": "edit", "id": edit });
    assert_eq!(shown(&app, doc.clone(), 2.5, 0.0), Some((2.0, 12.0)), "the first piece, 12 s in");
    assert_eq!(shown(&app, doc.clone(), 12.5, 0.0), Some((12.0, 32.0)), "the second piece, 32 s in");
}

/// A section downloaded for an edit's piece has a storyboard of its own, as a
/// file added has, and the VOD's moments in it show (e2e edit-footage-thumbnail:
/// "the section has its own storyboard, as a file added has, and hovering the
/// piece shows its frames"; storyboards.rs, module doc: "a fetched section").
#[test]
fn a_footage_section_has_its_own_storyboard_and_shows_its_vods_moments() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "x", "Their stream", 120.0);
    let section =
        app.paths().source_footage().join(format!("{}.mkv", vodscribe_core::footage::section_stem(vod.id, 9.0, 29.0)));
    Media { container: "matroska".into(), ..Media::video(20.0) }.write(&section);
    app.invoke("set_view", json!({ "view": vod_doc(vod.id) })).unwrap();
    board_of(&app, vod.id, &section);
    // 25.5 s of the VOD is 16.5 s into the section (it has its picture from 10 s in): the slot at 16 s, the VOD's 25 s.
    assert_eq!(shown(&app, vod_doc(vod.id), 25.5, 0.0), Some((25.0, 16.0)));
}

/// A clip's file has a storyboard of its own too (storyboards.rs, module
/// doc: "of the footage on this computer: a file added, a video kept, a
/// fetched section, a clip's file").
#[test]
fn a_clips_file_has_its_own_storyboard() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.invoke("set_view", json!({ "view": vod_doc(vod.id) })).unwrap();
    let file = PathBuf::from(clip.file_path.clone().unwrap());
    let board = board_of(&app, vod.id, &file);
    assert_eq!(Storyboard::open(&board).unwrap().layout.duration, 10.0, "the clip's own ten seconds");
}

/// Removing a VOD deletes its storyboards, once it's erased (storyboards.rs
/// `remove`: "Deletes VOD `id`'s storyboards (it's erased)"; the VOD's id
/// first in each name, "so all of a VOD's go with it").
#[test]
fn a_removed_vods_storyboards_are_deleted_and_the_others_stay() {
    let app = TestApp::new();
    let (gone, kept) = (app.add_video_file("Day 1.mp4", 60.0), app.add_video_file("Day 2.mp4", 60.0));
    let (gone_board, kept_board) = (made_board(&app, &gone), made_board(&app, &kept));
    app.invoke("delete_vod", json!({ "id": gone.id })).unwrap();
    app.erasures_done();
    assert!(!gone_board.exists(), "the removed VOD's storyboard was kept");
    assert!(kept_board.exists(), "another VOD's storyboard was deleted");
}

/// Deleting a VOD's kept video deletes the storyboard of that file (the
/// video's gone), and a storyboard of its other files stays (storyboards.rs
/// `forget_gone`: "Deletes VOD `id`'s storyboards of files no longer here
/// (its kept video deleted) … what's still here of it is made on").
#[test]
fn a_deleted_kept_video_takes_its_storyboard_and_the_others_stay() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "x", "Their stream", 120.0);
    let kept = app.paths().source_video(vod.id, "mp4");
    Media::video(120.0).write(&kept);
    app.db().set_media_path(vod.id, Some(&kept.to_string_lossy())).unwrap();
    let section =
        app.paths().source_footage().join(format!("{}.mkv", vodscribe_core::footage::section_stem(vod.id, 9.0, 29.0)));
    Media { container: "matroska".into(), ..Media::video(20.0) }.write(&section);
    app.invoke("set_view", json!({ "view": vod_doc(vod.id) })).unwrap();
    let (video_board, section_board) = (board_of(&app, vod.id, &kept), board_of(&app, vod.id, &section));
    app.invoke("delete_video", json!({ "id": vod.id })).unwrap();
    assert!(!video_board.exists(), "the deleted video's storyboard was kept");
    assert!(section_board.exists(), "the section's storyboard was deleted");
}

/// At the start, a storyboard of a file that's gone is deleted, but not one
/// of a file on a drive that's away (storyboards.rs `sweep`: "deletes the
/// storyboards of VODs and files no longer here (a section tidied away, a
/// clip's file deleted; not those on a drive that's away)").
#[test]
fn at_the_start_a_storyboard_of_a_file_gone_goes_but_not_one_on_a_drive_that_is_away() {
    let app = TestApp::new();
    let (deleted, away) = (app.add_video_file("deleted/Day 1.mp4", 60.0), app.add_video_file("away/Day 2.mp4", 60.0));
    let (deleted_board, away_board) = (made_board(&app, &deleted), made_board(&app, &away));
    let folder = app.quit();
    let source = |vod: &vodscribe_core::db::Vod| PathBuf::from(vod.source_path.clone().unwrap());
    std::fs::remove_file(source(&deleted)).unwrap();
    std::fs::remove_dir_all(source(&away).parent().unwrap()).unwrap();
    let app = TestApp::on(folder);
    app.state().storyboards.swept();
    assert!(!deleted_board.exists(), "the storyboard of a file that's gone was kept");
    assert!(away_board.exists(), "the storyboard of a file on a drive that's away was deleted");
    drop(app);
}

/// At the start, the storyboards missing, and those of files changed since,
/// are asked for and made, without the VOD being opened (storyboards.rs
/// `sweep`: "asks for those missing, of files changed since, or unfinished";
/// `Storyboards::run`: "asks it for what a quit (or a crash) left unmade").
#[test]
fn at_the_start_the_storyboards_missing_or_of_files_changed_since_are_made() {
    let app = TestApp::new();
    let (changed, missing) = (app.add_video_file("Day 1.mp4", 60.0), app.add_video_file("Day 2.mp4", 60.0));
    let board = made_board(&app, &changed);
    let folder = app.quit();
    let source = PathBuf::from(changed.source_path.clone().unwrap());
    Media::video(100.0).write(&source);
    let app = TestApp::on(folder);
    let missing_source = PathBuf::from(missing.source_path.clone().unwrap());
    eventually("the changed file's storyboard made again", || made(&board, &source));
    assert_eq!(Storyboard::open(&board).unwrap().layout.slots, 50);
    board_of(&app, missing.id, &missing_source);
}

/// An edit that's opened has the storyboards of its pieces' footage made
/// (storyboards.rs `opened`: "its footage's storyboards are made next (an
/// edit's in the order of its pieces)").
#[test]
fn an_opened_edits_footage_has_its_storyboards_made() {
    let app = TestApp::new();
    let (a, b) = (app.add_video_file("Day 1.mp4", 60.0), app.add_video_file("Day 2.mp4", 60.0));
    let edit = app.edit_of(&[(a.id, 10.0, 20.0), (b.id, 30.0, 35.0)]);
    app.invoke("set_view", json!({ "view": { "kind": "edit", "id": edit } })).unwrap();
    for vod in [&a, &b] {
        board_of(&app, vod.id, Path::new(vod.source_path.as_deref().unwrap()));
    }
}

/// Three VODs whose storyboards are asked for, the first under way and held
/// as its file is read (the fake ffmpeg's details), the other two waiting:
/// the queue's state at the moment a VOD is let go of or removed. Gives the
/// gate, the three VODs and their files.
fn one_under_way_two_waiting(app: &TestApp) -> (Gate, [vodscribe_core::db::Vod; 3], [PathBuf; 3]) {
    let reading = Gate::new();
    app.ffmpeg.holds_details(&reading);
    let vods = [
        app.add_video_file("Day 1.mp4", 60.0),
        app.add_video_file("Day 2.mp4", 60.0),
        app.add_video_file("Day 3.mp4", 60.0),
    ];
    let files = [0, 1, 2].map(|i| PathBuf::from(vods[i].source_path.clone().unwrap()));
    for (i, vod) in vods.iter().enumerate() {
        app.invoke("set_view", json!({ "view": vod_doc(vod.id) })).unwrap();
        if i == 0 {
            reading.reached();
        }
    }
    (reading, vods, files)
}

/// The queue has nothing under way or waiting.
fn storyboards_idle(app: &TestApp) -> bool {
    app.state().storyboards.jobs.snapshot() == (None, Vec::new())
}

/// How many slots of the storyboard of `source`, VOD `vod`'s file, are made
/// (none if there's no file).
fn slots_made(app: &TestApp, vod: i64, source: &Path) -> usize {
    Storyboard::open(&app.paths().source_storyboard(vod, source)).map_or(0, |b| b.made())
}

/// A VOD removed has the storyboard waiting for it taken out of the queue,
/// and the others go on (storyboards.rs `remove`: "Deletes VOD `id`'s
/// storyboards (it's erased), stopping what's making them").
#[test]
fn removing_a_vod_takes_its_waiting_storyboard_out_and_the_others_go_on() {
    let app = TestApp::new();
    let (reading, vods, files) = one_under_way_two_waiting(&app);
    // The waiting queue is the last VOD opened, then the one before it.
    remove(app.handle(), vods[1].id);
    reading.open();
    eventually("the queue is done", || storyboards_idle(&app));
    assert!(made(&app.paths().source_storyboard(vods[0].id, &files[0]), &files[0]), "the one under way");
    assert!(made(&app.paths().source_storyboard(vods[2].id, &files[2]), &files[2]), "the one waiting that stays");
    assert_eq!(slots_made(&app, vods[1].id, &files[1]), 0, "the removed VOD's storyboard was made");
}

/// A VOD removed while its storyboard is made has the making stopped (the
/// same doc: "stopping what's making them"), so no more of it is made.
#[test]
fn removing_a_vod_stops_the_storyboard_being_made_of_it() {
    let app = TestApp::new();
    let (reading, vods, files) = one_under_way_two_waiting(&app);
    remove(app.handle(), vods[0].id);
    reading.open();
    eventually("the queue is done", || storyboards_idle(&app));
    assert_eq!(slots_made(&app, vods[0].id, &files[0]), 0, "the removed VOD's storyboard went on");
    assert!(made(&app.paths().source_storyboard(vods[1].id, &files[1]), &files[1]));
    assert!(made(&app.paths().source_storyboard(vods[2].id, &files[2]), &files[2]));
}

/// A deletion waits for the storyboard being made of its VOD to let go of the
/// files, and takes out the one waiting (storyboards.rs `let_go`: "Stops
/// making VOD `id`'s storyboards, and waits (a while at most) for the batch
/// under way to end: its ffmpeg lets go of the VOD's files").
#[test]
fn letting_go_of_a_vod_waits_for_the_batch_under_way_and_takes_out_the_waiting() {
    let app = TestApp::new();
    let (reading, vods, files) = one_under_way_two_waiting(&app);
    // A waiting one: nothing to wait for.
    app.state().storyboards.let_go(vods[1].id);
    // The one under way: let go of once its batch ends.
    let letting = {
        let handle = app.handle().clone();
        let id = vods[0].id;
        std::thread::spawn(move || handle.state::<AppState>().storyboards.let_go(id))
    };
    let id = vods[0].id;
    eventually("the stop is asked", || !app.state().storyboards.jobs.is_wanted(|&j| j == id));
    reading.open();
    letting.join().unwrap();
    assert!(app.state().storyboards.jobs.snapshot().0 != Some(vods[0].id), "its batch is over");
    eventually("the queue is done", || storyboards_idle(&app));
    assert_eq!(slots_made(&app, vods[0].id, &files[0]), 0, "stopped at its first batch");
    assert_eq!(slots_made(&app, vods[1].id, &files[1]), 0, "the waiting one was taken out");
    assert!(made(&app.paths().source_storyboard(vods[2].id, &files[2]), &files[2]), "another VOD's goes on");
}
/// A site's picture isn't replaced, nor made, by the storyboard of the video
/// kept for it: only a file added has its library picture from its
/// storyboard (storyboards.rs, module doc: "A file's library picture is a
/// frame of its storyboard"; `make`: "its own file's picture (a file added)").
#[test]
fn a_site_vods_picture_is_left_alone_by_the_storyboard_of_its_kept_video() {
    let app = TestApp::new();
    // One with its picture from the site, its kept video sound alone; one
    // with none yet (its fetch failed), its kept video a picture; one with
    // none yet, its kept video sound alone.
    let (with, without, bare) = (
        app.add_site_vod("peertube", "with", "Their stream", 60.0),
        app.add_site_vod("peertube", "without", "Their other stream", 60.0),
        app.add_site_vod("peertube", "bare", "Their third stream", 60.0),
    );
    let thumbs = app.paths().thumbs();
    std::fs::create_dir_all(&thumbs).unwrap();
    let name = format!("{}-5.jpg", with.id);
    std::fs::write(thumbs.join(&name), b"the site's picture").unwrap();
    app.db().set_thumb(with.id, &name).unwrap();
    let keep = |vod: &vodscribe_core::db::Vod, media: &Media| {
        let kept = app.paths().source_video(vod.id, "mp4");
        media.write(&kept);
        app.db().set_media_path(vod.id, Some(&kept.to_string_lossy())).unwrap();
        kept
    };
    let (sound, picture) = (keep(&with, &Media::sound(60.0)), keep(&without, &Media::video(60.0)));
    keep(&bare, &Media::sound(60.0));
    for vod in [&with, &without, &bare] {
        app.invoke("set_view", json!({ "view": vod_doc(vod.id) })).unwrap();
        eventually("the queue is done", || storyboards_idle(&app));
    }
    assert!(!app.paths().source_storyboard(with.id, &sound).exists(), "a sound file has no storyboard");
    assert!(made(&app.paths().source_storyboard(without.id, &picture), &picture), "the video's was made");
    assert_eq!(app.db().vod(with.id).unwrap().unwrap().thumb.as_deref(), Some(name.as_str()), "the site's picture");
    assert_eq!(app.db().vod(without.id).unwrap().unwrap().thumb, None, "no picture made of a kept video");
    assert_eq!(app.db().vod(bare.id).unwrap().unwrap().thumb, None, "no picture said to be none for a kept sound");
}

/// A file ffmpeg can't read is reported and passed over: the VOD's other
/// files still have their storyboards made (storyboards.rs `make`: "each job
/// making those of the VOD's files that aren't made yet").
#[test]
fn a_file_that_cannot_be_read_does_not_stop_the_storyboards_of_the_vods_others() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let own = PathBuf::from(vod.source_path.clone().unwrap());
    std::fs::write(&own, b"not a video at all").unwrap();
    app.invoke("set_view", json!({ "view": vod_doc(vod.id) })).unwrap();
    let file = PathBuf::from(clip.file_path.clone().unwrap());
    board_of(&app, vod.id, &file);
    assert_eq!(slots_made(&app, vod.id, &own), 0);
}

/// A storyboard stopped isn't gone on with at the VOD's next file: one let go
/// of has made no more (storyboards.rs `make`: a canceled making ends the
/// job).
#[test]
fn a_stopped_storyboard_is_not_followed_by_the_vods_next_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let reading = Gate::new();
    app.ffmpeg.holds_details(&reading);
    app.invoke("set_view", json!({ "view": vod_doc(vod.id) })).unwrap();
    reading.reached();
    let letting = {
        let handle = app.handle().clone();
        let id = vod.id;
        std::thread::spawn(move || handle.state::<AppState>().storyboards.let_go(id))
    };
    let id = vod.id;
    eventually("the stop is asked", || !app.state().storyboards.jobs.is_wanted(|&j| j == id));
    reading.open();
    letting.join().unwrap();
    eventually("the queue is done", || storyboards_idle(&app));
    let file = PathBuf::from(clip.file_path.clone().unwrap());
    assert_eq!(slots_made(&app, vod.id, &file), 0, "the clip's storyboard was gone on with");
    assert!(!app.paths().source_storyboard(vod.id, &file).exists(), "not even begun");
}

/// A VOD's picture, once taken, isn't taken again when its storyboard is
/// asked for again, and one known to have none (an empty name) stays so
/// (storyboards.rs `has_picture`: "Whether VOD `id` has its picture (or is
/// known to have none)"; `make`: "its picture is taken again only if it's
/// missing").
#[test]
fn a_picture_there_or_known_to_be_none_is_not_taken_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    let source = PathBuf::from(vod.source_path.clone().unwrap());
    made_board(&app, &vod);
    eventually("the picture is taken", || app.db().vod(vod.id).unwrap().unwrap().thumb.is_some_and(|t| !t.is_empty()));
    let first = app.db().vod(vod.id).unwrap().unwrap().thumb.unwrap();
    let ask_again = || {
        app.state().storyboards.push(vod.id);
        eventually("the queue is done", || storyboards_idle(&app));
    };
    ask_again();
    assert_eq!(app.db().vod(vod.id).unwrap().unwrap().thumb.as_deref(), Some(first.as_str()), "taken again");
    // Known to have none: left so.
    app.db().set_thumb(vod.id, "").unwrap();
    ask_again();
    assert_eq!(app.db().vod(vod.id).unwrap().unwrap().thumb.as_deref(), Some(""), "a picture made for one with none");
    assert!(made(&app.paths().source_storyboard(vod.id, &source), &source));
    // Gone: taken again.
    app.db().set_thumb(vod.id, "9-9.jpg").unwrap();
    ask_again();
    let again = app.db().vod(vod.id).unwrap().unwrap().thumb.unwrap();
    assert!(app.paths().source_thumb(&again).is_file(), "{again}");
}

/// A VOD whose video was deleted is asked for again, for what's left of it
/// (storyboards.rs `forget_gone`: "what's still here of it is made on"), the
/// queue holding it once, whatever else waits.
#[test]
fn forgetting_a_vods_gone_files_asks_for_its_storyboards_again() {
    let app = TestApp::new();
    let (reading, vods, _files) = one_under_way_two_waiting(&app);
    forget_gone(app.handle(), vods[1].id);
    let (running, waiting) = app.state().storyboards.jobs.snapshot();
    assert_eq!(running, Some(vods[0].id));
    assert_eq!(waiting.iter().filter(|&&id| id == vods[1].id).count(), 1, "asked for once: {waiting:?}");
    assert!(waiting.contains(&vods[2].id));
    reading.open();
    eventually("the queue is done", || storyboards_idle(&app));
}

/// At the start, a storyboard of a VOD that's no longer in the library goes,
/// whether or not its file is still there (storyboards.rs `sweep`: "deletes
/// the storyboards of VODs and files no longer here"), and another's stays.
#[test]
fn at_the_start_the_storyboard_of_a_vod_not_in_the_library_goes() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    let board = made_board(&app, &vod);
    let stray = board.with_file_name("9999-00000000deadbeef.sb");
    std::fs::copy(&board, &stray).unwrap();
    let folder = app.quit();
    let app = TestApp::on(folder);
    app.state().storyboards.swept();
    assert!(!stray.exists(), "the storyboard of a VOD not in the library was kept");
    assert!(board.exists(), "the storyboard of a VOD in the library was deleted");
    drop(app);
}

/// Footage a time is past the end of isn't shown: a section's picture is the
/// section's, from where it has its first keyframe to its end (storyboards.rs
/// `find`; footage.rs `KEYFRAME_GAP`: "a section is copied, not encoded again,
/// so it has none until its first keyframe").
#[test]
fn a_section_shows_nothing_before_its_first_keyframe_nor_past_its_end() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "x", "Their stream", 120.0);
    let stem = vodscribe_core::footage::section_stem(vod.id, 9.0, 29.0);
    let section = app.paths().source_footage().join(format!("{stem}.mkv"));
    Media { container: "matroska".into(), ..Media::video(20.0) }.write(&section);
    app.invoke("set_view", json!({ "view": vod_doc(vod.id) })).unwrap();
    board_of(&app, vod.id, &section);
    assert_eq!(shown(&app, vod_doc(vod.id), 25.5, 0.0), Some((25.0, 16.0)), "inside");
    assert_eq!(shown(&app, vod_doc(vod.id), 28.9, 0.0), Some((27.0, 18.0)), "to the end");
    assert_eq!(shown(&app, vod_doc(vod.id), 29.0015, 0.0), None, "a moment past the end");
    assert_eq!(shown(&app, vod_doc(vod.id), 40.0, 0.0), None, "well past it");
    assert_eq!(shown(&app, vod_doc(vod.id), 12.0, 0.0), None, "before its picture starts");
}

/// An edit that's changed shows its new pieces at once, not the timeline it
/// had a moment ago (storyboards.rs `PLANNED_FOR`: "How long an edit's
/// timeline is used for its thumbnails, the edit unchanged").
#[test]
fn an_edit_changed_shows_its_new_pieces_at_once() {
    let app = TestApp::new();
    let (a, b) = (app.add_video_file("Day 1.mp4", 60.0), app.add_video_file("Day 2.mp4", 60.0));
    made_board(&app, &a);
    made_board(&app, &b);
    let edit = app.edit_of(&[(a.id, 10.0, 20.0)]);
    let doc = json!({ "kind": "edit", "id": edit });
    assert_eq!(shown(&app, doc.clone(), 2.5, 0.0), Some((2.0, 12.0)));
    let piece = app.db().edit_pieces(edit).unwrap()[0].id;
    app.db().set_piece_range(piece, 30.0, 40.0).unwrap();
    assert_eq!(shown(&app, doc, 2.5, 0.0), Some((2.0, 32.0)), "the edit's old timeline was used");
}

/// A storyboard still being made is read again as it grows, so the timeline's
/// picture comes as slots are made (storyboards.rs `frame_at`: "A storyboard
/// missing or unfinished is asked for first"; 0.6.0: "The pictures are made
/// in the background").
#[test]
fn a_storyboard_being_made_is_read_again_as_it_grows() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 1.mp4", 60.0);
    let source = PathBuf::from(vod.source_path.clone().unwrap());
    // One batch made, as a quit left it.
    let ffmpeg = vodscribe_core::tools::find_ffmpeg(&app.paths().bin()).unwrap();
    let file = app.paths().source_storyboard(vod.id, &source);
    let first = storyboard::make(&ffmpeg, &source, &file, &AtomicBool::new(false), |_| false);
    assert!(first.is_err_and(|e| vodscribe_core::is_canceled(&e)));
    // 40 s isn't in the first batch: nothing near it yet, and the rest is asked for.
    assert_eq!(shown(&app, vod_doc(vod.id), 40.2, 0.0), None);
    eventually("the rest is made", || made(&file, &source));
    eventually("the picture comes", || shown(&app, vod_doc(vod.id), 40.2, 0.0) == Some((40.0, 40.0)));
}

/// The storyboards wait while the player plays other footage, so playing
/// stays smooth (storyboards.rs, module doc: "the queue waits while the
/// player plays other footage (another VOD, or an edit not made of this
/// one)"): one batch is made, then the rest waits for the playing to stop.
#[test]
fn storyboards_wait_while_other_footage_plays_and_go_on_when_it_stops() {
    let app = TestApp::new();
    app.with_screen();
    let mut small = Media::video(60.0);
    let picture = small.video.as_mut().unwrap();
    (picture.width, picture.height) = (64, 36);
    let (playing, other) = (app.add_file("Playing.mp4", &small), app.add_file("Other.mp4", &small));
    app.invoke("player_open", json!({ "id": playing.id, "at": 10.0, "play": true })).unwrap();
    app.invoke("set_view", json!({ "view": vod_doc(other.id) })).unwrap();
    let source = PathBuf::from(other.source_path.clone().unwrap());
    let file = app.paths().source_storyboard(other.id, &source);
    eventually("its first batch is made", || Storyboard::open(&file).is_ok_and(|b| b.made() >= FIRST_BATCH));
    assert_eq!(Storyboard::open(&file).unwrap().made(), FIRST_BATCH, "it went on while another played");
    assert_eq!(app.state().storyboards.jobs.snapshot().0, Some(other.id), "still under way");
    app.invoke("player_pause", json!({})).unwrap();
    eventually("the rest is made", || made(&file, &source));
}
