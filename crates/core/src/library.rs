//! Moves the library's files from where versions before 0.7.2 recorded them
//! to where `Paths` builds them now, once, as the library opens (`settle`).
//!
//! Before 0.7.2 each row kept the full path of its file: a kept video, a
//! VOD's downloaded audio, a section of it, a file received from another
//! app's drop, a clip's or an edit's file (in the Videos folder, named for
//! its title). A library moved to another folder, restored on another
//! computer or under a renamed home folder lost them all. Now no path to a
//! file of the library's own is stored: each is built from its kind and its
//! row's id, and the row keeps only the extension (`MIGRATE_BUILT_PATHS`).
//!
//! The old columns are the journal, and stay for versions before 0.7.2.
//! Each row is done on its own: its file put where it goes, then its row
//! changed in one statement, which sets the extension and writes the file's
//! new full path into the old column (so 0.7.1, opened on this library,
//! still finds it). A row is done when its old column is the path its
//! extension builds; any other value is a file still to move: one 0.7.1
//! recorded, before the upgrade or after going back to it. A crash between
//! the two steps leaves the file in its new place and the row as it was;
//! the next start finds the file there and finishes the row. The app's
//! writers keep both columns the same way (`Db::set_media_path`…); nothing
//! here reads the old columns but this.
//!
//! Where a file is put:
//! - Inside this library's folder, it's renamed (the same disk).
//! - Outside it (a clip in the Videos folder; a file still in the library's
//!   old folder, the library having been copied), it's linked (a hard link,
//!   the same file under a second name: no space taken, and the original
//!   stays for whoever uses it, an editor's project) or, where it can't be
//!   (another disk), copied. The original is never touched.
//! - Not where the row says, it's looked for in this library's folder of its
//!   kind by its name (a library copied or moved: the rows say the old
//!   folder).
//! - Nowhere, the row forgets it, as the app does a file gone missing.
//!
//! A VOD's file on this computer (`source_path`) outside the library stays
//! where it is, its path kept: the user's own.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::db::Db;

/// What became of a file `settle` moved.
#[derive(Debug, PartialEq, Eq)]
enum Placed {
    /// It's where it goes now.
    There,
    /// It wasn't anywhere.
    Gone,
}

/// A kind of file a row records: how its rows are read (id, its VOD's id,
/// the old column, the extension), where its files go, and how a row is
/// written once its file is there.
struct Kind {
    what: &'static str,
    rows: &'static str,
    folder: fn(&crate::Paths) -> PathBuf,
    built: fn(&crate::Paths, i64, i64, &str) -> PathBuf,
    write: &'static str,
}

/// Every kind, in order: a VOD's audio first, as where it's the VOD's own
/// file or kept video it's known without a row (and those move after it).
const KINDS: [Kind; 5] = [
    Kind {
        what: "audio",
        rows: "SELECT id, id, audio_path, audio_ext FROM vods
               WHERE audio_path IS NOT NULL OR audio_ext IS NOT NULL",
        folder: crate::Paths::work,
        built: |p, id, _, ext| p.source_audio(id, ext),
        write: "UPDATE vods SET audio_ext = ?2, audio_path = ?3 WHERE id = ?1",
    },
    Kind {
        what: "kept video",
        rows: "SELECT id, id, media_path, video_ext FROM vods
               WHERE media_path IS NOT NULL OR video_ext IS NOT NULL",
        folder: crate::Paths::media,
        built: |p, id, _, ext| p.source_video(id, ext),
        write: "UPDATE vods SET video_ext = ?2, media_path = ?3 WHERE id = ?1",
    },
    Kind {
        what: "received file",
        rows: "SELECT id, id, source_path, received_ext FROM vods
               WHERE source_path IS NOT NULL OR received_ext IS NOT NULL",
        folder: crate::Paths::received,
        built: |p, id, _, ext| p.source_received(id, ext),
        write: "UPDATE vods SET received_ext = ?2, source_path = ?3 WHERE id = ?1",
    },
    Kind {
        what: "section",
        rows: "SELECT id, vod_id, audio, audio_ext FROM stretches WHERE audio IS NOT NULL OR audio_ext IS NOT NULL",
        folder: crate::Paths::work,
        built: |p, id, vod_id, ext| p.source_section(vod_id, id, ext),
        write: "UPDATE stretches SET audio_ext = ?2, audio = ?3 WHERE id = ?1",
    },
    Kind {
        what: "file",
        rows: "SELECT clip_id, clip_id, file_path, file_ext FROM renders
               WHERE file_path IS NOT NULL OR file_ext IS NOT NULL",
        folder: crate::Paths::clips,
        built: |p, id, _, ext| p.clip_file(id, ext),
        write: "UPDATE renders SET file_ext = ?2, file_path = ?3 WHERE clip_id = ?1",
    },
];

/// A row of a `Kind`: its id, its VOD's id, its old column and its extension.
type Row = (i64, i64, Option<String>, Option<String>);

/// What's to be done with a row of `kind`.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// Its file is where its extension builds, and its old column says so.
    Done,
    /// Its old column names a file to move (0.7.1 recorded it).
    Move(String),
    /// 0.7.1 forgot its file (gone back to, it deleted it): so does the row.
    Forget,
    /// None of the library's: a VOD's audio that's its own file or kept
    /// video (known without a row), or a file of the user's own.
    Leave,
}

fn step(files: &crate::Paths, kind: &Kind, (id, vod_id, old, ext): &Row, own: &[Option<String>]) -> Step {
    let built = ext.as_deref().map(|ext| crate::db::shown(&(kind.built)(files, *id, *vod_id, ext)));
    match (old, built) {
        (Some(old), Some(built)) if *old == built => Step::Done,
        (None, Some(_)) => Step::Forget,
        (None, None) => Step::Leave,
        (Some(old), _) if kind.what == "audio" && own.iter().any(|o| o.as_deref() == Some(old.as_str())) => {
            Step::Forget
        }
        // A file of the user's own stays where it is, its path kept.
        (Some(old), _) if kind.what == "received file" && !received(files, Path::new(old)) => Step::Leave,
        (Some(old), _) => Step::Move(old.clone()),
    }
}

/// The paths a VOD's audio may be recorded as that are its own file or kept
/// video (0.7.1 recorded those as its audio): its source and its video, as
/// recorded.
fn own_files(db: &Db, vod_id: i64) -> Result<Vec<Option<String>>> {
    let sql = "SELECT source_path, media_path FROM vods WHERE id = ?1";
    let row: Option<(Option<String>, Option<String>)> =
        db.conn.query_row(sql, [vod_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    Ok(row.map(|(a, b)| vec![a, b]).unwrap_or_default())
}

/// Moves every file still recorded where 0.7.1 put it (`Step::Move`) to
/// where it goes now, and records it there. Each row on its own; a row whose
/// file can't be moved (a disk full) is left to the next start, and said.
pub fn settle(db: &Db) -> Result<()> {
    let files = db.files().clone();
    let mut failed = 0;
    for kind in &KINDS {
        for row in select(db, kind.rows, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))? {
            let (id, vod_id) = (row.0, row.1);
            let done = (|| {
                let none: Option<String> = None;
                match step(&files, kind, &row, &own_files(db, vod_id)?) {
                    Step::Done | Step::Leave => {}
                    Step::Forget => {
                        db.conn.execute(kind.write, params![id, none, none])?;
                    }
                    Step::Move(old) => {
                        let built = |ext: &str| (kind.built)(&files, id, vod_id, ext);
                        let ext = settle_file(&old, &(kind.folder)(&files), &files.local, built)?;
                        let path = ext.as_deref().map(|ext| crate::db::shown(&built(ext)));
                        db.conn.execute(kind.write, params![id, ext, path])?;
                    }
                }
                anyhow::Ok(())
            })();
            if let Err(e) = done {
                eprintln!("{} {id}'s {}: not moved to where it goes now: {e:#}", kind_owner(kind), kind.what);
                failed += 1;
            }
        }
    }
    anyhow::ensure!(failed == 0, "{failed} not moved, left for the next start");
    Ok(())
}

fn kind_owner(kind: &Kind) -> &'static str {
    match kind.what {
        "section" => "Section",
        "file" => "Clip",
        _ => "VOD",
    }
}

/// How many rows still record a file to move or forget (`settle`'s work).
pub fn unsettled(db: &Db) -> Result<usize> {
    let files = db.files();
    let mut n = 0;
    for kind in &KINDS {
        for row in select(db, kind.rows, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))? {
            if !matches!(step(files, kind, &row, &own_files(db, row.1)?), Step::Done | Step::Leave) {
                n += 1;
            }
        }
    }
    Ok(n)
}

/// The rows `sql` selects, as `row` reads them.
fn select<T>(db: &Db, sql: &str, row: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>) -> Result<Vec<T>> {
    let mut stmt = db.conn.prepare(sql)?;
    Ok(stmt.query_map([], row)?.collect::<rusqlite::Result<_>>()?)
}

/// Puts the file a row recorded at `old` where `built` says a file of its
/// extension goes (`place`), and returns that extension, or None if it's
/// nowhere: not at `old`, nor in `folder` (this library's folder of its
/// kind, in `library`) by its name.
fn settle_file(old: &str, folder: &Path, library: &Path, built: impl Fn(&str) -> PathBuf) -> Result<Option<String>> {
    let old = Path::new(old);
    // In lower case, as the app names what it makes: a path recorded in
    // another case (Windows' and macOS's filesystems don't mind) is the same file.
    let Some(ext) = old.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase) else { return Ok(None) };
    let to = built(&ext);
    if place(old, folder, &to, library)? == Placed::Gone {
        return Ok(None);
    }
    Ok(Some(ext))
}

/// Puts the file recorded at `old` at `to`, as the module says: renamed if
/// it's in `library` (the library's folder), else linked or copied; looked
/// for in `folder` by its name if it isn't at `old`.
fn place(old: &Path, folder: &Path, to: &Path, library: &Path) -> Result<Placed> {
    // Done before, the row not yet changed (a crash between the two).
    if to.is_file() {
        return Ok(Placed::There);
    }
    let here = old.file_name().map(|name| folder.join(name));
    let Some(from) = [Some(old.to_path_buf()), here].into_iter().flatten().find(|p| p.is_file()) else {
        return Ok(Placed::Gone);
    };
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if inside(&from, library) {
        std::fs::rename(&from, to)?;
    } else if std::fs::hard_link(&from, to).is_err() {
        // Copied under a name of its own first, so a copy a crash cut short
        // is never taken for the file.
        let part = to.with_extension("settling");
        std::fs::copy(&from, &part)?;
        std::fs::rename(&part, to)?;
    }
    Ok(Placed::There)
}

/// Whether `path` is inside folder `dir`, as the filesystem compares names:
/// both as they are on disk (links followed), then without regard to case
/// on Windows and macOS, whose filesystems don't regard it. By whole names,
/// so `/data/lib2/x` isn't in `/data/lib`.
pub(crate) fn inside(path: &Path, dir: &Path) -> bool {
    let real = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let names = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| {
                let name = c.as_os_str().to_string_lossy();
                if cfg!(any(windows, target_os = "macos")) { name.to_lowercase() } else { name.into_owned() }
            })
            .collect()
    };
    let (path, dir) = (names(&real(path)), names(&real(dir)));
    path.len() > dir.len() && path.starts_with(&dir)
}

/// Whether the file a VOD's row recorded at `old` is one received from
/// another app's drop: in this library's folder for them, or (the library
/// moved) in a folder of that name, its file now in this library's.
fn received(files: &crate::Paths, old: &Path) -> bool {
    let folder = files.received();
    if inside(old, &folder) {
        return true;
    }
    let named = |p: Option<&Path>, name: &str| p.and_then(|p| p.file_name()).is_some_and(|n| n == name);
    let parent = old.parent();
    named(parent, "received")
        && named(parent.and_then(|p| p.parent()), "media")
        && !old.is_file()
        && old.file_name().is_some_and(|name| folder.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Paths;
    use crate::db::ClipStatus;
    use crate::edits::tests::info;
    use std::fs;

    /// Undoes `MIGRATE_BUILT_PATHS`, for a library as 0.7.1 left it.
    const BEFORE_072: &str = "ALTER TABLE vods DROP COLUMN video_ext; ALTER TABLE vods DROP COLUMN audio_ext;
        ALTER TABLE vods DROP COLUMN received_ext; ALTER TABLE stretches DROP COLUMN audio_ext;
        ALTER TABLE renders DROP COLUMN file_ext; ALTER TABLE vods DROP COLUMN channel_login;
        ALTER TABLE vod_annotations DROP COLUMN contexts; PRAGMA user_version = 36;";

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

    /// A library as 0.7.1 kept it, its files where 0.7.1 put them, recording
    /// their full paths: what `settle` moves, and what it leaves.
    #[test]
    fn files_recorded_before_072_go_where_they_are_built() {
        let root = crate::TempDir::new("settle");
        let lib = root.join("data/lib");
        // A folder whose name starts as the library's does: not in it.
        let sibling = root.join("data/lib2");
        let paths = Paths::new(&lib);
        paths.ensure().unwrap();
        let (kept, own, received, downloaded, clip_vod) = {
            let db = Db::open(&paths.db()).unwrap();
            let vod = |id: &str| db.add_vod(&info(id)).unwrap().0.id;
            (vod("kept"), vod("own"), vod("received"), vod("downloaded"), vod("clipped"))
        };
        let video = paths.media().join(format!("A talk [{kept:06}].mp4"));
        write(&video, b"video");
        let song = sibling.join("media/song.mp3");
        write(&song, b"song");
        let dropped = paths.received().join("from a zip.mkv");
        write(&dropped, b"dropped");
        let audio = paths.work().join(format!("{downloaded}.webm"));
        write(&audio, b"audio");
        let section = paths.work().join(format!("{downloaded}.s1.mka"));
        write(&section, b"section");
        // A clip in the Videos folder, as 0.7.1 made them; one of an edit
        // whose file was deleted outside the app.
        let in_videos = root.join("Videos/VODscribe/Channel/2026-10-15 A talk 00h00m01.000s-00h00m02.000s.mp4");
        write(&in_videos, b"clip");
        let (clip, gone, stretch) = {
            let db = Db::open(&paths.db()).unwrap();
            let clip = db.add_clip(clip_vod, 1.0, 2.0, "c", true).unwrap().id;
            let gone = db.add_clip(clip_vod, 3.0, 4.0, "d", true).unwrap().id;
            let stretch = db.request(downloaded, 10.0, Some(20.0)).unwrap().id;
            db.conn.execute_batch(BEFORE_072).unwrap();
            let s = |p: &Path| p.to_string_lossy().into_owned();
            let set = |sql: &str, id: i64, value: String| {
                db.conn.execute(sql, params![id, value]).unwrap();
            };
            set("UPDATE vods SET media_path = ?2, audio_path = ?2 WHERE id = ?1", kept, written_otherwise(&video));
            set("UPDATE vods SET source_path = ?2 WHERE id = ?1", own, s(&song));
            set("UPDATE vods SET source_path = ?2, audio_path = ?2 WHERE id = ?1", received, s(&dropped));
            set("UPDATE vods SET audio_path = ?2 WHERE id = ?1", downloaded, s(&audio));
            set("UPDATE stretches SET audio = ?2 WHERE id = ?1", stretch, s(&section));
            set("UPDATE renders SET file_path = ?2, status = 'ready' WHERE clip_id = ?1", clip, s(&in_videos));
            let lost = s(&root.join("Videos/VODscribe/lost.mp4"));
            set("UPDATE renders SET file_path = ?2, status = 'ready' WHERE clip_id = ?1", gone, lost);
            // And a Windows path from another computer's library.
            let windows = r"C:\Users\Someone\AppData\Local\app.vodscribe\work\9.m4a".to_string();
            set("UPDATE vods SET audio_path = ?2 WHERE id = ?1", own, windows);
            (clip, gone, stretch)
        };

        let db = Db::open(&paths.db()).unwrap();
        assert_eq!(unsettled(&db).unwrap(), 0, "every row settled");
        // Inside the library: renamed to where it's built.
        assert_eq!(db.media_path(kept).unwrap(), Some(crate::db::shown(&paths.source_video(kept, "mp4"))));
        assert_eq!(text(paths.source_video(kept, "mp4")), "video");
        assert!(!video.exists(), "moved, not copied");
        assert_eq!(db.audio_path(kept).unwrap(), db.media_path(kept).unwrap(), "its audio is its video, known from it");
        assert_eq!(db.audio_path(downloaded).unwrap(), Some(crate::db::shown(&paths.source_audio(downloaded, "webm"))));
        assert_eq!(text(paths.source_audio(downloaded, "webm")), "audio");
        let s = db.stretch(stretch).unwrap().unwrap();
        assert_eq!(s.audio, Some(crate::db::shown(&paths.source_section(downloaded, stretch, "mka"))));
        assert_eq!(text(s.audio.unwrap()), "section");
        // A file received from a drop is the library's own: moved too.
        let r = db.vod(received).unwrap().unwrap();
        assert!(r.received);
        assert_eq!(r.source_path, Some(crate::db::shown(&paths.source_received(received, "mkv"))));
        assert_eq!(text(r.source_path.unwrap()), "dropped");
        assert_eq!(db.audio_path(received).unwrap(), db.vod(received).unwrap().unwrap().source_path);
        // The user's own file, in a folder named like the library's, stays.
        let o = db.vod(own).unwrap().unwrap();
        assert_eq!((o.source_path.clone(), o.received), (Some(song.to_string_lossy().into_owned()), false));
        assert_eq!(text(&song), "song");
        assert_eq!(db.audio_path(own).unwrap(), o.source_path, "a Windows path that isn't here: forgotten");
        // A clip in the Videos folder: brought in, the original left there.
        let c = db.clip(clip).unwrap().unwrap();
        assert_eq!(
            (c.status, c.file_path.clone()),
            (ClipStatus::Ready, Some(crate::db::shown(&paths.clip_file(clip, "mp4"))))
        );
        assert_eq!(text(c.file_path.unwrap()), "clip");
        assert_eq!(text(&in_videos), "clip", "the original stays, for whoever uses it");
        assert_eq!(db.clip(gone).unwrap().unwrap().file_path, None, "its file was gone already");
    }

    /// The old columns keep the full path of each file where it's built, so
    /// 0.7.1, opened on this library, finds them; what 0.7.1 records or
    /// forgets meanwhile is taken in at the next start.
    #[test]
    fn going_back_to_071_and_forward_again() {
        let root = crate::TempDir::new("settle-back");
        let paths = Paths::new(root.join("lib"));
        paths.ensure().unwrap();
        let (a, b, clip) = {
            let db = Db::open(&paths.db()).unwrap();
            let a = db.add_vod(&info("a")).unwrap().0.id;
            let b = db.add_vod(&info("b")).unwrap().0.id;
            let video = paths.source_video(a, "mp4");
            write(&video, b"video");
            db.set_media_path(a, Some(&crate::db::shown(&video))).unwrap();
            let clip = db.add_clip(a, 1.0, 2.0, "c", true).unwrap().id;
            let file = paths.clip_file(clip, "mp4");
            write(&file, b"clip");
            db.set_clip_file(clip, 1.0, 2.0, ClipStatus::Ready, Some(&crate::db::shown(&file)), None).unwrap();
            (a, b, clip)
        };
        // What 0.7.1 reads: the full paths, where the files are.
        let old = |sql: &str, id: i64| -> Option<String> {
            let conn = rusqlite::Connection::open(paths.db()).unwrap();
            conn.query_row(sql, [id], |r| r.get(0)).unwrap()
        };
        let media = |id| old("SELECT media_path FROM vods WHERE id = ?1", id);
        let render = |id| old("SELECT file_path FROM renders WHERE clip_id = ?1", id);
        assert_eq!(media(a), Some(crate::db::shown(&paths.source_video(a, "mp4"))));
        assert_eq!(render(clip), Some(crate::db::shown(&paths.clip_file(clip, "mp4"))));
        assert_eq!(text(media(a).unwrap()), "video");
        // 0.7.1 at work: a video kept for b under its own name, and the clip's file deleted.
        let kept = paths.media().join(format!("B [{b:06}].webm"));
        write(&kept, b"b video");
        {
            let conn = rusqlite::Connection::open(paths.db()).unwrap();
            conn.execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![b, crate::db::shown(&kept)]).unwrap();
            conn.execute("UPDATE renders SET file_path = NULL WHERE clip_id = ?1", [clip]).unwrap();
        }
        let db = Db::open(&paths.db()).unwrap();
        assert_eq!(unsettled(&db).unwrap(), 0);
        assert_eq!(db.media_path(b).unwrap(), Some(crate::db::shown(&paths.source_video(b, "webm"))));
        assert_eq!(media(b), db.media_path(b).unwrap(), "recorded where it went, for 0.7.1 too");
        assert_eq!(text(paths.source_video(b, "webm")), "b video");
        assert_eq!(db.clip(clip).unwrap().unwrap().file_path, None, "forgotten, as 0.7.1 forgot it");
        assert_eq!(db.media_path(a).unwrap(), media(a), "a done row stays as it is");
    }

    /// A crash between a file's move and its row's change leaves the file
    /// moved and the row as it was: the next start finishes it.
    #[test]
    fn a_move_cut_short_is_finished_at_the_next_start() {
        let root = crate::TempDir::new("settle-crash");
        let paths = Paths::new(root.join("lib"));
        paths.ensure().unwrap();
        let id = {
            let db = Db::open(&paths.db()).unwrap();
            let id = db.add_vod(&info("x")).unwrap().0.id;
            db.conn.execute_batch(BEFORE_072).unwrap();
            let old = paths.media().join("Old name [000001].mp4");
            db.conn
                .execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![id, crate::db::shown(&old)])
                .unwrap();
            // Moved, then the crash.
            write(&paths.source_video(id, "mp4"), b"video");
            id
        };
        let db = Db::open(&paths.db()).unwrap();
        assert_eq!(db.media_path(id).unwrap(), Some(crate::db::shown(&paths.source_video(id, "mp4"))));
        assert_eq!(unsettled(&db).unwrap(), 0);
        // And opening again changes nothing.
        drop(db);
        let db = Db::open(&paths.db()).unwrap();
        assert_eq!(db.media_path(id).unwrap(), Some(crate::db::shown(&paths.source_video(id, "mp4"))));
    }

    /// The point of it all: a library copied to another folder (another
    /// computer's, a renamed home folder) finds every file of its own, those
    /// recorded by 0.7.1 too, though the folder they were in is gone.
    #[test]
    fn a_library_copied_elsewhere_finds_its_files() {
        let root = crate::TempDir::new("moved");
        let before = Paths::new(root.join("before"));
        before.ensure().unwrap();
        let (vod, old_vod, clip, edit) = {
            let db = Db::open(&before.db()).unwrap();
            let vod = db.add_vod(&info("new")).unwrap().0.id;
            let old_vod = db.add_vod(&info("old")).unwrap().0.id;
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

        let db = Db::open(&after.db()).unwrap();
        let there = |p: Option<String>| p.filter(|p| Path::new(p).is_file()).map(text);
        assert_eq!(there(db.media_path(vod).unwrap()).as_deref(), Some("video"));
        assert_eq!(there(db.playable_path(vod).unwrap()).as_deref(), Some("video"));
        assert_eq!(there(db.media_path(old_vod).unwrap()).as_deref(), Some("old video"));
        assert_eq!(there(db.clip(clip).unwrap().unwrap().file_path).as_deref(), Some("clip"));
        assert_eq!(there(db.edit(edit).unwrap().unwrap().file_path).as_deref(), Some("edit"));
        assert_eq!(unsettled(&db).unwrap(), 0);
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

    /// Paths are inside a folder by whole names, as the filesystem compares them.
    #[test]
    fn inside_is_by_whole_names() {
        let root = crate::TempDir::new("inside");
        let (lib, lib2) = (root.join("lib"), root.join("lib2"));
        write(&lib.join("a.mp4"), b"");
        write(&lib2.join("a.mp4"), b"");
        assert!(inside(&lib.join("a.mp4"), &lib));
        assert!(!inside(&lib2.join("a.mp4"), &lib), "a sibling whose name starts the same");
        assert!(!inside(&lib, &lib), "the folder itself");
        assert_eq!(
            inside(Path::new(&written_otherwise(&lib.join("a.mp4"))), &lib),
            cfg!(any(windows, target_os = "macos")) || written_otherwise(&lib) == lib.to_string_lossy(),
        );
    }
}
