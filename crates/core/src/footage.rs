//! Footage: where the picture and sound of a stretch of a VOD come from, for
//! edits' previews and renders (`edits::render_pieces`, `preview::plan`).
//! First the VOD's own file, where it plays from one here (a file on this
//! computer, or its video downloaded); then any clip's file covering the
//! stretch; then a section downloaded for edits into the footage folder
//! (`section`), a little longer than asked, so a piece grown a little needs
//! no new download. Sections are named for the VOD and the stretch they hold.

use std::fs;
use std::path::{Path, PathBuf};

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

/// A file holding a stretch of a VOD: `offset` is the VOD's time at the file's 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub path: PathBuf,
    pub offset: f64,
}

/// The footage of `start`..`end` of VOD `vod_id`, if there is some here.
pub fn find(db: &Db, dir: &Path, vod_id: i64, start: f64, end: f64) -> Result<Option<Source>> {
    let Some(vod) = db.vod(vod_id)? else { return Ok(None) };
    let own = vod.source_path.clone().or(db.media_path(vod_id)?);
    if let Some(path) = own.map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(Some(Source { path, offset: 0.0 }));
    }
    for clip in db.clips(vod_id)? {
        if clip.status == ClipStatus::Ready
            && clip.start <= start + EPSILON
            && clip.end >= end - EPSILON
            && let Some(path) = clip.file_path.map(PathBuf::from).filter(|p| p.is_file())
        {
            return Ok(Some(Source { path, offset: clip.start }));
        }
    }
    Ok(sections(dir, vod_id)
        .into_iter()
        .find(|(_, from, to)| {
            let picture = if *from > 0.0 { from + KEYFRAME_GAP } else { 0.0 };
            picture <= start + EPSILON && *to >= end - EPSILON
        })
        .map(|(path, from, _)| Source { path, offset: from }))
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

/// VOD `vod_id`'s sections in `dir`: each file, and what it holds, from and to.
fn sections(dir: &Path, vod_id: i64) -> Vec<(PathBuf, f64, f64)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let prefix = format!("{vod_id}-");
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.split('.').next()?;
            let (from, to) = stem.strip_prefix(&prefix)?.split_once('-')?;
            let (from, to) = (from.parse::<i64>().ok()?, to.parse::<i64>().ok()?);
            (e.path().is_file() && !name.contains(".part"))
                .then(|| (e.path(), from as f64 / 1000.0, to as f64 / 1000.0))
        })
        .collect()
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
        assert_eq!(find(&db, &dir, vod, 20.0, 30.0).unwrap(), Some(Source { path: section.clone(), offset: 5.0 }));
        assert_eq!(find(&db, &dir, vod, 40.0, 60.0).unwrap(), None, "past its end (and not a part file)");
        assert_eq!(find(&db, &dir, vod, 10.0, 20.0).unwrap(), None, "maybe before its first keyframe");

        // A clip's file covering the stretch comes first.
        let clip = db.add_clip(vod, 8.0, 30.0, "c", true).unwrap();
        let file = dir.join("clip.mp4");
        fs::write(&file, b"x").unwrap();
        db.set_clip_file(clip.id, 8.0, 30.0, ClipStatus::Ready, Some(&file.to_string_lossy()), None).unwrap();
        assert_eq!(find(&db, &dir, vod, 20.0, 30.0).unwrap(), Some(Source { path: file, offset: 8.0 }));

        // The VOD's own file, before both.
        let own = dir.join("own.mp4");
        fs::write(&own, b"x").unwrap();
        db.set_media_path(vod, Some(&own.to_string_lossy())).unwrap();
        assert_eq!(find(&db, &dir, vod, 20.0, 30.0).unwrap(), Some(Source { path: own, offset: 0.0 }));
    }

    #[test]
    fn sections_reach_past_what_is_asked() {
        assert_eq!(section(10.0, 20.0, Some(600.0)), (0.0, 50.0));
        assert_eq!(section(100.0, 590.0, Some(600.0)), (70.0, 600.0));
        assert_eq!(section(100.0, 120.0, None), (70.0, 150.0));
        assert_eq!(section_stem(7, 70.0, 150.5), "7-70000-150500");
    }
}
