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

use crate::db::{ClipStatus, Db, now};
use crate::footage::Source;
use crate::render::Piece;
use crate::time::Flicks;
use crate::transcript::Cue;

/// Stretches shorter than this are left out (a sliver between two struck lines).
const SLIVER: f64 = 0.04;

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
    /// A snippet (a clip on a clip bar, opened in the editor): the VOD whose
    /// bar it's on. None for a listed clip.
    pub snippet_of: Option<i64>,
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

/// A mark of a clip, as the record holds it: on a moment of a VOD, wherever
/// that plays in the clip (`place_marks`).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkAt {
    pub vod_id: i64,
    pub kind: String,
    pub at: Flicks,
    pub text: Option<String>,
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
    mut of: impl FnMut(i64) -> Result<Vec<(f64, f64, T)>>,
) -> Result<Vec<(f64, f64, T)>> {
    let mut known = std::collections::HashMap::new();
    let mut out = Vec::new();
    let mut at = 0.0;
    for p in pieces {
        let things = match known.entry(p.vod_id) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => e.insert(of(p.vod_id)?),
        };
        for (a, b) in p.spans() {
            for (start, end, thing) in things.iter().filter(|(s, e, _)| *e > a && *s < b) {
                let (start, end) = (start.max(a), end.min(b));
                if end - start >= SLIVER {
                    out.push((at + start - a, at + end - a, thing.clone()));
                }
            }
            at += b - a;
        }
    }
    Ok(out)
}

/// Captions for a clip of `pieces`, on its timeline (`cues_of`: a VOD's
/// captions, in its time).
pub fn clip_cues(pieces: &[EditPiece], mut cues_of: impl FnMut(i64) -> Result<Vec<Cue>>) -> Result<Vec<Cue>> {
    let placed = on_timeline(pieces, |vod| Ok(cues_of(vod)?.into_iter().map(|c| (c.start, c.end, c.text)).collect()))?;
    Ok(placed.into_iter().map(|(start, end, text)| Cue { start, end, text }).collect())
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
            for &(a, b) in &spans {
                if moment >= a && moment < b {
                    placed = Some(t + moment - a);
                } else if m.kind == "chapter" && moment < a && moment >= p.start {
                    placed = Some(t);
                }
                if placed.is_some() {
                    break;
                }
                t += b - a;
            }
            if let Some(start) = placed {
                out.push(crate::db::Mark {
                    id: *id,
                    vod_id: m.vod_id,
                    kind: m.kind.clone(),
                    start,
                    text: m.text.clone(),
                });
            }
        }
        at += spans.iter().map(|(a, b)| b - a).sum::<f64>();
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.id.cmp(&b.id)));
    out
}

/// The VOD, and its moment (seconds), playing at `time` of a clip of
/// `pieces`' timeline; at its very end, the last moment played.
pub fn source_at(pieces: &[EditPiece], time: f64) -> Option<(i64, f64)> {
    let mut at = 0.0;
    let mut last = None;
    for p in pieces {
        for (a, b) in p.spans() {
            if time >= at && time < at + b - a {
                return Some((p.vod_id, a + time - at));
            }
            at += b - a;
            last = Some((p.vod_id, b));
        }
    }
    last.filter(|_| (time - at).abs() < 1e-6)
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

/// What taking `from`..`to` of a clip of `pieces`' timeline out strikes:
/// for each piece it reaches, all it has struck then (in its VOD's time).
pub fn struck_by_removing(pieces: &[EditPiece], from: f64, to: f64) -> Vec<(i64, Vec<(f64, f64)>)> {
    let mut out = Vec::new();
    let mut at = 0.0;
    for p in pieces {
        let mut more = Vec::new();
        for (a, b) in p.spans() {
            let (start, end) = (from.max(at), to.min(at + b - a));
            if end > start {
                more.push((a + start - at, a + end - at));
            }
            at += b - a;
        }
        if !more.is_empty() {
            out.push((p.id, [p.struck.clone(), more].concat()));
        }
    }
    out
}

/// What to render for `pieces`: each one's spans, from the footage `find`
/// gives for its stretch of its VOD, or a hole's black and silence. Fails if
/// some footage isn't here yet.
pub fn render_pieces(pieces: &[EditPiece], find: impl Fn(i64, f64, f64) -> Option<Source>) -> Result<Vec<Piece>> {
    let mut out = Vec::new();
    for p in pieces {
        for (a, b) in p.spans() {
            if p.hole {
                out.push(Piece { source: None, start: a, end: b });
                continue;
            }
            let Some(source) = find(p.vod_id, a, b) else {
                bail!("The footage of “{}” isn't here yet.", p.title);
            };
            out.push(Piece { source: Some(source.path), start: a - source.offset, end: b - source.offset });
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
    /// of them. They go when the app next starts (not before, so what's
    /// undone meanwhile still has them).
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
                        (SELECT whole_of FROM clips o WHERE o.id = c.bar_of)
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
                        r.get::<_, Option<i64>>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((title, status, file_path, error, revision, rendered, snippet_of)) = row else { return Ok(None) };
        let pieces = self.edit_pieces(id)?;
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
            current: rendered == Some(revision),
            pieces: pieces.len(),
            length: ranges.iter().map(|r| r.end - r.start).sum(),
            vod_ids,
            ranges,
            snippet_of,
        }))
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
    pub fn delete_edit(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM clips WHERE id = ?1", [id])?;
        Ok(())
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
        // Its marks that show in it come too, on the same moments.
        let marks = self.clip_marks(from)?;
        let shown: Vec<MarkAt> = place_marks(&self.edit_pieces(from)?, &marks)
            .iter()
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
                db.insert_mark(into, m.vod_id, &m.kind, m.at, m.text.as_deref())?;
            }
            db.place(into, group, index)?;
            Ok(group)
        })
    }

    /// Clip `id`'s own marks, with their ids, in the order they were made at.
    pub fn clip_marks(&self, id: i64) -> Result<Vec<(i64, MarkAt)>> {
        let mut stmt =
            self.conn.prepare("SELECT id, vod_id, kind, at, text FROM marks WHERE clip_id = ?1 ORDER BY at, id")?;
        let rows = stmt.query_map([id], |r| {
            Ok((r.get(0)?, MarkAt { vod_id: r.get(1)?, kind: r.get(2)?, at: Flicks(r.get(3)?), text: r.get(4)? }))
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
    /// the edit's marks in it, copied.
    pub fn add_edit_clip(&self, edit: i64, from: f64, to: f64, title: &str, shown: bool) -> Result<crate::db::Clip> {
        let pieces = self.edit_pieces(edit)?;
        let parts = stretch_pieces(&pieces, from, to);
        if parts.is_empty() {
            bail!("Nothing of the edit plays there.");
        }
        let marks = self.clip_marks(edit)?;
        let inside: Vec<&MarkAt> = place_marks(&pieces, &marks)
            .iter()
            .filter(|m| m.start >= from && m.start < to)
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
                db.insert_mark(id, m.vod_id, &m.kind, m.at, m.text.as_deref())?;
            }
            Ok(id)
        };
        let id = if shown { self.change(edit, None, add)? } else { self.atomically(add)? };
        Ok(self.clip(id)?.expect("just inserted"))
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
                    db.insert_mark(id, m.vod_id, &m.kind, m.at, m.text.as_deref())?;
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
            "INSERT INTO marks (clip_id, vod_id, kind, at, text, source, created_at)
             SELECT ?1, vod_id, kind, at, text, source, ?5 FROM marks
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

    /// `start`..`end` seconds of VOD `vod_id`, within it and on the flick; an
    /// error if none of it is.
    fn within_vod(&self, vod_id: i64, start: f64, end: f64) -> Result<(Flicks, Flicks)> {
        let duration = self.vod_duration(vod_id)?;
        let (start, end) = (start.max(0.0), duration.map_or(end, |d| end.min(d)));
        anyhow::ensure!(end > start, "That's past the end of the video.");
        Ok((Flicks::from_seconds(start), Flicks::from_seconds(end)))
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

    /// Sets the stretches struck out of piece `id`, in seconds.
    pub fn set_piece_struck(&self, id: i64, struck: &[(f64, f64)]) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        let struck = struck_json(&struck_flicks(struck))?;
        self.change(edit_id, None, |db| {
            db.conn.execute("UPDATE items SET struck = ?2 WHERE id = ?1", params![id, struck])?;
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
    fn captions_follow_the_pieces_onto_the_clips_timeline() {
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
        let cue = |start, end, text: &str| Cue { start, end, text: text.into() };
        let cues = |vod| -> Result<Vec<Cue>> {
            Ok(match vod {
                1 => vec![cue(0.0, 2.0, "one"), cue(2.0, 4.0, "two"), cue(4.0, 6.0, "three"), cue(10.0, 12.0, "ten")],
                _ => vec![cue(100.0, 103.0, "elsewhere")],
            })
        };
        // VOD 1 from 1 to 6 s with 2-4 struck, then VOD 2 from 101 s, then VOD 1 again at 10.
        let pieces =
            [piece(1, 1, 1.0, 6.0, &[(2.0, 4.0)]), piece(2, 2, 101.0, 105.0, &[]), piece(3, 1, 10.0, 11.0, &[])];
        let got = clip_cues(&pieces, cues).unwrap();
        assert_eq!(
            got,
            [cue(0.0, 1.0, "one"), cue(1.0, 3.0, "three"), cue(3.0, 5.0, "elsewhere"), cue(7.0, 8.0, "ten"),]
        );
    }

    #[test]
    fn removing_a_stretch_strikes_it_from_each_piece_it_reaches() {
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
        // Pieces 10-20 (15-16 struck: plays 0-5 and 5-9 of the timeline) and 100-110 (9-19).
        let pieces = [piece(1, 10.0, 20.0, &[(15.0, 16.0)]), piece(2, 100.0, 110.0, &[])];
        assert_eq!(
            struck_by_removing(&pieces, 4.0, 11.0),
            [(1, vec![(15.0, 16.0), (14.0, 15.0), (16.0, 20.0)]), (2, vec![(100.0, 102.0)])]
        );
        assert_eq!(struck_by_removing(&pieces, 19.0, 30.0), [], "past the end");
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
        assert!(db.next_with_status(crate::db::Status::Queued).unwrap().is_none());
        // Added again, it's listed again, the same VOD.
        let (again, new) = db.add_vod(&info("x")).unwrap();
        assert_eq!((again.id, new, again.listed), (vod, false, true));
        assert_eq!(db.uses(vod).unwrap(), 1);
        assert_eq!(db.next_with_status(crate::db::Status::Queued).unwrap().map(|v| v.id), Some(vod));
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
        assert_eq!(source_at(&pieces, 13.0), Some((vod, 35.0)), "the very end");
        assert_eq!(source_at(&pieces, 14.0), None);

        // Undo puts them back as they were: the edit's state holds them.
        let state = db.edit_state(edit).unwrap().unwrap();
        assert!(db.move_mark_to(shown, vod, 16.0).unwrap());
        db.delete_mark(later).unwrap();
        db.restore_edit(edit, &state).unwrap();
        let back: Vec<f64> = db.clip_marks(edit).unwrap().iter().map(|(_, m)| m.at.seconds()).collect();
        assert_eq!(back, [13.0, 13.0, 15.0, 31.0, 50.0]);
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

        // Made again as a state holds it (undo bringing it back).
        let state = db.edit_state(clip.id).unwrap().unwrap();
        let again = db.add_bar_clip(edit, &state).unwrap();
        assert_eq!(db.edit_state(again).unwrap().unwrap(), state);
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
        db.set_piece_range(a_piece, -5.0, 18.0).unwrap();
        db.set_piece_struck(a_piece, &[(12.0, 14.0)]).unwrap();
        let p = &db.edit_pieces(edit.id).unwrap()[0];
        assert_eq!((p.start, p.end), (0.0, 18.0), "not before the VOD");
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
        let none = |_: i64, _: f64, _: f64| None;
        let err = render_pieces(&db.edit_pieces(edit.id).unwrap(), none).unwrap_err();
        assert!(err.to_string().contains("isn't here yet"), "{err}");
        let piece = db.edit_pieces(edit.id).unwrap()[0].id;
        db.set_piece_struck(piece, &[(12.0, 13.0)]).unwrap();
        // Footage of 5 s to 30 s of the VOD: its time 0 is the VOD's 5 s.
        let found = |_: i64, _: f64, _: f64| Some(Source { path: "/f/x.mp4".into(), offset: 5.0, section: None });
        let pieces = render_pieces(&db.edit_pieces(edit.id).unwrap(), found).unwrap();
        assert_eq!(
            pieces,
            [
                Piece { source: Some("/f/x.mp4".into()), start: 5.0, end: 7.0 },
                Piece { source: Some("/f/x.mp4".into()), start: 8.0, end: 15.0 },
            ]
        );
    }
}
