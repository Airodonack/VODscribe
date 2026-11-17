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

/// The footage of VOD `vod_id` here (`find` and `cover` look in it).
enum Here {
    /// Its own file: a file's, or its kept video. It has all of it.
    Own(PathBuf),
    /// Else each clip's file of it, then each section: from where it can
    /// play to where it ends (the VOD's time), and what it is.
    Files(Vec<(f64, f64, Source)>),
}

/// Where VOD `vod_id`'s footage is here; none if the VOD is gone.
fn here(db: &Db, dir: &Path, vod_id: i64) -> Result<Option<Here>> {
    let Some(vod) = db.vod(vod_id)? else { return Ok(None) };
    let own = vod.source_path.clone().or(db.media_path(vod_id)?);
    if let Some(path) = own.map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(Some(Here::Own(path)));
    }
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
    Ok(Some(Here::Files(files)))
}

/// The footage of `start`..`end` of VOD `vod_id`, if there is some here.
pub fn find(db: &Db, dir: &Path, vod_id: i64, start: f64, end: f64) -> Result<Option<Source>> {
    let files = match here(db, dir, vod_id)? {
        None => return Ok(None),
        Some(Here::Own(path)) => return Ok(Some(Source { path, offset: 0.0, section: None })),
        Some(Here::Files(files)) => files,
    };
    let found = files.into_iter().find(|(from, to, _)| *from <= start + EPSILON && *to >= end - EPSILON);
    let found = found.map(|(_, _, source)| source);
    // Used now, for tidying.
    if let Some(source) = found.as_ref().filter(|s| s.section.is_some()) {
        touch(&source.path);
    }
    Ok(found)
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
    let mut files = match here(db, dir, vod_id)? {
        None => return Ok(Vec::new()),
        Some(Here::Own(path)) => return Ok(vec![(start, end, Source { path, offset: 0.0, section: None })]),
        Some(Here::Files(files)) => files,
    };
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

/// Removes files in `dir` not used for `max_age` (`touch`), then the least recently
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
                .then(|| (e.path(), meta.accessed().unwrap_or(SystemTime::UNIX_EPOCH), meta.len()))
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

/// Marks `path` used now, so `tidy` keeps it: its access time, not its
/// modification time, which says what's in it (a storyboard is made of the
/// file as it was changed last, and made again if it changes).
pub fn touch(path: &Path) {
    used_at(path, SystemTime::now());
}

/// Marks `path` last used at `when`.
pub fn used_at(path: &Path, when: SystemTime) {
    if let Ok(f) = fs::File::options().append(true).open(path) {
        let _ = f.set_times(fs::FileTimes::new().set_accessed(when));
    }
}

#[cfg(test)]
mod tests;
