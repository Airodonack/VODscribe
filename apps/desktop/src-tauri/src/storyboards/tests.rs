//! The storyboards' specs (storyboards.rs): a video deleted while its
//! storyboard is made, the start's sweep beside the queue, the files'
//! names, and an edit's moments found in its pieces' footage.

use super::*;

use vodscribe_core::testkit::{Gate, eventually};

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
