//! What plays when: a timeline is stretches laid end to end, each a part of
//! a file (or nothing yet, where the footage isn't here), in the player's
//! own time. A file alone is one stretch; an edit is one stretch for each
//! of its pieces' spans (crates/core `edits`), from the footage found for
//! them, as a render is planned.

use std::path::Path;

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
}

impl Stretch {
    pub fn end(&self) -> f64 {
        self.at + self.length
    }

    /// Whether `other` is this stretch, or this one played further (more
    /// of its footage in): what plays up to this one's end is the same.
    pub fn continued_by(&self, other: &Stretch) -> bool {
        self.at == other.at
            && self.from == other.from
            && self.source == other.source
            && self.piece_id == other.piece_id
            && other.length >= self.length
    }

    /// Seconds into the source at timeline time `time`.
    pub fn file_time(&self, time: f64) -> f64 {
        self.from + (time - self.at)
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
            stretches: vec![Stretch { at: 0.0, length: duration, piece_id: None, source: Some(source), from: 0.0 }],
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
                stretches.push(Stretch { at: *at, length, piece_id: Some(piece), source: None, from: 0.0 });
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
                    });
                    at += to - from;
                    done = to;
                }
                wait(&mut stretches, &mut at, p.id, b - done);
            }
        }
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
    use std::path::PathBuf;

    use super::*;

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
