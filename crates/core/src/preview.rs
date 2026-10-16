//! Previews of edits: each stretch that plays (a piece less what's struck,
//! `edits::EditPiece::spans`), cut into segments of at most `LONGEST`, each
//! rendered on its own (`render::preview`) and cached by what it's made of,
//! so a change to an edit only makes the segments it changed. The page's
//! player feeds those near its playhead, in order, to one video.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::edits::EditPiece;
use crate::footage::Source;
use crate::render::Piece;

/// Changes when segments are made differently, so old ones aren't used.
const VERSION: u32 = 1;

/// A segment of an edit's preview: where it plays in the edit, for how long,
/// and its cache key (none while its clip's file isn't made).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Planned {
    pub key: Option<String>,
    pub at: f64,
    pub length: f64,
    pub piece_id: i64,
    /// What it's rendered from; not sent to the page.
    #[serde(skip)]
    pub source: Option<Piece>,
}

/// The longest a segment is, in seconds: a longer stretch is cut where its
/// VOD's time is a multiple of it, so each is quick to make, and trimming a
/// piece changes only the segments at its ends.
pub const LONGEST: f64 = 10.0;
/// The shortest a segment cut from a longer stretch is.
const SHORTEST: f64 = 2.0;

/// The segments `pieces` play as, in order, from the footage `find` gives
/// (asked once for each stretch).
pub fn plan(pieces: &[EditPiece], find: impl Fn(i64, f64, f64) -> Option<Source>) -> Vec<Planned> {
    let mut out = Vec::new();
    let mut at = 0.0;
    for p in pieces {
        for (a, b) in p.spans() {
            let found = find(p.vod_id, a, b);
            for (c, d) in cuts(a, b) {
                let source =
                    found.as_ref().map(|f| Piece { source: f.path.clone(), start: c - f.offset, end: d - f.offset });
                let key = source.as_ref().and_then(|piece| match found.as_ref().and_then(|f| f.section.as_deref()) {
                    Some(name) => Some(section_key(name, piece)),
                    None => key(piece),
                });
                out.push(Planned { key, at, length: d - c, piece_id: p.id, source });
                at += d - c;
            }
        }
    }
    out
}

/// `a`..`b` cut at each multiple of `LONGEST` in it, leaving none shorter than `SHORTEST`.
fn cuts(a: f64, b: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut from = a;
    let mut at = ((a / LONGEST).floor() + 1.0) * LONGEST;
    while at < b - SHORTEST {
        if at - from >= SHORTEST {
            out.push((from, at));
            from = at;
        }
        at += LONGEST;
    }
    out.push((from, b));
    out
}

/// A segment's cache key: its file (as it is now: its size and when it was
/// written, so a clip made again is a new file), the stretch, and `VERSION`.
/// None if the file isn't there.
pub fn key(piece: &Piece) -> Option<String> {
    let meta = fs::metadata(&piece.source).ok()?;
    let written = meta.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_nanos();
    let mut h = DefaultHasher::new();
    (VERSION, piece.source.as_os_str(), meta.len(), written).hash(&mut h);
    // To the microsecond: times as stored can differ in their last bits.
    ((piece.start * 1e6).round() as i64, (piece.end * 1e6).round() as i64).hash(&mut h);
    Some(format!("{:016x}", h.finish()))
}

/// The key of a segment of a section (`footage::section_stem`): by its name,
/// not its file, so it's the same while the section downloads and after.
pub fn section_key(name: &str, piece: &Piece) -> String {
    let mut h = DefaultHasher::new();
    (VERSION, "section", name).hash(&mut h);
    ((piece.start * 1e6).round() as i64, (piece.end * 1e6).round() as i64).hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Whether `key` is one `key` or `section_key` makes (so it names nothing but a file in the cache).
pub fn is_key(key: &str) -> bool {
    key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Where segment `key` is kept in `dir`.
pub fn path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.mp4"))
}

/// Removes segments in `dir` not used for `max_age`, then the least recently
/// used until they take at most `max_bytes`.
pub fn tidy(dir: &Path, max_age: Duration, max_bytes: u64) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut files: Vec<(PathBuf, SystemTime, u64)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| (e.path(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len()))
        })
        .collect();
    let now = SystemTime::now();
    files.retain(|(path, used, _)| {
        let old = now.duration_since(*used).unwrap_or_default() > max_age;
        if old {
            let _ = fs::remove_file(path);
        }
        !old
    });
    files.sort_by_key(|(_, used, _)| std::cmp::Reverse(*used));
    let mut total = 0;
    for (path, _, size) in files {
        total += size;
        if total > max_bytes {
            let _ = fs::remove_file(path);
        }
    }
}

/// Marks segment `path` used now, so `tidy` keeps it.
pub fn touch(path: &Path) {
    if let Ok(f) = fs::File::options().append(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ClipStatus, Db};
    use crate::fetch::VodInfo;

    #[test]
    fn an_edit_plays_as_a_segment_per_stretch() {
        let dir = crate::TempDir::new("preview");
        let db = Db::open(Path::new(":memory:")).unwrap();
        let info = VodInfo {
            platform: "youtube".into(),
            platform_id: "x".into(),
            url: "https://youtu.be/x".into(),
            title: "x".into(),
            channel: None,
            duration: Some(600.0),
            thumbnail: None,
            published: None,
        };
        let vod = db.add_vod(&info).unwrap().0.id;
        // Clip a's file is the footage of 10 s to 20 s; there's none of 30 s to 34 s.
        let a = db.add_clip(vod, 10.0, 20.0, "a", true).unwrap();
        let file = dir.join("a.mp4");
        fs::write(&file, b"made").unwrap();
        db.set_clip_file(a.id, 10.0, 20.0, ClipStatus::Ready, Some(&file.to_string_lossy()), None).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        db.add_piece(edit, vod, (30.0, 34.0), "b", None).unwrap();
        let footage = dir.join("footage");
        let find = |vod: i64, a: f64, b: f64| crate::footage::find(&db, &footage, vod, a, b).unwrap();
        let first = db.edit_pieces(edit).unwrap()[0].id;
        db.set_piece_struck(first, &[(12.0, 14.0)]).unwrap();

        let planned = plan(&db.edit_pieces(edit).unwrap(), find);
        let spans: Vec<(f64, f64)> = planned.iter().map(|p| (p.at, p.length)).collect();
        assert_eq!(spans, [(0.0, 2.0), (2.0, 6.0), (8.0, 4.0)], "two stretches of a, then b");
        assert!(planned[0].key.is_some() && planned[0].key != planned[1].key);
        assert_eq!(planned[1].source.as_ref().map(|s| (s.start, s.end)), Some((4.0, 10.0)), "in its clip's file");
        assert_eq!(planned[2].key, None, "no footage of b yet");
        assert!(is_key(planned[0].key.as_deref().unwrap()));
        assert!(!is_key("../../etc/passwd") && !is_key("0123456789abcdeg"));

        // The same stretch of the same file is the same segment; the file made again isn't.
        let again = plan(&db.edit_pieces(edit).unwrap(), find);
        assert_eq!(again[0].key, planned[0].key);
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&file, b"made again").unwrap();
        assert_ne!(plan(&db.edit_pieces(edit).unwrap(), find)[0].key, planned[0].key);

        // A section's segments are known by its name: the same downloading and done.
        fs::create_dir_all(&footage).unwrap();
        let section = footage.join(format!("{}.mkv", crate::footage::section_stem(vod, 0.0, 64.0)));
        fs::write(&section, b"in").unwrap();
        let from_section = plan(&db.edit_pieces(edit).unwrap(), find)[2].key.clone();
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&section, b"all in").unwrap();
        assert!(from_section.is_some());
        assert_eq!(plan(&db.edit_pieces(edit).unwrap(), find)[2].key, from_section);
    }

    #[test]
    fn long_stretches_are_cut_on_the_vods_tens() {
        assert_eq!(cuts(3.0, 9.0), [(3.0, 9.0)]);
        assert_eq!(cuts(3.0, 31.0), [(3.0, 10.0), (10.0, 20.0), (20.0, 31.0)], "none under 2 s");
        assert_eq!(cuts(9.0, 25.0), [(9.0, 20.0), (20.0, 25.0)]);
        assert_eq!(cuts(10.0, 20.0), [(10.0, 20.0)]);
        assert_eq!(cuts(0.0, 3600.0).len(), 360);
    }

    #[test]
    fn tidying_keeps_the_newest_within_the_cap() {
        let dir = crate::TempDir::new("preview-tidy");
        for (name, age) in [("old", 100), ("mid", 20), ("new", 0)] {
            let p = dir.join(format!("{name}.mp4"));
            fs::write(&p, [0u8; 100]).unwrap();
            let when = SystemTime::now() - Duration::from_secs(age * 86_400);
            fs::File::options().append(true).open(&p).unwrap().set_modified(when).unwrap();
        }
        tidy(&dir, Duration::from_secs(30 * 86_400), 150);
        let left: Vec<bool> = ["old", "mid", "new"].iter().map(|n| dir.join(format!("{n}.mp4")).exists()).collect();
        assert_eq!(left, [false, false, true], "too old, then over the cap");
    }
}
