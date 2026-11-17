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
