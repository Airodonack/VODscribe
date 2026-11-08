//! The specs of setting aside a library that won't open, and of the start
//! after it. Each test is named for what it says the app does and cites
//! where that's said.

use std::path::{Path, PathBuf};

use vodscribe_core::Paths;
use vodscribe_core::db::Db;
use vodscribe_core::testkit::TempDir;

/// A library that won't open, as the recovery screen finds it, with the
/// files `of` it names written in its folders.
fn broken_library(name: &str, of: impl Fn(&Paths) -> Vec<PathBuf>) -> (TempDir, Paths, Vec<PathBuf>) {
    let dir = TempDir::new(name);
    let paths = Paths::new(dir.join("data"));
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a database").unwrap();
    assert!(Db::open_in(&paths).is_err());
    let files = of(&paths);
    for file in &files {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"kept").unwrap();
    }
    (dir, paths, files)
}

/// "Start a new library" (`set_library_aside`), then the next start's
/// tidying on the new, empty library, as `start_library` and the clips'
/// queue run it; returns the set-aside library's layout.
fn set_aside_and_start(paths: &Paths) -> Paths {
    let aside = vodscribe_core::db::set_aside(paths).unwrap();
    paths.ensure().unwrap();
    let fresh = Db::open_in(paths).unwrap();
    crate::tidy_at_start(paths, &fresh);
    crate::clips::tidy_at_start(paths, &fresh).unwrap();
    Paths::new(aside)
}

/// Checks each of `files` is in the set-aside library `aside`, where moving
/// its folders back finds it, and gone from the new one at `paths`, where a
/// VOD or clip given the same id would write over it.
fn kept_aside(paths: &Paths, aside: &Paths, files: &[PathBuf]) {
    for file in files {
        let moved = aside.local.join(file.strip_prefix(&paths.local).unwrap());
        let left = file.exists();
        assert!(
            moved.is_file(),
            "{} wasn't kept in the set-aside library (left in the new one: {left})",
            file.display()
        );
        assert!(!file.exists(), "{} was left in the new library", file.display());
    }
}

/// Starting a new library "sets the old one aside, never deleting it"
/// (recovery.rs; the screen says "Nothing has been deleted"): its VODs'
/// kept videos, a site's that the site may have taken down since, and the
/// files it received from other apps' drops are kept at the next start.
#[test]
fn a_library_set_aside_keeps_its_videos_at_the_next_start() {
    let (_dir, paths, files) =
        broken_library("aside-videos", |p| vec![p.source_video(1, "mp4"), p.source_received(1, "mp4")]);
    let aside = set_aside_and_start(&paths);
    kept_aside(&paths, &aside, &files);
}

/// The same for its VODs' loudness over time, read from audio that may be
/// gone since (`Paths::peaks`).
#[test]
fn a_library_set_aside_keeps_its_loudness_at_the_next_start() {
    let (_dir, paths, files) = broken_library("aside-peaks", |p| vec![p.peaks().join("1.peaks")]);
    let aside = set_aside_and_start(&paths);
    kept_aside(&paths, &aside, &files);
}

/// The same for its clips' and edits' files, the names they were handed
/// out by, and the list of those it couldn't delete: a new library's clip 1
/// would write over the old one's.
#[test]
fn a_library_set_aside_keeps_its_clips_at_the_next_start() {
    let (_dir, paths, files) = broken_library("aside-clips", |p| {
        vec![p.clip_file(1, "mp4"), p.clip_names(1).join("Day 3 clip.mp4"), p.leftovers()]
    });
    let aside = set_aside_and_start(&paths);
    kept_aside(&paths, &aside, &files);
}

/// The same for the copies of its file kept from before an upgrade
/// (`library-v<N>.sqlite`), often the way back from an upgrade that broke
/// it: the new library's first open deletes those over 30 days old.
#[test]
fn a_library_set_aside_keeps_its_copies_from_before_an_upgrade() {
    let (_dir, paths, files) = broken_library("aside-copies", |p| vec![p.root.join("library-v40.sqlite")]);
    let month_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 24 * 3600);
    std::fs::File::options().write(true).open(&files[0]).unwrap().set_modified(month_ago).unwrap();
    let aside = set_aside_and_start(&paths);
    kept_aside(&paths, &aside, &files);
}

/// The same for what else the start, or the queues after it, tidy by the
/// library's rows: its VODs' pictures, their storyboards, the footage its
/// edits use, its clips' first frames and its downloads under way.
#[test]
fn a_library_set_aside_keeps_its_pictures_and_footage_at_the_next_start() {
    let (_dir, paths, files) = broken_library("aside-rest", |p| {
        vec![
            p.source_thumb("1-1700000000000.jpg"),
            p.source_storyboard(1, Path::new("/videos/Day 3.mp4")),
            p.source_footage().join(format!("{}.mkv", vodscribe_core::footage::section_stem(1, 0.0, 60.0))),
            p.clip_thumb(1),
            p.source_audio(1, "m4a.part"),
        ]
    });
    let aside = set_aside_and_start(&paths);
    kept_aside(&paths, &aside, &files);
}
