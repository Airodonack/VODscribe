//! What plays when (`Timeline`): a file alone, a VOD as far as its video is
//! in, and an edit's pieces end to end from the footage found for them,
//! waiting where it isn't here. No ffmpeg: the stretches are worked out, not played.

use std::path::PathBuf;

use vodscribe_core::edits::EditPiece;
use vodscribe_core::footage::Source;
use vodscribe_player::timeline::{Sound, Stretch, Timeline};

/// Piece `id` of an edit: VOD `vod_id` from `start` to `end`.
fn piece(id: i64, vod_id: i64, start: f64, end: f64) -> EditPiece {
    EditPiece {
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

/// Footage in `path`, whose 0 is `offset` seconds of the VOD.
fn footage(path: &str, offset: f64) -> Source {
    Source { path: PathBuf::from(path), offset, section: None }
}

/// A VOD whose video is still downloading plays as far as it's in, the
/// rest waiting for its footage, and more coming in is the same stretch
/// played further (timeline.rs `Timeline::covering`: "A VOD whose video is
/// still downloading plays this way, as far as it's in, grown as more
/// comes"; CHANGELOG.md, 0.6.0: "it plays in VODscribe as it comes in,
/// waiting where it hasn't yet").
#[test]
fn a_vod_downloading_plays_what_is_in_and_waits_for_the_rest() {
    let source = |from: f64| Source { path: "/f/section.mkv".into(), offset: from, section: Some("1-0-600000".into()) };
    // In to 42 s of 600: footage to there, then a wait to the end.
    let timeline = Timeline::covering(600.0, vec![(0.0, 42.0, source(0.0))]);
    let spans: Vec<_> = timeline.stretches().iter().map(|s| (s.at, s.length, s.source.is_some())).collect();
    assert_eq!(spans, [(0.0, 42.0, true), (42.0, 558.0, false)]);
    assert_eq!(timeline.duration(), 600.0);
    // More in: the same stretch, played further, so playing goes on unbroken.
    let grown = Timeline::covering(600.0, vec![(0.0, 90.0, source(0.0))]);
    assert!(timeline.stretches()[0].continued_by(&grown.stretches()[0]));
    // Footage from 100 s (a section of it) and none before: a wait first.
    let later = Timeline::covering(600.0, vec![(100.0, 200.0, source(70.0))]);
    let spans: Vec<_> = later.stretches().iter().map(|s| (s.at, s.length, s.source.is_some(), s.from)).collect();
    assert_eq!(spans, [(0.0, 100.0, false, 0.0), (100.0, 100.0, true, 30.0), (200.0, 400.0, false, 0.0)]);
    // Nothing in yet: all of it waits.
    assert_eq!(Timeline::covering(600.0, Vec::new()).stretches().len(), 1);
    // All of it in: one stretch, nothing waiting.
    let whole = Timeline::covering(600.0, vec![(0.0, 600.0, source(0.0))]);
    assert_eq!(whole.stretches().len(), 1, "{:?}", whole.stretches());
    // Footage already covered, or past the end, adds nothing.
    let extra = vec![(0.0, 42.0, source(0.0)), (10.0, 30.0, source(0.0)), (700.0, 800.0, source(700.0))];
    assert_eq!(Timeline::covering(600.0, extra), timeline);
}

/// A stretch whose sound came from a file of its own after its picture is
/// a new stretch, not the old one played further (timeline.rs
/// `Stretch::continued_by`: "its sound too (a sound that came after the
/// picture isn't)").
#[test]
fn a_stretch_whose_sound_came_after_isnt_the_same_stretch() {
    let picture =
        Stretch { at: 0.0, length: 4.0, piece_id: None, source: Some(footage("v.mp4", 0.0)), from: 0.0, sound: None };
    let with_sound =
        Stretch { length: 6.0, sound: Some(Sound { path: PathBuf::from("a.mp4"), offset: 0.0 }), ..picture.clone() };
    assert!(!picture.continued_by(&with_sound));
    assert!(with_sound.continued_by(&Stretch { length: 8.0, ..with_sound.clone() }));
}

/// A stretch's sound from a file of its own is read at the same moment of
/// the VOD as its picture (timeline.rs `Sound`: "`path`, whose 0 is
/// `offset` seconds of the VOD"; `Stretch::sound_time`: "its source's own,
/// if it has none").
#[test]
fn a_sound_of_its_own_is_read_at_the_same_moment_of_the_vod() {
    // The picture's file starts at VOD 10 s, the sound's at VOD 9.5 s.
    let stretch = Stretch {
        at: 0.0,
        length: 5.0,
        piece_id: None,
        source: Some(footage("v.mp4", 10.0)),
        from: 2.0,
        sound: Some(Sound { path: PathBuf::from("a.webm"), offset: 9.5 }),
    };
    // Timeline 1 s: picture file 3 s (VOD 13 s), sound file 3.5 s (VOD 13 s too).
    assert_eq!(stretch.file_time(1.0), 3.0);
    assert_eq!(stretch.sound_time(1.0), 3.5);
    // Without one, the sound is the picture's file's own.
    let alone = Stretch { sound: None, ..stretch };
    assert_eq!(alone.sound_time(1.0), 3.0);
}

/// An edit's pieces are laid end to end from the footage found for them,
/// a piece with none waiting for it (timeline.rs `Timeline::of`: "each span
/// of each from the footage `cover` gives for it ... What isn't here plays
/// nothing, for now: it waits").
#[test]
fn an_edit_is_laid_end_to_end_from_the_footage_found() {
    let pieces = [piece(1, 7, 10.0, 14.0), piece(3, 7, 16.0, 20.0), piece(2, 8, 5.0, 8.0)];
    // VOD 7's footage is a section from 8 s; VOD 8 has none yet.
    let find = |vod: i64, a: f64, b: f64| {
        let source = Source { path: PathBuf::from("7.mkv"), offset: 8.0, section: Some("s".into()) };
        if vod == 7 { vec![(a, b, source)] } else { vec![] }
    };
    let t = Timeline::of(&pieces, find);
    let s = t.stretches();
    assert_eq!(s.len(), 3);
    assert_eq!((s[0].at, s[0].length, s[0].from), (0.0, 4.0, 2.0));
    assert_eq!((s[1].at, s[1].length, s[1].from), (4.0, 4.0, 8.0));
    assert_eq!((s[2].at, s[2].length), (8.0, 3.0));
    assert!(s[2].source.is_none());
    assert_eq!(t.duration(), 11.0);
    // Times map through a stretch both ways.
    assert_eq!(s[1].file_time(5.0), 9.0);
    assert_eq!(s[1].time(9.0), 5.0);
    // The stretch at a time: the last one at and past the end.
    assert_eq!(t.index_at(0.0), Some(0));
    assert_eq!(t.index_at(4.0), Some(1));
    assert_eq!(t.index_at(7.999), Some(1));
    assert_eq!(t.index_at(11.0), Some(2));
    assert_eq!(t.index_at(99.0), Some(2));
    assert_eq!(Timeline::default().index_at(0.0), None);
}

/// An edit's footage still coming plays as far as it's in, and more coming
/// in is the same stretch played further (timeline.rs `Timeline::of`: "the
/// stretches of it ... that are here, in order", "What isn't here plays
/// nothing, for now: it waits").
#[test]
fn footage_still_coming_plays_as_far_as_it_is_in() {
    let pieces = [piece(1, 7, 10.0, 20.0)];
    let source = Source { path: PathBuf::from("7.mkv"), offset: 0.0, section: Some("s".into()) };
    let upto = |until: f64| {
        let source = source.clone();
        move |_, a: f64, _| vec![(a, until, source.clone())]
    };
    // In to 13 s of the VOD: three seconds play, seven wait.
    let t = Timeline::of(&pieces, upto(13.0));
    let s = t.stretches();
    assert_eq!(s.len(), 2);
    assert_eq!((s[0].at, s[0].length, s[0].from), (0.0, 3.0, 10.0));
    assert!(s[0].source.is_some());
    assert_eq!((s[1].at, s[1].length), (3.0, 7.0));
    assert!(s[1].source.is_none());
    assert_eq!(t.duration(), 10.0);
    // More in: the first stretch is the same one, played further.
    let later = Timeline::of(&pieces, upto(18.0));
    assert!(s[0].continued_by(&later.stretches()[0]));
    assert!(!later.stretches()[0].continued_by(&s[0]), "not shorter");
    // None in yet: the whole span waits.
    let none = Timeline::of(&pieces, upto(10.0));
    assert_eq!(none.stretches().len(), 1);
    assert!(none.stretches()[0].source.is_none());
}

/// A long piece plays from each section of footage there is and waits
/// between them (timeline.rs `Timeline::of`, from `footage::cover`: "in
/// order, apart").
#[test]
fn a_long_piece_plays_from_each_section_and_waits_between() {
    // A whole VOD of 600 s: sections 0-100 and 300-400 here.
    let pieces = [piece(1, 7, 0.0, 600.0)];
    let section = |from: f64| footage(&format!("{from}.mkv"), from);
    let t = Timeline::of(&pieces, |_, _, _| vec![(0.0, 100.0, section(0.0)), (300.0, 400.0, section(300.0))]);
    let s: Vec<(f64, f64, f64, bool)> =
        t.stretches().iter().map(|s| (s.at, s.length, s.from, s.source.is_some())).collect();
    assert_eq!(
        s,
        [(0.0, 100.0, 0.0, true), (100.0, 200.0, 0.0, false), (300.0, 100.0, 0.0, true), (400.0, 200.0, 0.0, false)]
    );
    assert_eq!(t.duration(), 600.0);
}
