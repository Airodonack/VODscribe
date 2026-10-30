//! Clips as the record keeps them (schema v22): a clip is a document, with
//! a title, a revision and a tree of items, each a piece (a stretch of one
//! VOD, less the stretches struck out of it, which are jumped over) or a
//! group (a whole clip placed inside another; later). A piece is its own:
//! made from a clip or a stretch of the timeline, it keeps no tie to either,
//! so changing one piece (trimming it, later growing it) changes no other.
//!
//! Three kinds of clip share the table. A *listed* clip has a place in the
//! library's list (`position`): what the interface calls an edit, or a VOD's
//! own clip (`whole_of`: one piece over the whole VOD, the document the VOD
//! view works on). A *snippet* has none, and hangs off a listed clip's clip
//! bar (`bar_of`): what the interface calls a clip. Every time in the record
//! is in flicks (`crate::time`); this module takes and gives seconds at its
//! edge for the interface, converting here.
//!
//! Rendered with `crate::render`, from the footage of each stretch
//! (`crate::footage`).

use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::db::{ClipStatus, Db, MARK_DETAIL_COLUMNS, MarkDetail, mark_detail, now};
use crate::footage::Source;
use crate::render::Piece;
use crate::time::Flicks;
use crate::transcript::{Cue, Segment, Word};

/// Stretches shorter than this are left out (a sliver between two struck lines).
const SLIVER: f64 = 0.04;
/// How near a stretch's end a cut is to be at it: within half a
/// frame (at 25 fps), as marks sit on frames and lines end where they end.
const AT_END: f64 = 0.02;

/// A listed clip other than a VOD's own, for the interface's list of edits.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Edit {
    pub id: i64,
    pub title: String,
    /// The rendered file's: waiting (never rendered), cutting (rendering), ready or error.
    pub status: ClipStatus,
    pub file_path: Option<String>,
    pub error: Option<String>,
    /// The revision it's at (`Db::change`).
    pub revision: i64,
    /// Whether the rendered file is of the edit as it is now.
    pub current: bool,
    /// How many pieces it has, and how long it plays, in seconds.
    pub pieces: usize,
    pub length: f64,
    /// The VODs its pieces are of.
    pub vod_ids: Vec<i64>,
    /// What it plays of each VOD, in order: its pieces less what's struck.
    pub ranges: Vec<EditRange>,
    /// A snippet (a clip on a clip bar, opened in the editor): whose bar it's
    /// on, a VOD's or an edit's. None for a listed clip.
    pub snippet_of: Option<crate::db::ClipBar>,
}

/// A stretch of a VOD an edit plays (`Edit::ranges`).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EditRange {
    pub vod_id: i64,
    pub start: f64,
    pub end: f64,
}

/// A row of the library's list: VODs and edits share one order.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum LibraryItem {
    Vod(i64),
    Edit(i64),
}

/// What a clip holds, as `Db::edit_state` takes it and `Db::restore_edit`
/// puts it back: for undo.
#[derive(Debug, Clone, PartialEq)]
pub struct EditState {
    pub title: String,
    /// Its items in the clip's order, each group before what's in it.
    pub items: Vec<ItemState>,
    /// Its own marks (chapters, highlights), each on a VOD's moment.
    pub marks: Vec<MarkAt>,
}

/// Something put in an edit (`Db::put`): several things picked, dropped on
/// it or added to it from a menu.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum Put {
    /// `start`..`end` of VOD `vod_id` (a stretch of its timeline, or a clip
    /// of its clip bar, named `title`).
    Range { vod_id: i64, start: f64, end: f64, title: Option<String> },
    /// All of VOD `id`, as one piece, named for it.
    Vod { id: i64 },
    /// Clip `id` of an edit's clip bar, as it is.
    Clip { id: i64 },
    /// `from`..`to` seconds of clip `clip`'s timeline (an edit's): what plays there.
    Span { clip: i64, from: f64, to: f64 },
    /// A highlight of VOD `vod_id`, `start`..`end`, named `title`: on a
    /// timeline a group named for it, one band as a clip's pill goes in; on a
    /// clip bar a clip of it.
    Highlight { vod_id: i64, start: f64, end: f64, title: String },
}

/// Where `Db::put` puts things in an edit: on its timeline, at `index` of
/// its top level (or last), or on its clip bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Onto {
    Timeline(Option<usize>),
    Bar,
}

/// A mark of a clip, as the record holds it: on a moment of a VOD, wherever
/// that plays in the clip (`place_marks`).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkAt {
    pub vod_id: i64,
    pub kind: String,
    pub at: Flicks,
    pub text: Option<String>,
    /// What an imported mark brings, and where it came from; its end in
    /// seconds of the VOD, as `at` is.
    pub detail: MarkDetail,
}

/// An item as an `EditState` holds it: what it is and where, not which row
/// it was.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemState {
    /// The group it's in, by its place in `EditState::items`; None: the
    /// clip's top level.
    pub parent: Option<usize>,
    /// The VOD it's a piece of; None for a group.
    pub vod_id: Option<i64>,
    pub start: Option<Flicks>,
    pub end: Option<Flicks>,
    pub title: String,
    pub struck: Vec<(Flicks, Flicks)>,
    pub origin: Option<Flicks>,
}

/// An item of a clip as the record holds it (`Db::item_rows`): a piece, or
/// a group (`vod_id` None) holding the items whose `parent` it is.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemRow {
    pub id: i64,
    pub parent: Option<i64>,
    pub position: i64,
    pub vod_id: Option<i64>,
    pub start: Option<Flicks>,
    pub end: Option<Flicks>,
    pub title: String,
    pub struck: Vec<(Flicks, Flicks)>,
    pub origin: Option<Flicks>,
}

/// The group an edit's piece is in at the edit's top level (`EditPiece::group`).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct GroupOf {
    pub id: i64,
    pub title: String,
}

/// A piece of a clip as the record holds it: `start`..`end` of VOD `vod_id`
/// (`end` None: to the VOD's end), less the `struck` stretches.
#[derive(Debug, Clone, PartialEq)]
pub struct PieceRow {
    pub id: i64,
    pub clip_id: i64,
    pub vod_id: i64,
    pub start: Flicks,
    pub end: Option<Flicks>,
    pub title: String,
    pub struck: Vec<(Flicks, Flicks)>,
    /// A realized piece: the VOD's time its file starts at.
    pub origin: Option<Flicks>,
}

/// A piece of an edit, in seconds for the interface: `start`..`end` of VOD
/// `vod_id`, less the `struck` stretches.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EditPiece {
    pub id: i64,
    pub edit_id: i64,
    pub vod_id: i64,
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub struck: Vec<(f64, f64)>,
    pub vod_title: String,
    /// Its footage is gone for good (its file was deleted outside the app,
    /// say): it plays and renders as black and silence. Said by the app,
    /// which looks for the files (`footage::lost`).
    pub hole: bool,
    /// The group it's in at the edit's top level, if it's in one: a clip
    /// dropped into the edit, which shows as one.
    pub group: Option<GroupOf>,
}

impl EditPiece {
    /// What plays of it, in its VOD's time: `start`..`end` less what's struck.
    pub fn spans(&self) -> Vec<(f64, f64)> {
        spans(self.start, self.end, &self.struck)
    }
}

/// A stretch a clip plays (`played`): `from`..`to` of its piece's VOD, from
/// `at` seconds into the clip's timeline.
#[derive(Debug, Clone, Copy)]
pub struct Played<'a> {
    pub piece: &'a EditPiece,
    pub from: f64,
    pub to: f64,
    pub at: f64,
}

impl Played<'_> {
    /// Where it ends on the clip's timeline.
    pub fn end(&self) -> f64 {
        self.at + self.to - self.from
    }
}

/// What a clip of `pieces` plays, in order: each piece's spans, end to end
/// on its timeline. The one walk of a clip's timeline.
pub fn played(pieces: &[EditPiece]) -> impl Iterator<Item = Played<'_>> {
    let spans = pieces.iter().flat_map(|piece| piece.spans().into_iter().map(move |(from, to)| (piece, from, to)));
    spans.scan(0.0, |at, (piece, from, to)| {
        let span = Played { piece, from, to, at: *at };
        *at += to - from;
        Some(span)
    })
}

/// `start`..`end` less the `struck` stretches (any order, overlapping or
/// not), leaving out slivers.
pub fn spans(start: f64, end: f64, struck: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut cuts: Vec<(f64, f64)> = struck.iter().copied().filter(|(a, b)| b > a).collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = start;
    for (a, b) in cuts {
        if a > at {
            out.push((at, a.min(end)));
        }
        at = at.max(b);
        if at >= end {
            break;
        }
    }
    if at < end {
        out.push((at, end));
    }
    out.retain(|(a, b)| b - a >= SLIVER);
    out
}

/// What each piece's VOD has (`of`: things over stretches of a VOD's time,
/// `(start, end, thing)`) where it plays in a clip of `pieces`, on the
/// clip's timeline: each piece's played stretches end to end, each thing
/// cut to the stretch it shows in, slivers left out.
pub fn on_timeline<T: Clone>(
    pieces: &[EditPiece],
    of: impl FnMut(i64) -> Result<Vec<(f64, f64, T)>>,
) -> Result<Vec<(f64, f64, T)>> {
    on_timeline_cut(pieces, of, |thing, _| Some(thing.clone()))
}

/// What of a thing over a stretch of a VOD shows where only part of it
/// plays (`on_timeline_cut`): `from`..`to` of it, in the VOD's time, put
/// `shift` seconds later on the clip's timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub from: f64,
    pub to: f64,
    pub shift: f64,
    /// `to` is where the stretch that plays ends (a join, or the clip's end).
    pub at_end: bool,
}

/// As `on_timeline`, with `cut` saying what of each thing shows in the
/// stretch of it that plays (none: it's left out).
pub fn on_timeline_cut<T, U>(
    pieces: &[EditPiece],
    mut of: impl FnMut(i64) -> Result<Vec<(f64, f64, T)>>,
    mut cut: impl FnMut(&T, Shown) -> Option<U>,
) -> Result<Vec<(f64, f64, U)>> {
    let mut known = std::collections::HashMap::new();
    let mut out = Vec::new();
    for Played { piece, from: a, to: b, at } in played(pieces) {
        let things = match known.entry(piece.vod_id) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => e.insert(of(piece.vod_id)?),
        };
        for (start, end, thing) in things.iter().filter(|(s, e, _)| *e > a && *s < b) {
            let (start, end) = (start.max(a), end.min(b));
            if end - start < SLIVER {
                continue;
            }
            let shown = Shown { from: start, to: end, shift: at - a, at_end: end >= b - 1e-9 };
            if let Some(shown) = cut(thing, shown) {
                out.push((at + start - a, at + end - a, shown));
            }
        }
    }
    Ok(out)
}

/// What of transcript line `segment` shows where only `shown` of it plays:
/// all of it, or the words said there (each where most of it is), moved onto
/// the clip's timeline; none if no word is. A line without word times shows whole.
pub fn segment_shown(segment: &Segment, shown: Shown) -> Option<Segment> {
    let whole = shown.from <= segment.start + 1e-6 && shown.to >= segment.end - 1e-6;
    let words: Vec<Word> = segment
        .words
        .iter()
        .filter(|w| whole || (w.start + w.end) / 2.0 >= shown.from && (w.start + w.end) / 2.0 < shown.to)
        // Within what plays: a word begun just before it starts with it.
        .map(|w| Word {
            text: w.text.clone(),
            start: w.start.max(shown.from) + shown.shift,
            end: w.end.min(shown.to) + shown.shift,
        })
        .collect();
    if segment.words.is_empty() || whole {
        let text = segment.text.clone();
        return Some(Segment { start: shown.from + shown.shift, end: shown.to + shown.shift, text, words });
    }
    let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    let (first, last) = (words.first()?, words.last()?);
    Some(Segment { start: first.start, end: last.end, text, words })
}

/// A clip of `pieces`' transcript, on its timeline (`segments_of`: a VOD's
/// lines, in its time): each line where it plays, only the words said there.
pub fn clip_segments(
    pieces: &[EditPiece],
    mut segments_of: impl FnMut(i64) -> Result<Vec<Segment>>,
) -> Result<Vec<Segment>> {
    let placed = on_timeline_cut(
        pieces,
        |vod| Ok(segments_of(vod)?.into_iter().map(|s| (s.start, s.end, s)).collect()),
        segment_shown,
    )?;
    Ok(placed.into_iter().map(|(_, _, s)| s).collect())
}

/// Captions for a clip of `pieces`, on its timeline (`segments_of`: a VOD's
/// lines, in its time), made from the words that play: each within the
/// stretch it plays in, then held a moment as a VOD's are, over no other.
pub fn clip_cues(pieces: &[EditPiece], mut segments_of: impl FnMut(i64) -> Result<Vec<Segment>>) -> Result<Vec<Cue>> {
    let placed = on_timeline_cut(
        pieces,
        |vod| Ok(segments_of(vod)?.into_iter().map(|s| (s.start, s.end, s)).collect()),
        |segment, shown| {
            let (from, to) = (shown.from + shown.shift, shown.to + shown.shift);
            let line = segment_shown(segment, shown)?;
            let cues = crate::transcript::cues(std::slice::from_ref(&line));
            // Whether it runs to where the stretch that plays ends (a join, or the end).
            let cues = cues.into_iter().map(|c| {
                (Cue { start: c.start.max(from), end: c.end.min(to), text: c.text }, shown.at_end && c.end >= to)
            });
            Some(cues.collect::<Vec<_>>())
        },
    )?;
    let mut out: Vec<(Cue, bool)> = placed.into_iter().flat_map(|(_, _, cues)| cues).collect();
    out.sort_by(|a, b| a.0.start.total_cmp(&b.0.start));
    // One that runs to a join is held there a moment, as a VOD's are after
    // their last word (so captions don't blink off at each join), but never
    // over the next, nor past the clip's end.
    for i in 0..out.len().saturating_sub(1) {
        let next = out[i + 1].0.start;
        let (cue, at_join) = &mut out[i];
        if *at_join && cue.end < next {
            cue.end = (cue.end + crate::transcript::CUE_HOLD).min(next);
        }
        cue.end = cue.end.min(next).max(cue.start);
    }
    Ok(out.into_iter().map(|(c, _)| c).collect())
}

/// Where a clip of `pieces` shows its `marks` on its timeline, in time
/// order. A highlight shows where its moment plays; a chapter, where its
/// moment plays or, if that's trimmed away, where its piece plays again
/// after it (a chapter keeps its start in what's left). A moment played
/// more than once shows where it's first played.
pub fn place_marks(pieces: &[EditPiece], marks: &[(i64, MarkAt)]) -> Vec<crate::db::Mark> {
    let mut out: Vec<crate::db::Mark> = Vec::new();
    let mut at = 0.0;
    for p in pieces {
        let spans = p.spans();
        for (id, m) in marks.iter().filter(|(_, m)| m.vod_id == p.vod_id) {
            if out.iter().any(|o| o.id == *id) {
                continue;
            }
            let moment = m.at.seconds();
            let mut t = at;
            let mut placed = None;
            for (i, &(a, b)) in spans.iter().enumerate() {
                if moment >= a && moment < b {
                    placed = Some((t + moment - a, i, t));
                } else if m.kind == "chapter" && moment < a && moment >= p.start {
                    placed = Some((t, i, t));
                }
                if placed.is_some() {
                    break;
                }
                t += b - a;
            }
            if let Some((start, i, t)) = placed {
                let end = m.detail.end.map(|end| placed_end(&spans[i..], t, end)).filter(|&end| end > start);
                out.push(crate::db::Mark {
                    id: *id,
                    vod_id: m.vod_id,
                    kind: m.kind.clone(),
                    start,
                    text: m.text.clone(),
                    detail: MarkDetail { end, ..m.detail.clone() },
                });
            }
        }
        at += spans.iter().map(|(a, b)| b - a).sum::<f64>();
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.id.cmp(&b.id)));
    out
}

/// Where a mark's `end` (seconds of its VOD) is on a clip's timeline, its
/// start in `spans` (what plays of its piece from there on; the first plays
/// from `t`): what plays of it up to there, so a stretch struck out of it
/// shortens it. One that ends where its piece doesn't play (struck, or past
/// the piece) ends where its piece last plays before then: a highlight
/// runs as long as it's in the clip.
fn placed_end(spans: &[(f64, f64)], mut t: f64, end: f64) -> f64 {
    let mut out = t;
    for &(a, b) in spans {
        if end <= a {
            break;
        }
        out = t + end.min(b) - a;
        t += b - a;
    }
    out
}

/// The VOD, and its moment (seconds), playing at `time` of a clip of
/// `pieces`' timeline; at its very end, a millisecond before (the end
/// itself plays nothing, and a mark there wouldn't show).
pub fn source_at(pieces: &[EditPiece], time: f64) -> Option<(i64, f64)> {
    let mut last = None;
    for span in played(pieces) {
        if time >= span.at && time < span.end() {
            return Some((span.piece.vod_id, span.from + time - span.at));
        }
        last = Some(span);
    }
    let last = last.filter(|span| (time - span.end()).abs() < 1e-6)?;
    Some((last.piece.vod_id, (last.to - 0.001).max(last.from)))
}

/// A piece of a stretch of a clip (`stretch_pieces`): `start`..`end` of VOD
/// `vod_id`, less `struck`, titled `title`.
#[derive(Debug, Clone, PartialEq)]
pub struct StretchPiece {
    pub vod_id: i64,
    pub start: f64,
    pub end: f64,
    pub struck: Vec<(f64, f64)>,
    pub title: String,
}

/// Whether a clip bar's clip of `pieces` is cut straight from its VOD (one
/// piece, nothing struck, not a group) rather than rendered as an edit is.
pub fn cut_whole(pieces: &[EditPiece]) -> bool {
    matches!(pieces, [p] if p.struck.is_empty() && p.group.is_none())
}

/// What `from`..`to` of a clip of `pieces`' timeline plays, as pieces of
/// their own: each piece it reaches, trimmed to it, with what's jumped over
/// in between struck.
pub fn stretch_pieces(pieces: &[EditPiece], from: f64, to: f64) -> Vec<StretchPiece> {
    let mut out = Vec::new();
    let mut at = 0.0;
    for p in pieces {
        let mut reached: Vec<(f64, f64)> = Vec::new();
        for (a, b) in p.spans() {
            let (s, e) = (from.max(at), to.min(at + b - a));
            if e - s >= SLIVER {
                reached.push((a + s - at, a + e - at));
            }
            at += b - a;
        }
        if let (Some(first), Some(last)) = (reached.first(), reached.last()) {
            let struck = reached.windows(2).map(|w| (w[0].1, w[1].0)).collect();
            out.push(StretchPiece { vod_id: p.vod_id, start: first.0, end: last.1, struck, title: p.title.clone() });
        }
    }
    out
}

/// What taking a slice of a clip's timeline out does to a piece it reaches
/// (`cutting_out`), in its VOD's seconds.
#[derive(Debug, Clone, PartialEq)]
pub enum Cutting {
    /// All of it goes.
    Whole,
    /// It keeps `start`..`end` (the slice took its beginning or its end).
    Keep(f64, f64),
    /// It's split: the first part ends at `first_end`, the second starts at
    /// `second_start`, and what's between goes.
    Split { first_end: f64, second_start: f64 },
}

/// Taking `from`..`to` of a clip of `pieces`' timeline out: for each piece
/// it reaches, what becomes of it. A slice is a slice, however it was
/// picked: the pieces split at its ends and the middle goes (nothing is
/// struck). Within a sliver of a piece's edge counts as at it.
pub fn cutting_out(pieces: &[EditPiece], from: f64, to: f64) -> Vec<(i64, Cutting)> {
    let mut out = Vec::new();
    let mut at = 0.0;
    for p in pieces {
        let spans = p.spans();
        let length: f64 = spans.iter().map(|(a, b)| b - a).sum();
        let (lo, hi) = (from.max(at) - at, to.min(at + length) - at);
        at += length;
        if hi - lo < SLIVER {
            continue;
        }
        // The VOD's moment `t` seconds into the piece: where a span ends
        // (`ending`), or where the next starts.
        let source = |t: f64, ending: bool| {
            let mut into = 0.0;
            for &(a, b) in &spans {
                if t < into + b - a || (ending && t <= into + b - a) {
                    return a + t - into;
                }
                into += b - a;
            }
            p.end
        };
        let (keeps_start, keeps_end) = (lo > SLIVER, hi < length - SLIVER);
        out.push((
            p.id,
            match (keeps_start, keeps_end) {
                (false, false) => Cutting::Whole,
                (true, false) => Cutting::Keep(p.start, source(lo, true)),
                (false, true) => Cutting::Keep(source(hi, false), p.end),
                (true, true) => Cutting::Split { first_end: source(lo, true), second_start: source(hi, false) },
            },
        ));
    }
    out
}

/// Whether `cover` (`footage::cover` of `start`..`end`) has all of it, with no gap.
pub fn covers(cover: &[(f64, f64, Source)], start: f64, end: f64) -> bool {
    let mut at = start;
    for (from, to, _) in cover {
        if *from > at + 1e-6 {
            return false;
        }
        at = at.max(*to);
    }
    at >= end - 1e-6
}

/// What to render for `pieces`: each one's spans, from the footage `cover`
/// gives for its stretch of its VOD (`footage::cover`: file by file), and
/// where none is, a hole's black and silence. Fails if some footage of a
/// piece that isn't a hole isn't here yet.
pub fn render_pieces(
    pieces: &[EditPiece],
    cover: impl Fn(i64, f64, f64) -> Vec<(f64, f64, Source)>,
) -> Result<Vec<Piece>> {
    let mut out = Vec::new();
    for p in pieces {
        let gap = |out: &mut Vec<Piece>, from: f64, to: f64| {
            if to - from < 1e-6 {
                return Ok(());
            }
            if !p.hole {
                bail!("The footage of “{}” isn't here yet.", p.title);
            }
            out.push(Piece { source: None, start: 0.0, end: to - from });
            Ok(())
        };
        for (a, b) in p.spans() {
            let mut done = a;
            for (from, to, source) in cover(p.vod_id, a, b) {
                let (from, to) = (from.max(done), to.min(b));
                if to - from < 1e-6 {
                    continue;
                }
                gap(&mut out, done, from)?;
                out.push(Piece { source: Some(source.path), start: from - source.offset, end: to - source.offset });
                done = to;
            }
            gap(&mut out, done, b)?;
        }
    }
    if out.is_empty() {
        bail!("There's nothing in this edit to render.");
    }
    Ok(out)
}

/// Struck stretches as the record keeps them: a JSON list of flick pairs.
pub(crate) fn struck_json(struck: &[(Flicks, Flicks)]) -> Result<String> {
    Ok(serde_json::to_string(&struck.iter().map(|(a, b)| (a.0, b.0)).collect::<Vec<_>>())?)
}

pub(crate) fn parse_struck(json: &str) -> Vec<(Flicks, Flicks)> {
    serde_json::from_str::<Vec<(i64, i64)>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|(a, b)| (Flicks(a), Flicks(b)))
        .collect()
}

/// `struck` in seconds, to the flick.
pub(crate) fn struck_flicks(struck: &[(f64, f64)]) -> Vec<(Flicks, Flicks)> {
    struck.iter().map(|&(a, b)| (Flicks::from_seconds(a), Flicks::from_seconds(b))).collect()
}

pub(crate) const PIECE_COLUMNS: &str = "id, clip_id, vod_id, start, end, struck, title, origin";

pub(crate) fn piece_from_row(r: &Row) -> rusqlite::Result<PieceRow> {
    Ok(PieceRow {
        id: r.get("id")?,
        clip_id: r.get("clip_id")?,
        vod_id: r.get("vod_id")?,
        start: Flicks(r.get("start")?),
        end: r.get::<_, Option<i64>>("end")?.map(Flicks),
        struck: parse_struck(&r.get::<_, String>("struck")?),
        title: r.get("title")?,
        origin: r.get::<_, Option<i64>>("origin")?.map(Flicks),
    })
}

/// A change asked against revision `base` of clip `clip`, which is at
/// revision `now`: made from what the clip held before someone else's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stale {
    pub clip: i64,
    pub base: i64,
    pub now: i64,
}

impl std::fmt::Display for Stale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "That clip has changed since (revision {}, not {}).", self.now, self.base)
    }
}

impl std::error::Error for Stale {}

/// Where a new clip goes.
pub(crate) enum Place {
    /// At the top of the library's list (as new VODs go).
    Listed,
    /// On listed clip `bar_of`'s clip bar, after its others.
    Bar(i64),
}

impl Db {
    /// The library's list, VODs and edits, in its order.
    pub fn library_order(&self) -> Result<Vec<LibraryItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT whole_of, id FROM clips WHERE position IS NOT NULL ORDER BY position, whole_of IS NULL, id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(match r.get::<_, Option<i64>>(0)? {
                Some(vod_id) => LibraryItem::Vod(vod_id),
                None => LibraryItem::Edit(r.get(1)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Puts the library's list in the order of `items`.
    pub fn reorder_library(&self, items: &[LibraryItem]) -> Result<()> {
        self.atomically(|db| {
            for (position, item) in items.iter().enumerate() {
                let (sql, id) = match item {
                    LibraryItem::Vod(id) => ("UPDATE clips SET position = ?2 WHERE whole_of = ?1", id),
                    LibraryItem::Edit(id) => ("UPDATE clips SET position = ?2 WHERE id = ?1 AND whole_of IS NULL", id),
                };
                db.conn.execute(sql, params![id, position as i64])?;
            }
            Ok(())
        })
    }

    /// VOD `vod_id`'s own clip (the one over the whole VOD), if the VOD is there.
    pub fn vod_clip(&self, vod_id: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT id FROM clips WHERE whole_of = ?1", [vod_id], |r| r.get(0)).optional()?)
    }

    /// How many pieces of clips use VOD `vod_id` (a source), its own clip's
    /// included; 0 if it's unused, or gone.
    pub fn uses(&self, vod_id: i64) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT pieces FROM vods WHERE id = ?1", [vod_id], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }

    /// The sources nothing uses: no entry lists them and no clip holds any
    /// of them. They're erased as they're removed, their rows kept as gone
    /// sources till the app next starts (so what's undone meanwhile still
    /// finds them, holes), and go then.
    pub fn unused_sources(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM vods WHERE pieces <= 0 ORDER BY id")?;
        Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Takes VOD `vod_id` out of the library's list: its own clip goes, with
    /// its marks and the snippets on its bar. The VOD stays a source while
    /// other clips use it; true if nothing does now.
    pub fn remove_entry(&self, vod_id: i64) -> Result<bool> {
        self.conn.execute("DELETE FROM clips WHERE whole_of = ?1", [vod_id])?;
        Ok(self.uses(vod_id)? <= 0)
    }

    /// Makes VOD `vod_id`'s own clip, over the whole VOD, at the top of the
    /// library's list: as the VOD is added.
    pub(crate) fn add_vod_clip(&self, vod_id: i64, title: &str) -> Result<i64> {
        let id = self.new_clip(title, Place::Listed)?;
        self.conn.execute("UPDATE clips SET whole_of = ?2 WHERE id = ?1", params![id, vod_id])?;
        self.conn.execute(
            "INSERT INTO items (clip_id, position, kind, vod_id, start, end) VALUES (?1, 0, 'piece', ?2, 0, NULL)",
            params![id, vod_id],
        )?;
        Ok(id)
    }

    /// A new, empty clip titled `title`, where `place` says; its id.
    pub(crate) fn new_clip(&self, title: &str, place: Place) -> Result<i64> {
        match place {
            Place::Listed => self.conn.execute(
                "INSERT INTO clips (title, created_at, position)
                 VALUES (?1, ?2, (SELECT coalesce(min(position), 0) - 1 FROM clips))",
                params![title, now()],
            )?,
            Place::Bar(bar_of) => self.conn.execute(
                "INSERT INTO clips (title, created_at, bar_of, bar_position)
                 VALUES (?1, ?2, ?3, (SELECT coalesce(max(bar_position), 0) + 1 FROM clips WHERE bar_of = ?3))",
                params![title, now(), bar_of],
            )?,
        };
        let id = self.conn.last_insert_rowid();
        self.conn.execute("INSERT INTO renders (clip_id) VALUES (?1)", [id])?;
        Ok(id)
    }

    /// Every edit (listed clips other than VODs' own), in their order.
    pub fn edits(&self) -> Result<Vec<Edit>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM clips WHERE position IS NOT NULL AND whole_of IS NULL ORDER BY position, id")?;
        let ids: Vec<i64> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.into_iter().filter_map(|id| self.edit(id).transpose()).collect()
    }

    pub fn edit(&self, id: i64) -> Result<Option<Edit>> {
        let row = self
            .conn
            .query_row(
                "SELECT c.title, r.status, r.file_path, r.error, c.revision, r.revision,
                        c.bar_of, (SELECT whole_of FROM clips o WHERE o.id = c.bar_of), r.made_of
                 FROM clips c JOIN renders r ON r.clip_id = c.id WHERE c.id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        crate::db::ClipBar::of(r.get(6)?, r.get(7)?),
                        r.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()?;
        let Some((title, status, file_path, error, revision, rendered, snippet_of, made_of)) = row else {
            return Ok(None);
        };
        let pieces = self.edit_pieces(id)?;
        // Of the edit as it is if made of what it's made of now (a file made
        // before that was recorded: of the revision it was made at).
        let current = match made_of {
            Some(made) => made == self.made_of(id, &title, &pieces)?,
            None => rendered == Some(revision),
        };
        let ranges: Vec<EditRange> = pieces
            .iter()
            .flat_map(|p| p.spans().into_iter().map(|(start, end)| EditRange { vod_id: p.vod_id, start, end }))
            .collect();
        let mut vod_ids: Vec<i64> = pieces.iter().map(|p| p.vod_id).collect();
        vod_ids.sort_unstable();
        vod_ids.dedup();
        Ok(Some(Edit {
            id,
            title,
            status: ClipStatus::parse(&status),
            file_path,
            error,
            revision,
            current,
            pieces: pieces.len(),
            length: ranges.iter().map(|r| r.end - r.start).sum(),
            vod_ids,
            ranges,
            snippet_of,
        }))
    }

    /// The chapters a file of clip `id` (of `pieces`) has, on its timeline:
    /// each one's start and title ("Chapter n" if it has none).
    pub fn file_chapters(&self, id: i64, pieces: &[EditPiece]) -> Result<Vec<(f64, String)>> {
        Ok(place_marks(pieces, &self.clip_marks(id)?)
            .into_iter()
            .filter(|m| m.kind == "chapter")
            .enumerate()
            .map(|(i, m)| (m.start, m.text.unwrap_or_else(|| format!("Chapter {}", i + 1))))
            .collect())
    }

    /// What a file of clip `id` (titled `title`, of `pieces`) is made of: its
    /// name, what plays (each VOD's stretches, in order, to the microsecond)
    /// and its chapters. A change that leaves this as it was (a highlight, a
    /// cut, a group opened) leaves its file as it is. A clip bar's clip cut
    /// straight from its VOD (`cut_whole`) has neither name nor chapters in
    /// its file: only what plays counts.
    pub fn made_of(&self, id: i64, title: &str, pieces: &[EditPiece]) -> Result<String> {
        let us = |t: f64| (t * 1e6).round() as i64;
        let plays: Vec<(i64, i64, i64)> =
            pieces.iter().flat_map(|p| p.spans().into_iter().map(|(a, b)| (p.vod_id, us(a), us(b)))).collect();
        if self.is_snippet(id)? == Some(true) && cut_whole(pieces) {
            return Ok(serde_json::to_string(&("", plays, ()))?);
        }
        let chapters: Vec<(i64, String)> =
            self.file_chapters(id, pieces)?.into_iter().map(|(start, text)| (us(start), text)).collect();
        Ok(serde_json::to_string(&(title, plays, chapters))?)
    }

    /// Records that clip `id`'s file, just made, is of what the clip holds
    /// now (`made_of`).
    pub fn set_made_now(&self, id: i64) -> Result<()> {
        let title: String = self.conn.query_row("SELECT title FROM clips WHERE id = ?1", [id], |r| r.get(0))?;
        self.set_made_of(id, &self.made_of(id, &title, &self.edit_pieces(id)?)?)
    }

    /// Records that clip `id`'s file was made of `made_of` (`made_of`).
    pub fn set_made_of(&self, id: i64, made_of: &str) -> Result<()> {
        self.conn.execute("UPDATE renders SET made_of = ?2 WHERE clip_id = ?1", params![id, made_of])?;
        Ok(())
    }

    /// A new, empty edit, at the top of the library's list (as new VODs go).
    pub fn add_edit(&self, title: &str) -> Result<Edit> {
        let id = self.new_clip(title, Place::Listed)?;
        Ok(self.edit(id)?.expect("just inserted"))
    }

    pub fn rename_edit(&self, id: i64, title: &str) -> Result<()> {
        self.change(id, None, |db| {
            db.conn.execute("UPDATE clips SET title = ?2 WHERE id = ?1", params![id, title])?;
            Ok(())
        })
    }

    /// Deletes clip `id`, with its items, marks and snippets.
    /// Deletes edit `id`, if it's one (a clip of the library's list, not a
    /// VOD's own nor one on a clip bar); false if it isn't.
    pub fn delete_edit(&self, id: i64) -> Result<bool> {
        let deleted =
            self.conn.execute("DELETE FROM clips WHERE id = ?1 AND position IS NOT NULL AND whole_of IS NULL", [id])?;
        Ok(deleted > 0)
    }

    /// Clip `id`'s pieces at its top level, in order, as the record holds them.
    pub fn piece_rows(&self, clip_id: i64) -> Result<Vec<PieceRow>> {
        let sql = format!(
            "SELECT {PIECE_COLUMNS} FROM items WHERE clip_id = ?1 AND parent IS NULL AND kind = 'piece' ORDER BY position, id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([clip_id], piece_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn piece_row(&self, id: i64) -> Result<Option<PieceRow>> {
        let sql = format!("SELECT {PIECE_COLUMNS} FROM items WHERE id = ?1 AND kind = 'piece'");
        Ok(self.conn.query_row(&sql, [id], piece_from_row).optional()?)
    }

    /// Where piece `row` ends, in seconds: its end, or its VOD's (0 if that
    /// isn't known yet).
    fn piece_end(&self, row: &PieceRow) -> Result<f64> {
        Ok(match row.end {
            Some(end) => end.seconds(),
            None => self.vod_duration(row.vod_id)?.unwrap_or(0.0),
        })
    }

    fn vod_duration(&self, vod_id: i64) -> Result<Option<f64>> {
        Ok(self.conn.query_row("SELECT duration FROM vods WHERE id = ?1", [vod_id], |r| r.get(0)).optional()?.flatten())
    }

    /// Clip `clip_id`'s items in its order: each group followed by what's in
    /// it, depth first.
    pub fn item_rows(&self, clip_id: i64) -> Result<Vec<ItemRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent, position, vod_id, start, end, struck, title, origin FROM items
             WHERE clip_id = ?1 ORDER BY position, id",
        )?;
        let all: Vec<ItemRow> = stmt
            .query_map([clip_id], |r| {
                Ok(ItemRow {
                    id: r.get(0)?,
                    parent: r.get(1)?,
                    position: r.get(2)?,
                    vod_id: r.get(3)?,
                    start: r.get::<_, Option<i64>>(4)?.map(Flicks),
                    end: r.get::<_, Option<i64>>(5)?.map(Flicks),
                    struck: parse_struck(&r.get::<_, String>(6)?),
                    title: r.get(7)?,
                    origin: r.get::<_, Option<i64>>(8)?.map(Flicks),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        fn walk(all: &[ItemRow], parent: Option<i64>, out: &mut Vec<ItemRow>) {
            for item in all.iter().filter(|i| i.parent == parent) {
                out.push(item.clone());
                if item.vod_id.is_none() {
                    walk(all, Some(item.id), out);
                }
            }
        }
        let mut out = Vec::with_capacity(all.len());
        walk(&all, None, &mut out);
        Ok(out)
    }

    /// Edit `id`'s pieces, in order (those in groups where the groups are),
    /// in seconds.
    pub fn edit_pieces(&self, id: i64) -> Result<Vec<EditPiece>> {
        let items = self.item_rows(id)?;
        // Each item's group at the top level, if it's in one.
        let mut top: std::collections::HashMap<i64, GroupOf> = std::collections::HashMap::new();
        for item in &items {
            let group = match item.parent {
                None if item.vod_id.is_none() => Some(GroupOf { id: item.id, title: item.title.clone() }),
                None => None,
                Some(parent) => top.get(&parent).cloned(),
            };
            if let Some(group) = group {
                top.insert(item.id, group);
            }
        }
        let mut out = Vec::new();
        for item in items.iter().filter(|i| i.vod_id.is_some()) {
            let Some(row) = self.piece_row(item.id)? else { continue };
            let vod_title =
                self.conn.query_row("SELECT title FROM vods WHERE id = ?1", [row.vod_id], |r| r.get(0)).optional()?;
            let end = self.piece_end(&row)?;
            out.push(EditPiece {
                id: row.id,
                edit_id: row.clip_id,
                vod_id: row.vod_id,
                start: row.start.seconds(),
                end,
                title: row.title,
                struck: row.struck.iter().map(|(a, b)| (a.seconds(), b.seconds())).collect(),
                vod_title: vod_title.unwrap_or_default(),
                hole: false,
                group: top.get(&item.id).cloned(),
            });
        }
        Ok(out)
    }

    /// Puts a copy of clip `from` (a snippet, or another listed clip) in
    /// clip `into` as a group, at `index` of its top level (or last): its
    /// items as they are, with ids of their own, titled as `from` is. It's
    /// the group's own from then on: changing `from` changes nothing here.
    pub fn add_group(&self, into: i64, from: i64, index: Option<usize>) -> Result<i64> {
        let title: String = self
            .conn
            .query_row("SELECT title FROM clips WHERE id = ?1", [from], |r| r.get(0))
            .optional()?
            .context("That clip is gone.")?;
        let items = self.item_rows(from)?;
        anyhow::ensure!(into != from, "A clip can't go inside itself.");
        // Its chapters and highlights that show in it come too, on the same
        // moments (cuts are where its own stretches were, not its).
        let marks = self.clip_marks(from)?;
        let shown: Vec<MarkAt> = place_marks(&self.edit_pieces(from)?, &marks)
            .iter()
            .filter(|m| m.kind != "cut")
            .filter_map(|m| marks.iter().find(|(id, _)| *id == m.id).map(|(_, at)| at.clone()))
            .collect();
        self.change(into, None, |db| {
            db.conn.execute(
                "INSERT INTO items (clip_id, position, kind, title) VALUES (?1, 0, 'group', ?2)",
                params![into, title],
            )?;
            let group = db.conn.last_insert_rowid();
            let mut ids = std::collections::HashMap::new();
            for item in &items {
                let parent = item.parent.map_or(Some(group), |p| ids.get(&p).copied());
                db.conn.execute(
                    "INSERT INTO items (clip_id, parent, position, kind, vod_id, start, end, struck, title, origin)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        into,
                        parent,
                        item.position,
                        if item.vod_id.is_some() { "piece" } else { "group" },
                        item.vod_id,
                        item.start.map(|f| f.0),
                        item.end.map(|f| f.0),
                        struck_json(&item.struck)?,
                        item.title,
                        item.origin.map(|f| f.0)
                    ],
                )?;
                ids.insert(item.id, db.conn.last_insert_rowid());
            }
            for m in &shown {
                db.insert_mark_with(into, m.vod_id, &m.kind, m.at, m.text.as_deref(), &m.detail)?;
            }
            db.place(into, group, index)?;
            Ok(group)
        })
    }

    /// Puts `start`..`end` of VOD `vod_id` in clip `into` as a group titled
    /// `title`, holding it as one piece, at `index` of its top level (or
    /// last), with the VOD's chapters and highlights there: a highlight goes
    /// in as a clip's pill does, one band named for it. The group's id.
    pub fn add_stretch_group(
        &self,
        into: i64,
        vod_id: i64,
        (start, end): (f64, f64),
        title: &str,
        index: Option<usize>,
    ) -> Result<i64> {
        self.vod(vod_id)?.context("That VOD is gone.")?;
        let (start, end) = self.within_vod(vod_id, start, end)?;
        self.change(into, None, |db| {
            db.conn.execute(
                "INSERT INTO items (clip_id, position, kind, title) VALUES (?1, 0, 'group', ?2)",
                params![into, title],
            )?;
            let group = db.conn.last_insert_rowid();
            let piece = db.insert_piece(into, vod_id, (start, end), title, &[])?;
            db.conn.execute("UPDATE items SET parent = ?2 WHERE id = ?1", params![piece, group])?;
            db.copy_vod_marks(into, vod_id, start, end)?;
            db.place(into, group, index)?;
            Ok(group)
        })
    }

    /// Clip `id`'s own marks, with their ids, in the order they were made at.
    pub fn clip_marks(&self, id: i64) -> Result<Vec<(i64, MarkAt)>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, vod_id, kind, at, text, {MARK_DETAIL_COLUMNS} FROM marks WHERE clip_id = ?1 ORDER BY at, id"
        ))?;
        let rows = stmt.query_map([id], |r| {
            let at = MarkAt {
                vod_id: r.get(1)?,
                kind: r.get(2)?,
                at: Flicks(r.get(3)?),
                text: r.get(4)?,
                detail: mark_detail(r, 5)?,
            };
            Ok((r.get(0)?, at))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Puts a mark on clip `clip`, on VOD `vod_id`'s moment `at` (seconds of
    /// it), as a change to the clip; its id.
    pub fn add_clip_mark(&self, clip: i64, vod_id: i64, kind: &str, at: f64, text: Option<&str>) -> Result<i64> {
        self.change(clip, None, |db| db.insert_mark(clip, vod_id, kind, Flicks::from_seconds(at), text))
    }

    /// Makes `from`..`to` of edit `edit`'s timeline a clip on its clip bar
    /// (only dragged out, unless `shown`): the pieces that play there, and
    /// the edit's chapters and highlights in it, copied.
    /// A clip there already of exactly that stretch is given instead (C
    /// pressed twice), put on the bar if `shown` and it was only dragged out.
    pub fn add_edit_clip(&self, edit: i64, from: f64, to: f64, title: &str, shown: bool) -> Result<crate::db::Clip> {
        self.check_edit_span(edit, from, to)?;
        let pieces = self.edit_pieces(edit)?;
        let parts = stretch_pieces(&pieces, from, to);
        if parts.is_empty() {
            bail!("Nothing of the edit plays there.");
        }
        if let Some(clip) = self.bar_clip_of(edit, &parts)? {
            if shown && !clip.shown {
                self.show_clip(clip.id)?;
            }
            return Ok(self.clip(clip.id)?.expect("on the bar"));
        }
        let marks = self.clip_marks(edit)?;
        let inside: Vec<&MarkAt> = place_marks(&pieces, &marks)
            .iter()
            .filter(|m| m.kind != "cut" && m.start >= from && m.start < to)
            .filter_map(|m| marks.iter().find(|(id, _)| *id == m.id).map(|(_, at)| at))
            .collect();
        let add = |db: &Db| {
            let id = db.new_clip(title, Place::Bar(edit))?;
            db.conn.execute("UPDATE clips SET shown = ?2 WHERE id = ?1", params![id, shown])?;
            for (i, p) in parts.iter().enumerate() {
                let range = (Flicks::from_seconds(p.start), Flicks::from_seconds(p.end));
                let piece = db.insert_piece(id, p.vod_id, range, &p.title, &struck_flicks(&p.struck))?;
                db.conn.execute("UPDATE items SET position = ?2 WHERE id = ?1", params![piece, i as i64])?;
            }
            for m in &inside {
                db.insert_mark_with(id, m.vod_id, &m.kind, m.at, m.text.as_deref(), &m.detail)?;
            }
            Ok(id)
        };
        let id = if shown { self.change(edit, None, add)? } else { self.atomically(add)? };
        Ok(self.clip(id)?.expect("just inserted"))
    }

    /// The clip on edit `edit`'s clip bar of exactly what plays from `from`
    /// to `to` of its timeline, if there's one (`add_edit_clip` gives it).
    pub fn edit_clip_of(&self, edit: i64, from: f64, to: f64) -> Result<Option<crate::db::Clip>> {
        let parts = stretch_pieces(&self.edit_pieces(edit)?, from, to);
        if parts.is_empty() {
            return Ok(None);
        }
        self.bar_clip_of(edit, &parts)
    }

    /// The clip on edit `edit`'s clip bar that plays just what `parts` do.
    fn bar_clip_of(&self, edit: i64, parts: &[StretchPiece]) -> Result<Option<crate::db::Clip>> {
        let near = |a: f64, b: f64| (a - b).abs() < 0.001;
        let same_spans = |a: &[(f64, f64)], b: &[(f64, f64)]| {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(x.0, y.0) && near(x.1, y.1))
        };
        for clip in self.bar_clips(edit)? {
            let pieces = self.edit_pieces(clip.id)?;
            let same = pieces.len() == parts.len()
                && pieces
                    .iter()
                    .zip(parts)
                    .all(|(p, s)| p.vod_id == s.vod_id && same_spans(&p.spans(), &spans(s.start, s.end, &s.struck)));
            if same {
                return Ok(Some(clip));
            }
        }
        Ok(None)
    }

    /// A clip on listed clip `bar`'s clip bar, made anew as `state` has it
    /// (undo bringing back one an edit's change took away); its id.
    pub fn add_bar_clip(&self, bar: i64, state: &EditState) -> Result<i64> {
        self.change(bar, None, |db| {
            let id = db.new_clip(&state.title, Place::Bar(bar))?;
            db.restore_edit(id, state)?;
            Ok(id)
        })
    }

    /// Puts `items` in edit `edit`, one after another in their order, where
    /// `onto` says, as one change (all of them, or none if one can't go):
    /// several things picked, dropped on the edit or added from a menu. On
    /// its timeline each is a piece (a clip of an edit's clip bar, a group);
    /// on its clip bar each is a clip of its own, and one already there is
    /// left as it is. The clips made on the bar, in order.
    pub fn put(&self, edit: i64, items: &[Put], onto: Onto) -> Result<Vec<i64>> {
        self.edit(edit)?.context("That edit is gone.")?;
        self.atomically(|db| {
            let mut made = Vec::new();
            // Where the next one goes on the timeline: after those put so far.
            let mut index = match onto {
                Onto::Timeline(index) => index,
                Onto::Bar => None,
            };
            for item in items {
                match (item, onto) {
                    (Put::Clip { id }, Onto::Timeline(_)) => {
                        db.add_group(edit, *id, index)?;
                        index = index.map(|i| i + 1);
                    }
                    (Put::Clip { id }, Onto::Bar) => {
                        let clip = db.clip(*id)?.context("That clip is gone.")?;
                        if clip.bar != Some(crate::db::ClipBar::Edit { id: edit }) {
                            let state = db.edit_state(clip.id)?.context("That clip is gone.")?;
                            made.push(db.add_bar_clip(edit, &state)?);
                        }
                    }
                    (Put::Highlight { vod_id, start, end, title }, Onto::Timeline(_)) => {
                        db.add_stretch_group(edit, *vod_id, (*start, *end), title, index)?;
                        index = index.map(|i| i + 1);
                    }
                    (Put::Span { clip, from, to }, _) if *clip == edit && onto == Onto::Bar => {
                        // One on the bar already is left as it is.
                        if db.edit_clip_of(edit, *from, *to)?.is_none() {
                            let title = db.span_title(edit, *from, *to)?;
                            made.push(db.add_edit_clip(edit, *from, *to, &title, true)?.id);
                        }
                    }
                    _ => {
                        let (title, parts) = db.put_parts(item)?;
                        if onto == Onto::Bar {
                            made.push(db.add_parts_clip(edit, &title, &parts)?);
                        } else {
                            db.add_parts(edit, &parts, index)?;
                            index = index.map(|i| i + parts.len());
                        }
                    }
                }
            }
            Ok(made)
        })
    }

    /// What `item` (not a clip of an edit's clip bar) plays, as pieces, and
    /// its name: a stretch's own or for what's said at its start, a VOD's title.
    fn put_parts(&self, item: &Put) -> Result<(String, Vec<StretchPiece>)> {
        let part = |vod_id, start, end, title: &str| StretchPiece {
            vod_id,
            start,
            end,
            struck: Vec::new(),
            title: title.to_string(),
        };
        Ok(match item {
            Put::Range { vod_id, start, end, title } => {
                self.vod(*vod_id)?.context("That VOD is gone.")?;
                let title = match title {
                    Some(title) => title.clone(),
                    None => crate::clips::title(&self.words_between(*vod_id, *start, *end)?, *start),
                };
                (title.clone(), vec![part(*vod_id, *start, *end, &title)])
            }
            Put::Highlight { vod_id, start, end, title } => {
                self.vod(*vod_id)?.context("That VOD is gone.")?;
                (title.clone(), vec![part(*vod_id, *start, *end, title)])
            }
            Put::Vod { id } => {
                let vod = self.vod(*id)?.context("That VOD is gone.")?;
                let end =
                    vod.info.duration.with_context(|| format!("How long “{}” is isn't known yet.", vod.info.title))?;
                (vod.info.title.clone(), vec![part(*id, 0.0, end, &vod.info.title)])
            }
            Put::Span { clip, from, to } => {
                self.check_edit_span(*clip, *from, *to)?;
                let parts = stretch_pieces(&self.edit_pieces(*clip)?, *from, *to);
                anyhow::ensure!(!parts.is_empty(), "Nothing of the edit plays there.");
                (self.span_title(*clip, *from, *to)?, parts)
            }
            Put::Clip { .. } => unreachable!("a clip is put as a group or a copy"),
        })
    }

    /// The name for `from`..`to` of clip `clip`'s timeline made a clip: what's
    /// said at its start.
    pub fn span_title(&self, clip: i64, from: f64, to: f64) -> Result<String> {
        self.check_edit_span(clip, from, to)?;
        let parts = stretch_pieces(&self.edit_pieces(clip)?, from, to);
        let first = parts.first().context("Nothing of the edit plays there.")?;
        Ok(crate::clips::title(&self.words_between(first.vod_id, first.start, first.end)?, first.start))
    }

    /// Puts `parts` in clip `clip` as pieces, in order, from `index` of its
    /// top level (or last), with the chapters and highlights of their VODs in them.
    fn add_parts(&self, clip: i64, parts: &[StretchPiece], index: Option<usize>) -> Result<()> {
        self.change(clip, None, |db| {
            for (i, p) in parts.iter().enumerate() {
                let (start, end) = db.within_vod(p.vod_id, p.start, p.end)?;
                let new = db.insert_piece(clip, p.vod_id, (start, end), &p.title, &struck_flicks(&p.struck))?;
                db.copy_vod_marks(clip, p.vod_id, start, end)?;
                db.place(clip, new, index.map(|n| n + i))?;
            }
            Ok(())
        })
    }

    /// A clip on listed clip `bar`'s clip bar titled `title`, of `parts` in
    /// order, with the chapters and highlights of their VODs in them; its id.
    fn add_parts_clip(&self, bar: i64, title: &str, parts: &[StretchPiece]) -> Result<i64> {
        self.change(bar, None, |db| {
            let id = db.new_clip(title, Place::Bar(bar))?;
            db.add_parts(id, parts, None)?;
            Ok(id)
        })
    }

    /// For each stretch (`from`, `to`: seconds of edit `edit`'s timeline),
    /// the first `cap` chat messages of what plays there and how many it
    /// has: each piece's VOD's chat where it plays, at its time in the edit.
    pub fn edit_chat_windows(
        &self,
        edit: i64,
        ranges: &[(f64, f64)],
        cap: usize,
    ) -> Result<Vec<crate::db::ChatWindow>> {
        let pieces = self.edit_pieces(edit)?;
        let mut platforms = std::collections::HashMap::new();
        let mut out = Vec::with_capacity(ranges.len());
        for &(from, to) in ranges {
            let mut messages = Vec::new();
            let mut total = 0;
            for span in played(&pieces) {
                let (s, e) = (from.max(span.at), to.min(span.end()));
                if e <= s {
                    continue;
                }
                let vod = span.piece.vod_id;
                let (src, end) = (span.from + s - span.at, span.from + e - span.at);
                let [w]: [crate::db::ChatWindow; 1] =
                    self.chat_windows(vod, &[(src, end)], cap)?.try_into().expect("one range");
                total += w.total;
                if let std::collections::hash_map::Entry::Vacant(e) = platforms.entry(vod) {
                    e.insert(self.vod(vod)?.map(|v| v.info.platform));
                }
                for mut m in w.messages {
                    // A moment played twice in the window: its messages once, where first played.
                    if messages.iter().any(|seen: &crate::db::ChatLine| seen.id == m.id) {
                        continue;
                    }
                    m.time = s + m.time - src;
                    m.platform = platforms[&vod].clone();
                    messages.push(m);
                }
            }
            messages.truncate(cap);
            out.push(crate::db::ChatWindow { messages, total });
        }
        Ok(out)
    }

    /// Edit `edit`'s chat events (subs, gifts, tips) where they play, on its
    /// timeline, in order: each piece's VOD's, from the stretches it plays.
    pub fn edit_chat_events(&self, edit: i64) -> Result<Vec<crate::db::ChatLine>> {
        let pieces = self.edit_pieces(edit)?;
        let mut known: std::collections::HashMap<i64, (Vec<crate::db::ChatLine>, Option<String>)> =
            std::collections::HashMap::new();
        let mut out = Vec::new();
        for Played { piece, from: a, to: b, at } in played(&pieces) {
            if let std::collections::hash_map::Entry::Vacant(e) = known.entry(piece.vod_id) {
                e.insert((self.chat_events(piece.vod_id)?, self.vod(piece.vod_id)?.map(|v| v.info.platform)));
            }
            let (events, platform) = &known[&piece.vod_id];
            for event in events.iter().filter(|e| e.time >= a && e.time < b) {
                let mut event = event.clone();
                event.time = at + event.time - a;
                event.platform = platform.clone();
                out.push(event);
            }
        }
        out.sort_by(|a, b| a.time.total_cmp(&b.time));
        Ok(out)
    }

    /// How many chat messages play in each of `bins` equal stretches of
    /// edit `edit`'s timeline (`duration` seconds), and in all: each piece's
    /// VOD's chat where it plays.
    pub fn edit_chat_histogram(&self, edit: i64, duration: f64, bins: usize) -> Result<(u32, Vec<u32>)> {
        let mut out = vec![0u32; bins];
        let mut count = 0;
        // Counted even before the length is known (an edit just opened):
        // the count says whether it has chat at all.
        let binned = duration > 0.0 && bins > 0;
        let mut stmt = self.conn.prepare("SELECT time FROM chat WHERE vod_id = ?1 AND time >= ?2 AND time < ?3")?;
        let pieces = self.edit_pieces(edit)?;
        for Played { piece, from: a, to: b, at } in played(&pieces) {
            for time in stmt.query_map(params![piece.vod_id, a, b], |r| r.get::<_, f64>(0))? {
                let t = at + time? - a;
                if binned {
                    out[((t / duration * bins as f64) as usize).min(bins - 1)] += 1;
                }
                count += 1;
            }
        }
        Ok((count, out))
    }

    /// Moves mark `id` to VOD `vod_id`'s moment `at` (seconds of it); false if it's gone.
    pub fn move_mark_to(&self, id: i64, vod_id: i64, at: f64) -> Result<bool> {
        let clip = self.conn.query_row("SELECT clip_id FROM marks WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        let Some(clip) = clip else { return Ok(false) };
        self.change(clip, None, |db| {
            db.conn.execute(
                "UPDATE marks SET vod_id = ?2, at = ?3 WHERE id = ?1",
                params![id, vod_id, Flicks::from_seconds(at).0],
            )?;
            Ok(())
        })?;
        Ok(true)
    }

    /// What clip `id` holds (its title, pieces and marks), to be put back
    /// later with `restore_edit` (for undo); None if it's gone.
    pub fn edit_state(&self, id: i64) -> Result<Option<EditState>> {
        let Some(title) =
            self.conn.query_row("SELECT title FROM clips WHERE id = ?1", [id], |r| r.get(0)).optional()?
        else {
            return Ok(None);
        };
        let rows = self.item_rows(id)?;
        let items = rows
            .iter()
            .map(|i| ItemState {
                parent: i.parent.and_then(|p| rows.iter().position(|r| r.id == p)),
                vod_id: i.vod_id,
                start: i.start,
                end: i.end,
                title: i.title.clone(),
                struck: i.struck.clone(),
                origin: i.origin,
            })
            .collect();
        let marks = self.clip_marks(id)?.into_iter().map(|(_, m)| m).collect();
        Ok(Some(EditState { title, items, marks }))
    }

    /// Puts clip `id` back as `state` had it: its title, and its items in
    /// order (made anew, so with new ids), less pieces of a VOD since deleted.
    /// An error if the clip's gone.
    pub fn restore_edit(&self, id: i64, state: &EditState) -> Result<()> {
        self.change(id, None, |db| {
            db.conn.execute("UPDATE clips SET title = ?2 WHERE id = ?1", params![id, state.title])?;
            db.conn.execute("DELETE FROM items WHERE clip_id = ?1", [id])?;
            let mut ids: Vec<Option<i64>> = Vec::with_capacity(state.items.len());
            for (i, item) in state.items.iter().enumerate() {
                let parent = item.parent.and_then(|p| ids.get(p).copied().flatten());
                let position = state.items[..i].iter().filter(|o| o.parent == item.parent).count() as i64;
                let kind = if item.vod_id.is_some() { "piece" } else { "group" };
                let made = db.conn.execute(
                    "INSERT INTO items (clip_id, parent, position, kind, vod_id, start, end, title, struck, origin)
                     SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
                     WHERE ?5 IS NULL OR EXISTS (SELECT 1 FROM vods WHERE id = ?5)",
                    params![
                        id,
                        parent,
                        position,
                        kind,
                        item.vod_id,
                        item.start.map(|f| f.0),
                        item.end.map(|f| f.0),
                        item.title,
                        struck_json(&item.struck)?,
                        item.origin.map(|f| f.0)
                    ],
                )?;
                ids.push((made > 0).then(|| db.conn.last_insert_rowid()));
            }
            db.conn.execute("DELETE FROM marks WHERE clip_id = ?1", [id])?;
            for m in &state.marks {
                if db.vod(m.vod_id)?.is_some() {
                    db.insert_mark_with(id, m.vod_id, &m.kind, m.at, m.text.as_deref(), &m.detail)?;
                }
            }
            Ok(())
        })
    }

    /// Every piece of every edit (listed clips other than VODs' own), as
    /// (VOD, start, end) in seconds: the footage edits use, kept however old.
    /// Not snippets': once made, a snippet's own file is its stretch's footage.
    pub fn piece_ranges(&self) -> Result<Vec<(i64, f64, f64)>> {
        let sql = format!(
            "SELECT {PIECE_COLUMNS} FROM items WHERE kind = 'piece'
               AND clip_id IN (SELECT id FROM clips WHERE position IS NOT NULL AND whole_of IS NULL)"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<PieceRow> = stmt.query_map([], piece_from_row)?.collect::<rusqlite::Result<_>>()?;
        rows.into_iter().map(|p| Ok((p.vod_id, p.start.seconds(), self.piece_end(&p)?))).collect()
    }

    /// The clip piece `id` is in, if it's there.
    pub fn piece_edit(&self, id: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT clip_id FROM items WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    /// The one writer of clips' documents: runs `f`'s writes to clip `clip`
    /// as one change, one transaction that moves the clip to its next
    /// revision. Against revision `base`, if given: refused (`Stale`) if the
    /// clip is at another by then. A snippet in a clip bar is part of the
    /// listed clip's document as the bar shows it, so a change to it moves
    /// that clip's revision too. An error if the clip's gone.
    pub fn change<T>(&self, clip: i64, base: Option<i64>, f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
        self.atomically(|db| {
            let (now, bar_of): (i64, Option<i64>) = db
                .conn
                .query_row("SELECT revision, CASE WHEN shown THEN bar_of END FROM clips WHERE id = ?1", [clip], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?
                .context("That clip is gone.")?;
            if let Some(base) = base.filter(|&b| b != now) {
                return Err(Stale { clip, base, now }.into());
            }
            let out = f(db)?;
            for id in std::iter::once(clip).chain(bar_of) {
                db.conn.execute("UPDATE clips SET revision = revision + 1 WHERE id = ?1", [id])?;
            }
            Ok(out)
        })
    }

    /// Whether clip `id` is a snippet (on a clip bar) rather than a listed
    /// clip; None if it's gone.
    pub fn is_snippet(&self, id: i64) -> Result<Option<bool>> {
        Ok(self.conn.query_row("SELECT bar_of IS NOT NULL FROM clips WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    /// Clip `id`'s revision now; None if it's gone.
    pub fn revision(&self, id: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT revision FROM clips WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    /// Puts `start`..`end` of VOD `vod_id` in clip `edit_id` as a piece titled
    /// `title`, at `index` (or last). It's a copy: nothing else changes with it.
    pub fn add_piece(
        &self,
        edit_id: i64,
        vod_id: i64,
        (start, end): (f64, f64),
        title: &str,
        index: Option<usize>,
    ) -> Result<()> {
        let (start, end) = self.within_vod(vod_id, start, end)?;
        self.change(edit_id, None, |db| {
            let new = db.insert_piece(edit_id, vod_id, (start, end), title, &[])?;
            db.copy_vod_marks(edit_id, vod_id, start, end)?;
            db.place(edit_id, new, index)
        })
    }

    /// Copies VOD `vod_id`'s chapters and highlights from `start` to `end`
    /// into clip `into`: a stretch of it made a clip, or put in one, brings
    /// them with it.
    pub(crate) fn copy_vod_marks(&self, into: i64, vod_id: i64, start: Flicks, end: Flicks) -> Result<()> {
        self.conn.execute(
            "INSERT INTO marks (clip_id, vod_id, kind, at, text, end_at, category, note, source, people, icon, created_at)
             SELECT ?1, vod_id, kind, at, text, end_at, category, note, source, people, icon, ?5 FROM marks
             WHERE clip_id = (SELECT id FROM clips WHERE whole_of = ?2) AND kind != 'cut' AND at >= ?3 AND at < ?4",
            params![into, vod_id, start.0, end.0, now()],
        )?;
        Ok(())
    }

    /// Inserts a piece of `vod_id` in clip `clip_id`, unplaced (position 0).
    pub(crate) fn insert_piece(
        &self,
        clip_id: i64,
        vod_id: i64,
        (start, end): (Flicks, Flicks),
        title: &str,
        struck: &[(Flicks, Flicks)],
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO items (clip_id, position, kind, vod_id, start, end, title, struck) VALUES (?1, 0, 'piece', ?2, ?3, ?4, ?5, ?6)",
            params![clip_id, vod_id, start.0, end.0, title, struck_json(struck)?],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Moves piece `id` to `index` in its clip.
    pub fn move_piece(&self, id: i64, index: usize) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        self.change(edit_id, None, |db| db.place(edit_id, id, Some(index)))
    }

    /// Numbers item `id`'s siblings in clip `clip_id` (the items in the same
    /// group, or at the top level) in order, with `id` at `index` (or last).
    fn place(&self, clip_id: i64, id: i64, index: Option<usize>) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM items WHERE clip_id = ?1 AND id != ?2
               AND parent IS (SELECT parent FROM items WHERE id = ?2) ORDER BY position, id",
        )?;
        let mut ids: Vec<i64> = stmt.query_map(params![clip_id, id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.insert(index.unwrap_or(ids.len()).min(ids.len()), id);
        for (position, id) in ids.iter().enumerate() {
            self.conn.execute("UPDATE items SET position = ?2 WHERE id = ?1", params![id, position as i64])?;
        }
        Ok(())
    }

    /// `start`..`end` seconds of VOD `vod_id` (`vod_span`), on the flick.
    fn within_vod(&self, vod_id: i64, start: f64, end: f64) -> Result<(Flicks, Flicks)> {
        let (start, end) = self.vod_span(vod_id, start, end)?;
        Ok((Flicks::from_seconds(start), Flicks::from_seconds(end)))
    }

    /// `start`..`end` seconds of VOD `vod_id` if it's a span of it
    /// (`clips::check_span`), its end cut at the VOD's.
    pub fn vod_span(&self, vod_id: i64, start: f64, end: f64) -> Result<(f64, f64)> {
        let duration = self.vod_duration(vod_id)?;
        crate::clips::check_span(start, end, duration).map_err(anyhow::Error::msg)?;
        Ok((start, duration.map_or(end, |d| end.min(d))))
    }

    /// Whether `from`..`to` seconds is a span of clip `clip`'s timeline (an
    /// edit's: as long as what its pieces play), `clips::check_span`.
    pub fn check_edit_span(&self, clip: i64, from: f64, to: f64) -> Result<()> {
        let length: f64 = self.edit_pieces(clip)?.iter().flat_map(|p| p.spans()).map(|(a, b)| b - a).sum();
        // An edit with nothing in it has no end to be past.
        anyhow::ensure!(length > 0.0, "Nothing of the edit plays there.");
        crate::clips::check_span(from, to, Some(length)).map_err(anyhow::Error::msg)
    }

    /// Sets piece `id` to `start`..`end` seconds of its VOD (within the VOD).
    pub fn set_piece_range(&self, id: i64, start: f64, end: f64) -> Result<()> {
        let Some(piece) = self.piece_row(id)? else { return Ok(()) };
        let (start, end) = self.within_vod(piece.vod_id, start, end)?;
        self.change(piece.clip_id, None, |db| {
            db.conn.execute("UPDATE items SET start = ?2, end = ?3 WHERE id = ?1", params![id, start.0, end.0])?;
            Ok(())
        })
    }

    /// Sets the stretches struck out of piece `id`, in seconds. Nothing
    /// strikes stretches now (a slice taken out splits pieces), but pieces
    /// from before may have some: the tests make such pieces with it.
    #[cfg(test)]
    pub(crate) fn set_piece_struck(&self, id: i64, struck: &[(f64, f64)]) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        let struck = struck_json(&struck_flicks(struck))?;
        self.change(edit_id, None, |db| {
            db.conn.execute("UPDATE items SET struck = ?2 WHERE id = ?1", params![id, struck])?;
            Ok(())
        })
    }

    /// Takes `from`..`to` of edit `edit`'s timeline out, as one change: the
    /// pieces it reaches split at its ends, and what's between goes
    /// (`cutting_out`); a group left empty goes too. Its cuts at either end,
    /// and any in it, go as well: where what's left meets is a real split.
    pub fn remove_edit_stretch(&self, edit: i64, from: f64, to: f64) -> Result<()> {
        self.check_edit_span(edit, from, to)?;
        let pieces = self.edit_pieces(edit)?;
        let spent: Vec<i64> = place_marks(&pieces, &self.clip_marks(edit)?)
            .into_iter()
            .filter(|m| m.kind == "cut" && m.start >= from - AT_END && m.start <= to + AT_END)
            .map(|m| m.id)
            .collect();
        self.change(edit, None, |db| {
            for (id, cutting) in cutting_out(&pieces, from, to) {
                let Some(p) = pieces.iter().find(|p| p.id == id) else { continue };
                match cutting {
                    Cutting::Whole => {
                        db.conn.execute("DELETE FROM items WHERE id = ?1", [id])?;
                    }
                    Cutting::Keep(start, end) => db.set_piece_range(id, start, end)?,
                    Cutting::Split { first_end, second_start } => {
                        // The second part split off where it starts, then the
                        // first cut short where it ends.
                        db.split_piece(id, second_start)?;
                        db.set_piece_range(id, p.start, first_end)?;
                    }
                }
            }
            // Groups emptied, from the innermost out.
            while db.conn.execute(
                "DELETE FROM items WHERE clip_id = ?1 AND kind = 'group'
                 AND NOT EXISTS (SELECT 1 FROM items i WHERE i.parent = items.id)",
                [edit],
            )? > 0
            {}
            for id in &spent {
                db.conn.execute("DELETE FROM marks WHERE id = ?1", [id])?;
            }
            Ok(())
        })
    }

    /// Splits piece `id` in two at `at` seconds (its VOD's time), the second
    /// right after the first; both keep what's struck. False if `at` isn't
    /// inside it.
    pub fn split_piece(&self, id: i64, at: f64) -> Result<bool> {
        let Some(piece) = self.piece_row(id)? else { return Ok(false) };
        let (start, end) = (piece.start.seconds(), self.piece_end(&piece)?);
        if !(at > start + SLIVER && at < end - SLIVER) {
            return Ok(false);
        }
        let at = Flicks::from_seconds(at);
        let parent: Option<i64> = self.conn.query_row("SELECT parent FROM items WHERE id = ?1", [id], |r| r.get(0))?;
        let index = self
            .item_rows(piece.clip_id)?
            .iter()
            .filter(|i| i.parent == parent)
            .position(|i| i.id == id)
            .map(|i| i + 1);
        self.change(piece.clip_id, None, |db| {
            db.conn.execute("UPDATE items SET end = ?2 WHERE id = ?1", params![id, at.0])?;
            let end = piece.end.unwrap_or(Flicks::from_seconds(end));
            let new = db.insert_piece(piece.clip_id, piece.vod_id, (at, end), &piece.title, &piece.struck)?;
            // Beside it, in its group if it's in one, and from where it came from too.
            db.conn.execute(
                "UPDATE items SET parent = ?2, origin = (SELECT origin FROM items WHERE id = ?3) WHERE id = ?1",
                params![new, parent, id],
            )?;
            db.place(piece.clip_id, new, index)
        })?;
        Ok(true)
    }

    pub fn remove_piece(&self, id: i64) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        self.change(edit_id, None, |db| {
            db.conn.execute("DELETE FROM items WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Takes several things out of edit `edit` at once, as one change: the
    /// stretches `stretches` of its timeline (as `remove_edit_stretch`, from
    /// the last back, so each one's times are still where they were when it
    /// was picked), then its pieces `pieces` (as `remove_piece`). A piece of
    /// another edit, or one that's gone, is left alone.
    pub fn remove_from_edit(&self, edit: i64, stretches: &[(f64, f64)], pieces: &[i64]) -> Result<()> {
        let mut stretches = stretches.to_vec();
        stretches.sort_by(|a, b| b.0.total_cmp(&a.0));
        self.atomically(|db| {
            for (from, to) in stretches {
                db.remove_edit_stretch(edit, from, to)?;
            }
            for &piece in pieces {
                if db.piece_edit(piece)? == Some(edit) {
                    db.remove_piece(piece)?;
                }
            }
            Ok(())
        })
    }

    /// Edit `id`'s video was dragged out of the app, so another app may use it.
    pub fn hand_out_edit(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE renders SET handed_out = 1 WHERE clip_id = ?1", [id])?;
        Ok(())
    }

    /// Whether edit `id`'s video was dragged out of the app; and it's the app's
    /// own again (`take_back`: rendered to a new file).
    pub fn edit_handed_out(&self, id: i64, take_back: bool) -> Result<bool> {
        let out =
            self.conn.query_row("SELECT handed_out FROM renders WHERE clip_id = ?1", [id], |r| r.get(0)).optional()?;
        if take_back {
            self.conn.execute("UPDATE renders SET handed_out = 0 WHERE clip_id = ?1", [id])?;
        }
        Ok(out.unwrap_or(false))
    }

    /// Records edit `id`'s render: its status, file (kept if None) and error,
    /// and the revision it's of, when done.
    pub fn set_edit_render(
        &self,
        id: i64,
        status: ClipStatus,
        file: Option<&str>,
        error: Option<&str>,
        version: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE renders SET status = ?2, file_path = coalesce(?3, file_path), error = ?4,
               revision = coalesce(?5, revision)
             WHERE clip_id = ?1",
            params![id, status.as_str(), file, error, version],
        )?;
        Ok(())
    }

    /// Edits left rendering (or waiting to) when the app stopped: waiting
    /// again, to be rendered when asked.
    pub fn reset_renders(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE renders SET status = 'waiting'
             WHERE status = 'cutting' AND clip_id IN (SELECT id FROM clips WHERE position IS NOT NULL)",
            [],
        )?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fetch::VodInfo;
    use crate::time::Rate;
    use std::path::Path;

    #[test]
    fn struck_lines_are_jumped_over() {
        assert_eq!(spans(0.0, 10.0, &[]), [(0.0, 10.0)]);
        assert_eq!(spans(0.0, 10.0, &[(6.0, 7.0), (2.0, 3.0)]), [(0.0, 2.0), (3.0, 6.0), (7.0, 10.0)]);
        assert_eq!(spans(0.0, 10.0, &[(-1.0, 1.0), (9.0, 12.0)]), [(1.0, 9.0)], "at the ends: trimmed");
        assert_eq!(spans(0.0, 10.0, &[(2.0, 5.0), (4.0, 6.0)]), [(0.0, 2.0), (6.0, 10.0)], "overlapping");
        assert_eq!(spans(0.0, 10.0, &[(0.0, 10.0)]), [], "all of it");
        assert_eq!(spans(0.0, 10.0, &[(2.0, 5.0), (5.02, 8.0)]), [(0.0, 2.0), (8.0, 10.0)], "no slivers");
    }

    #[test]
    fn lines_and_captions_follow_the_pieces_with_the_words_that_play() {
        let piece = |id, vod_id, start, end, struck: &[(f64, f64)]| EditPiece {
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
        };
        let word = |text: &str, start: f64| Word { text: text.into(), start, end: start + 1.0 };
        let line =
            |start: f64, end: f64, text: &str, words: Vec<Word>| Segment { start, end, text: text.into(), words };
        let lines = |vod| -> Result<Vec<Segment>> {
            Ok(match vod {
                1 => vec![
                    line(0.0, 4.0, "a b c d", vec![word("a", 0.0), word("b", 1.0), word("c", 2.0), word("d", 3.0)]),
                    line(10.0, 11.0, "ten", vec![word("ten", 10.0)]),
                ],
                _ => vec![line(100.0, 103.0, "elsewhere", vec![])],
            })
        };
        // VOD 1 from 1 to 2.6 s (b, and most of c), VOD 2 from 101 s (a
        // line without word times), VOD 1 from 3 s (d, the line split
        // between them), then at 10 (all of "ten").
        let pieces = [
            piece(1, 1, 1.0, 2.6, &[]),
            piece(2, 2, 101.0, 102.0, &[]),
            piece(3, 1, 3.0, 4.0, &[]),
            piece(4, 1, 10.0, 11.0, &[]),
        ];
        let got: Vec<(f64, f64, String)> =
            clip_segments(&pieces, lines).unwrap().into_iter().map(|s| (s.start, s.end, s.text)).collect();
        let want = [(0.0, 1.6, "b c"), (1.6, 2.6, "elsewhere"), (2.6, 3.6, "d"), (3.6, 4.6, "ten")];
        assert_eq!(got.len(), want.len(), "{got:?}");
        for ((start, end, text), (s, e, t)) in got.iter().zip(want) {
            assert!((start - s).abs() < 1e-9 && (end - e).abs() < 1e-9 && text == t, "{got:?}");
        }
        // Captions are made from the words that play: none of "a".
        let cues = clip_cues(&pieces, lines).unwrap();
        assert_eq!(cues.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["b c", "elsewhere", "d", "ten"]);
        // Each within the stretch it plays in ("c" runs past its piece's end at 2.6 s), none over the next.
        let within = [(0.0, 1.6), (1.6, 2.6), (2.6, 3.6), (3.6, 4.6)];
        for (c, (from, to)) in cues.iter().zip(within) {
            assert!(c.start >= from - 1e-9 && c.end <= to + 1e-9 && c.end > c.start, "{cues:?}");
        }
    }

    #[test]
    fn removing_a_slice_splits_the_pieces_it_reaches() {
        let piece = |id, start, end, struck: &[(f64, f64)]| EditPiece {
            id,
            edit_id: 1,
            vod_id: 1,
            start,
            end,
            title: String::new(),
            struck: struck.to_vec(),
            vod_title: String::new(),
            hole: false,
            group: None,
        };
        // Pieces 10-20 (15-16 struck: plays 0-5 and 5-9 of the timeline),
        // 100-110 (9-19) and 200-210 (19-29).
        let pieces = [piece(1, 10.0, 20.0, &[(15.0, 16.0)]), piece(2, 100.0, 110.0, &[]), piece(3, 200.0, 210.0, &[])];
        assert_eq!(
            cutting_out(&pieces, 4.0, 11.0),
            [(1, Cutting::Keep(10.0, 14.0)), (2, Cutting::Keep(102.0, 110.0))],
            "the end of one and the start of the next"
        );
        assert_eq!(
            cutting_out(&pieces, 21.0, 24.0),
            [(3, Cutting::Split { first_end: 202.0, second_start: 205.0 })],
            "the middle of one: split"
        );
        assert_eq!(cutting_out(&pieces, 8.98, 19.01), [(2, Cutting::Whole)], "all of one, give or take a sliver");
        assert_eq!(
            cutting_out(&pieces, 5.0, 7.0),
            [(1, Cutting::Split { first_end: 15.0, second_start: 18.0 })],
            "across a struck stretch: what played before it, and from 2 s after it"
        );
        assert_eq!(cutting_out(&pieces, 29.0, 40.0), [], "past the end");
    }

    pub(crate) fn info(id: &str) -> VodInfo {
        VodInfo {
            platform: "youtube".into(),
            platform_id: id.into(),
            url: format!("https://youtu.be/{id}"),
            title: format!("VOD {id}"),
            channel: None,
            duration: Some(600.0),
            thumbnail: None,
            published: None,
            started_at: None,
        }
    }

    #[test]
    fn a_vod_has_its_own_clip_over_all_of_it() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let clip = db.vod_clip(vod).unwrap().unwrap();
        let pieces = db.piece_rows(clip).unwrap();
        assert_eq!((pieces.len(), pieces[0].vod_id, pieces[0].start, pieces[0].end), (1, vod, Flicks::ZERO, None));
        assert_eq!(db.edit_pieces(clip).unwrap()[0].end, 600.0, "to the VOD's end");
        assert!(db.edits().unwrap().is_empty(), "not an edit");
        assert!(!db.delete_edit(clip).unwrap(), "nor deleted as one");
        assert_eq!(db.vod_clip(vod).unwrap(), Some(clip));
        assert_eq!(db.library_order().unwrap(), [LibraryItem::Vod(vod)]);
        // Its frame rate, once read.
        assert_eq!(db.rate(vod).unwrap(), None);
        db.set_rate(vod, Some(Rate::new(30000, 1001))).unwrap();
        assert_eq!(db.rate(vod).unwrap(), Some(Rate::new(30000, 1001)));
        db.delete_vod(vod).unwrap();
        assert_eq!(db.vod_clip(vod).unwrap(), None, "goes with the VOD");
    }

    #[test]
    fn every_change_to_a_clip_is_a_revision() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let whole = db.vod_clip(vod).unwrap().unwrap();
        let edit = db.add_edit("e").unwrap().id;
        let rev = |id| db.revision(id).unwrap().unwrap();
        assert_eq!((rev(whole), rev(edit)), (0, 0));

        // Edits: each writer moves the edit on by one.
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        let piece = db.piece_rows(edit).unwrap()[0].id;
        db.set_piece_range(piece, 10.0, 30.0).unwrap();
        db.set_piece_struck(piece, &[(12.0, 13.0)]).unwrap();
        assert!(db.split_piece(piece, 20.0).unwrap());
        db.move_piece(piece, 1).unwrap();
        db.rename_edit(edit, "f").unwrap();
        db.remove_piece(piece).unwrap();
        let state = db.edit_state(edit).unwrap().unwrap();
        db.restore_edit(edit, &state).unwrap();
        assert_eq!(rev(edit), 8);
        assert_eq!(db.edit(edit).unwrap().unwrap().revision, 8);

        // A VOD's own clip: its marks, and its clip bar.
        let mark = db.add_mark(vod, "cut", 5.0, None).unwrap().id;
        assert!(db.move_mark(mark, 6.0).unwrap());
        assert!(db.set_mark_text(mark, Some("t")).unwrap());
        db.delete_mark(mark).unwrap();
        db.restore_marks(vod, &[]).unwrap();
        assert_eq!(rev(whole), 5);
        let snippet = db.add_clip(vod, 1.0, 2.0, "s", true).unwrap().id;
        assert_eq!((rev(whole), rev(snippet)), (6, 0));
        // A snippet in the bar is part of its clip's document as well as its own.
        db.set_clip_range(snippet, 1.0, 3.0).unwrap();
        db.rename_clip(snippet, "t").unwrap();
        assert_eq!((rev(whole), rev(snippet)), (8, 2));
        db.delete_clip(snippet).unwrap();
        assert_eq!(rev(whole), 9);
        // One only dragged out isn't in the bar until it's kept.
        let out = db.add_clip(vod, 1.0, 2.0, "o", false).unwrap().id;
        db.rename_clip(out, "p").unwrap();
        assert_eq!(rev(whole), 9);
        db.show_clip(out).unwrap();
        assert_eq!(rev(whole), 10);

        // What's made of a clip isn't a change to it.
        db.set_edit_render(edit, ClipStatus::Ready, Some("/v/e.mp4"), None, Some(8)).unwrap();
        db.hand_out_edit(edit).unwrap();
        assert_eq!(rev(edit), 8);
        assert!(db.edit(edit).unwrap().unwrap().current);
    }

    #[test]
    fn sources_count_the_pieces_that_use_them() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        db.request(vod, 0.0, None).unwrap();
        let other = db.add_vod(&info("y")).unwrap().0.id;
        assert_eq!(db.uses(vod).unwrap(), 1, "its own clip");
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        let piece = db.piece_rows(edit).unwrap()[0].id;
        assert!(db.split_piece(piece, 15.0).unwrap());
        db.add_clip(vod, 1.0, 2.0, "s", true).unwrap();
        assert_eq!(db.uses(vod).unwrap(), 4);
        assert_eq!(db.piece_ranges().unwrap(), [(vod, 10.0, 15.0), (vod, 15.0, 20.0)], "edits' footage, not snippets'");
        db.remove_piece(piece).unwrap();
        assert_eq!(db.uses(vod).unwrap(), 3);
        let state = db.edit_state(edit).unwrap().unwrap();
        db.restore_edit(edit, &state).unwrap();
        assert_eq!(db.uses(vod).unwrap(), 3, "put back as it was");

        // Out of the list: its own clip and snippet go; the edit still uses it.
        assert!(!db.remove_entry(vod).unwrap());
        assert_eq!(db.uses(vod).unwrap(), 1);
        assert!(db.unused_sources().unwrap().is_empty());
        db.delete_edit(edit).unwrap();
        assert_eq!(db.unused_sources().unwrap(), [vod]);
        assert!(db.remove_entry(other).unwrap(), "used by nothing else");
        assert_eq!(db.unused_sources().unwrap(), [vod, other]);
        // Out of the list, it's a source only: not listed, its work held.
        assert_eq!(db.listed_vods().unwrap().len(), 0);
        assert_eq!(db.vods().unwrap().len(), 2);
        assert!(!db.vod(vod).unwrap().unwrap().listed);
        assert!(db.next_stretch(crate::db::Status::Queued, false).unwrap().is_none());
        // Added again, it's listed again, the same VOD.
        let (again, new) = db.add_vod(&info("x")).unwrap();
        assert_eq!((again.id, new, again.listed), (vod, false, true));
        assert_eq!(db.uses(vod).unwrap(), 1);
        assert_eq!(db.next_stretch(crate::db::Status::Queued, false).unwrap().map(|(v, _)| v.id), Some(vod));
    }

    /// As a VOD removed from the library is erased at once, its row kept
    /// for the session as a gone source (desktop vods.rs, `erase_unused`):
    /// an undo in an edit that puts back a piece of it still finds the row
    /// (a hole), and adding it back adds it anew, not the empty row.
    #[test]
    fn a_vod_erased_after_its_removal_is_added_anew_and_its_pieces_put_back_are_holes() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        let had = db.edit_state(edit).unwrap().unwrap();
        db.remove_piece(db.piece_rows(edit).unwrap()[0].id).unwrap();
        assert!(db.remove_entry(vod).unwrap(), "used by nothing now");
        db.set_gone(vod).unwrap();
        assert_eq!(db.erase_left(vod).unwrap(), 0);
        db.tidy_indexes().unwrap();
        // Undone in the edit: the piece is back, of a gone source.
        db.restore_edit(edit, &had).unwrap();
        assert_eq!(db.uses(vod).unwrap(), 1);
        assert!(db.is_gone(vod).unwrap());
        // Taken out again; then the VOD's link added back: a new VOD, listed,
        // not gone; the old row has gone with what was left of it, and the
        // piece the edit had of it with it.
        db.remove_piece(db.piece_rows(edit).unwrap()[0].id).unwrap();
        let (again, new) = db.add_vod(&info("x")).unwrap();
        assert!(new && again.id != vod && again.listed);
        assert!(!db.is_gone(again.id).unwrap());
        assert!(db.vod(vod).unwrap().is_none());
        // One still used stays a gone source when added back (as before):
        // listed again, its pieces holes.
        let used = db.add_vod(&info("y")).unwrap().0.id;
        db.add_piece(edit, used, (1.0, 2.0), "b", None).unwrap();
        assert!(!db.remove_entry(used).unwrap());
        db.set_gone(used).unwrap();
        let (back, new) = db.add_vod(&info("y")).unwrap();
        assert_eq!((back.id, new, back.listed), (used, false, true));
    }

    #[test]
    fn a_clip_dropped_in_is_a_group_of_its_own() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let snippet = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        let group = db.add_group(edit, snippet, Some(0)).unwrap();
        let pieces = db.edit_pieces(edit).unwrap();
        let shape = |pieces: &[EditPiece]| {
            pieces.iter().map(|p| (p.start, p.end, p.group.as_ref().map(|g| g.title.clone()))).collect::<Vec<_>>()
        };
        assert_eq!(shape(&pieces), [(1.0, 2.0, Some("Hello".into())), (10.0, 20.0, None)]);
        assert_eq!(pieces[0].group.as_ref().map(|g| g.id), Some(group));
        // A copy: the snippet changing changes nothing in the edit.
        db.set_clip_range(snippet, 1.0, 5.0).unwrap();
        assert_eq!(db.edit_pieces(edit).unwrap()[0].end, 2.0);
        assert_eq!(db.uses(vod).unwrap(), 4, "its clip, the snippet, the piece, the group's piece");

        // An edit dropped into another: groups within groups, all in order.
        let outer = db.add_edit("outer").unwrap().id;
        db.add_group(outer, edit, None).unwrap();
        assert_eq!(
            shape(&db.edit_pieces(outer).unwrap()),
            [(1.0, 2.0, Some("e".into())), (10.0, 20.0, Some("e".into()))]
        );
        assert!(db.add_group(outer, outer, None).is_err(), "not into itself");

        // Split inside a group: both halves in it.
        let inner = db.edit_pieces(edit).unwrap()[0].id;
        assert!(db.split_piece(inner, 1.5).unwrap());
        assert_eq!(
            shape(&db.edit_pieces(edit).unwrap()),
            [(1.0, 1.5, Some("Hello".into())), (1.5, 2.0, Some("Hello".into())), (10.0, 20.0, None)]
        );
        // Moved as one; undone as one; removed as one.
        let state = db.edit_state(edit).unwrap().unwrap();
        db.move_piece(group, 1).unwrap();
        assert_eq!(shape(&db.edit_pieces(edit).unwrap())[0], (10.0, 20.0, None));
        db.restore_edit(edit, &state).unwrap();
        assert_eq!(db.edit_state(edit).unwrap().unwrap(), state, "groups and all put back");
        let group = db.edit_pieces(edit).unwrap()[0].group.clone().unwrap().id;
        db.remove_piece(group).unwrap();
        assert_eq!(shape(&db.edit_pieces(edit).unwrap()), [(10.0, 20.0, None)]);
    }

    #[test]
    fn undo_keeps_where_a_realized_piece_came_from() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        let piece = db.piece_rows(edit).unwrap()[0].id;
        let new = db.add_realized(vod, 10.0, 20.0).unwrap();
        assert!(db.realize_piece(piece, new, db.revision(edit).unwrap().unwrap()).unwrap());
        let state = db.edit_state(edit).unwrap().unwrap();
        db.rename_edit(edit, "f").unwrap();
        db.restore_edit(edit, &state).unwrap();
        assert_eq!(db.piece_rows(edit).unwrap()[0].origin, Some(Flicks::from_seconds(10.0)));
    }

    #[test]
    fn an_edits_marks_show_where_their_moments_play() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        // 10-20 s with 12-14 struck (plays 0-2, 2-8), then 30-35 s (8-13).
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
        let first = db.piece_rows(edit).unwrap()[0].id;
        db.set_piece_struck(first, &[(12.0, 14.0)]).unwrap();
        let pieces = db.edit_pieces(edit).unwrap();
        let mark = |kind: &str, at: f64| db.add_clip_mark(edit, vod, kind, at, None).unwrap();
        let shown = mark("highlight", 15.0);
        let hidden = mark("highlight", 13.0);
        let kept = mark("chapter", 13.0);
        let later = mark("chapter", 31.0);
        let outside = mark("chapter", 50.0);
        let placed = place_marks(&pieces, &db.clip_marks(edit).unwrap());
        let at = |id| placed.iter().find(|m| m.id == id).map(|m| m.start);
        assert_eq!(at(shown), Some(3.0));
        assert_eq!(at(hidden), None, "a highlight whose moment is struck hides");
        assert_eq!(at(kept), Some(2.0), "a chapter whose start is struck starts where its piece plays again");
        assert_eq!(at(later), Some(9.0));
        assert_eq!(at(outside), None, "a moment the edit doesn't play");
        assert_eq!(source_at(&pieces, 3.0), Some((vod, 15.0)));
        assert_eq!(source_at(&pieces, 9.0), Some((vod, 31.0)));
        assert_eq!(source_at(&pieces, 13.0), Some((vod, 34.999)), "the very end: just before it");
        // A mark there shows, at the end.
        let end = db.add_clip_mark(edit, vod, "highlight", 34.999, None).unwrap();
        let placed = place_marks(&pieces, &db.clip_marks(edit).unwrap());
        assert!(placed.iter().any(|m| m.id == end && (m.start - 12.999).abs() < 1e-9));
        assert_eq!(source_at(&pieces, 14.0), None);

        // Undo puts them back as they were: the edit's state holds them.
        let state = db.edit_state(edit).unwrap().unwrap();
        assert!(db.move_mark_to(shown, vod, 16.0).unwrap());
        db.delete_mark(later).unwrap();
        db.restore_edit(edit, &state).unwrap();
        let back: Vec<f64> = db.clip_marks(edit).unwrap().iter().map(|(_, m)| m.at.seconds()).collect();
        assert_eq!(back, [13.0, 13.0, 15.0, 31.0, 34.999, 50.0]);
    }

    #[test]
    fn removing_a_stretch_between_split_marks_makes_them_a_real_split() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        let mark = |kind: &str, at: f64| db.add_clip_mark(edit, vod, kind, at, None).unwrap();
        let (left, right, after) = (mark("cut", 13.0), mark("cut", 16.0), mark("cut", 18.0));
        let star = mark("highlight", 14.0);
        let placed = || place_marks(&db.edit_pieces(edit).unwrap(), &db.clip_marks(edit).unwrap());
        assert_eq!(placed().iter().map(|m| m.start).collect::<Vec<_>>(), [3.0, 4.0, 6.0, 8.0]);

        // 3-6 s out (from mark to mark, its ends a hair off the frames).
        db.remove_edit_stretch(edit, 3.01, 5.99).unwrap();
        let ids: Vec<i64> = db.clip_marks(edit).unwrap().iter().map(|(id, _)| *id).collect();
        assert!(!ids.contains(&left) && !ids.contains(&right), "the marks at its ends are a real split now");
        assert!(ids.contains(&star), "a highlight in it stays, hidden while its moment is struck");
        let now = placed();
        assert_eq!(now.len(), 1);
        assert!(now[0].id == after && (now[0].start - 5.02).abs() < 1e-6, "18 s, 2.98 s sooner");
        // Two pieces now, nothing struck: 10-13.01 s and 15.99-20 s.
        let now: Vec<(f64, f64, usize)> =
            db.edit_pieces(edit).unwrap().iter().map(|p| (p.start, p.end, p.struck.len())).collect();
        assert_eq!(now.len(), 2);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(near(now[0].0, 10.0) && near(now[0].1, 13.01) && near(now[1].0, 15.99) && near(now[1].1, 20.0));
        assert!(now.iter().all(|p| p.2 == 0), "nothing struck");
    }

    #[test]
    fn removing_all_of_a_groups_pieces_takes_the_group_too() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let clip = db.add_edit("clip").unwrap().id;
        db.add_piece(clip, vod, (10.0, 14.0), "a", None).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        db.add_group(edit, clip, None).unwrap();
        db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
        // The group's split within, then all of it (0-4 s) taken out.
        db.remove_edit_stretch(edit, 1.0, 2.0).unwrap();
        assert_eq!(db.edit_pieces(edit).unwrap().len(), 3);
        db.remove_edit_stretch(edit, 0.0, 3.0).unwrap();
        let left: Vec<(f64, f64)> = db.edit_pieces(edit).unwrap().iter().map(|p| (p.start, p.end)).collect();
        assert_eq!(left, [(30.0, 35.0)]);
        let groups: i64 = db
            .conn
            .query_row("SELECT count(*) FROM items WHERE clip_id = ?1 AND kind = 'group'", [edit], |r| r.get(0))
            .unwrap();
        assert_eq!(groups, 0, "the emptied group goes");
    }

    #[test]
    fn a_rendered_file_is_out_of_date_only_when_what_it_holds_changes() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        let made = || {
            let e = db.edit(edit).unwrap().unwrap();
            db.made_of(edit, &e.title, &db.edit_pieces(edit).unwrap()).unwrap()
        };
        db.set_edit_render(edit, ClipStatus::Ready, Some("/x.mp4"), None, db.revision(edit).unwrap()).unwrap();
        db.set_made_of(edit, &made()).unwrap();
        let current = || db.edit(edit).unwrap().unwrap().current;
        assert!(current());
        db.add_clip_mark(edit, vod, "highlight", 12.0, None).unwrap();
        db.add_clip_mark(edit, vod, "cut", 13.0, None).unwrap();
        assert!(current(), "a highlight and a cut leave the file as it is");
        let chapter = db.add_clip_mark(edit, vod, "chapter", 14.0, Some("Start")).unwrap();
        assert!(!current(), "a chapter is in the file");
        assert_eq!(db.file_chapters(edit, &db.edit_pieces(edit).unwrap()).unwrap(), [(4.0, "Start".to_string())]);
        db.delete_mark(chapter).unwrap();
        assert!(current(), "and back as it was");
        db.remove_edit_stretch(edit, 1.0, 2.0).unwrap();
        assert!(!current(), "what plays changed");
    }

    #[test]
    fn a_clip_cut_from_its_vod_is_out_of_date_only_when_what_plays_changes() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let clip = db.add_clip(vod, 10.0, 20.0, "c", true).unwrap().id;
        db.set_edit_render(clip, ClipStatus::Ready, Some("/c.mp4"), None, None).unwrap();
        db.set_made_now(clip).unwrap();
        let current = || db.edit(clip).unwrap().unwrap().current;
        assert!(current());
        db.add_clip_mark(clip, vod, "chapter", 14.0, Some("Start")).unwrap();
        db.add_clip_mark(clip, vod, "highlight", 15.0, None).unwrap();
        assert!(current(), "a cut file has no chapters");
        // In two pieces it's rendered as an edit, with its chapters.
        let piece = db.piece_rows(clip).unwrap()[0].id;
        assert!(db.split_piece(piece, 15.0).unwrap());
        assert!(!current(), "rendered now");
        db.set_made_now(clip).unwrap();
        assert!(current());
        db.add_clip_mark(clip, vod, "chapter", 17.0, Some("Later")).unwrap();
        assert!(!current(), "a chapter is in a rendered file");
    }

    #[test]
    fn a_clip_on_any_clip_bar_is_a_snippet_not_an_edit() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 10.0), "a", None).unwrap();
        let of_vod = db.add_clip(vod, 1.0, 2.0, "v", true).unwrap().id;
        let of_edit = db.add_edit_clip(edit, 1.0, 2.0, "e", true).unwrap().id;
        let snippet_of = |id| db.edit(id).unwrap().unwrap().snippet_of;
        assert_eq!(snippet_of(of_vod), Some(crate::db::ClipBar::Vod { id: vod }));
        // Opened in the editor, the page lists what isn't a snippet as an edit.
        assert_eq!(snippet_of(of_edit), Some(crate::db::ClipBar::Edit { id: edit }), "an edit's bar");
        assert_eq!(snippet_of(edit), None, "a listed edit");
        assert_eq!(db.edits().unwrap().iter().map(|e| e.id).collect::<Vec<_>>(), [edit]);
    }

    #[test]
    fn a_stretch_of_an_edit_makes_a_clip_of_what_plays_there() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let other = db.add_vod(&info("y")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        // 10-20 s of x with 12-14 struck (plays 0-2, 2-8), then 30-35 s of y (8-13).
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        db.add_piece(edit, other, (30.0, 35.0), "b", None).unwrap();
        let first = db.piece_rows(edit).unwrap()[0].id;
        db.set_piece_struck(first, &[(12.0, 14.0)]).unwrap();
        db.add_clip_mark(edit, other, "chapter", 31.0, Some("inside")).unwrap();
        db.add_clip_mark(edit, other, "chapter", 34.0, Some("outside")).unwrap();
        let pieces = db.edit_pieces(edit).unwrap();
        assert_eq!(
            stretch_pieces(&pieces, 1.0, 10.0),
            [
                StretchPiece { vod_id: vod, start: 11.0, end: 20.0, struck: vec![(12.0, 14.0)], title: "a".into() },
                StretchPiece { vod_id: other, start: 30.0, end: 32.0, struck: vec![], title: "b".into() },
            ]
        );

        let before = db.revision(edit).unwrap().unwrap();
        let clip = db.add_edit_clip(edit, 1.0, 10.0, "c", true).unwrap();
        assert!(db.revision(edit).unwrap().unwrap() > before, "a change to the edit: its clip bar");
        assert_eq!(
            (clip.bar, clip.vod_id, clip.start, clip.end),
            (Some(crate::db::ClipBar::Edit { id: edit }), vod, 11.0, 20.0)
        );
        assert_eq!(db.bar_clips(edit).unwrap().iter().map(|c| c.id).collect::<Vec<_>>(), [clip.id]);
        assert_eq!(db.edit_pieces(clip.id).unwrap().len(), 2);
        assert_eq!(db.clip(clip.id).unwrap().unwrap().length, 9.0, "7 s of x, 2 of y");
        let marks: Vec<_> = db.clip_marks(clip.id).unwrap().into_iter().map(|(_, m)| m.text).collect();
        assert_eq!(marks, [Some("inside".to_string())], "the edit's marks in the stretch come with it");
        assert!(db.add_edit_clip(edit, 20.0, 30.0, "d", true).is_err(), "past the end: nothing plays");

        // The same stretch again (C pressed twice): the clip made already, no change.
        let revision = db.revision(edit).unwrap().unwrap();
        assert_eq!(db.add_edit_clip(edit, 1.0, 10.0, "c again", true).unwrap().id, clip.id);
        assert_eq!(db.bar_clips(edit).unwrap().len(), 1);
        assert_eq!(db.revision(edit).unwrap().unwrap(), revision, "nothing changed");
        // One only dragged out, then clipped: that one, put on the bar.
        let out = db.add_edit_clip(edit, 0.0, 1.0, "out", false).unwrap();
        assert!(!out.shown);
        let kept = db.add_edit_clip(edit, 0.0, 1.0, "out", true).unwrap();
        assert_eq!((kept.id, kept.shown), (out.id, true));
        assert_eq!(db.bar_clips(edit).unwrap().len(), 2);
        // A stretch on the bar already, put there again: left as it is.
        assert!(db.put(edit, &[Put::Span { clip: edit, from: 1.0, to: 10.0 }], Onto::Bar).unwrap().is_empty());
        assert_eq!(db.bar_clips(edit).unwrap().len(), 2);

        // Made again as a state holds it (undo bringing it back).
        let state = db.edit_state(clip.id).unwrap().unwrap();
        let again = db.add_bar_clip(edit, &state).unwrap();
        assert_eq!(db.edit_state(again).unwrap().unwrap(), state);
    }

    #[test]
    fn several_things_put_in_an_edit_go_in_their_order_as_one_change() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let x = db.add_vod(&info("x")).unwrap().0.id;
        let y = db.add_vod(&info("y")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, x, (0.0, 10.0), "first", None).unwrap();
        db.add_piece(edit, x, (20.0, 30.0), "last", None).unwrap();
        let range = |start, end, title: &str| Put::Range { vod_id: x, start, end, title: Some(title.into()) };
        let shape = |id| {
            db.edit_pieces(id).unwrap().iter().map(|p| (p.vod_id, p.start, p.end, p.title.clone())).collect::<Vec<_>>()
        };

        // Onto the timeline between its two pieces: in the order given.
        let before = db.revision(edit).unwrap().unwrap();
        let made = db.put(edit, &[range(40.0, 41.0, "b"), range(50.0, 52.0, "a")], Onto::Timeline(Some(1))).unwrap();
        assert!(made.is_empty(), "no clips on the timeline");
        assert!(db.revision(edit).unwrap().unwrap() > before, "a change to the edit");
        assert_eq!(
            shape(edit),
            [
                (x, 0.0, 10.0, "first".into()),
                (x, 40.0, 41.0, "b".into()),
                (x, 50.0, 52.0, "a".into()),
                (x, 20.0, 30.0, "last".into())
            ]
        );

        // A whole VOD, and a stretch of the edit's own timeline (what plays
        // from 9 to 11 s: the ends of two pieces), at the end.
        db.put(edit, &[Put::Vod { id: y }, Put::Span { clip: edit, from: 9.0, to: 11.0 }], Onto::Timeline(None))
            .unwrap();
        let now = shape(edit);
        assert_eq!(now[4], (y, 0.0, 600.0, "VOD y".into()));
        assert_eq!(&now[5..], [(x, 9.0, 10.0, "first".into()), (x, 40.0, 41.0, "b".into())]);

        // Onto the clip bar: a clip of each, in order; one of its own stays as it is.
        let own = db.add_edit_clip(edit, 0.0, 5.0, "own", true).unwrap().id;
        let before = db.revision(edit).unwrap().unwrap();
        let made = db
            .put(
                edit,
                &[range(1.0, 2.0, "one"), Put::Clip { id: own }, Put::Span { clip: edit, from: 0.0, to: 2.0 }],
                Onto::Bar,
            )
            .unwrap();
        assert!(db.revision(edit).unwrap().unwrap() > before, "a change to the edit");
        assert_eq!(made.len(), 2);
        let bar: Vec<(i64, f64, f64)> = db.bar_clips(edit).unwrap().iter().map(|c| (c.id, c.start, c.end)).collect();
        assert_eq!(bar, [(own, 0.0, 5.0), (made[0], 1.0, 2.0), (made[1], 0.0, 2.0)]);
        assert_eq!(db.clip(made[0]).unwrap().unwrap().title, "one");

        // A clip of another edit's clip bar: on the timeline a group, on the bar a copy.
        let other = db.add_edit("o").unwrap().id;
        db.put(other, &[Put::Clip { id: own }], Onto::Timeline(None)).unwrap();
        assert_eq!(db.edit_pieces(other).unwrap()[0].group.as_ref().map(|g| g.title.as_str()), Some("own"));
        let copy = db.put(other, &[Put::Clip { id: own }], Onto::Bar).unwrap();
        assert_eq!(db.edit_state(copy[0]).unwrap().unwrap().items, db.edit_state(own).unwrap().unwrap().items);

        // A highlight: on the timeline a group named for it, holding its
        // stretch, the chapter in it coming too; on the bar a clip of it.
        let lit = db.add_edit("h").unwrap().id;
        db.add_mark(x, "chapter", 45.0, Some("inside")).unwrap();
        let highlight = Put::Highlight { vod_id: x, start: 44.0, end: 48.0, title: "Close call".into() };
        db.put(lit, &[range(0.0, 1.0, "before"), highlight.clone()], Onto::Timeline(None)).unwrap();
        db.put(lit, std::slice::from_ref(&highlight), Onto::Timeline(Some(0))).unwrap();
        let got: Vec<_> = db
            .edit_pieces(lit)
            .unwrap()
            .iter()
            .map(|p| (p.start, p.end, p.title.clone(), p.group.as_ref().map(|g| g.title.clone())))
            .collect();
        assert_eq!(
            got,
            [
                (44.0, 48.0, "Close call".into(), Some("Close call".into())),
                (0.0, 1.0, "before".into(), None),
                (44.0, 48.0, "Close call".into(), Some("Close call".into())),
            ]
        );
        let chapters = db.clip_marks(lit).unwrap().into_iter().filter(|(_, m)| m.kind == "chapter").count();
        assert_eq!(chapters, 2, "each group brings the chapter in its stretch");
        let made = db.put(lit, &[highlight], Onto::Bar).unwrap();
        assert_eq!(db.clip(made[0]).unwrap().unwrap().title, "Close call");

        // One that can't go: none of them do.
        let had = shape(edit);
        let gone = Put::Vod { id: 999 };
        assert!(db.put(edit, &[range(1.0, 2.0, "c"), gone], Onto::Timeline(None)).is_err());
        assert_eq!(shape(edit), had, "nothing half done");
    }

    #[test]
    fn clips_stretches_and_groups_bring_their_marks() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        db.add_mark(vod, "chapter", 12.0, Some("in")).unwrap();
        db.add_mark(vod, "highlight", 15.0, None).unwrap();
        db.add_mark(vod, "cut", 13.0, None).unwrap();
        db.add_mark(vod, "chapter", 25.0, Some("out")).unwrap();
        let kinds = |clip| -> Vec<(String, f64)> {
            db.clip_marks(clip).unwrap().into_iter().map(|(_, m)| (m.kind, m.at.seconds())).collect()
        };
        let want = vec![("chapter".to_string(), 12.0), ("highlight".to_string(), 15.0)];
        // A clip of 10-20 s: its chapter and highlight, not the cut or what's outside.
        let clip = db.add_clip(vod, 10.0, 20.0, "c", true).unwrap().id;
        assert_eq!(kinds(clip), want);
        // A stretch put in an edit.
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "s", None).unwrap();
        assert_eq!(kinds(edit), want);
        // A clip put in another as a group: its marks that show in it.
        let other = db.add_edit("f").unwrap().id;
        db.add_group(other, clip, None).unwrap();
        assert_eq!(kinds(other), want);
        let pieces = db.edit_pieces(other).unwrap();
        let placed: Vec<f64> = place_marks(&pieces, &db.clip_marks(other).unwrap()).iter().map(|m| m.start).collect();
        assert_eq!(placed, [2.0, 5.0]);
    }

    #[test]
    fn imported_marks_keep_their_detail_in_an_edit() {
        use crate::db::MarkDetail;
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let own = db.vod_clip(vod).unwrap().unwrap();
        let imported = |end: Option<f64>, category: Option<&str>, icon: Option<&str>| MarkDetail {
            end,
            category: category.map(Into::into),
            note: category.map(|_| "Why it stands out".into()),
            cast: if category.is_some() { vec!["Marlowe".into(), "Ada".into()] } else { Vec::new() },
            icon: icon.map(Into::into),
            source: crate::annotations::SOURCE.into(),
            imported_as: None,
        };
        let funny = imported(Some(14.0), Some("funny"), None);
        let game = imported(None, None, Some("🎮"));
        db.insert_mark_with(own, vod, "highlight", Flicks::from_seconds(10.0), Some("Ha"), &funny).unwrap();
        db.insert_mark_with(own, vod, "chapter", Flicks::from_seconds(5.0), Some("Play"), &game).unwrap();
        // Each mark of clip `id` as its timeline shows it: kind, start, and detail.
        let shown = |id: i64| -> Vec<(String, f64, MarkDetail)> {
            place_marks(&db.edit_pieces(id).unwrap(), &db.clip_marks(id).unwrap())
                .into_iter()
                .map(|m| (m.kind, m.start, m.detail))
                .collect()
        };
        let at = |end: Option<f64>, d: &MarkDetail| MarkDetail { end, ..d.clone() };
        // 4-20 s of the VOD: the chapter at 1 s, the highlight 6-10 s.
        let whole =
            vec![("chapter".to_string(), 1.0, game.clone()), ("highlight".to_string(), 6.0, at(Some(10.0), &funny))];

        // A stretch put on a timeline.
        let edit = db.add_edit("e").unwrap().id;
        db.put(edit, &[Put::Range { vod_id: vod, start: 4.0, end: 20.0, title: None }], Onto::Timeline(None)).unwrap();
        assert_eq!(shown(edit), whole, "put on a timeline");
        // Undo of a change brings them back as they were.
        let state = db.edit_state(edit).unwrap().unwrap();
        db.restore_edit(edit, &state).unwrap();
        assert_eq!(shown(edit), whole, "after undo");
        // A whole VOD put in: each on its own moment.
        let all = db.add_edit("v").unwrap().id;
        db.put(all, &[Put::Vod { id: vod }], Onto::Timeline(None)).unwrap();
        let on_vod = vec![("chapter".to_string(), 5.0, game.clone()), ("highlight".to_string(), 10.0, funny.clone())];
        assert_eq!(shown(all), on_vod, "a whole VOD put");
        // The highlight dragged onto an edit, or Make new edit (or A): it, one band.
        let made = db.add_edit("h").unwrap().id;
        let put = Put::Highlight { vod_id: vod, start: 10.0, end: 14.0, title: "Ha".into() };
        db.put(made, &[put], Onto::Timeline(None)).unwrap();
        assert_eq!(shown(made), [("highlight".to_string(), 0.0, at(Some(4.0), &funny))], "a highlight put");
        // A clip of the VOD's clip bar put in an edit, as a group.
        let clip = db.add_clip(vod, 4.0, 20.0, "c", true).unwrap().id;
        assert_eq!(shown(clip), whole, "a clip of the VOD");
        let grouped = db.add_edit("g").unwrap().id;
        db.put(grouped, &[Put::Clip { id: clip }], Onto::Timeline(None)).unwrap();
        assert_eq!(shown(grouped), whole, "a clip put as a group");
        // A stretch of the edit made a clip on its bar, and a range put on the bar.
        let bar = db.add_edit_clip(edit, 0.0, 16.0, "b", true).unwrap().id;
        assert_eq!(shown(bar), whole, "a stretch of the edit made a clip");
        let on_bar =
            db.put(edit, &[Put::Range { vod_id: vod, start: 4.0, end: 20.0, title: None }], Onto::Bar).unwrap();
        assert_eq!(shown(on_bar[0]), whole, "put on the clip bar");

        // A highlight whose end is cut off ends where its stretch does; one
        // whose middle is struck is shorter by that much.
        let short = db.add_edit("s").unwrap().id;
        db.add_piece(short, vod, (8.0, 12.0), "a", None).unwrap();
        assert_eq!(shown(short), [("highlight".to_string(), 2.0, at(Some(4.0), &funny))], "ends at the piece's end");
        let first = db.piece_rows(short).unwrap()[0].id;
        db.set_piece_range(first, 8.0, 20.0).unwrap();
        db.set_piece_struck(first, &[(11.0, 12.0)]).unwrap();
        assert_eq!(shown(short), [("highlight".to_string(), 2.0, at(Some(5.0), &funny))], "less what's struck");
        db.set_piece_struck(first, &[(13.0, 16.0)]).unwrap();
        assert_eq!(shown(short), [("highlight".to_string(), 2.0, at(Some(5.0), &funny))], "to where it's struck");
    }

    #[test]
    fn a_change_against_an_old_revision_is_refused() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        let rename = |title: &'static str| {
            move |db: &Db| {
                db.conn.execute("UPDATE clips SET title = ?2 WHERE id = ?1", params![edit, title])?;
                Ok(())
            }
        };
        db.change(edit, Some(0), rename("a")).unwrap();
        let e = db.change(edit, Some(0), rename("b")).unwrap_err();
        assert_eq!(e.downcast_ref::<Stale>(), Some(&Stale { clip: edit, base: 0, now: 1 }));
        assert_eq!(db.edit(edit).unwrap().unwrap().title, "a", "nothing written");
        db.change(edit, Some(1), rename("c")).unwrap();
        assert!(db.change(edit + 100, None, rename("d")).is_err(), "a clip that isn't there");
    }

    /// Each command that takes a span checked it its own way: some let one
    /// start before 0, one capped a VOD's clips at 3 hours, and they said so
    /// in different words. Every one now goes by `clips::check_span`.
    #[test]
    fn every_span_is_checked_the_same_way() {
        use crate::clips::{ENDS_BEFORE_START, PAST_THE_END, check_span};
        fn said<T>(r: Result<T>) -> Option<String> {
            r.err().map(|e| format!("{e:#}"))
        }
        let refused = Some(ENDS_BEFORE_START.to_string());
        let past = Some(PAST_THE_END.to_string());

        assert_eq!(check_span(1.0, 2.0, Some(10.0)), Ok(()));
        assert_eq!(check_span(0.0, 4.0 * 3600.0, None), Ok(()), "any length");
        assert_eq!(check_span(2.0, 2.0, None), Err(ENDS_BEFORE_START));
        assert_eq!(check_span(-1.0, 2.0, None), Err(ENDS_BEFORE_START));
        assert_eq!(check_span(f64::NAN, 2.0, None), Err(ENDS_BEFORE_START));
        assert_eq!(check_span(1.0, f64::INFINITY, None), Err(ENDS_BEFORE_START));
        assert_eq!(check_span(10.0, 12.0, Some(10.0)), Err(PAST_THE_END));

        let db = Db::open(Path::new(":memory:")).unwrap();
        let long = db.add_vod(&VodInfo { duration: Some(5.0 * 3600.0), ..info("long") }).unwrap().0.id;
        let vod = db.add_vod(&info("x")).unwrap().0.id;

        // A VOD's clip (`add_clip`, `set_clip_range`): longer than 3 hours too.
        let clip = db.add_clip(long, 0.0, 4.0 * 3600.0, "long", true).unwrap();
        assert_eq!(clip.end, 4.0 * 3600.0);
        assert_eq!(said(db.add_clip(vod, -1.0, 2.0, "c", true)), refused, "a clip from before 0");
        assert_eq!(said(db.add_clip(vod, 600.0, 610.0, "c", true)), past);
        assert_eq!(db.add_clip(vod, 590.0, 700.0, "c", true).unwrap().end, 600.0, "cut at the VOD's end");
        db.set_clip_range(clip.id, 60.0, 4.5 * 3600.0).unwrap();
        assert_eq!(said(db.set_clip_range(clip.id, -1.0, 2.0)), refused, "moved to before 0");
        assert_eq!(said(db.set_clip_range(clip.id, 3.0, 2.0)), refused);
        assert_eq!(said(db.set_clip_range(clip.id, 6.0 * 3600.0, 7.0 * 3600.0)), past);

        // An edit's pieces (`add_piece`, `put`, `set_piece_range`).
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, long, (0.0, 4.0 * 3600.0), "long", None).unwrap();
        assert_eq!(said(db.add_piece(edit, vod, (-1.0, 2.0), "a", None)), refused, "a piece from before 0");
        assert_eq!(said(db.add_piece(edit, vod, (f64::NAN, 2.0), "a", None)), refused);
        let range = |start, end| Put::Range { vod_id: vod, start, end, title: None };
        let highlight = |start, end| Put::Highlight { vod_id: vod, start, end, title: "h".into() };
        for onto in [Onto::Timeline(None), Onto::Bar] {
            assert_eq!(said(db.put(edit, &[range(-1.0, 2.0)], onto)), refused, "{onto:?}");
            assert_eq!(said(db.put(edit, &[highlight(-1.0, 2.0)], onto)), refused, "{onto:?}");
            assert_eq!(said(db.put(edit, &[range(700.0, 710.0)], onto)), past, "{onto:?}");
            let span = Put::Span { clip: edit, from: -1.0, to: 2.0 };
            assert_eq!(said(db.put(edit, &[span], onto)), refused, "{onto:?}");
        }
        let piece = db.piece_rows(edit).unwrap()[0].id;
        assert_eq!(said(db.set_piece_range(piece, -1.0, 2.0)), refused, "trimmed to before 0");
        assert_eq!(said(db.set_piece_range(piece, 5.0, 4.0)), refused);
        db.set_piece_range(piece, 0.0, 4.5 * 3600.0).unwrap();

        // Spans of the edit's timeline (4.5 hours long now): a clip of one
        // (`add_edit_clip`, its title `span_title`), taking one out
        // (`remove_edit_stretch`, `remove_from_edit`).
        let bar = db.add_edit_clip(edit, 0.0, 4.0 * 3600.0, "long", true).unwrap();
        assert_eq!(db.clip(bar.id).unwrap().unwrap().length, 4.0 * 3600.0, "longer than 3 hours");
        assert_eq!(said(db.span_title(edit, -1.0, 2.0)), refused);
        assert_eq!(said(db.add_edit_clip(edit, -1.0, 2.0, "c", true)), refused, "an edit's clip from before 0");
        assert_eq!(said(db.add_edit_clip(edit, 5.0 * 3600.0, 5.5 * 3600.0, "c", true)), past);
        assert_eq!(said(db.remove_edit_stretch(edit, -1.0, 2.0)), refused, "taken out from before 0");
        assert_eq!(said(db.remove_edit_stretch(edit, 2.0, f64::NAN)), refused);
        assert_eq!(said(db.remove_from_edit(edit, &[(-1.0, 2.0)], &[])), refused);
        assert_eq!(said(db.remove_from_edit(edit, &[(5.0 * 3600.0, 5.5 * 3600.0)], &[])), past);
        assert_eq!(db.edit_pieces(edit).unwrap()[0].end, 4.5 * 3600.0, "nothing taken out");
    }

    #[test]
    fn pieces_stay_within_their_vod() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap();
        assert!(db.add_piece(edit.id, vod, (700.0, 800.0), "past", None).is_err(), "all past the end (600 s)");
        db.add_piece(edit.id, vod, (590.0, 650.0), "a", None).unwrap();
        let piece = &db.edit_pieces(edit.id).unwrap()[0];
        assert_eq!((piece.start, piece.end), (590.0, 600.0), "cut at the end");
        assert!(db.set_piece_range(piece.id, 700.0, 800.0).is_err(), "trimmed past the end");
        let piece = &db.edit_pieces(edit.id).unwrap()[0];
        assert_eq!((piece.start, piece.end), (590.0, 600.0), "left as it was");
    }

    #[test]
    fn edits_hold_pieces_of_vods_in_order() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("Race night").unwrap();
        assert_eq!((edit.pieces, edit.length, edit.current), (0, 0.0, false));
        db.add_piece(edit.id, vod, (10.0, 20.0), "a", None).unwrap();
        db.add_piece(edit.id, vod, (30.0, 35.0), "b", Some(0)).unwrap();
        let titles = |db: &Db| db.edit_pieces(edit.id).unwrap().iter().map(|p| p.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles(&db), ["b", "a"]);
        assert_eq!(db.edit(edit.id).unwrap().unwrap().vod_ids, [vod]);
        let pieces = db.edit_pieces(edit.id).unwrap();
        db.move_piece(pieces[0].id, 1).unwrap();
        assert_eq!(titles(&db), ["a", "b"]);
        // Grown or trimmed within the VOD, and a line struck.
        let a_piece = db.edit_pieces(edit.id).unwrap()[0].id;
        assert!(db.set_piece_range(a_piece, -5.0, 18.0).is_err(), "not before the VOD");
        db.set_piece_range(a_piece, 0.0, 18.0).unwrap();
        db.set_piece_struck(a_piece, &[(12.0, 14.0)]).unwrap();
        let p = &db.edit_pieces(edit.id).unwrap()[0];
        assert_eq!((p.start, p.end), (0.0, 18.0));
        db.set_piece_range(a_piece, 10.0, 18.0).unwrap();
        let p = &db.edit_pieces(edit.id).unwrap()[0];
        assert_eq!(p.spans(), [(10.0, 12.0), (14.0, 18.0)]);
        let ranges = db.edit(edit.id).unwrap().unwrap().ranges;
        assert_eq!(
            ranges.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>(),
            [(10.0, 12.0), (14.0, 18.0), (30.0, 35.0)]
        );
        assert_eq!(db.edit(edit.id).unwrap().unwrap().length, 6.0 + 5.0);
        // Kept in flicks: what went in comes out, to the flick.
        let row = db.piece_row(a_piece).unwrap().unwrap();
        assert_eq!((row.start, row.end), (Flicks::from_seconds(10.0), Some(Flicks::from_seconds(18.0))));
        assert_eq!(row.struck, [(Flicks::from_seconds(12.0), Flicks::from_seconds(14.0))]);
        // Split in two where it's asked, the second right after the first.
        let before = db.edit_pieces(edit.id).unwrap();
        assert!(db.split_piece(before[0].id, 16.0).unwrap());
        assert!(!db.split_piece(before[0].id, 3.0).unwrap(), "not inside it");
        let split = db.edit_pieces(edit.id).unwrap();
        assert_eq!(split.len(), 3);
        assert_eq!(((split[0].start, split[0].end), (split[1].start, split[1].end)), ((10.0, 16.0), (16.0, 18.0)));
        assert_eq!(split[1].struck, [(12.0, 14.0)]);
        assert_eq!(split[1].title, "a");
        // Pieces are their own: changing one leaves the others be.
        db.set_piece_range(split[1].id, 16.0, 40.0).unwrap();
        let now = db.edit_pieces(edit.id).unwrap();
        assert_eq!(((now[0].start, now[0].end), (now[1].start, now[1].end)), ((10.0, 16.0), (16.0, 40.0)));
        // Rendered files say whether they're of the edit as it is.
        let version = db.revision(edit.id).unwrap();
        db.set_edit_render(edit.id, ClipStatus::Ready, Some("/v/e.mp4"), None, version).unwrap();
        assert!(db.edit(edit.id).unwrap().unwrap().current);
        db.remove_piece(split[1].id).unwrap();
        let after = db.edit(edit.id).unwrap().unwrap();
        assert!(!after.current);
        assert_eq!(after.file_path.as_deref(), Some("/v/e.mp4"));
        // Deleting the VOD takes its pieces with it.
        db.delete_vod(vod).unwrap();
        assert_eq!(db.edit(edit.id).unwrap().unwrap().pieces, 0);
        db.delete_edit(edit.id).unwrap();
        assert!(db.edits().unwrap().is_empty());
    }

    #[test]
    fn vods_and_edits_share_one_order() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let a = db.add_vod(&info("a")).unwrap().0.id;
        let e = db.add_edit("e").unwrap().id;
        let b = db.add_vod(&info("b")).unwrap().0.id;
        use LibraryItem::{Edit, Vod};
        assert_eq!(db.library_order().unwrap(), [Vod(b), Edit(e), Vod(a)], "newest on top");
        db.reorder_library(&[Vod(a), Vod(b), Edit(e)]).unwrap();
        assert_eq!(db.library_order().unwrap(), [Vod(a), Vod(b), Edit(e)]);
        assert_eq!(db.vods().unwrap().iter().map(|v| v.id).collect::<Vec<_>>(), [a, b]);
        assert_eq!(serde_json::to_string(&Edit(e)).unwrap(), format!(r#"{{"kind":"edit","id":{e}}}"#));
    }

    /// Several things taken out of an edit at once (remove_from_edit) took
    /// any piece named, of any edit: one of another edit's went from it
    /// with no undo there, recorded in this edit's history instead.
    #[test]
    fn taking_pieces_out_of_an_edit_leaves_other_edits_alone() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let (mine, other) = (db.add_edit("mine").unwrap().id, db.add_edit("other").unwrap().id);
        for edit in [mine, other] {
            db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
            db.add_piece(edit, vod, (30.0, 40.0), "b", None).unwrap();
        }
        let ids = |edit| db.edit_pieces(edit).unwrap().iter().map(|p| p.id).collect::<Vec<_>>();
        let (a, theirs) = (ids(mine)[0], ids(other)[0]);
        db.remove_from_edit(mine, &[(18.0, 20.0)], &[a, theirs, 9999]).unwrap();
        assert_eq!(ids(other).len(), 2, "another edit's piece stays");
        let left = db.edit_pieces(mine).unwrap();
        assert_eq!(left.iter().map(|p| (p.start, p.end)).collect::<Vec<_>>(), [(30.0, 38.0)]);
    }

    #[test]
    fn an_edit_is_put_back_as_it_was() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        db.add_piece(edit, vod, (30.0, 34.0), "b", None).unwrap();
        let a = db.edit_pieces(edit).unwrap()[0].id;
        db.set_piece_struck(a, &[(12.0, 13.0)]).unwrap();
        let before = db.edit_state(edit).unwrap().unwrap();

        db.remove_piece(a).unwrap();
        db.rename_edit(edit, "renamed").unwrap();
        db.add_piece(edit, vod, (1.0, 2.0), "c", Some(0)).unwrap();
        assert_ne!(db.edit_state(edit).unwrap().unwrap(), before);

        db.restore_edit(edit, &before).unwrap();
        assert_eq!(db.edit_state(edit).unwrap().unwrap(), before, "title, pieces, order and strikes");
        assert_eq!(db.edit_state(9999).unwrap(), None);
    }

    #[test]
    fn renders_need_the_footage() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap();
        db.add_piece(edit.id, vod, (10.0, 20.0), "a", None).unwrap();
        let none = |_: i64, _: f64, _: f64| vec![];
        let err = render_pieces(&db.edit_pieces(edit.id).unwrap(), none).unwrap_err();
        assert!(err.to_string().contains("isn't here yet"), "{err}");
        let piece = db.edit_pieces(edit.id).unwrap()[0].id;
        db.set_piece_struck(piece, &[(12.0, 13.0)]).unwrap();
        // Footage of 5 s to 30 s of the VOD: its time 0 is the VOD's 5 s.
        let file = |path: &str, offset: f64| Source { path: path.into(), offset, section: None };
        let found = |_: i64, a: f64, b: f64| vec![(a, b, file("/f/x.mp4", 5.0))];
        let pieces = render_pieces(&db.edit_pieces(edit.id).unwrap(), found).unwrap();
        assert_eq!(
            pieces,
            [
                Piece { source: Some("/f/x.mp4".into()), start: 5.0, end: 7.0 },
                Piece { source: Some("/f/x.mp4".into()), start: 8.0, end: 15.0 },
            ]
        );
        // Two sections, and nothing from 17 s: a hole's piece plays what's
        // kept, black and silence for the rest; any other waits for it.
        let partly = |_: i64, a: f64, b: f64| {
            vec![
                (a, 15.0_f64.min(b), file("/f/s1.mkv", 0.0)),
                (15.0_f64.max(a), 17.0_f64.min(b), file("/f/s2.mkv", 14.0)),
            ]
        };
        assert!(render_pieces(&db.edit_pieces(edit.id).unwrap(), partly).is_err());
        let mut gone = db.edit_pieces(edit.id).unwrap();
        gone[0].hole = true;
        assert_eq!(
            render_pieces(&gone, partly).unwrap(),
            [
                Piece { source: Some("/f/s1.mkv".into()), start: 10.0, end: 12.0 },
                Piece { source: Some("/f/s1.mkv".into()), start: 13.0, end: 15.0 },
                Piece { source: Some("/f/s2.mkv".into()), start: 1.0, end: 3.0 },
                Piece { source: None, start: 0.0, end: 3.0 },
            ]
        );
        assert!(covers(&partly(1, 10.0, 17.0), 10.0, 17.0) && !covers(&partly(1, 10.0, 20.0), 10.0, 20.0));
    }
}
