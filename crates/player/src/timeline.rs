//! What plays when: a timeline is stretches laid end to end, each a part of
//! a file (or nothing yet, where the footage isn't here), in the player's
//! own time. A file alone is one stretch; an edit is one stretch for each
//! of its pieces' spans (crates/core `edits`), from the footage found for
//! them, as a render is planned.

use std::path::{Path, PathBuf};

use vodscribe_core::edits::EditPiece;
use vodscribe_core::footage::Source;

/// A part of the timeline, from `at` for `length` seconds, playing `source`
/// from `from` seconds into it (if the footage is here).
#[derive(Clone, Debug, PartialEq)]
pub struct Stretch {
    pub at: f64,
    pub length: f64,
    /// The edit piece it's of, if any.
    pub piece_id: Option<i64>,
    pub source: Option<Source>,
    pub from: f64,
    /// Where its sound comes from, if not the source's file: a file of its
    /// own (a site's player gives sound and picture apart: embed.rs).
    pub sound: Option<Sound>,
}

/// A stretch's sound from a file of its own: `path`, whose 0 is `offset`
/// seconds of the VOD (as a `Source`'s is).
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub path: PathBuf,
    pub offset: f64,
}

impl Stretch {
    pub fn end(&self) -> f64 {
        self.at + self.length
    }

    /// Whether `other` is this stretch, or this one played further (more
    /// of its footage in): what plays up to this one's end is the same, its
    /// sound too (a sound that came after the picture isn't).
    pub fn continued_by(&self, other: &Stretch) -> bool {
        self.at == other.at
            && self.from == other.from
            && self.source == other.source
            && self.sound == other.sound
            && self.piece_id == other.piece_id
            && other.length >= self.length
    }

    /// Seconds into the source at timeline time `time`.
    pub fn file_time(&self, time: f64) -> f64 {
        self.from + (time - self.at)
    }

    /// Seconds into its sound's file at timeline time `time` (its source's own, if it has none).
    pub fn sound_time(&self, time: f64) -> f64 {
        match (&self.sound, &self.source) {
            (Some(sound), Some(source)) => self.file_time(time) + source.offset - sound.offset,
            _ => self.file_time(time),
        }
    }

    /// Timeline time of `file_time` seconds into the source.
    pub fn time(&self, file_time: f64) -> f64 {
        self.at + (file_time - self.from)
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Timeline {
    stretches: Vec<Stretch>,
}

/// Stretches shorter than this aren't worth a frame.
const SLIVER: f64 = 1e-6;

impl Timeline {
    /// One file, played whole.
    pub fn file(path: &Path, duration: f64) -> Timeline {
        let source = Source { path: path.to_path_buf(), offset: 0.0, section: None };
        Timeline {
            stretches: vec![Stretch {
                at: 0.0,
                length: duration,
                piece_id: None,
                source: Some(source),
                from: 0.0,
                sound: None,
            }],
        }
    }

    /// A clip's `pieces` (an edit's, or a VOD's own), each span of each from the footage `cover` gives
    /// for it: the stretches of it (from, to: the VOD's time) that are here,
    /// in order, each with its file (`footage::cover`). What isn't here plays
    /// nothing, for now: it waits.
    pub fn of(pieces: &[EditPiece], cover: impl Fn(i64, f64, f64) -> Vec<(f64, f64, Source)>) -> Timeline {
        let mut stretches = Vec::new();
        let mut at = 0.0;
        let wait = |stretches: &mut Vec<Stretch>, at: &mut f64, piece: i64, length: f64| {
            if length >= SLIVER {
                stretches.push(Stretch {
                    at: *at,
                    length,
                    piece_id: Some(piece),
                    source: None,
                    from: 0.0,
                    sound: None,
                });
                *at += length;
            }
        };
        for p in pieces {
            for (a, b) in p.spans() {
                let mut done = a;
                for (from, to, source) in cover(p.vod_id, a, b) {
                    let (from, to) = (from.max(done), to.min(b));
                    if to - from < SLIVER {
                        continue;
                    }
                    wait(&mut stretches, &mut at, p.id, from - done);
                    let start = from - source.offset;
                    stretches.push(Stretch {
                        at,
                        length: to - from,
                        piece_id: Some(p.id),
                        source: Some(source),
                        from: start,
                        sound: None,
                    });
                    at += to - from;
                    done = to;
                }
                wait(&mut stretches, &mut at, p.id, b - done);
            }
        }
        Timeline { stretches }
    }

    /// A VOD `duration` long played from the footage `cover` gives for all
    /// of it (`footage::cover`: in order, apart), as far as it's here: the
    /// rest, between and after, waits for its footage. A VOD whose video is
    /// still downloading plays this way, as far as it's in, grown as more
    /// comes (`Player::set_timeline`).
    pub fn covering(duration: f64, cover: Vec<(f64, f64, Source)>) -> Timeline {
        let mut stretches = Vec::new();
        let mut at = 0.0;
        let wait = |stretches: &mut Vec<Stretch>, from: f64, to: f64| {
            if to - from >= SLIVER {
                stretches.push(Stretch {
                    at: from,
                    length: to - from,
                    piece_id: None,
                    source: None,
                    from: 0.0,
                    sound: None,
                });
            }
        };
        for (from, to, source) in cover {
            let (from, to) = (from.max(at), to.min(duration));
            if to - from < SLIVER {
                continue;
            }
            wait(&mut stretches, at, from);
            let start = from - source.offset;
            stretches.push(Stretch {
                at: from,
                length: to - from,
                piece_id: None,
                source: Some(source),
                from: start,
                sound: None,
            });
            at = to;
        }
        wait(&mut stretches, at, duration);
        Timeline { stretches }
    }

    /// Stretches as given, laid end to end as they are.
    pub fn from_stretches(stretches: Vec<Stretch>) -> Timeline {
        Timeline { stretches }
    }

    pub fn stretches(&self) -> &[Stretch] {
        &self.stretches
    }

    pub fn duration(&self) -> f64 {
        self.stretches.last().map_or(0.0, Stretch::end)
    }

    /// The stretch `time` is in (the last, at or past the end), by index.
    pub fn index_at(&self, time: f64) -> Option<usize> {
        if self.stretches.is_empty() {
            return None;
        }
        let i = self.stretches.iter().position(|s| time < s.end() - SLIVER).unwrap_or(self.stretches.len() - 1);
        Some(i)
    }

    pub fn at(&self, time: f64) -> Option<&Stretch> {
        self.index_at(time).map(|i| &self.stretches[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vod_downloading_plays_what_is_in_and_waits_for_the_rest() {
        let source =
            |from: f64| Source { path: "/f/section.mkv".into(), offset: from, section: Some("1-0-600000".into()) };
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
    }

    fn piece(id: i64, vod_id: i64, start: f64, end: f64, struck: &[(f64, f64)]) -> EditPiece {
        EditPiece {
            id,
            edit_id: 1,
            vod_id,
            start,
            end,
            title: String::new(),
            struck: struck.to_vec(),
            vod_title: String::new(),
            hole: false,
            group: None,
        }
    }

    #[test]
    fn a_stretch_whose_sound_came_after_isnt_the_same_stretch() {
        let picture = Stretch {
            at: 0.0,
            length: 4.0,
            piece_id: None,
            source: Some(Source { path: PathBuf::from("v.mp4"), offset: 0.0, section: None }),
            from: 0.0,
            sound: None,
        };
        let with_sound = Stretch {
            length: 6.0,
            sound: Some(Sound { path: PathBuf::from("a.mp4"), offset: 0.0 }),
            ..picture.clone()
        };
        assert!(!picture.continued_by(&with_sound));
        assert!(with_sound.continued_by(&Stretch { length: 8.0, ..with_sound.clone() }));
    }

    #[test]
    fn a_sound_of_its_own_is_read_at_the_same_moment_of_the_vod() {
        // The picture's file starts at VOD 10 s, the sound's at VOD 9.5 s.
        let stretch = Stretch {
            at: 0.0,
            length: 5.0,
            piece_id: None,
            source: Some(Source { path: PathBuf::from("v.mp4"), offset: 10.0, section: None }),
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

    #[test]
    fn an_edit_is_laid_end_to_end_from_the_footage_found() {
        let pieces = [piece(1, 7, 10.0, 20.0, &[(14.0, 16.0)]), piece(2, 8, 5.0, 8.0, &[])];
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

    #[test]
    fn footage_still_coming_plays_as_far_as_it_is_in() {
        let pieces = [piece(1, 7, 10.0, 20.0, &[])];
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

    #[test]
    fn a_long_piece_plays_from_each_section_and_waits_between() {
        // A whole VOD of 600 s: sections 0-100 and 300-400 here.
        let pieces = [piece(1, 7, 0.0, 600.0, &[])];
        let section = |from: f64| Source { path: PathBuf::from(format!("{from}.mkv")), offset: from, section: None };
        let t = Timeline::of(&pieces, |_, _, _| vec![(0.0, 100.0, section(0.0)), (300.0, 400.0, section(300.0))]);
        let s: Vec<(f64, f64, f64, bool)> =
            t.stretches().iter().map(|s| (s.at, s.length, s.from, s.source.is_some())).collect();
        assert_eq!(
            s,
            [
                (0.0, 100.0, 0.0, true),
                (100.0, 200.0, 0.0, false),
                (300.0, 100.0, 0.0, true),
                (400.0, 200.0, 0.0, false)
            ]
        );
        assert_eq!(t.duration(), 600.0);
    }
}
