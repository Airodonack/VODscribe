//! Footage: where the picture and sound of a stretch of a VOD come from, for
//! a VOD from a site and edits as they play (the app's own player), edits as
//! they're rendered (`edits::render_pieces`), and clips cut from sites.
//! First the VOD's own file, where it plays from one here (a file on this
//! computer, or its video downloaded); then any clip's file covering the
//! stretch; then a section downloaded for edits into the footage folder
//! (`section`), a little longer than asked, so a piece grown a little needs
//! no new download. Sections are named for the VOD and the stretch they hold.
//! One still downloading (`Growing`) plays as far as it's in.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::db::{ClipStatus, Db};

/// How much more of the VOD a section holds than asked, either side.
pub const MARGIN: f64 = 30.0;

/// How far into a section its picture can take to start: a section is copied,
/// not encoded again, so it has none until its first keyframe. (One from the
/// VOD's start has one there.)
pub const KEYFRAME_GAP: f64 = 10.0;

/// Leeway in matching a stretch to footage's ends: a millisecond.
const EPSILON: f64 = 0.001;

/// How far behind the end of what's whole of a section downloading its
/// footage is used: its last frames may need ones after them to show.
const BEHIND: f64 = 1.0;

/// A file holding a stretch of a VOD: `offset` is the VOD's time at the file's 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub path: PathBuf,
    pub offset: f64,
    /// A section's name (`section_stem`). It holds the same while it
    /// downloads and after, so what's made of it is known by that, not by the file.
    pub section: Option<String>,
}

/// A section of VOD `vod_id` (`from`..`to`) downloading into `path`, whole
/// up to `whole` (the VOD's time), as `mkv::written` reads it.
#[derive(Debug, Clone)]
pub struct Growing {
    pub vod_id: i64,
    pub from: f64,
    pub to: f64,
    pub path: PathBuf,
    pub whole: f64,
}

impl Growing {
    /// How far its footage can be used: a little behind what's whole.
    pub fn usable(&self) -> f64 {
        (self.whole - BEHIND).min(self.to)
    }

    /// The footage of `start`..`end` of VOD `vod_id` in it, if that's in already.
    pub fn find(&self, vod_id: i64, start: f64, end: f64) -> Option<Source> {
        self.reach(vod_id, start, end).filter(|(_, until)| *until >= end - EPSILON).map(|(source, _)| source)
    }

    /// The footage of VOD `vod_id` from `start` in it, and how far towards
    /// `end` it's in already (the VOD's time), if some is.
    pub fn reach(&self, vod_id: i64, start: f64, end: f64) -> Option<(Source, f64)> {
        let until = self.usable().min(end);
        (vod_id == self.vod_id && picture_from(self.from) <= start + EPSILON && until > start + EPSILON).then(|| {
            let source = Source {
                path: self.path.clone(),
                offset: self.from,
                section: Some(section_stem(vod_id, self.from, self.to)),
            };
            (source, until)
        })
    }
}

/// Where a section from `from` has its picture from (`KEYFRAME_GAP`).
fn picture_from(from: f64) -> f64 {
    if from > 0.0 { from + KEYFRAME_GAP } else { 0.0 }
}

/// The footage of `start`..`end` of VOD `vod_id`, if there is some here.
pub fn find(db: &Db, dir: &Path, vod_id: i64, start: f64, end: f64) -> Result<Option<Source>> {
    let Some(vod) = db.vod(vod_id)? else { return Ok(None) };
    let own = vod.source_path.clone().or(db.media_path(vod_id)?);
    if let Some(path) = own.map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(Some(Source { path, offset: 0.0, section: None }));
    }
    for clip in db.clips(vod_id)? {
        if clip.status == ClipStatus::Ready
            && clip.start <= start + EPSILON
            && clip.end >= end - EPSILON
            && let Some(path) = clip.file_path.map(PathBuf::from).filter(|p| p.is_file())
        {
            return Ok(Some(Source { path, offset: clip.start, section: None }));
        }
    }
    let found = sections(dir, vod_id)
        .into_iter()
        .find(|(_, from, to)| picture_from(*from) <= start + EPSILON && *to >= end - EPSILON);
    // Used now, for tidying.
    if let Some((path, _, _)) = &found {
        touch(path);
    }
    Ok(found.map(|(path, from, to)| Source { path, offset: from, section: Some(section_stem(vod_id, from, to)) }))
}

/// Where the footage of `start`..`end` of VOD `vod_id` is here, in order:
/// each stretch (from, to: the VOD's time) and the file it plays from, the
/// one reaching furthest at each point (the VOD's own file, a clip's, a
/// section, or `growing`, the section downloading, as far as it's in).
/// What none of them holds is left out: there the player waits.
pub fn cover(
    db: &Db,
    dir: &Path,
    vod_id: i64,
    start: f64,
    end: f64,
    growing: Option<&Growing>,
) -> Result<Vec<(f64, f64, Source)>> {
    let Some(vod) = db.vod(vod_id)? else { return Ok(Vec::new()) };
    let own = vod.source_path.clone().or(db.media_path(vod_id)?);
    if let Some(path) = own.map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(vec![(start, end, Source { path, offset: 0.0, section: None })]);
    }
    // Each file: from where it can play, to where it ends, and what it is.
    let mut files: Vec<(f64, f64, Source)> = Vec::new();
    for clip in db.clips(vod_id)? {
        if clip.status == ClipStatus::Ready
            && let Some(path) = clip.file_path.map(PathBuf::from).filter(|p| p.is_file())
        {
            files.push((clip.start, clip.end, Source { path, offset: clip.start, section: None }));
        }
    }
    for (path, from, to) in sections(dir, vod_id) {
        files.push((
            picture_from(from),
            to,
            Source { path, offset: from, section: Some(section_stem(vod_id, from, to)) },
        ));
    }
    if let Some(g) = growing.filter(|g| g.vod_id == vod_id) {
        let source = Source { path: g.path.clone(), offset: g.from, section: Some(section_stem(vod_id, g.from, g.to)) };
        files.push((picture_from(g.from), g.usable(), source));
    }
    let mut out: Vec<(f64, f64, Source)> = Vec::new();
    let mut at = start;
    while at < end - EPSILON {
        let here = files
            .iter()
            .filter(|(from, to, _)| *from <= at + EPSILON && *to > at + EPSILON)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match here {
            Some((_, to, source)) => {
                let to = to.min(end);
                // Used now, for tidying.
                if source.section.is_some() && source.path.is_file() {
                    touch(&source.path);
                }
                out.push((at, to, source.clone()));
                at = to;
            }
            // Nothing here: on from where the next file starts, if any does.
            None => match files.iter().map(|(from, _, _)| *from).filter(|from| *from > at).min_by(f64::total_cmp) {
                Some(next) if next < end => at = next,
                _ => break,
            },
        }
    }
    Ok(out)
}

/// The section to download for `start`..`end` of a VOD `duration` seconds
/// long (if known): `MARGIN` more either side, within the VOD.
pub fn section(start: f64, end: f64, duration: Option<f64>) -> (f64, f64) {
    let to = end + MARGIN;
    ((start - MARGIN).max(0.0), duration.map_or(to, |d| to.min(d)))
}

/// What a section of VOD `vod_id` from `from` to `to` seconds is named, without its extension.
pub fn section_stem(vod_id: i64, from: f64, to: f64) -> String {
    format!("{vod_id}-{}-{}", (from * 1000.0).round() as i64, (to * 1000.0).round() as i64)
}

/// The VOD and stretch (from, to) a file named as a section holds (`section_stem`).
pub fn section_of(path: &Path) -> Option<(i64, f64, f64)> {
    let name = path.file_name()?.to_str()?;
    let mut parts = name.split('.').next()?.splitn(3, '-');
    let vod_id = parts.next()?.parse().ok()?;
    let (from, to) = (parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<i64>().ok()?);
    Some((vod_id, from as f64 / 1000.0, to as f64 / 1000.0))
}

/// VOD `vod_id`'s sections in `dir`: each file, and what it holds, from and to.
pub fn sections(dir: &Path, vod_id: i64) -> Vec<(PathBuf, f64, f64)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().is_file() && !e.file_name().to_string_lossy().contains(".part"))
        .filter_map(|e| {
            let (id, from, to) = section_of(&e.path())?;
            (id == vod_id).then(|| (e.path(), from, to))
        })
        .collect()
}

/// Deletes VOD `vod_id`'s sections in `dir` (it's removed), downloaded or not.
pub fn remove(dir: &Path, vod_id: i64) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if section_of(&entry.path()).is_some_and(|(id, _, _)| id == vod_id) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Removes files in `dir` not used for `max_age`, then the least recently
/// used until they take at most `max_bytes` (the app passes no limit, or 0
/// to remove all at once); never those `kept` says to keep (footage an edit
/// uses: its site may not have the VOD any more).
pub fn tidy(dir: &Path, max_age: Duration, max_bytes: u64, kept: impl Fn(&Path) -> bool) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut files: Vec<(PathBuf, SystemTime, u64)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            // Not a part file: one being moved into place.
            let part = e.file_name().to_string_lossy().contains(".part");
            (meta.is_file() && !part && !kept(&e.path()))
                .then(|| (e.path(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len()))
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

/// Marks `path` used now, so `tidy` keeps it.
pub fn touch(path: &Path) {
    if let Ok(f) = fs::File::options().append(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edits::tests::info;

    #[test]
    fn footage_is_the_vods_file_a_clips_or_a_section() {
        let dir = crate::TempDir::new("footage");
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        assert_eq!(find(&db, &dir, vod, 10.0, 20.0).unwrap(), None, "nothing yet");

        // A section downloaded for edits: 5 s to 50 s.
        let section = dir.join(format!("{}.mp4", section_stem(vod, 5.0, 50.0)));
        fs::write(&section, b"x").unwrap();
        fs::write(dir.join(format!("{}.mp4.part", section_stem(vod, 0.0, 600.0))), b"x").unwrap();
        assert_eq!(
            find(&db, &dir, vod, 20.0, 30.0).unwrap(),
            Some(Source { path: section.clone(), offset: 5.0, section: Some(section_stem(vod, 5.0, 50.0)) })
        );
        assert_eq!(find(&db, &dir, vod, 40.0, 60.0).unwrap(), None, "past its end (and not a part file)");
        assert_eq!(find(&db, &dir, vod, 10.0, 20.0).unwrap(), None, "maybe before its first keyframe");

        // A clip's file covering the stretch comes first.
        let clip = db.add_clip(vod, 8.0, 30.0, "c", true).unwrap();
        let file = dir.join("clip.mp4");
        fs::write(&file, b"x").unwrap();
        db.set_clip_file(clip.id, 8.0, 30.0, ClipStatus::Ready, Some(&file.to_string_lossy()), None).unwrap();
        assert_eq!(find(&db, &dir, vod, 20.0, 30.0).unwrap(), Some(Source { path: file, offset: 8.0, section: None }));

        // The VOD's own file, before both.
        let own = dir.join("own.mp4");
        fs::write(&own, b"x").unwrap();
        db.set_media_path(vod, Some(&own.to_string_lossy())).unwrap();
        assert_eq!(find(&db, &dir, vod, 20.0, 30.0).unwrap(), Some(Source { path: own, offset: 0.0, section: None }));
    }

    #[test]
    fn a_long_stretch_plays_from_each_section_with_waits_between() {
        let dir = crate::TempDir::new("cover");
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let file = |from: f64, to: f64| {
            let path = dir.join(format!("{}.mkv", section_stem(vod, from, to)));
            fs::write(&path, b"x").unwrap();
            path
        };
        // Sections 0-100 and 80-200 (its picture from 90), then 300-400 (from 310).
        let (a, b, c) = (file(0.0, 100.0), file(80.0, 200.0), file(300.0, 400.0));
        let growing = Growing { vod_id: vod, from: 380.0, to: 500.0, path: "/w/g.part".into(), whole: 451.0 };
        let got: Vec<(f64, f64, PathBuf)> = cover(&db, &dir, vod, 0.0, 600.0, Some(&growing))
            .unwrap()
            .into_iter()
            .map(|(from, to, s)| (from, to, s.path))
            .collect();
        assert_eq!(
            got,
            [(0.0, 100.0, a), (100.0, 200.0, b), (310.0, 400.0, c), (400.0, 450.0, PathBuf::from("/w/g.part")),],
            "the furthest reaching at each point; 200-310 and from 450 wait"
        );
        assert_eq!(cover(&db, &dir, vod, 210.0, 300.0, None).unwrap(), [], "none of it here");
    }

    #[test]
    fn a_section_downloading_plays_as_far_as_it_is_in() {
        let g = Growing { vod_id: 3, from: 40.0, to: 120.0, path: "/w/s.part".into(), whole: 75.0 };
        let found = g.find(3, 50.0, 74.0).unwrap();
        assert_eq!((found.offset, found.section.as_deref()), (40.0, Some("3-40000-120000")));
        assert!(g.find(3, 50.0, 74.5).is_none(), "too near what's whole");
        assert!(g.find(3, 45.0, 60.0).is_none(), "maybe before its first keyframe");
        assert!(g.find(4, 50.0, 60.0).is_none(), "another VOD");
    }

    #[test]
    fn a_section_downloading_reaches_as_far_as_it_is_in() {
        let g = Growing { vod_id: 3, from: 40.0, to: 120.0, path: "/w/s.part".into(), whole: 75.0 };
        let (source, until) = g.reach(3, 50.0, 100.0).unwrap();
        assert_eq!((source.offset, until), (40.0, 74.0), "a second behind what's whole");
        assert_eq!(g.reach(3, 50.0, 60.0).unwrap().1, 60.0, "all of a span that's in");
        assert!(g.reach(3, 74.0, 100.0).is_none(), "none of it in yet");
        assert!(g.reach(3, 45.0, 60.0).is_none(), "maybe before its first keyframe");
    }

    #[test]
    fn sections_reach_past_what_is_asked() {
        assert_eq!(section(10.0, 20.0, Some(600.0)), (0.0, 50.0));
        assert_eq!(section(100.0, 590.0, Some(600.0)), (70.0, 600.0));
        assert_eq!(section(100.0, 120.0, None), (70.0, 150.0));
        assert_eq!(section_stem(7, 70.0, 150.5), "7-70000-150500");
    }

    #[test]
    fn tidying_keeps_the_newest_within_the_cap() {
        let dir = crate::TempDir::new("footage-tidy");
        for (name, age) in [("old", 100), ("mid", 20), ("new", 0)] {
            let p = dir.join(format!("{name}.mp4"));
            fs::write(&p, [0u8; 100]).unwrap();
            let when = SystemTime::now() - Duration::from_secs(age * 86_400);
            fs::File::options().append(true).open(&p).unwrap().set_modified(when).unwrap();
        }
        tidy(&dir, Duration::from_secs(30 * 86_400), 150, |_| false);
        let left: Vec<bool> = ["old", "mid", "new"].iter().map(|n| dir.join(format!("{n}.mp4")).exists()).collect();
        assert_eq!(left, [false, false, true], "too old, then over the cap");
    }

    #[test]
    fn tidying_keeps_what_edits_use_however_old() {
        let dir = crate::TempDir::new("footage-tidy-kept");
        for name in ["used", "unused"] {
            let p = dir.join(format!("{name}.mkv"));
            fs::write(&p, [0u8; 100]).unwrap();
            let when = SystemTime::now() - Duration::from_secs(100 * 86_400);
            fs::File::options().append(true).open(&p).unwrap().set_modified(when).unwrap();
        }
        tidy(&dir, Duration::from_secs(30 * 86_400), 50, |p| p.ends_with("used.mkv"));
        assert!(dir.join("used.mkv").exists(), "an edit uses it");
        assert!(!dir.join("unused.mkv").exists());
    }

    #[test]
    fn finding_a_section_marks_it_used() {
        let dir = crate::TempDir::new("footage-find-touch");
        let db = Db::open(&dir.join("library.db")).unwrap();
        let (vod, _) = db.add_vod(&crate::edits::tests::info("a")).unwrap();
        let path = dir.join(format!("{}.mkv", section_stem(vod.id, 0.0, 100.0)));
        fs::write(&path, [0u8; 10]).unwrap();
        let old = SystemTime::now() - Duration::from_secs(40 * 86_400);
        fs::File::options().append(true).open(&path).unwrap().set_modified(old).unwrap();
        assert!(find(&db, &dir, vod.id, 10.0, 20.0).unwrap().is_some());
        let used = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(SystemTime::now().duration_since(used).unwrap() < Duration::from_secs(60), "dated now");
    }

    #[test]
    fn removing_a_vod_removes_its_sections_only() {
        let dir = crate::TempDir::new("footage-remove");
        for name in ["7-0-1000.mkv", "7-0-2000.part.mkv", "70-0-1000.mkv", "8-0-1000.mkv", "notes.txt"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        remove(&dir, 7);
        let left: Vec<bool> = ["7-0-1000.mkv", "7-0-2000.part.mkv", "70-0-1000.mkv", "8-0-1000.mkv", "notes.txt"]
            .iter()
            .map(|n| dir.join(n).exists())
            .collect();
        assert_eq!(left, [false, false, true, true, true]);
        assert_eq!(section_of(Path::new("12-1500-90250.mkv")), Some((12, 1.5, 90.25)));
        assert_eq!(section_of(Path::new("notes.txt")), None);
    }
}
