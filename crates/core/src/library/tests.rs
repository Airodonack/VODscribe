use super::*;
use crate::Paths;
use crate::db::ClipStatus;
use crate::edits::tests::info;
use crate::testkit::old_column;
use std::fs;

thread_local! {
    /// Hard links fail, as from another disk (`hard_link`).
    pub(super) static NO_LINKS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The format 0.7.1 kept its library in, before `MIGRATE_BUILT_PATHS`.
const FORMAT_071: usize = 36;

/// `db`, its files settled as the app does at start.
fn settled(db: Db) -> Db {
    settle(&db).unwrap();
    db
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn text(path: impl AsRef<Path>) -> String {
    String::from_utf8(fs::read(path).unwrap()).unwrap()
}

/// A path as another way of writing it would give it: in capitals on
/// Windows and macOS, whose filesystems don't regard case, with
/// backslashes on Windows.
fn written_otherwise(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.to_uppercase().replace('/', "\\")
    } else if cfg!(target_os = "macos") {
        s.to_uppercase()
    } else {
        s.into_owned()
    }
}

fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// An empty library in a folder of its own, with 0.7.1's clip folder
/// beside it (`root/Videos/VODscribe`).
fn new_library(name: &str) -> (crate::TempDir, Paths) {
    let root = crate::TempDir::new(name);
    let paths = Paths::new(root.join("lib")).with_old_clips(root.join("Videos/VODscribe"));
    paths.ensure().unwrap();
    (root, paths)
}

/// A VOD added to the library `paths` and the library left as 0.7.1 keeps
/// one: the columns 0.7.2 added dropped. Rows are added in `add`, which is
/// given the database before the drop, and its answer given back.
fn as_071<T>(paths: &Paths, add: impl FnOnce(&Db) -> T) -> T {
    let db = settled(Db::open_in(paths).unwrap());
    let added = add(&db);
    drop(db);
    crate::testkit::back_to(&paths.db(), FORMAT_071);
    added
}

fn vod_in(db: &Db, name: &str) -> i64 {
    db.add_vod(&info(name)).unwrap().0.id
}

/// A kept video recorded where 0.7.1 put it, in the library's folder for
/// them: renamed to where it's built, whatever case the path was written in.
/// (0.7.2 Fixes: "A moved library finds its files."; `library.rs`'s module
/// doc.)
#[test]
fn a_kept_video_recorded_before_072_goes_where_it_is_built() {
    let (_root, paths) = new_library("settle-video");
    let video = paths.media().join("A talk [000001].mp4");
    write(&video, b"video");
    let vod = as_071(&paths, |db| {
        let vod = vod_in(db, "kept");
        let sql = "UPDATE vods SET media_path = ?2 WHERE id = ?1";
        db.conn.execute(sql, params![vod, written_otherwise(&video)]).unwrap();
        vod
    });
    let db = settled(Db::open_in(&paths).unwrap());
    assert_eq!(db.media_path(vod).unwrap(), Some(crate::db::shown(&paths.source_video(vod, "mp4"))));
    assert_eq!(text(paths.source_video(vod, "mp4")), "video");
}

/// A VOD's downloaded audio recorded where 0.7.1 put it: its extension
/// recorded, its path the one it's built at (0.7.2 Fixes: "A moved library
/// finds its files."; `library.rs`'s module doc).
#[test]
fn a_vods_audio_recorded_before_072_goes_where_it_is_built() {
    let (_root, paths) = new_library("settle-audio");
    let vod = as_071(&paths, |db| vod_in(db, "downloaded"));
    let audio = paths.work().join(format!("{vod}.webm"));
    write(&audio, b"audio");
    let sql = "UPDATE vods SET audio_path = ?2 WHERE id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![vod, audio.to_string_lossy()]).unwrap();
    let db = settled(Db::open_in(&paths).unwrap());
    assert_eq!(db.audio_path(vod).unwrap(), Some(crate::db::shown(&paths.source_audio(vod, "webm"))));
    assert_eq!(text(paths.source_audio(vod, "webm")), "audio");
}

/// A file received from another app's drop, recorded where 0.7.1 put it
/// (0.7.2 Fixes: "A moved library finds its files."; `library.rs`'s module
/// doc): it's the library's own, so it moves to where it's built, and stays
/// the VOD's received file and its audio.
#[test]
fn a_received_file_recorded_before_072_goes_where_it_is_built() {
    let (_root, paths) = new_library("settle-received");
    let dropped = paths.received().join("from a zip.mkv");
    write(&dropped, b"dropped");
    let vod = as_071(&paths, |db| {
        let vod = vod_in(db, "received");
        let sql = "UPDATE vods SET source_path = ?2, audio_path = ?2 WHERE id = ?1";
        db.conn.execute(sql, params![vod, dropped.to_string_lossy()]).unwrap();
        vod
    });
    let db = settled(Db::open_in(&paths).unwrap());
    let moved = db.vod(vod).unwrap().unwrap();
    assert!(moved.received);
    assert_eq!(moved.source_path, Some(crate::db::shown(&paths.source_received(vod, "mkv"))));
    assert_eq!(text(moved.source_path.clone().unwrap()), "dropped");
    assert_eq!(db.audio_path(vod).unwrap(), moved.source_path);
}

/// A section of a VOD (a transcribed stretch's audio) recorded where 0.7.1
/// put it, in the work folder (0.7.2 Fixes: "A moved library finds its
/// files."; `library.rs`'s module doc).
#[test]
fn a_section_recorded_before_072_goes_where_it_is_built() {
    let (_root, paths) = new_library("settle-section");
    let (vod, stretch) = as_071(&paths, |db| {
        let vod = vod_in(db, "downloaded");
        (vod, db.request(vod, 10.0, Some(20.0)).unwrap().id)
    });
    let section = paths.work().join(format!("{vod}.s{stretch}.mka"));
    write(&section, b"section");
    let sql = "UPDATE stretches SET audio = ?2 WHERE id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![stretch, section.to_string_lossy()]).unwrap();
    let db = settled(Db::open_in(&paths).unwrap());
    let moved = db.stretch(stretch).unwrap().unwrap().audio;
    assert_eq!(moved, Some(crate::db::shown(&paths.source_section(vod, stretch, "mka"))));
    assert_eq!(text(moved.unwrap()), "section");
}

/// A clip's and an edit's file, as 0.7.1 made them in the Videos folder,
/// named for their titles: each is its file at the id's name in the
/// library, and still ready (0.7.2 Fixes: "A moved library finds its
/// files."; `library.rs`'s module doc).
#[test]
fn a_clips_or_edits_file_recorded_before_072_goes_where_it_is_built() {
    let (root, paths) = new_library("settle-clip-edit");
    let (clip, edit) = as_071(&paths, |db| {
        let vod = vod_in(db, "clipped");
        (db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id, db.add_edit("e").unwrap().id)
    });
    let made = root.join("Videos/VODscribe/Channel/2026-10-15 A talk 00h00m01.000s-00h00m02.000s.mp4");
    let rendered = root.join("Videos/VODscribe/e.m4a");
    write(&made, b"clip");
    write(&rendered, b"edit");
    let conn = rusqlite::Connection::open(paths.db()).unwrap();
    for (id, file) in [(clip, &made), (edit, &rendered)] {
        let sql = "UPDATE renders SET file_path = ?2, status = 'ready' WHERE clip_id = ?1";
        conn.execute(sql, params![id, file.to_string_lossy()]).unwrap();
    }
    drop(conn);
    let db = settled(Db::open_in(&paths).unwrap());
    let c = db.clip(clip).unwrap().unwrap();
    assert_eq!(
        (c.status, c.file_path.clone()),
        (ClipStatus::Ready, Some(crate::db::shown(&paths.clip_file(clip, "mp4"))))
    );
    assert_eq!(text(c.file_path.unwrap()), "clip");
    let e = db.edit(edit, crate::edits::tests::at_first()).unwrap().unwrap();
    assert_eq!(e.file_path, Some(crate::db::shown(&paths.clip_file(edit, "m4a"))));
    assert_eq!(text(e.file_path.unwrap()), "edit");
}

/// The old columns keep the full path of each file where it's built, so
/// 0.7.1, opened on this library, finds it (`library.rs`'s module doc).
#[test]
fn a_settled_rows_old_column_names_its_new_place_for_071() {
    let (root, paths) = new_library("settle-old-columns");
    let (vod, stretch, clip) = as_071(&paths, |db| {
        let vod = vod_in(db, "v");
        (vod, db.request(vod, 10.0, Some(20.0)).unwrap().id, db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id)
    });
    let video = paths.media().join(format!("Talk [{vod:06}].mp4"));
    let section = paths.work().join(format!("{vod}.s{stretch}.mka"));
    let made = root.join("Videos/VODscribe/Channel/c.mp4");
    for file in [&video, &section, &made] {
        write(file, b"x");
    }
    let conn = rusqlite::Connection::open(paths.db()).unwrap();
    conn.execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![vod, video.to_string_lossy()]).unwrap();
    conn.execute("UPDATE stretches SET audio = ?2 WHERE id = ?1", params![stretch, section.to_string_lossy()]).unwrap();
    let sql = "UPDATE renders SET file_path = ?2, status = 'ready' WHERE clip_id = ?1";
    conn.execute(sql, params![clip, made.to_string_lossy()]).unwrap();
    drop(conn);
    drop(settled(Db::open_in(&paths).unwrap()));
    let shown = |p: PathBuf| Some(crate::db::shown(&p));
    assert_eq!(old_column(&paths, "vods", "media_path", vod), shown(paths.source_video(vod, "mp4")));
    assert_eq!(old_column(&paths, "stretches", "audio", stretch), shown(paths.source_section(vod, stretch, "mka")));
    assert_eq!(old_column(&paths, "renders", "file_path", clip), shown(paths.clip_file(clip, "mp4")));
}

/// What 0.7.1 records or forgets after it's gone back to is taken in at the
/// next start: a kept video it recorded under its own name is moved again,
/// recorded where it went, and a clip file it deleted is forgotten; a row
/// done before stays as it is (`library.rs`'s module doc).
#[test]
fn a_file_071_recorded_after_going_back_is_moved_again() {
    let (_root, paths) = new_library("settle-back");
    let (a, b, clip) = {
        let db = settled(Db::open(&paths.db()).unwrap());
        let a = vod_in(&db, "a");
        let b = vod_in(&db, "b");
        let video = paths.source_video(a, "mp4");
        write(&video, b"video");
        db.set_media_path(a, Some(&crate::db::shown(&video))).unwrap();
        let clip = db.add_clip(a, 1.0, 2.0, "c", true).unwrap().id;
        let file = paths.clip_file(clip, "mp4");
        write(&file, b"clip");
        db.set_clip_file(clip, 1.0, 2.0, ClipStatus::Ready, Some(&crate::db::shown(&file)), None).unwrap();
        (a, b, clip)
    };
    let media = |id| old_column(&paths, "vods", "media_path", id);
    let done = media(a);
    // 0.7.1 at work: a video kept for b under its own name, and the clip's file deleted.
    let kept = paths.media().join(format!("B [{b:06}].webm"));
    write(&kept, b"b video");
    {
        let conn = rusqlite::Connection::open(paths.db()).unwrap();
        conn.execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![b, crate::db::shown(&kept)]).unwrap();
        conn.execute("UPDATE renders SET file_path = NULL WHERE clip_id = ?1", [clip]).unwrap();
    }
    let db = settled(Db::open(&paths.db()).unwrap());
    assert_eq!(db.media_path(b).unwrap(), Some(crate::db::shown(&paths.source_video(b, "webm"))));
    assert_eq!(media(b), db.media_path(b).unwrap(), "recorded where it went, for 0.7.1 too");
    assert_eq!(text(paths.source_video(b, "webm")), "b video");
    assert_eq!(db.clip(clip).unwrap().unwrap().file_path, None, "forgotten, as 0.7.1 forgot it");
    assert_eq!(db.media_path(a).unwrap(), done, "a done row stays as it is");
}

/// A crash between a file's move and its row's change leaves the file
/// moved and the row as it was: the next start finishes it (`library.rs`'s
/// module doc).
#[test]
fn a_move_cut_short_is_finished_at_the_next_start() {
    let (_root, paths) = new_library("settle-crash");
    let id = as_071(&paths, |db| vod_in(db, "x"));
    let old = paths.media().join("Old name [000001].mp4");
    rusqlite::Connection::open(paths.db())
        .unwrap()
        .execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![id, crate::db::shown(&old)])
        .unwrap();
    // Moved, then the crash.
    write(&paths.source_video(id, "mp4"), b"video");
    let db = settled(Db::open_in(&paths).unwrap());
    assert_eq!(db.media_path(id).unwrap(), Some(crate::db::shown(&paths.source_video(id, "mp4"))));
    // And opening again changes nothing.
    drop(db);
    let db = settled(Db::open_in(&paths).unwrap());
    assert_eq!(db.media_path(id).unwrap(), Some(crate::db::shown(&paths.source_video(id, "mp4"))));
}

/// A link a crash cut short (`<id>.settling`) goes at the next start
/// (library.rs, `remove_leftovers`).
#[test]
fn a_link_a_crash_cut_short_goes_at_the_next_start() {
    let (_root, paths) = new_library("settling");
    let left = [paths.clips().join("7.settling"), paths.media().join("3.settling")];
    for file in &left {
        write(file, b"half");
    }
    drop(settled(Db::open_in(&paths).unwrap()));
    for file in &left {
        assert!(!file.exists(), "{} left", file.display());
    }
}

/// A file in the library's own folders is renamed to where it goes (the
/// same disk): moved, not copied (`library.rs`'s module doc).
#[test]
fn a_file_inside_the_library_is_renamed() {
    let (_root, paths) = new_library("settle-rename");
    let vod = as_071(&paths, |db| vod_in(db, "kept"));
    let old = paths.media().join(format!("Old [{vod:06}].mp4"));
    write(&old, b"video");
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&old).unwrap());
    let sql = "UPDATE vods SET media_path = ?2 WHERE id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![vod, old.to_string_lossy()]).unwrap();
    drop(settled(Db::open_in(&paths).unwrap()));
    let to = paths.source_video(vod, "mp4");
    assert_eq!(text(&to), "video");
    assert!(!old.exists(), "the old name is gone");
    #[cfg(unix)]
    assert_eq!(std::os::unix::fs::MetadataExt::ino(&fs::metadata(&to).unwrap()), inode, "the same file, not a copy");
}

/// A clip 0.7.1 made in its folder (the Videos folder) is linked, the same
/// file under a second name, and the original stays for whoever uses it, an
/// editor's project (`library.rs`'s module doc).
#[test]
fn a_clip_in_071s_folder_is_linked_and_the_original_stays_for_its_editor() {
    let (root, paths) = new_library("settle-link");
    let clip = as_071(&paths, |db| {
        let vod = vod_in(db, "v");
        db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id
    });
    let made = root.join("Videos/VODscribe/Channel/2026-10-15 A talk 00h00m01.000s-00h00m02.000s.mp4");
    write(&made, b"clip");
    let sql = "UPDATE renders SET file_path = ?2, status = 'ready' WHERE clip_id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![clip, made.to_string_lossy()]).unwrap();
    drop(settled(Db::open_in(&paths).unwrap()));
    let linked = paths.clip_file(clip, "mp4");
    assert_eq!(text(&linked), "clip");
    assert_eq!(text(&made), "clip", "the original stays");
    // The same file: a change through one shows in the other.
    fs::write(&made, b"changed").unwrap();
    assert_eq!(text(&linked), "changed", "a link, not a copy");
}

/// A clip 0.7.1 made in the Videos folder on another disk can't be
/// linked in: copying them all would hold up the first start, so the
/// row stays as 0.7.1 wrote it and the clip is read from there; the
/// file stays when the clip goes (`own_file` in the app) (`library.rs`'s
/// module doc).
#[test]
fn a_clip_on_another_disk_is_read_where_071_made_it() {
    let (root, paths) = new_library("settle-other-disk");
    let made = root.join("Videos/VODscribe/Channel/2026-10-15 A talk 00h00m01.000s-00h00m02.000s.mp4");
    write(&made, b"clip");
    let clip = as_071(&paths, |db| {
        let vod = vod_in(db, "v");
        db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id
    });
    let sql = "UPDATE renders SET file_path = ?2, status = 'ready' WHERE clip_id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![clip, crate::db::shown(&made)]).unwrap();
    NO_LINKS.set(true);
    let db = settled(Db::open_in(&paths).unwrap());
    NO_LINKS.set(false);
    let c = db.clip(clip).unwrap().unwrap();
    assert_eq!((c.status, c.file_path), (ClipStatus::Ready, Some(crate::db::shown(&made))));
    assert!(!paths.clip_file(clip, "mp4").exists(), "not copied");
    // On the same disk, at a later start, it's linked in.
    drop(db);
    let db = settled(Db::open_in(&paths).unwrap());
    let c = db.clip(clip).unwrap().unwrap();
    assert_eq!(c.file_path, Some(crate::db::shown(&paths.clip_file(clip, "mp4"))));
    assert_eq!((text(c.file_path.unwrap()), text(&made)), ("clip".into(), "clip".into()));
}

/// 0.7.1, gone back to, makes a clip that was handed out again: its new
/// file in the Videos folder, the old one left at clips/<id>.mp4 for the
/// app that took it. Forward again, the clip is its new file (`library.rs`'s
/// module doc).
#[test]
fn a_clip_made_again_by_071_is_its_new_file() {
    let (root, paths) = new_library("settle-stale");
    let clip = {
        let db = settled(Db::open_in(&paths).unwrap());
        let vod = vod_in(&db, "v");
        let clip = db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id;
        let file = paths.clip_file(clip, "mp4");
        write(&file, b"before the resize");
        db.set_clip_file(clip, 1.0, 2.0, ClipStatus::Ready, Some(&crate::db::shown(&file)), None).unwrap();
        db.hand_out_clip(clip).unwrap();
        clip
    };
    // 0.7.1: resized, made again under its title in the Videos folder;
    // handed out, the old file stays (take_back_clip).
    let new = root.join("Videos/VODscribe/Channel/2026-11-06 c 00h00m01.000s-00h00m03.000s.mp4");
    write(&new, b"after the resize");
    {
        let conn = rusqlite::Connection::open(paths.db()).unwrap();
        conn.execute(
            "UPDATE renders SET file_path = ?2, handed_out = 0, status = 'ready' WHERE clip_id = ?1",
            params![clip, crate::db::shown(&new)],
        )
        .unwrap();
    }
    let db = settled(Db::open_in(&paths).unwrap());
    let c = db.clip(clip).unwrap().unwrap();
    assert_eq!(text(c.file_path.unwrap()), "after the resize");
    assert_eq!(text(&new), "after the resize", "the original stays");
}

/// The point of it all: a library copied to another folder (another
/// computer's, a renamed home folder) finds every file of its own, those
/// recorded by 0.7.1 too, though the folder they were in is gone
/// (`library.rs`'s module doc).
#[test]
fn a_library_copied_elsewhere_finds_its_files_in_its_own_folders() {
    let root = crate::TempDir::new("moved");
    let before = Paths::new(root.join("before"));
    before.ensure().unwrap();
    let (vod, old_vod, clip, edit) = {
        let db = settled(Db::open(&before.db()).unwrap());
        let vod = vod_in(&db, "new");
        let old_vod = vod_in(&db, "old");
        let video = before.source_video(vod, "mp4");
        write(&video, b"video");
        db.set_media_path(vod, Some(&crate::db::shown(&video))).unwrap();
        let clip = db.add_clip(vod, 1.0, 2.0, "c", true).unwrap();
        let file = before.clip_file(clip.id, "mp4");
        write(&file, b"clip");
        db.set_clip_file(clip.id, 1.0, 2.0, ClipStatus::Ready, Some(&crate::db::shown(&file)), None).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        let render = before.clip_file(edit, "m4a");
        write(&render, b"edit");
        db.set_edit_render(edit, ClipStatus::Ready, Some(&crate::db::shown(&render)), None, None).unwrap();
        // One as 0.7.1 recorded it, its path in the folder that goes
        // (left by a 0.7.1 run on a library 0.7.2 had opened).
        let old = before.media().join(format!("Old [{old_vod:06}].webm"));
        write(&old, b"old video");
        db.conn
            .execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![old_vod, crate::db::shown(&old)])
            .unwrap();
        (vod, old_vod, clip.id, edit)
    };
    let after = Paths::new(root.join("after"));
    copy_folder(&before.root, &after.root);
    fs::remove_dir_all(&before.root).unwrap();

    let db = settled(Db::open(&after.db()).unwrap());
    let there = |p: Option<String>| p.filter(|p| Path::new(p).is_file()).map(text);
    assert_eq!(there(db.media_path(vod).unwrap()).as_deref(), Some("video"));
    assert_eq!(there(db.playable_path(vod).unwrap()).as_deref(), Some("video"));
    assert_eq!(there(db.media_path(old_vod).unwrap()).as_deref(), Some("old video"));
    assert_eq!(there(db.clip(clip).unwrap().unwrap().file_path).as_deref(), Some("clip"));
    assert_eq!(
        there(db.edit(edit, crate::edits::tests::at_first()).unwrap().unwrap().file_path).as_deref(),
        Some("edit")
    );
}

/// A 0.7.1 library copied, the original kept: its kept video is the
/// copy's own file, not a link to the original's beside its own
/// (`library.rs`'s module doc).
#[test]
fn a_copied_library_takes_its_own_copy_over_the_others() {
    let root = crate::TempDir::new("settle-copied");
    let before = Paths::new(root.join("before"));
    before.ensure().unwrap();
    let vod = as_071(&before, |db| vod_in(db, "v"));
    let old = before.media().join(format!("Title [{vod:06}].mp4"));
    write(&old, b"video");
    let sql = "UPDATE vods SET media_path = ?2 WHERE id = ?1";
    rusqlite::Connection::open(before.db()).unwrap().execute(sql, params![vod, crate::db::shown(&old)]).unwrap();
    let after = Paths::new(root.join("after"));
    copy_folder(&before.root, &after.root);
    let db = settled(Db::open(&after.db()).unwrap());
    assert_eq!(text(db.media_path(vod).unwrap().unwrap()), "video");
    let left = after.media().join(format!("Title [{vod:06}].mp4"));
    assert!(!left.exists(), "the copy's own file left behind, a second copy of the video");
    assert!(old.exists(), "the original's untouched");
}

/// A clip whose file was deleted outside the app before the upgrade reads
/// as gone, not as Ready with no file: the clip bar lets it go, as before
/// (`forget_gone_clips`); a file found nowhere is forgotten (`library.rs`'s
/// module doc).
#[test]
fn a_file_found_nowhere_is_forgotten_and_a_clip_is_left_to_be_seen_gone() {
    let (root, paths) = new_library("settle-gone");
    let lost = crate::db::shown(&root.join("Videos/VODscribe/lost.mp4"));
    let (vod, clip) = as_071(&paths, |db| {
        let vod = vod_in(db, "v");
        (vod, db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id)
    });
    fs::create_dir_all(root.join("Videos/VODscribe")).unwrap();
    let conn = rusqlite::Connection::open(paths.db()).unwrap();
    conn.execute("UPDATE renders SET file_path = ?2, status = 'ready' WHERE clip_id = ?1", params![clip, lost])
        .unwrap();
    // A kept video in the library's folder, but not there.
    let missing = paths.media().join(format!("Missing [{vod:06}].mp4"));
    conn.execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![vod, crate::db::shown(&missing)]).unwrap();
    drop(conn);
    let db = settled(Db::open_in(&paths).unwrap());
    assert_eq!(db.media_path(vod).unwrap(), None, "forgotten");
    assert_eq!(old_column(&paths, "vods", "media_path", vod), None, "0.7.1's column too");
    let c = db.clip(clip).unwrap().unwrap();
    assert!(
        !(c.status == ClipStatus::Ready && c.file_path.is_none()),
        "Ready with no file: never made again, and forget_gone_clips skips it"
    );
    let file = c.file_path.unwrap();
    assert!(!Path::new(&file).exists() && Path::new(&file).parent().unwrap().is_dir(), "{file}: gone, as the bar sees");
}

/// A library reached through a link (macOS's temporary folder is under
/// /var, a link to /private/var; a home folder moved and linked back) is
/// still the library's folder for a file that isn't there: found nowhere,
/// it's forgotten in 0.7.1's column too, as in any library (`library.rs`'s
/// module doc).
#[cfg(unix)]
#[test]
fn a_file_found_nowhere_in_a_library_under_a_link_is_forgotten_in_both_columns() {
    let root = crate::TempDir::new("settle-gone-link");
    fs::create_dir_all(root.join("real")).unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
    let paths = Paths::new(root.join("link/lib"));
    paths.ensure().unwrap();
    let vod = as_071(&paths, |db| vod_in(db, "v"));
    let missing = paths.media().join(format!("Missing [{vod:06}].mp4"));
    let sql = "UPDATE vods SET media_path = ?2 WHERE id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![vod, crate::db::shown(&missing)]).unwrap();
    let db = settled(Db::open_in(&paths).unwrap());
    assert_eq!(db.media_path(vod).unwrap(), None, "forgotten");
    assert_eq!(old_column(&paths, "vods", "media_path", vod), None, "0.7.1's column too");
}

/// settle takes files only from the library's own folders and 0.7.1's
/// clip folder: a row naming another file of the user's, or a network
/// path (never looked at), is left as it was, the file where it is
/// ("nothing of the user's is brought into the library", `library.rs`'s
/// module doc).
#[test]
fn a_row_naming_another_folder_is_left_its_path_never_looked_at() {
    let (root, paths) = new_library("settle-elsewhere");
    let private = root.join("Documents/taxes.pdf");
    write(&private, b"private");
    let (vod, clip, stretch) = as_071(&paths, |db| {
        let vod = vod_in(db, "v");
        (vod, db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id, db.request(vod, 10.0, Some(20.0)).unwrap().id)
    });
    let conn = rusqlite::Connection::open(paths.db()).unwrap();
    let sql = "UPDATE vods SET media_path = ?2, audio_path = ?3 WHERE id = ?1";
    conn.execute(sql, params![vod, crate::db::shown(&private), r"\\server\share\a.m4a"]).unwrap();
    let sql = "UPDATE renders SET file_path = ?2, status = 'ready' WHERE clip_id = ?1";
    conn.execute(sql, params![clip, "//server/share/c.mp4"]).unwrap();
    conn.execute("UPDATE stretches SET audio = ?2 WHERE id = ?1", params![stretch, r"\\?\UNC\server\share\s.mka"])
        .unwrap();
    drop(conn);
    let db = settled(Db::open_in(&paths).unwrap());
    assert!(!paths.source_video(vod, "pdf").exists(), "the user's file brought into the library");
    assert_eq!(db.media_path(vod).unwrap(), None);
    assert_eq!(text(&private), "private");
    let old = |sql: &str, id: i64| -> Option<String> { db.conn.query_row(sql, [id], |r| r.get(0)).unwrap() };
    assert_eq!(old("SELECT media_path FROM vods WHERE id = ?1", vod), Some(crate::db::shown(&private)), "kept");
    assert_eq!(old("SELECT audio_path FROM vods WHERE id = ?1", vod).as_deref(), Some(r"\\server\share\a.m4a"));
    assert_eq!(old("SELECT file_path FROM renders WHERE clip_id = ?1", clip).as_deref(), Some("//server/share/c.mp4"));
    assert_eq!(
        old("SELECT audio FROM stretches WHERE id = ?1", stretch).as_deref(),
        Some(r"\\?\UNC\server\share\s.mka")
    );
}

/// A library from someone else, its extension columns crafted to name
/// a file outside it (`clips/<id>./../../../victim.txt`): read as no file,
/// or as one where the library builds it, so deleting the clip or the
/// VOD's files deletes nothing of the user's ("nothing of the user's is
/// brought into the library", `library.rs`'s module doc).
#[test]
fn a_crafted_extension_never_names_a_file_outside_the_library() {
    let (root, paths) = new_library("crafted-ext");
    let victim = root.join("victim.txt");
    write(&victim, b"the user's own");
    let ext = "/../../../victim.txt";
    let (vod, clip, stretch) = {
        let db = settled(Db::open_in(&paths).unwrap());
        let vod = vod_in(&db, "v");
        let clip = db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id;
        let stretch = db.request(vod, 10.0, Some(20.0)).unwrap().id;
        // Each old column set to the path its crafted extension builds,
        // so settle takes the row for done.
        let built = |p: PathBuf| crate::db::shown(&p);
        let sql = "UPDATE renders SET file_ext = ?2, file_path = ?3, status = 'ready' WHERE clip_id = ?1";
        db.conn.execute(sql, params![clip, ext, built(paths.clip_file(clip, ext))]).unwrap();
        db.conn
            .execute(
                "UPDATE vods SET video_ext = ?2, media_path = ?3, audio_ext = ?2, audio_path = ?4 WHERE id = ?1",
                params![vod, ext, built(paths.source_video(vod, ext)), built(paths.source_audio(vod, ext))],
            )
            .unwrap();
        let sql = "UPDATE stretches SET audio_ext = ?2, audio = ?3 WHERE id = ?1";
        db.conn.execute(sql, params![stretch, ext, built(paths.source_section(vod, stretch, ext))]).unwrap();
        (vod, clip, stretch)
    };
    // The crafted library's folders come with it: "<id>." in each.
    for dir in [paths.clips(), paths.media(), paths.work()] {
        fs::create_dir_all(dir.join(format!("{clip}."))).unwrap();
        fs::create_dir_all(dir.join(format!("{vod}."))).unwrap();
    }
    fs::create_dir_all(paths.work().join(format!("{vod}.s{stretch}."))).unwrap();
    let db = settled(Db::open_in(&paths).unwrap());
    let named = [
        db.clip(clip).unwrap().unwrap().file_path,
        db.media_path(vod).unwrap(),
        db.audio_path(vod).unwrap(),
        db.playable_path(vod).unwrap(),
        db.stretch(stretch).unwrap().unwrap().audio,
    ];
    for file in named.into_iter().flatten() {
        let real = fs::canonicalize(&file).ok();
        assert!(real != fs::canonicalize(&victim).ok(), "{file} is the user's file outside the library");
        // What deleting the clip or the VOD's files does with it.
        let _ = fs::remove_file(&file);
    }
    assert_eq!(text(&victim), "the user's own");
}

/// A VOD's file on this computer outside the library stays where it is,
/// its path kept: the user's own (`library.rs`'s module doc).
#[test]
fn a_vods_own_file_outside_the_library_stays_where_it_is() {
    let (root, paths) = new_library("settle-own");
    let song = root.join("Music/song.mp3");
    write(&song, b"song");
    let vod = as_071(&paths, |db| vod_in(db, "own"));
    let sql = "UPDATE vods SET source_path = ?2 WHERE id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![vod, song.to_string_lossy()]).unwrap();
    let db = settled(Db::open_in(&paths).unwrap());
    let own = db.vod(vod).unwrap().unwrap();
    assert_eq!((own.source_path, own.received), (Some(song.to_string_lossy().into_owned()), false));
    assert_eq!(text(&song), "song");
    assert!(!paths.source_received(vod, "mp3").exists(), "brought in as a received file");
}

/// 0.7.1 recorded a VOD's own file, or its kept video, as its audio too.
/// Such an audio isn't a file of its own to move: the row forgets it (the
/// audio is known from the file) and the file is left where it is
/// (library.rs, `own_files` and `step`).
#[test]
fn a_vods_audio_that_is_its_own_file_or_kept_video_is_left() {
    let (root, paths) = new_library("settle-audio-own");
    let (own, kept) = as_071(&paths, |db| (vod_in(db, "own"), vod_in(db, "kept")));
    let song = root.join("Music/song.mp3");
    let video = paths.media().join(format!("Talk [{kept:06}].mp4"));
    write(&song, b"song");
    write(&video, b"video");
    let conn = rusqlite::Connection::open(paths.db()).unwrap();
    let sql = "UPDATE vods SET source_path = ?2, audio_path = ?2 WHERE id = ?1";
    conn.execute(sql, params![own, song.to_string_lossy()]).unwrap();
    let sql = "UPDATE vods SET media_path = ?2, audio_path = ?2 WHERE id = ?1";
    conn.execute(sql, params![kept, video.to_string_lossy()]).unwrap();
    drop(conn);
    let db = settled(Db::open_in(&paths).unwrap());
    let audio = |id| {
        db.conn.query_row("SELECT audio_path FROM vods WHERE id = ?1", [id], |r| r.get::<_, Option<String>>(0)).unwrap()
    };
    assert_eq!((audio(own), audio(kept)), (None, None), "no audio of its own recorded");
    assert_eq!(db.audio_path(own).unwrap(), Some(song.to_string_lossy().into_owned()), "known from its file");
    assert_eq!(db.audio_path(kept).unwrap(), Some(crate::db::shown(&paths.source_video(kept, "mp4"))));
    assert_eq!(text(&song), "song");
    assert_eq!(text(paths.source_video(kept, "mp4")), "video");
    assert!(fs::read_dir(paths.work()).unwrap().next().is_none(), "nothing put in the audio folder");
}

/// A file received from a drop in a library that has moved: its row names
/// the folder it was in, which isn't here, and the file is known by the
/// name of that folder (`media/received`) and its name, found in this
/// library's own (library.rs, `received`).
#[test]
fn a_received_file_of_a_moved_library_is_known_by_its_folders_name() {
    let (root, paths) = new_library("settle-received-moved");
    let vod = as_071(&paths, |db| vod_in(db, "received"));
    write(&paths.received().join("from a zip.mkv"), b"dropped");
    let gone = root.join("old home/lib/media/received/from a zip.mkv");
    let sql = "UPDATE vods SET source_path = ?2 WHERE id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![vod, gone.to_string_lossy()]).unwrap();
    let db = settled(Db::open_in(&paths).unwrap());
    let moved = db.vod(vod).unwrap().unwrap();
    assert!(moved.received, "still a received file, not the user's own");
    assert_eq!(moved.source_path, Some(crate::db::shown(&paths.source_received(vod, "mkv"))));
    assert_eq!(text(paths.source_received(vod, "mkv")), "dropped");
}

/// A row whose file can't be moved (a disk full) is left to the next start,
/// and said: `settle` fails saying how many, the other rows are still
/// done, and the next start moves it (library.rs, `settle`). A folder that
/// refuses new names is the disk that refuses.
#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_moved_is_left_to_the_next_start_and_said() {
    use std::os::unix::fs::PermissionsExt;
    /// Lets the folder be written again, so the test's folder can go.
    struct Refusing(PathBuf);
    impl Drop for Refusing {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
    let (_root, paths) = new_library("settle-refused");
    let (vod, stretch) = as_071(&paths, |db| {
        let vod = vod_in(db, "v");
        (vod, db.request(vod, 10.0, Some(20.0)).unwrap().id)
    });
    let video = paths.media().join(format!("Old [{vod:06}].mp4"));
    let section = paths.work().join(format!("{vod}.s{stretch}.mka"));
    write(&video, b"video");
    write(&section, b"section");
    let conn = rusqlite::Connection::open(paths.db()).unwrap();
    conn.execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![vod, video.to_string_lossy()]).unwrap();
    conn.execute("UPDATE stretches SET audio = ?2 WHERE id = ?1", params![stretch, section.to_string_lossy()]).unwrap();
    drop(conn);
    let db = Db::open_in(&paths).unwrap();
    let refusing = Refusing(paths.media());
    fs::set_permissions(paths.media(), fs::Permissions::from_mode(0o555)).unwrap();
    let said = settle(&db).unwrap_err().to_string();
    assert_eq!(said, "1 not moved, left for the next start");
    assert_eq!(text(&video), "video", "left where it was");
    let recorded = old_column(&paths, "vods", "media_path", vod);
    assert_eq!(recorded, Some(video.to_string_lossy().into_owned()));
    assert_eq!(text(paths.source_section(vod, stretch, "mka")), "section", "the other row was done");
    drop(refusing);
    settle(&db).unwrap();
    assert_eq!(text(paths.source_video(vod, "mp4")), "video");
    assert_eq!(db.media_path(vod).unwrap(), Some(crate::db::shown(&paths.source_video(vod, "mp4"))));
}

/// Paths are inside a folder by whole names, as the filesystem compares
/// them: not by their start, not in any case on Windows and macOS, and a
/// network path (`\\server\share`, `//server/share`) is in no local folder
/// (library.rs, `named_inside` and `inside`).
#[test]
fn inside_is_by_whole_names_and_without_case_on_windows_and_macos_and_a_network_path_is_in_no_folder() {
    let root = crate::TempDir::new("inside");
    let (lib, lib2) = (root.join("lib"), root.join("lib2"));
    write(&lib.join("a.mp4"), b"");
    write(&lib2.join("a.mp4"), b"");
    assert!(inside(&lib.join("a.mp4"), &lib));
    assert!(!inside(&lib2.join("a.mp4"), &lib), "a sibling whose name starts the same");
    assert!(!inside(&lib, &lib), "the folder itself");
    assert!(!named_inside(&lib, &lib), "the folder itself, as written");
    assert!(!named_inside(&lib2.join("a.mp4"), &lib), "a sibling whose name starts the same, as written");
    assert!(named_inside(&lib.join("a.mp4"), &lib));
    let capitals = cfg!(any(windows, target_os = "macos")) || written_otherwise(&lib) == lib.to_string_lossy();
    assert_eq!(inside(Path::new(&written_otherwise(&lib.join("a.mp4"))), &lib), capitals);
    assert_eq!(named_inside(Path::new(&written_otherwise(&lib.join("a.mp4"))), &lib), capitals);
    for network in [r"\\server\share\a.mp4", "//server/share/a.mp4", r"\\?\UNC\server\share\a.mp4"] {
        assert!(!named_inside(Path::new(network), &lib), "{network}");
        assert!(!may_take(Path::new(network), &lib), "{network}");
    }
    let server = Path::new("//server/share");
    assert!(!may_take(&server.join("a.mp4"), &server.join("..")), "a network folder isn't a local one");
    assert!(may_take(&lib.join("a.mp4"), &lib));
    assert!(!may_take(&lib2.join("a.mp4"), &lib));
}

/// The rule of what the app may take or delete is looked at on disk too:
/// a link inside the folder to a file elsewhere isn't the app's to take
/// (the doc comment of `may_take` in library.rs).
#[cfg(unix)]
#[test]
fn a_link_inside_the_folder_to_a_file_elsewhere_is_not_the_apps_to_take() {
    let root = crate::TempDir::new("may-take-link");
    let (lib, elsewhere) = (root.join("lib"), root.join("elsewhere"));
    write(&lib.join("a.mp4"), b"");
    write(&elsewhere.join("b.mp4"), b"");
    std::os::unix::fs::symlink(elsewhere.join("b.mp4"), lib.join("link.mp4")).unwrap();
    assert!(may_take(&lib.join("a.mp4"), &lib));
    assert!(!may_take(&lib.join("link.mp4"), &lib));
}

/// A path with `.` or `..` in it isn't taken, though it would come to a
/// file inside the folder: it's written as inside only by whole names
/// (the doc comment of `named_inside` in library.rs).
#[test]
fn a_path_with_dots_in_it_is_not_the_apps_to_take() {
    let root = crate::TempDir::new("may-take-dots");
    let lib = root.join("lib");
    write(&lib.join("a.mp4"), b"");
    assert!(!may_take(&lib.join("sub/../a.mp4"), &lib));
}

/// A row done already (its old column is the path its extension builds) is
/// left as it is, though its file has since gone: settle forgets only what
/// 0.7.1 recorded and couldn't find (`library.rs`'s module doc).
#[test]
fn a_row_already_done_is_left_as_it_is_when_its_file_has_gone() {
    let (_root, paths) = new_library("settle-done");
    let vod = {
        let db = settled(Db::open_in(&paths).unwrap());
        let vod = vod_in(&db, "v");
        let video = paths.source_video(vod, "mp4");
        write(&video, b"video");
        db.set_media_path(vod, Some(&crate::db::shown(&video))).unwrap();
        vod
    };
    fs::remove_file(paths.source_video(vod, "mp4")).unwrap();
    drop(settled(Db::open_in(&paths).unwrap()));
    let built = Some(crate::db::shown(&paths.source_video(vod, "mp4")));
    assert_eq!(old_column(&paths, "vods", "media_path", vod), built);
    assert_eq!(old_column(&paths, "vods", "video_ext", vod).as_deref(), Some("mp4"));
}

/// A link in the library's folder to a file elsewhere is never taken into
/// the library, even when a row names a file of that name: "nothing of the
/// user's is brought into the library" (`library.rs`'s module doc).
#[cfg(unix)]
#[test]
fn a_link_in_the_library_to_the_users_file_is_not_taken_in() {
    let (root, paths) = new_library("settle-link-in");
    let vod = as_071(&paths, |db| vod_in(db, "v"));
    let private = root.join("Documents/talk.mp4");
    write(&private, b"private");
    fs::create_dir_all(paths.media()).unwrap();
    std::os::unix::fs::symlink(&private, paths.media().join("Talk.mp4")).unwrap();
    // A row from a library that has moved: its folder isn't here.
    let named = root.join("old home/lib/media/Talk.mp4");
    let sql = "UPDATE vods SET media_path = ?2 WHERE id = ?1";
    rusqlite::Connection::open(paths.db()).unwrap().execute(sql, params![vod, named.to_string_lossy()]).unwrap();
    drop(settled(Db::open_in(&paths).unwrap()));
    assert_eq!(text(&private), "private", "the user's file stays where it is");
    assert!(!paths.source_video(vod, "mp4").exists(), "brought into the library");
    assert_eq!(
        old_column(&paths, "vods", "media_path", vod),
        Some(named.to_string_lossy().into_owned()),
        "its row as it was"
    );
}

/// A file of the user's own that only looks like a moved library's received
/// one stays where it is, its path kept: it isn't in a `media/received`
/// folder, or is still there, though the library has a received file of
/// the same name (`library.rs`'s module doc and `received`).
#[test]
fn a_file_outside_the_library_named_like_a_received_one_stays_where_it_is() {
    let (root, paths) = new_library("settle-not-received");
    let ours = ["a.mkv", "b.mkv", "c.mkv"];
    for name in ours {
        write(&paths.received().join(name), b"ours");
    }
    let wrong_parent = root.join("home/lib/media/other/a.mkv");
    let wrong_grandparent = root.join("home/lib/data/received/b.mkv");
    let still_there = root.join("home/lib/media/received/c.mkv");
    write(&still_there, b"theirs");
    let outside = [&wrong_parent, &wrong_grandparent, &still_there];
    let vods = as_071(&paths, |db| ours.map(|name| vod_in(db, name)));
    let conn = rusqlite::Connection::open(paths.db()).unwrap();
    for (vod, path) in vods.iter().zip(outside) {
        let sql = "UPDATE vods SET source_path = ?2 WHERE id = ?1";
        conn.execute(sql, params![vod, path.to_string_lossy()]).unwrap();
    }
    drop(conn);
    let db = settled(Db::open_in(&paths).unwrap());
    for (vod, path) in vods.iter().zip(outside) {
        let v = db.vod(*vod).unwrap().unwrap();
        assert_eq!((v.source_path, v.received), (Some(path.to_string_lossy().into_owned()), false), "{path:?}");
    }
    for name in ours {
        assert_eq!(text(paths.received().join(name)), "ours", "{name} taken as another's");
    }
    assert_eq!(text(&still_there), "theirs");
}
