//! Moves the library's files from where versions before 0.7.2 recorded them
//! to where `Paths` builds them now, once, as the app starts (`settle`).
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
//! - In 0.7.1's clip folder (VODscribe in the Videos folder,
//!   `Paths::old_clips`), it's linked (a hard link, the same file under a
//!   second name: no space taken, and the original stays for whoever uses
//!   it, an editor's project). Where it can't be (another disk), its row is
//!   left as 0.7.1 wrote it and the clip is read from there
//!   (`db::clip_file_of`): a first start isn't held up copying them all.
//! - Not where the row says, or where `settle` doesn't take files from, it's
//!   looked for in this library's folder of its kind by its name (a library
//!   copied or moved: the rows say the old folder). This library's own copy
//!   is taken over the other's, so a copied library holds each file once.
//! - Nowhere, the row forgets it, as the app does a file gone missing; a
//!   clip's or an edit's row is left as it was, so the app sees its file
//!   gone, as before 0.7.2 (`forget_gone_clips`).
//!
//! Files are taken only from those folders. A row naming any other (a
//! library from someone else, a network path) is left as it was, its path
//! never looked at: nothing of the user's is brought into the library, and
//! nothing is lost for a version that knows the path.
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
    /// It stays where the row says: not a folder files are taken from, or
    /// in 0.7.1's clip folder on another disk.
    Kept,
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
        // (Its second column, a VOD's id for the others, is which make the
        // file is: `Paths::clip_make`.)
        rows: "SELECT clip_id, coalesce(make, 0), file_path, file_ext FROM renders
               WHERE file_path IS NOT NULL OR file_ext IS NOT NULL",
        folder: crate::Paths::clips,
        built: |p, id, make, ext| p.clip_make(id, make, ext),
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
    let built = crate::db::ext_of(ext.clone()).map(|ext| crate::db::shown(&(kind.built)(files, *id, *vod_id, &ext)));
    match (old, built) {
        (Some(old), Some(built)) if *old == built => Step::Done,
        // 0.7.1 forgot the file, or the extension isn't one the app writes.
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
    remove_leftovers(&files);
    let mut failed = 0;
    for kind in &KINDS {
        let rows: Vec<Row> = select(db, kind.rows, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        for row in rows {
            let (id, vod_id) = (row.0, row.1);
            let done = (|| {
                let none: Option<String> = None;
                // The row as it was, read as unknown: not the file its
                // extension builds, which 0.7.1 replaced since.
                let keep = |old: &str| -> Result<()> {
                    if row.3.is_some() {
                        db.conn.execute(kind.write, params![id, none, old])?;
                    }
                    Ok(())
                };
                match step(&files, kind, &row, &own_files(db, vod_id)?) {
                    Step::Done | Step::Leave => {}
                    Step::Forget => {
                        db.conn.execute(kind.write, params![id, none, none])?;
                    }
                    Step::Move(old) => {
                        let built = |ext: &str| (kind.built)(&files, id, vod_id, ext);
                        match settle_file(&old, kind, &files, built)? {
                            (Placed::There, ext) => {
                                let path = ext.as_deref().map(|ext| crate::db::shown(&built(ext)));
                                db.conn.execute(kind.write, params![id, ext, path])?;
                            }
                            // A clip's file read from where 0.7.1 left it, or
                            // seen as gone (`db::clip_file_of`), in a folder
                            // that's there, as the clip bar looks for one.
                            (Placed::Gone | Placed::Kept, _) if kind.what == "file" => {
                                std::fs::create_dir_all(files.clips())?;
                                keep(&old)?;
                            }
                            (Placed::Gone, _) => {
                                db.conn.execute(kind.write, params![id, none, none])?;
                            }
                            (Placed::Kept, _) => keep(&old)?,
                        }
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

/// Links a crash cut short (`place`, `<name>.settling`), in the folders
/// files are put in.
fn remove_leftovers(files: &crate::Paths) {
    for dir in [files.work(), files.media(), files.received(), files.clips()] {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "settling") && entry.file_type().is_ok_and(|t| t.is_file()) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn kind_owner(kind: &Kind) -> &'static str {
    match kind.what {
        "section" => "Section",
        "file" => "Clip",
        _ => "VOD",
    }
}

/// The rows `sql` selects, as `row` reads them.
fn select<T>(db: &Db, sql: &str, row: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>) -> Result<Vec<T>> {
    let mut stmt = db.conn.prepare(sql)?;
    Ok(stmt.query_map([], row)?.collect::<rusqlite::Result<_>>()?)
}

/// Puts the file a row of `kind` recorded at `old` where `built` says a
/// file of its extension goes (`place`), and returns what became of it,
/// with that extension.
fn settle_file(
    old: &str,
    kind: &Kind,
    files: &crate::Paths,
    built: impl Fn(&str) -> PathBuf,
) -> Result<(Placed, Option<String>)> {
    let old = Path::new(old);
    // In lower case, as the app names what it makes: a path recorded in
    // another case (Windows' and macOS's filesystems don't mind) is the same file.
    let ext = crate::db::ext_of(old.extension().and_then(|e| e.to_str()).map(str::to_owned));
    let Some(ext) = ext else { return Ok((Placed::Gone, None)) };
    let to = built(&ext);
    // Clips and edits only: 0.7.1 made nothing else there.
    let clips = files.old_clips.as_deref().filter(|_| kind.what == "file");
    let placed = place(old, &(kind.folder)(files), &to, &files.local, clips)?;
    Ok((placed, Some(ext)))
}

/// Puts the file recorded at `old` at `to`, as the module says: renamed if
/// it's in `library` (the library's folder), linked if it's in `clips`
/// (0.7.1's clip folder), left there if it can't be; looked for in `folder`
/// by its name if it isn't at `old`, or `old` is in neither.
fn place(old: &Path, folder: &Path, to: &Path, library: &Path, clips: Option<&Path>) -> Result<Placed> {
    // Not even looked at elsewhere (a network path would be reached).
    let in_library = may_take(old, library);
    let in_clips = !in_library && clips.is_some_and(|dir| may_take(old, dir));
    let here = old.file_name().map(|name| folder.join(name)).filter(|p| p != to && may_take(p, library));
    // This library's own first: the row's, then the one by its name.
    let from = [(in_library || in_clips).then(|| old.to_path_buf()), here].into_iter().flatten().find(|p| p.is_file());
    let Some(from) = from else {
        return Ok(if to.is_file() {
            // Moved before, the row not yet changed (a crash between the two).
            Placed::There
        } else if in_library || in_clips {
            Placed::Gone
        } else {
            Placed::Kept
        });
    };
    // Linked before, the row not yet changed. A different file is one 0.7.1
    // wrote after: what the row records now, so it replaces the one there.
    if to.is_file() && same_file(&from, to) {
        return Ok(Placed::There);
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if from != old || in_library {
        std::fs::rename(&from, to)?;
        return Ok(Placed::There);
    }
    // Linked under a name of its own first, then put in place, so a file
    // there already is replaced in one step (and a link a crash left is
    // removed at the next start: `remove_leftovers`).
    let part = to.with_extension("settling");
    let _ = std::fs::remove_file(&part);
    if hard_link(&from, &part).is_err() {
        return Ok(Placed::Kept);
    }
    std::fs::rename(&part, to)?;
    Ok(Placed::There)
}

/// A hard link from `from` at `to`; in a test that says so (`tests::NO_LINKS`),
/// none, as across disks.
fn hard_link(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::NO_LINKS.get() {
        return Err(std::io::Error::other("another disk"));
    }
    std::fs::hard_link(from, to)
}

/// Whether `a` and `b` are the same file: the same length and time changed,
/// as a link or a rename keeps them, and a file written since doesn't.
fn same_file(a: &Path, b: &Path) -> bool {
    let stat = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok()));
    stat(a).is_some_and(|a| Some(a) == stat(b))
}

/// Whether the file at `path` is the app's to take (`settle`) or delete
/// from folder `dir`: inside it by its name as written (`named_inside`),
/// looked at only then, and inside it on disk too, so a link there to a
/// file elsewhere isn't.
pub fn may_take(path: &Path, dir: &Path) -> bool {
    named_inside(path, dir) && inside(path, dir)
}

/// Whether `path`, as written, is inside folder `dir`, without looking at
/// the disk: a full path, no `.` or `..` in it, starting with `dir`'s names
/// (in any case on Windows and macOS). A network path (`\\server\share`,
/// `//server/share`) is in no local folder.
pub(crate) fn named_inside(path: &Path, dir: &Path) -> bool {
    use std::path::Component;
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::CurDir | Component::ParentDir)) {
        return false;
    }
    let names = |p: &Path| -> Vec<String> { p.components().map(|c| name_of(c.as_os_str())).collect() };
    let (path, dir) = (names(path), names(dir));
    path.len() > dir.len() && path.starts_with(&dir)
}

/// A path's name as the filesystem compares it: without regard to case on
/// Windows and macOS.
fn name_of(name: &std::ffi::OsStr) -> String {
    let name = name.to_string_lossy();
    if cfg!(any(windows, target_os = "macos")) { name.to_lowercase() } else { name.into_owned() }
}

/// Whether `path` is inside folder `dir`, as the filesystem compares names:
/// both as they are on disk (links followed), then without regard to case
/// on Windows and macOS, whose filesystems don't regard it. By whole names,
/// so `/data/lib2/x` isn't in `/data/lib`. A path that isn't there (a file
/// gone) is its nearest folder that is, as on disk, then the rest of its
/// names: else a library reached through a link (macOS's /var, a Windows
/// short name) would hold none of its missing files.
pub(crate) fn inside(path: &Path, dir: &Path) -> bool {
    let real = |p: &Path| {
        let there = p.ancestors().find_map(|a| Some((dunce::canonicalize(a).ok()?, p.strip_prefix(a).ok()?)));
        there.map_or_else(|| p.to_path_buf(), |(real, rest)| real.join(rest))
    };
    let names = |p: &Path| -> Vec<String> { p.components().map(|c| name_of(c.as_os_str())).collect() };
    let (path, dir) = (names(&real(path)), names(&real(dir)));
    path.len() > dir.len() && path.starts_with(&dir)
}

/// Whether the file a VOD's row recorded at `old` is one received from
/// another app's drop: in this library's folder for them, or (the library
/// moved) in a folder of that name, its file now in this library's.
fn received(files: &crate::Paths, old: &Path) -> bool {
    let folder = files.received();
    if may_take(old, &folder) {
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
mod tests;
