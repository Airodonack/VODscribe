//! Clips as the record keeps them (schema v22): a clip is a document, with
//! a title, a revision and a tree of items, each a piece (a stretch of one
//! VOD) or a group (a whole clip placed inside another). A piece is its own:
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

use crate::db::{CLIP_COLUMNS, CLIP_JOIN, ClipStatus, Db, MARK_DETAIL_COLUMNS, MarkDetail, mark_detail, now};
use crate::footage::Source;
use crate::render::Piece;
use crate::settings::ClipEncoding;
use crate::time::Flicks;
use crate::transcript::{Cue, Segment, Word};

/// Stretches shorter than this are left out (a sliver).
pub(crate) const SLIVER: f64 = 0.04;
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
    /// Whether the rendered file is of the edit as it is now, made as clips
    /// are made now (`clips::made_as`).
    pub current: bool,
    /// What the rendered file was made with; None if it isn't rendered, or
    /// was before that was recorded (`Clip::encoding`).
    pub encoding: Option<ClipEncoding>,
    /// How many pieces it has, and how long it plays, in seconds.
    pub pieces: usize,
    pub length: f64,
    /// The VODs its pieces are of.
    pub vod_ids: Vec<i64>,
    /// What it plays of each VOD, in order: its pieces.
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
    pub origin: Option<Flicks>,
}

/// The group an edit's piece is in at the edit's top level (`EditPiece::group`).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct GroupOf {
    pub id: i64,
    pub title: String,
}

/// A piece of a clip as the record holds it: `start`..`end` of VOD `vod_id`
/// (`end` None: to the VOD's end).
#[derive(Debug, Clone, PartialEq)]
pub struct PieceRow {
    pub id: i64,
    pub clip_id: i64,
    pub vod_id: i64,
    pub start: Flicks,
    pub end: Option<Flicks>,
    pub title: String,
    /// A realized piece: the VOD's time its file starts at.
    pub origin: Option<Flicks>,
}

/// A piece of an edit, in seconds for the interface: `start`..`end` of VOD
/// `vod_id`.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EditPiece {
    pub id: i64,
    pub edit_id: i64,
    pub vod_id: i64,
    pub start: f64,
    pub end: f64,
    pub title: String,
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
    /// What plays of it, in its VOD's time: `start`..`end` (`spans`).
    pub fn spans(&self) -> Vec<(f64, f64)> {
        spans(self.start, self.end)
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

/// What a piece of `start`..`end` plays: all of it, or nothing if it's a sliver.
pub fn spans(start: f64, end: f64) -> Vec<(f64, f64)> {
    if end - start >= SLIVER { vec![(start, end)] } else { Vec::new() }
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
/// after it (a chapter keeps its start in what's left). A chapter whose
/// moment no piece plays, nor plays again after it within its piece (taken
/// out with Remove, or trimmed off), starts where
/// the next piece of its VOD begins: the first, in the edit's order, that
/// plays after its moment; one past them all shows nowhere. A moment played
/// more than once shows where it's first played.
pub fn place_marks(pieces: &[EditPiece], marks: &[(i64, MarkAt)]) -> Vec<crate::db::Mark> {
    let mut out: Vec<crate::db::Mark> = Vec::new();
    let place = |id: i64, m: &MarkAt, start: f64, spans: &[(f64, f64)], t: f64| {
        let end = m.detail.end.map(|end| placed_end(spans, t, end)).filter(|&end| end > start);
        crate::db::Mark {
            id,
            vod_id: m.vod_id,
            kind: m.kind.clone(),
            start,
            text: m.text.clone(),
            detail: MarkDetail { end, ..m.detail.clone() },
        }
    };
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
                out.push(place(*id, m, start, &spans[i..], t));
            }
        }
        at += spans.iter().map(|(a, b)| b - a).sum::<f64>();
    }
    // The chapters not placed yet go where the next piece begins. This walk
    // comes after the first, so a later piece that plays the moment itself
    // wins over an earlier one that only begins after it.
    let mut at = 0.0;
    for p in pieces {
        let spans = p.spans();
        for (id, m) in marks.iter().filter(|(_, m)| m.kind == "chapter" && m.vod_id == p.vod_id) {
            if out.iter().any(|o| o.id == *id) {
                continue;
            }
            let moment = m.at.seconds();
            let mut t = at;
            for (i, &(a, b)) in spans.iter().enumerate() {
                if a > moment {
                    out.push(place(*id, m, t, &spans[i..], t));
                    break;
                }
                t += b - a;
            }
        }
        at += spans.iter().map(|(a, b)| b - a).sum::<f64>();
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.id.cmp(&b.id)));
    out
}

/// Where a mark's `end` (seconds of its VOD) is on a clip's timeline, its
/// start in `spans` (what plays of its piece from there on; the first plays
/// from `t`): what plays of it up to there. One that ends where its piece
/// doesn't play (past the piece) ends where its piece last plays before
/// then: a highlight runs as long as it's in the clip.
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
/// `vod_id`, titled `title`.
#[derive(Debug, Clone, PartialEq)]
pub struct StretchPiece {
    pub vod_id: i64,
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// Whether a clip bar's clip of `pieces` is cut straight from its VOD (one
/// piece, not a group) rather than rendered as an edit is.
pub fn cut_whole(pieces: &[EditPiece]) -> bool {
    matches!(pieces, [p] if p.group.is_none())
}

/// What `from`..`to` of a clip of `pieces`' timeline plays, as pieces of
/// their own: each piece it reaches, trimmed to it.
pub fn stretch_pieces(pieces: &[EditPiece], from: f64, to: f64) -> Vec<StretchPiece> {
    let mut out = Vec::new();
    for span in played(pieces) {
        let (s, e) = (from.max(span.at), to.min(span.end()));
        if e - s >= SLIVER {
            let (start, end) = (span.from + s - span.at, span.from + e - span.at);
            out.push(StretchPiece { vod_id: span.piece.vod_id, start, end, title: span.piece.title.clone() });
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
/// picked: the pieces split at its ends and the middle goes. Within a
/// sliver of a piece's edge counts as at it.
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

pub(crate) const PIECE_COLUMNS: &str = "id, clip_id, vod_id, start, end, title, origin";

pub(crate) fn piece_from_row(r: &Row) -> rusqlite::Result<PieceRow> {
    Ok(PieceRow {
        id: r.get("id")?,
        clip_id: r.get("clip_id")?,
        vod_id: r.get("vod_id")?,
        start: Flicks(r.get("start")?),
        end: r.get::<_, Option<i64>>("end")?.map(Flicks),
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

    /// The sources removed with their videos that clips still use: out of
    /// the list and gone (`set_gone`), their pieces holes. Their erasure may
    /// not have finished, the app quitting first.
    pub fn gone_sources_used(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM vods WHERE gone = 1 AND pieces > 0
             AND NOT EXISTS (SELECT 1 FROM clips WHERE clips.whole_of = vods.id) ORDER BY id",
        )?;
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

    /// A new, empty clip titled `title`, where `place` says; its id, one
    /// never given before (`db::MIGRATE_V42`). Every clip, edit and VOD's
    /// own clip is made here.
    pub(crate) fn new_clip(&self, title: &str, place: Place) -> Result<i64> {
        self.new_clip_as(title, place, None)
    }

    /// `new_clip`, under id `old` if that's given and no clip has it: a
    /// deleted clip brought back by undo keeps its id, so what still holds
    /// it (the page, the clip's history) finds it. No other clip can have
    /// been given it (`db::MIGRATE_V42`), and the counter stays past it.
    fn new_clip_as(&self, title: &str, place: Place, old: Option<i64>) -> Result<i64> {
        let free = |id: i64| -> Result<bool> {
            Ok(self.conn.query_row("SELECT NOT EXISTS (SELECT 1 FROM clips WHERE id = ?1)", [id], |r| r.get(0))?)
        };
        let id: i64 = match old {
            Some(id) if free(id)? => id,
            _ => self.conn.query_row(
                "SELECT max(next, (SELECT coalesce(max(id), 0) + 1 FROM clips)) FROM clip_ids",
                [],
                |r| r.get(0),
            )?,
        };
        match place {
            Place::Listed => self.conn.execute(
                "INSERT INTO clips (id, title, created_at, position)
                 VALUES (?1, ?2, ?3, (SELECT coalesce(min(position), 0) - 1 FROM clips))",
                params![id, title, now()],
            )?,
            Place::Bar(bar_of) => self.conn.execute(
                "INSERT INTO clips (id, title, created_at, bar_of, bar_position)
                 VALUES (?1, ?2, ?3, ?4, (SELECT coalesce(max(bar_position), 0) + 1 FROM clips WHERE bar_of = ?4))",
                params![id, title, now(), bar_of],
            )?,
        };
        // A clip brought back goes on from the last make it had (`GONE_MAKES`).
        self.conn.execute(
            "INSERT INTO renders (clip_id, make) VALUES (?1, (SELECT make FROM temp.gone_makes WHERE id = ?1))",
            [id],
        )?;
        Ok(id)
    }

    /// Every edit (listed clips other than VODs' own), in their order, each
    /// `current` as `edit` says with clips made with `now`.
    pub fn edits(&self, now: ClipEncoding) -> Result<Vec<Edit>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM clips WHERE position IS NOT NULL AND whole_of IS NULL ORDER BY position, id")?;
        let ids: Vec<i64> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.into_iter().filter_map(|id| self.edit(id, now).transpose()).collect()
    }

    /// Edit `id`; `current` if its file is of it as it is and made as one is
    /// made with `now` (Settings' `clip_encoding`, the caller's to read).
    pub fn edit(&self, id: i64, now: ClipEncoding) -> Result<Option<Edit>> {
        let row = self
            .conn
            .query_row(
                "SELECT c.title, r.status, r.file_ext, r.error, c.revision, r.revision,
                        c.bar_of, (SELECT whole_of FROM clips o WHERE o.id = c.bar_of), r.made_of, r.file_path,
                        r.encoding, r.make
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
                        r.get::<_, Option<String>>(9)?,
                        r.get::<_, Option<String>>(10)?.and_then(|e| serde_json::from_str(&e).ok()),
                        r.get::<_, Option<i64>>(11)?,
                    ))
                },
            )
            .optional()?;
        let Some((title, status, file_ext, error, revision, rendered, snippet_of, made_of, old, encoding, make)) = row
        else {
            return Ok(None);
        };
        let file_path = crate::db::clip_file_of(&self.files, id, file_ext, make, old);
        let pieces = self.edit_pieces(id)?;
        // Of the edit as it is if made of what it's made of now (a file made
        // before that was recorded: of the revision it was made at), and as
        // clips are made now: Settings › Encoding changed since, it's made
        // again, as a clip is.
        let made_as =
            file_path.as_deref().is_none_or(|f| crate::clips::made_as(std::path::Path::new(f), encoding, now));
        let current = made_as
            && match made_of {
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
            encoding,
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

    /// What a file of clip `id` made now would be made of (`made_of`): taken
    /// as its make begins, and recorded with the file (`set_made_of`), so a
    /// change made meanwhile leaves it not current, to be made again.
    pub fn made_of_now(&self, id: i64) -> Result<String> {
        let title: String = self.conn.query_row("SELECT title FROM clips WHERE id = ?1", [id], |r| r.get(0))?;
        self.made_of(id, &title, &self.edit_pieces(id)?)
    }

    /// Records that clip `id`'s file, just made, is of what the clip holds
    /// now (`made_of`).
    pub fn set_made_now(&self, id: i64) -> Result<()> {
        self.set_made_of(id, &self.made_of_now(id)?)
    }

    /// Records that clip `id`'s file was made of `made_of` (`made_of`).
    pub fn set_made_of(&self, id: i64, made_of: &str) -> Result<()> {
        self.conn.execute("UPDATE renders SET made_of = ?2 WHERE clip_id = ?1", params![id, made_of])?;
        Ok(())
    }

    /// A new, empty edit, at the top of the library's list (as new VODs go).
    pub fn add_edit(&self, title: &str) -> Result<Edit> {
        let id = self.new_clip(title, Place::Listed)?;
        // Not rendered: how clips are made doesn't come into it.
        Ok(self.edit(id, ClipEncoding::default())?.expect("just inserted"))
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

    pub(crate) fn vod_duration(&self, vod_id: i64) -> Result<Option<f64>> {
        Ok(self.conn.query_row("SELECT duration FROM vods WHERE id = ?1", [vod_id], |r| r.get(0)).optional()?.flatten())
    }

    /// Clip `clip_id`'s items in its order: each group followed by what's in
    /// it, depth first.
    pub fn item_rows(&self, clip_id: i64) -> Result<Vec<ItemRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent, position, vod_id, start, end, title, origin FROM items
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
                    title: r.get(6)?,
                    origin: r.get::<_, Option<i64>>(7)?.map(Flicks),
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
        let state = self.edit_state(from)?.context("That clip is gone.")?;
        anyhow::ensure!(into != from, "A clip can't go inside itself.");
        // Its chapters and highlights that show in it come too, on the same
        // moments (cuts are where its own stretches were, not its).
        let marks = self.clip_marks(from)?;
        let shown: Vec<MarkAt> = place_marks(&self.edit_pieces(from)?, &marks)
            .iter()
            .filter(|m| m.kind != "cut")
            .filter_map(|m| marks.iter().find(|(id, _)| *id == m.id).map(|(_, at)| at.clone()))
            .collect();
        self.change(into, None, |db| db.insert_group(into, &EditState { marks: shown, ..state }, index))
    }

    /// Puts what `state` holds in clip `into` as a group titled as it is, at
    /// `index` of its top level (or last), with `state`'s marks: a clip
    /// dropped in (`add_group`), or one pasted (`Db::paste`). Not a change of
    /// its own: the caller's. The group's id.
    pub(crate) fn insert_group(&self, into: i64, state: &EditState, index: Option<usize>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO items (clip_id, position, kind, title) VALUES (?1, 0, 'group', ?2)",
            params![into, state.title],
        )?;
        let group = self.conn.last_insert_rowid();
        self.insert_items(into, state, Some(group))?;
        for m in &state.marks {
            self.insert_mark_with(into, m.vod_id, &m.kind, m.at, m.text.as_deref(), &m.detail)?;
        }
        self.place(into, group, index)?;
        Ok(group)
    }

    /// Inserts `state`'s items in clip `into`, with ids of their own, those
    /// at its top level in group `parent` (None: at the clip's top level,
    /// unplaced, for the caller to `place`). Its marks aren't; the new
    /// top-level items' ids, in order.
    pub(crate) fn insert_items(&self, into: i64, state: &EditState, parent: Option<i64>) -> Result<Vec<i64>> {
        let mut ids: Vec<i64> = Vec::with_capacity(state.items.len());
        let mut top = Vec::new();
        for (i, item) in state.items.iter().enumerate() {
            let within = item.parent.map_or(parent, |p| ids.get(p).copied());
            let position = state.items[..i].iter().filter(|o| o.parent == item.parent).count() as i64;
            self.conn.execute(
                "INSERT INTO items (clip_id, parent, position, kind, vod_id, start, end, title, origin)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    into,
                    within,
                    position,
                    if item.vod_id.is_some() { "piece" } else { "group" },
                    item.vod_id,
                    item.start.map(|f| f.0),
                    item.end.map(|f| f.0),
                    item.title,
                    item.origin.map(|f| f.0)
                ],
            )?;
            let id = self.conn.last_insert_rowid();
            ids.push(id);
            if item.parent.is_none() {
                top.push(id);
            }
        }
        Ok(top)
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
            let piece = db.insert_piece(into, vod_id, (start, end), title)?;
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
        let inside = self.marks_inside(edit, &pieces, from, to)?;
        let add = |db: &Db| {
            let id = db.new_clip(title, Place::Bar(edit))?;
            db.conn.execute("UPDATE clips SET shown = ?2 WHERE id = ?1", params![id, shown])?;
            for (i, p) in parts.iter().enumerate() {
                let range = (Flicks::from_seconds(p.start), Flicks::from_seconds(p.end));
                let piece = db.insert_piece(id, p.vod_id, range, &p.title)?;
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

    /// Clip `clip`'s chapters and highlights (of `pieces`, its pieces) whose
    /// moments play in `from`..`to` of its timeline: what a stretch of it
    /// made a clip, or copied, brings.
    pub(crate) fn marks_inside(&self, clip: i64, pieces: &[EditPiece], from: f64, to: f64) -> Result<Vec<MarkAt>> {
        let marks = self.clip_marks(clip)?;
        Ok(place_marks(pieces, &marks)
            .iter()
            .filter(|m| m.kind != "cut" && m.start >= from && m.start < to)
            .filter_map(|m| marks.iter().find(|(id, _)| *id == m.id).map(|(_, at)| at.clone()))
            .collect())
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

    /// The clip on VOD `vod_id`'s clip bar (in it, or only dragged out) listed
    /// at exactly `start`..`end` seconds of it, its end cut at the VOD's
    /// (`vod_span`), if there's one: its first piece's stretch, whatever the
    /// editor changed after it (a piece added), as the page finds it
    /// (clips.svelte.ts `find`). The app gives it (C on its stretch), as
    /// `edit_clip_of` for an edit.
    pub fn vod_clip_of(&self, vod_id: i64, start: f64, end: f64) -> Result<Option<crate::db::Clip>> {
        Ok(self.vod_clips_at(vod_id, start, end)?.into_iter().next())
    }

    /// The clip on VOD `vod_id`'s clip bar (in it, or only dragged out) that
    /// plays just `start`..`end` seconds of it, its end cut at the VOD's: one
    /// piece as it came, as `add_clip` makes it (`Clip::plain`).
    /// The app gives it for a stretch dragged from a VOD, the one shown or a
    /// piece of an edit, whose file has to be just that stretch of its footage.
    pub fn vod_clip_playing(&self, vod_id: i64, start: f64, end: f64) -> Result<Option<crate::db::Clip>> {
        Ok(self.vod_clips_at(vod_id, start, end)?.into_iter().find(|c| c.plain))
    }

    /// The clips on VOD `vod_id`'s clip bar listed at `start`..`end` seconds of
    /// it (`vod_clip_of`), in the bar's order: one query.
    fn vod_clips_at(&self, vod_id: i64, start: f64, end: f64) -> Result<Vec<crate::db::Clip>> {
        let (start, end) = self.vod_span(vod_id, start, end)?;
        let sql = format!(
            "SELECT {CLIP_COLUMNS} FROM clips c {CLIP_JOIN}
             WHERE c.bar_of = (SELECT id FROM clips WHERE whole_of = ?1)
               AND abs(i.start - ?2) < ?4 AND abs(i.end - ?3) < ?4
             ORDER BY c.bar_position, c.id"
        );
        let (start, end, near) =
            (Flicks::from_seconds(start).0, Flicks::from_seconds(end).0, Flicks::from_seconds(0.001).0);
        let mut stmt = self.conn.prepare(&sql)?;
        let clips = stmt.query_map(params![vod_id, start, end, near], |r| crate::db::clip_from_row(r, &self.files))?;
        Ok(clips.collect::<rusqlite::Result<_>>()?)
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
                    .all(|(p, s)| p.vod_id == s.vod_id && same_spans(&p.spans(), &spans(s.start, s.end)));
            if same {
                return Ok(Some(clip));
            }
        }
        Ok(None)
    }

    /// A clip on listed clip `bar`'s clip bar, made anew as `state` has it
    /// (undo bringing back one an edit's change took away); its id.
    pub fn add_bar_clip(&self, bar: i64, state: &EditState) -> Result<i64> {
        self.bring_back_bar_clip(bar, None, state)
    }

    /// `add_bar_clip` for a clip undo brings back: under its own id, `old`,
    /// if no clip has it (`new_clip_as`), else a new one; its id.
    pub fn bring_back_bar_clip(&self, bar: i64, old: Option<i64>, state: &EditState) -> Result<i64> {
        self.change(bar, None, |db| {
            let id = db.new_clip_as(&state.title, Place::Bar(bar), old)?;
            db.restore_edit(id, state)?;
            Ok(id)
        })
    }

    /// Moves clips `ids` (in their order) of listed clip `bar`'s clip bar to
    /// just after its clip `after`; where they are if `after` isn't there.
    /// Part of a change (`change`), as the paste is.
    pub fn place_after_on_bar(&self, bar: i64, ids: &[i64], after: i64) -> Result<()> {
        let mut order: Vec<i64> =
            self.bar_clips(bar)?.into_iter().map(|c| c.id).filter(|id| !ids.contains(id)).collect();
        let Some(at) = order.iter().position(|&id| id == after) else { return Ok(()) };
        order.splice(at + 1..at + 1, ids.iter().copied());
        self.order_bar(&order)
    }

    /// Puts clips `ids` of one clip bar in that order (undo putting a bar
    /// back as it was). Part of a change (`change`).
    pub fn order_bar(&self, ids: &[i64]) -> Result<()> {
        for (i, id) in ids.iter().enumerate() {
            self.conn
                .execute("UPDATE clips SET bar_position = ?2 WHERE id = ?1", rusqlite::params![id, i as i64 + 1])?;
        }
        Ok(())
    }

    /// Puts `items` in edit `edit`, one after another in their order, where
    /// `onto` says, as one change (all of them, or none if one can't go):
    /// several things picked, dropped on the edit or added from a menu. On
    /// its timeline each is a piece (a clip of an edit's clip bar, a group);
    /// on its clip bar each is a clip of its own, and one already there is
    /// left as it is. The clips made on the bar, in order.
    pub fn put(&self, edit: i64, items: &[Put], onto: Onto) -> Result<Vec<i64>> {
        self.revision(edit)?.context("That edit is gone.")?;
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
    pub(crate) fn put_parts(&self, item: &Put) -> Result<(String, Vec<StretchPiece>)> {
        let part = |vod_id, start, end, title: &str| StretchPiece { vod_id, start, end, title: title.to_string() };
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
                let new = db.insert_piece(clip, p.vod_id, (start, end), &p.title)?;
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
                    "INSERT INTO items (clip_id, parent, position, kind, vod_id, start, end, title, origin)
                     SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
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

    /// Clip `id`'s title (an edit's, or a clip's on a clip bar); None if it's gone.
    pub fn clip_title(&self, id: i64) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT title FROM clips WHERE id = ?1", [id], |r| r.get(0)).optional()?)
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
            let new = db.insert_piece(edit_id, vod_id, (start, end), title)?;
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
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO items (clip_id, position, kind, vod_id, start, end, title) VALUES (?1, 0, 'piece', ?2, ?3, ?4, ?5)",
            params![clip_id, vod_id, start.0, end.0, title],
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
    pub(crate) fn place(&self, clip_id: i64, id: i64, index: Option<usize>) -> Result<()> {
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
    pub(crate) fn within_vod(&self, vod_id: i64, start: f64, end: f64) -> Result<(Flicks, Flicks)> {
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
    /// right after the first. False if `at` isn't inside it.
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
            let new = db.insert_piece(piece.clip_id, piece.vod_id, (at, end), &piece.title)?;
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
        // (2, its own file itself: `Db::hand_out_clip_file`, says more, and stays.)
        self.conn.execute("UPDATE renders SET handed_out = max(handed_out, 1) WHERE clip_id = ?1", [id])?;
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
        let (ext, make) = file.map(|f| crate::db::built_make(&self.files, id, f)).transpose()?.unzip();
        self.conn.execute(
            "UPDATE renders SET status = ?2, file_ext = coalesce(?3, file_ext), file_path = coalesce(?6, file_path),
               error = ?4, revision = coalesce(?5, revision), make = CASE WHEN ?3 IS NULL THEN make ELSE ?7 END
             WHERE clip_id = ?1",
            params![id, status.as_str(), ext, error, version, file, make],
        )?;
        Ok(())
    }

    /// Edits left rendering (or asked to, `ClipStatus::Cutting`) when the app
    /// stopped: rendered again at the start (`clips::Cutter::run`). Those
    /// only waiting (changed since their file) are rendered when asked.
    pub fn unfinished_renders(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT clip_id FROM renders
             WHERE status = 'cutting' AND clip_id IN (SELECT id FROM clips WHERE position IS NOT NULL)
             ORDER BY clip_id",
        )?;
        Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
pub(crate) mod tests;
