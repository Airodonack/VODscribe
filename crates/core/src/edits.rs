//! Edits: a video made of stretches of VODs, one after another. Each piece is
//! a stretch of one VOD with a title, less the lines struck out of it, which
//! are jumped over. A piece is its own: made from a clip or a stretch of the
//! timeline, it keeps no tie to either, so changing one piece (trimming it,
//! later growing it) changes no other. Rendered with `crate::render`, from
//! the footage of each stretch (`crate::footage`).

use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::db::{ClipStatus, Db, now};
use crate::footage::Source;
use crate::render::Piece;

/// Stretches shorter than this are left out (a sliver between two struck lines).
const SLIVER: f64 = 0.04;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Edit {
    pub id: i64,
    pub title: String,
    /// The rendered file's: waiting (never rendered), cutting (rendering), ready or error.
    pub status: ClipStatus,
    pub file_path: Option<String>,
    pub error: Option<String>,
    /// Whether the rendered file is of the edit as it is now.
    pub current: bool,
    /// How many pieces it has, and how long it plays, in seconds.
    pub pieces: usize,
    pub length: f64,
    /// The VODs its pieces are of.
    pub vod_ids: Vec<i64>,
    /// What it plays of each VOD, in order: its pieces less what's struck.
    pub ranges: Vec<EditRange>,
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

/// What an edit holds, as `Db::edit_state` takes it and `Db::restore_edit`
/// puts it back: for undo.
#[derive(Debug, Clone, PartialEq)]
pub struct EditState {
    pub title: String,
    pub pieces: Vec<PieceState>,
}

/// A piece as an `EditState` holds it: what it is, not which row it was.
#[derive(Debug, Clone, PartialEq)]
pub struct PieceState {
    pub vod_id: i64,
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub struck: Vec<(f64, f64)>,
}

/// A piece of an edit: `start`..`end` of VOD `vod_id`, less the `struck` stretches.
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

/// What to render for `pieces`: each one's spans, from the footage `find`
/// gives for its stretch of its VOD. Fails if some footage isn't here yet.
pub fn render_pieces(pieces: &[EditPiece], find: impl Fn(i64, f64, f64) -> Option<Source>) -> Result<Vec<Piece>> {
    let mut out = Vec::new();
    for p in pieces {
        for (a, b) in p.spans() {
            let Some(source) = find(p.vod_id, a, b) else {
                bail!("The footage of “{}” isn't here yet.", p.title);
            };
            out.push(Piece { source: source.path, start: a - source.offset, end: b - source.offset });
        }
    }
    if out.is_empty() {
        bail!("There's nothing in this edit to render.");
    }
    Ok(out)
}

const EDIT_COLUMNS: &str = "id, title, status, file_path, error, version, rendered_version";

fn piece_row(r: &Row) -> rusqlite::Result<(i64, i64, i64, f64, f64, String, String)> {
    Ok((
        r.get("id")?,
        r.get("edit_id")?,
        r.get("vod_id")?,
        r.get("start")?,
        r.get("end")?,
        r.get("title")?,
        r.get("struck")?,
    ))
}

impl Db {
    /// The library's list, VODs and edits, in its order.
    pub fn library_order(&self) -> Result<Vec<LibraryItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT 'vod', id, position FROM vods UNION ALL SELECT 'edit', id, position FROM edits
             ORDER BY 3, 1 DESC, 2 DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let id = r.get(1)?;
            Ok(if r.get::<_, String>(0)? == "vod" { LibraryItem::Vod(id) } else { LibraryItem::Edit(id) })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Puts the library's list in the order of `items`.
    pub fn reorder_library(&self, items: &[LibraryItem]) -> Result<()> {
        self.atomically(|db| {
            for (position, item) in items.iter().enumerate() {
                let (sql, id) = match item {
                    LibraryItem::Vod(id) => ("UPDATE vods SET position = ?2 WHERE id = ?1", id),
                    LibraryItem::Edit(id) => ("UPDATE edits SET position = ?2 WHERE id = ?1", id),
                };
                db.conn.execute(sql, params![id, position as i64])?;
            }
            Ok(())
        })
    }

    /// Every edit, in their order.
    pub fn edits(&self) -> Result<Vec<Edit>> {
        let mut stmt = self.conn.prepare(&format!("SELECT {EDIT_COLUMNS} FROM edits ORDER BY position, id"))?;
        let ids: Vec<i64> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.into_iter().filter_map(|id| self.edit(id).transpose()).collect()
    }

    pub fn edit(&self, id: i64) -> Result<Option<Edit>> {
        let row = self
            .conn
            .query_row(&format!("SELECT {EDIT_COLUMNS} FROM edits WHERE id = ?1"), [id], |r| {
                Ok((
                    r.get::<_, String>("title")?,
                    r.get::<_, String>("status")?,
                    r.get::<_, Option<String>>("file_path")?,
                    r.get::<_, Option<String>>("error")?,
                    r.get::<_, i64>("version")?,
                    r.get::<_, Option<i64>>("rendered_version")?,
                ))
            })
            .optional()?;
        let Some((title, status, file_path, error, version, rendered)) = row else { return Ok(None) };
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
            current: rendered == Some(version),
            pieces: pieces.len(),
            length: ranges.iter().map(|r| r.end - r.start).sum(),
            vod_ids,
            ranges,
        }))
    }

    /// A new, empty edit, at the top of the library's list (as new VODs go).
    pub fn add_edit(&self, title: &str) -> Result<Edit> {
        self.conn.execute(
            "INSERT INTO edits (title, position, created_at)
             VALUES (?1, (SELECT coalesce(min(p), 0) - 1 FROM (SELECT min(position) AS p FROM vods UNION ALL SELECT min(position) FROM edits)), ?2)",
            params![title, now()],
        )?;
        Ok(self.edit(self.conn.last_insert_rowid())?.expect("just inserted"))
    }

    pub fn rename_edit(&self, id: i64, title: &str) -> Result<()> {
        self.conn.execute("UPDATE edits SET title = ?2 WHERE id = ?1", params![id, title])?;
        Ok(())
    }

    pub fn delete_edit(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM edits WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Edit `id`'s pieces, in order.
    pub fn edit_pieces(&self, id: i64) -> Result<Vec<EditPiece>> {
        let mut stmt = self.conn.prepare("SELECT * FROM edit_pieces WHERE edit_id = ?1 ORDER BY position, id")?;
        let rows: Vec<_> = stmt.query_map([id], piece_row)?.collect::<rusqlite::Result<_>>()?;
        let mut out = Vec::new();
        for (id, edit_id, vod_id, start, end, title, struck) in rows {
            let vod_title = self.conn.query_row("SELECT title FROM vods WHERE id = ?1", [vod_id], |r| r.get(0))?;
            let struck = serde_json::from_str(&struck).unwrap_or_default();
            out.push(EditPiece { id, edit_id, vod_id, start, end, title, struck, vod_title });
        }
        Ok(out)
    }

    /// What edit `id` holds (its title and pieces), to be put back later
    /// with `restore_edit` (for undo); None if it's gone.
    pub fn edit_state(&self, id: i64) -> Result<Option<EditState>> {
        let Some(title) =
            self.conn.query_row("SELECT title FROM edits WHERE id = ?1", [id], |r| r.get(0)).optional()?
        else {
            return Ok(None);
        };
        let pieces = self
            .edit_pieces(id)?
            .into_iter()
            .map(|p| PieceState { vod_id: p.vod_id, start: p.start, end: p.end, title: p.title, struck: p.struck })
            .collect();
        Ok(Some(EditState { title, pieces }))
    }

    /// Puts edit `id` back as `state` had it: its title, and its pieces in
    /// order (made anew, so with new ids), less any of a VOD since deleted.
    /// Nothing if the edit's gone.
    pub fn restore_edit(&self, id: i64, state: &EditState) -> Result<()> {
        self.atomically(|db| {
            if db.conn.execute("UPDATE edits SET title = ?2 WHERE id = ?1", params![id, state.title])? == 0 {
                return Ok(());
            }
            db.conn.execute("DELETE FROM edit_pieces WHERE edit_id = ?1", [id])?;
            for (position, p) in state.pieces.iter().enumerate() {
                db.conn.execute(
                    "INSERT INTO edit_pieces (edit_id, vod_id, position, start, end, title, struck)
                     SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7 WHERE EXISTS (SELECT 1 FROM vods WHERE id = ?2)",
                    params![id, p.vod_id, position as i64, p.start, p.end, p.title, serde_json::to_string(&p.struck)?],
                )?;
            }
            db.changed(id)
        })
    }

    /// Every edit's pieces, as (VOD, start, end): the footage edits use.
    pub fn piece_ranges(&self) -> Result<Vec<(i64, f64, f64)>> {
        let mut stmt = self.conn.prepare("SELECT vod_id, start, end FROM edit_pieces")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The edit piece `id` is in, if it's there.
    pub fn piece_edit(&self, id: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT edit_id FROM edit_pieces WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    /// Marks edit `id` changed, so its rendered file is out of date.
    fn changed(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE edits SET version = version + 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Puts `start`..`end` of VOD `vod_id` in edit `edit_id` as a piece titled
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
        self.atomically(|db| {
            db.conn.execute(
                "INSERT INTO edit_pieces (edit_id, vod_id, position, start, end, title) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
                params![edit_id, vod_id, start, end, title],
            )?;
            let new = db.conn.last_insert_rowid();
            db.place(edit_id, new, index)?;
            db.changed(edit_id)
        })
    }

    /// Moves piece `id` to `index` in its edit.
    pub fn move_piece(&self, id: i64, index: usize) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        self.atomically(|db| {
            db.place(edit_id, id, Some(index))?;
            db.changed(edit_id)
        })
    }

    /// Numbers edit `edit_id`'s pieces in order, with piece `id` at `index` (or last).
    fn place(&self, edit_id: i64, id: i64, index: Option<usize>) -> Result<()> {
        let mut stmt =
            self.conn.prepare("SELECT id FROM edit_pieces WHERE edit_id = ?1 AND id != ?2 ORDER BY position, id")?;
        let mut ids: Vec<i64> = stmt.query_map(params![edit_id, id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.insert(index.unwrap_or(ids.len()).min(ids.len()), id);
        for (position, id) in ids.iter().enumerate() {
            self.conn.execute("UPDATE edit_pieces SET position = ?2 WHERE id = ?1", params![id, position as i64])?;
        }
        Ok(())
    }

    /// `start`..`end` of VOD `vod_id`, within it; an error if none of it is.
    fn within_vod(&self, vod_id: i64, start: f64, end: f64) -> Result<(f64, f64)> {
        let duration: Option<f64> = self
            .conn
            .query_row("SELECT duration FROM vods WHERE id = ?1", [vod_id], |r| r.get(0))
            .optional()?
            .flatten();
        let (start, end) = (start.max(0.0), duration.map_or(end, |d| end.min(d)));
        anyhow::ensure!(end > start, "That's past the end of the video.");
        Ok((start, end))
    }

    /// Sets piece `id` to `start`..`end` of its VOD (within the VOD).
    pub fn set_piece_range(&self, id: i64, start: f64, end: f64) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        let vod_id: i64 = self.conn.query_row("SELECT vod_id FROM edit_pieces WHERE id = ?1", [id], |r| r.get(0))?;
        let (start, end) = self.within_vod(vod_id, start, end)?;
        self.conn.execute("UPDATE edit_pieces SET start = ?2, end = ?3 WHERE id = ?1", params![id, start, end])?;
        self.changed(edit_id)
    }

    /// Sets the stretches struck out of piece `id`.
    pub fn set_piece_struck(&self, id: i64, struck: &[(f64, f64)]) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        self.conn
            .execute("UPDATE edit_pieces SET struck = ?2 WHERE id = ?1", params![id, serde_json::to_string(struck)?])?;
        self.changed(edit_id)
    }

    /// Splits piece `id` in two at `at` (its VOD's time), the second right
    /// after the first; both keep what's struck. False if `at` isn't inside it.
    pub fn split_piece(&self, id: i64, at: f64) -> Result<bool> {
        let Some((_, edit_id, vod_id, start, end, title, struck)) =
            self.conn.query_row("SELECT * FROM edit_pieces WHERE id = ?1", [id], piece_row).optional()?
        else {
            return Ok(false);
        };
        if !(at > start + SLIVER && at < end - SLIVER) {
            return Ok(false);
        }
        let index = self.edit_pieces(edit_id)?.iter().position(|p| p.id == id).map(|i| i + 1);
        self.atomically(|db| {
            db.conn.execute("UPDATE edit_pieces SET end = ?2 WHERE id = ?1", params![id, at])?;
            db.conn.execute(
                "INSERT INTO edit_pieces (edit_id, vod_id, position, start, end, title, struck)
                 VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6)",
                params![edit_id, vod_id, at, end, title, struck],
            )?;
            let new = db.conn.last_insert_rowid();
            db.place(edit_id, new, index)?;
            db.changed(edit_id)
        })?;
        Ok(true)
    }

    pub fn remove_piece(&self, id: i64) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        self.conn.execute("DELETE FROM edit_pieces WHERE id = ?1", [id])?;
        self.changed(edit_id)
    }

    /// The edit's version now, to say which one a render is of.
    pub fn edit_version(&self, id: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT version FROM edits WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    /// Edit `id`'s video was dragged out of the app, so another app may use it.
    pub fn hand_out_edit(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE edits SET handed_out = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Whether edit `id`'s video was dragged out of the app; and it's the app's
    /// own again (`take_back`: rendered to a new file).
    pub fn edit_handed_out(&self, id: i64, take_back: bool) -> Result<bool> {
        let out = self.conn.query_row("SELECT handed_out FROM edits WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        if take_back {
            self.conn.execute("UPDATE edits SET handed_out = 0 WHERE id = ?1", [id])?;
        }
        Ok(out.unwrap_or(false))
    }

    /// Records edit `id`'s render: its status, file (kept if None) and error,
    /// and the version it's of, when done.
    pub fn set_edit_render(
        &self,
        id: i64,
        status: ClipStatus,
        file: Option<&str>,
        error: Option<&str>,
        version: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE edits SET status = ?2, file_path = coalesce(?3, file_path), error = ?4,
               rendered_version = coalesce(?5, rendered_version)
             WHERE id = ?1",
            params![id, status.as_str(), file, error, version],
        )?;
        Ok(())
    }

    /// Edits left rendering (or waiting to) when the app stopped: waiting
    /// again, to be rendered when asked.
    pub fn reset_renders(&self) -> Result<()> {
        self.conn.execute("UPDATE edits SET status = 'waiting' WHERE status = 'cutting'", [])?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fetch::VodInfo;
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
        let version = db.edit_version(edit.id).unwrap();
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
                Piece { source: "/f/x.mp4".into(), start: 5.0, end: 7.0 },
                Piece { source: "/f/x.mp4".into(), start: 8.0, end: 15.0 },
            ]
        );
    }
}
