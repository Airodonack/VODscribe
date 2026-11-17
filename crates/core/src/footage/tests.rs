//! The specs of `footage.rs`: where a stretch of a VOD plays from (its own
//! file, a clip's, a section downloaded for edits, the section still
//! downloading), the sections' extent and names, and their tidying.
//! Tidying by age reads the files' own dates against the system clock, so
//! a file's last use is set back (`used_at`) rather than waited for.

use super::*;
use crate::edits::tests::info;

/// Footage is the VOD's own file first, then a clip's file covering the
/// stretch, then a section downloaded for edits, never a part file
/// (footage.rs's module doc: "First the VOD's own file, where it plays from
/// one here (a file on this computer, or its video downloaded); then any
/// clip's file covering the stretch; then a section downloaded for edits").
#[test]
fn footage_is_the_vods_file_a_clips_or_a_section() {
    let dir = crate::TempDir::new("footage");
    let db = Db::open(&dir.join("library.sqlite")).unwrap();
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
    let file = db.files().clip_file(clip.id, "mp4");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"x").unwrap();
    db.set_clip_file(clip.id, 8.0, 30.0, ClipStatus::Ready, Some(&file.to_string_lossy()), None).unwrap();
    assert_eq!(find(&db, &dir, vod, 20.0, 30.0).unwrap(), Some(Source { path: file, offset: 8.0, section: None }));

    // The VOD's own file, before both.
    let own = db.files().source_video(vod, "mp4");
    fs::create_dir_all(own.parent().unwrap()).unwrap();
    fs::write(&own, b"x").unwrap();
    db.set_media_path(vod, Some(&own.to_string_lossy())).unwrap();
    assert_eq!(find(&db, &dir, vod, 20.0, 30.0).unwrap(), Some(Source { path: own, offset: 0.0, section: None }));
}

/// A stretch plays from the file reaching furthest at each point, and
/// what none holds is left out (footage.rs `cover`: "the one reaching
/// furthest at each point ... What none of them holds is left out: there
/// the player waits").
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

/// A section still downloading holds a stretch once it's in, a second
/// behind what's whole (footage.rs's module doc: "One still downloading
/// (`Growing`) plays as far as it's in"; `Growing::find`).
#[test]
fn a_section_downloading_plays_as_far_as_it_is_in() {
    let g = Growing { vod_id: 3, from: 40.0, to: 120.0, path: "/w/s.part".into(), whole: 75.0 };
    let found = g.find(3, 50.0, 74.0).unwrap();
    assert_eq!((found.offset, found.section.as_deref()), (40.0, Some("3-40000-120000")));
    assert!(g.find(3, 50.0, 74.5).is_none(), "too near what's whole");
    assert!(g.find(3, 45.0, 60.0).is_none(), "maybe before its first keyframe");
    assert!(g.find(4, 50.0, 60.0).is_none(), "another VOD");
}

/// How far towards a stretch's end a section downloading is in
/// (footage.rs `Growing::reach`: "how far towards `end` it's in already").
#[test]
fn a_section_downloading_reaches_as_far_as_it_is_in() {
    let g = Growing { vod_id: 3, from: 40.0, to: 120.0, path: "/w/s.part".into(), whole: 75.0 };
    let (source, until) = g.reach(3, 50.0, 100.0).unwrap();
    assert_eq!((source.offset, until), (40.0, 74.0), "a second behind what's whole");
    assert_eq!(g.reach(3, 50.0, 60.0).unwrap().1, 60.0, "all of a span that's in");
    assert!(g.reach(3, 74.0, 100.0).is_none(), "none of it in yet");
    assert!(g.reach(3, 45.0, 60.0).is_none(), "maybe before its first keyframe");
}

/// A section holds half a minute more either side, within the VOD, and is
/// named for the VOD and the stretch it holds (footage.rs's module doc: "a
/// little longer than asked, so a piece grown a little needs no new
/// download. Sections are named for the VOD and the stretch they hold";
/// `section`, `section_stem`).
#[test]
fn sections_reach_past_what_is_asked() {
    assert_eq!(section(10.0, 20.0, Some(600.0)), (0.0, 50.0));
    assert_eq!(section(100.0, 590.0, Some(600.0)), (70.0, 600.0));
    assert_eq!(section(100.0, 120.0, None), (70.0, 150.0));
    assert_eq!(section_stem(7, 70.0, 150.5), "7-70000-150500");
}

/// Tidying removes what's unused for longer than asked, then the least
/// recently used over the cap (footage.rs `tidy`: "Removes files in `dir`
/// not used for `max_age` (`touch`), then the least recently used until
/// they take at most `max_bytes`").
#[test]
fn tidying_keeps_the_newest_within_the_cap() {
    let dir = crate::TempDir::new("footage-tidy");
    for (name, age) in [("old", 100), ("mid", 20), ("new", 0)] {
        let p = dir.join(format!("{name}.mp4"));
        fs::write(&p, [0u8; 100]).unwrap();
        let when = SystemTime::now() - Duration::from_secs(age * 86_400);
        used_at(&p, when);
    }
    tidy(&dir, Duration::from_secs(30 * 86_400), 150, |_| false);
    let left: Vec<bool> = ["old", "mid", "new"].iter().map(|n| dir.join(format!("{n}.mp4")).exists()).collect();
    assert_eq!(left, [false, false, true], "too old, then over the cap");
}

/// Tidying never removes what an edit uses (footage.rs `tidy`: "never
/// those `kept` says to keep (footage an edit uses: its site may not have
/// the VOD any more)").
#[test]
fn tidying_keeps_what_edits_use_however_old() {
    let dir = crate::TempDir::new("footage-tidy-kept");
    for name in ["used", "unused"] {
        let p = dir.join(format!("{name}.mkv"));
        fs::write(&p, [0u8; 100]).unwrap();
        let when = SystemTime::now() - Duration::from_secs(100 * 86_400);
        used_at(&p, when);
    }
    tidy(&dir, Duration::from_secs(30 * 86_400), 50, |p| p.ends_with("used.mkv"));
    assert!(dir.join("used.mkv").exists(), "an edit uses it");
    assert!(!dir.join("unused.mkv").exists());
}

/// Found (a clip cut, a frame shown) or covering an edit's piece (an
/// edit planned, played, its thumbnails shown), a section is marked used
/// for tidying, and its modification time stays: what's made of the file
/// (its storyboard) is made of it as it was changed last, and made again
/// if it changes, so a look at it mustn't count as a change.
#[test]
fn finding_a_section_marks_it_used_and_leaves_it_unchanged() {
    let dir = crate::TempDir::new("footage-find-touch");
    let db = Db::open(&dir.join("library.db")).unwrap();
    let (vod, _) = db.add_vod(&crate::edits::tests::info("a")).unwrap();
    let path = dir.join(format!("{}.mkv", section_stem(vod.id, 0.0, 100.0)));
    fs::write(&path, [0u8; 10]).unwrap();
    let old = SystemTime::now() - Duration::from_secs(40 * 86_400);
    fs::File::options().append(true).open(&path).unwrap().set_modified(old).unwrap();
    let changed = fs::metadata(&path).unwrap().modified().unwrap();
    for look in ["found", "covering"] {
        used_at(&path, old);
        match look {
            "found" => assert!(find(&db, &dir, vod.id, 10.0, 20.0).unwrap().is_some()),
            _ => assert_eq!(cover(&db, &dir, vod.id, 10.0, 20.0, None).unwrap().len(), 1),
        }
        let meta = fs::metadata(&path).unwrap();
        let used = meta.accessed().unwrap();
        assert!(SystemTime::now().duration_since(used).unwrap() < Duration::from_secs(60), "{look}: dated now");
        assert_eq!(meta.modified().unwrap(), changed, "{look}: its modification time changed");
    }
    tidy(&dir, Duration::from_secs(30 * 86_400), u64::MAX, |_| false);
    assert!(path.exists(), "used now, it's kept");
}

/// Removing a VOD deletes its sections, downloaded or not, and no other
/// VOD's or file (footage.rs `remove`: "Deletes VOD `vod_id`'s sections in
/// `dir` (it's removed), downloaded or not"; `section_of`).
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

/// A VOD with a file of its own plays all of any stretch from it, at its
/// own times (footage.rs `Here::Own`: "Its own file: a file's, or its kept
/// video. It has all of it"; `cover`: "the VOD's own file, a clip's, a
/// section").
#[test]
fn a_vods_own_file_covers_all_of_any_stretch() {
    let dir = crate::TempDir::new("footage-cover-own");
    let db = Db::open(&dir.join("library.sqlite")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    // A section too, which the VOD's own file comes before.
    fs::write(dir.join(format!("{}.mkv", section_stem(vod, 0.0, 100.0))), b"x").unwrap();
    let own = db.files().source_video(vod, "mp4");
    fs::create_dir_all(own.parent().unwrap()).unwrap();
    fs::write(&own, b"x").unwrap();
    db.set_media_path(vod, Some(&own.to_string_lossy())).unwrap();
    let got = cover(&db, &dir, vod, 250.0, 900.0, None).unwrap();
    assert_eq!(got, [(250.0, 900.0, Source { path: own, offset: 0.0, section: None })]);
}

/// A section still downloading is used for its own VOD only, beside what's
/// here (footage.rs `cover`: "or `growing`, the section downloading, as far
/// as it's in").
#[test]
fn a_section_downloading_covers_only_its_own_vod() {
    let dir = crate::TempDir::new("footage-cover-growing");
    let db = Db::open(&dir.join("library.sqlite")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let other = db.add_vod(&info("y")).unwrap().0.id;
    let growing = Growing { vod_id: vod, from: 0.0, to: 120.0, path: "/w/g.part".into(), whole: 61.0 };
    let paths = |id: i64| -> Vec<(f64, f64, PathBuf)> {
        cover(&db, &dir, id, 0.0, 100.0, Some(&growing)).unwrap().into_iter().map(|(a, b, s)| (a, b, s.path)).collect()
    };
    assert_eq!(paths(vod), [(0.0, 60.0, PathBuf::from("/w/g.part"))], "a second behind what's whole");
    assert_eq!(paths(other), [], "another VOD's download");
}

/// Tidying leaves a file still being moved into place, however old, even
/// when it removes all at once (footage.rs `tidy`: "Not a part file: one
/// being moved into place"; "or 0 to remove all at once").
#[test]
fn tidying_leaves_a_section_being_moved_into_place() {
    let dir = crate::TempDir::new("footage-tidy-part");
    let long_ago = SystemTime::now() - Duration::from_secs(100 * 86_400);
    for name in ["7-0-60000.part.mkv", "7-0-60000.mkv"] {
        fs::write(dir.join(name), [0u8; 100]).unwrap();
        used_at(&dir.join(name), long_ago);
    }
    tidy(&dir, Duration::ZERO, 0, |_| false);
    assert!(dir.join("7-0-60000.part.mkv").exists(), "the section being moved into place went");
    assert!(!dir.join("7-0-60000.mkv").exists(), "all at once, but the one being moved");
}

/// A stretch is matched to footage's ends within a millisecond: one
/// starting a little before a section's picture does, or ending a little
/// past its end, is still in it (footage.rs `EPSILON`: "Leeway in matching
/// a stretch to footage's ends: a millisecond").
#[test]
fn a_stretch_is_matched_to_footages_ends_within_a_millisecond() {
    let dir = crate::TempDir::new("footage-epsilon");
    let db = Db::open(&dir.join("library.sqlite")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    // From 5 s to 50 s: its picture from 15 s (`KEYFRAME_GAP`).
    fs::write(dir.join(format!("{}.mkv", section_stem(vod, 5.0, 50.0))), b"x").unwrap();
    assert!(find(&db, &dir, vod, 14.9995, 50.0005).unwrap().is_some(), "within a millisecond of its ends");
    assert!(find(&db, &dir, vod, 14.99, 50.0).unwrap().is_none(), "a hundredth before its picture");
    assert!(find(&db, &dir, vod, 15.0, 50.01).unwrap().is_none(), "a hundredth past its end");
}

/// A section just begun downloading, none of it in a second behind what's
/// whole, covers nothing yet (footage.rs's module doc: "One still
/// downloading (`Growing`) plays as far as it's in"; `cover`: "What none of
/// them holds is left out").
#[test]
fn a_section_just_begun_downloading_covers_nothing_yet() {
    let dir = crate::TempDir::new("footage-cover-begun");
    let db = Db::open(&dir.join("library.sqlite")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let growing = Growing { vod_id: vod, from: 0.0, to: 120.0, path: "/w/g.part".into(), whole: 0.5 };
    assert_eq!(cover(&db, &dir, vod, 0.0, 100.0, Some(&growing)).unwrap(), []);
}

/// Tidying removes what's unused for longer than asked however little
/// room it takes, and keeps the rest (footage.rs `tidy`: "Removes files in
/// `dir` not used for `max_age`").
#[test]
fn tidying_removes_what_is_unused_too_long_however_little_it_takes() {
    let dir = crate::TempDir::new("footage-tidy-age");
    for (name, days) in [("old", 31), ("recent", 29)] {
        let p = dir.join(format!("{name}.mkv"));
        fs::write(&p, [0u8; 10]).unwrap();
        used_at(&p, SystemTime::now() - Duration::from_secs(days * 86_400));
    }
    tidy(&dir, Duration::from_secs(30 * 86_400), u64::MAX, |_| false);
    assert!(!dir.join("old.mkv").exists(), "unused for longer than asked, it stayed");
    assert!(dir.join("recent.mkv").exists());
}

/// Files that take exactly the cap are all kept (footage.rs `tidy`: "until
/// they take at most `max_bytes`").
#[test]
fn tidying_keeps_files_that_take_exactly_the_cap() {
    let dir = crate::TempDir::new("footage-tidy-cap");
    for name in ["a", "b"] {
        fs::write(dir.join(format!("{name}.mkv")), [0u8; 100]).unwrap();
    }
    tidy(&dir, Duration::from_secs(86_400), 200, |_| false);
    assert!(dir.join("a.mkv").exists() && dir.join("b.mkv").exists(), "200 bytes is at most 200");
}
