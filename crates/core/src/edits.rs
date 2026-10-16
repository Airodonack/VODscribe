//! Edits: a video made of stretches of clips, one after another. Each piece
//! is a stretch of one clip (all of it to start; trimmed at either end), less
//! the lines struck out of it, which are jumped over. Clips are the material,
//! so one clip can be in several edits; deleting a clip takes it out of them.
//! Rendered with `crate::render`, from the clips' files.

use std::path::PathBuf;

use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::db::{CLIP_COLUMNS, Clip, ClipStatus, Db, clip_from_row, now};
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
    /// The VODs its clips are of, and the clips.
    pub vod_ids: Vec<i64>,
    pub clip_ids: Vec<i64>,
    /// The clip its first piece is of: its first frame is the edit's picture.
    pub first_clip: Option<i64>,
}

/// A row of the library's list: VODs and edits share one order.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum LibraryItem {
    Vod(i64),
    Edit(i64),
}

/// A piece of an edit: `start`..`end` of its clip's VOD (within the clip),
/// less the `struck` stretches.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EditPiece {
    pub id: i64,
    pub edit_id: i64,
    pub start: f64,
    pub end: f64,
    pub struck: Vec<(f64, f64)>,
    pub clip: Clip,
    pub vod_title: String,
}

impl EditPiece {
    /// What plays of it, in its VOD's time: `start`..`end` less what's struck,
    /// and within its clip (whose range may have moved since).
    pub fn spans(&self) -> Vec<(f64, f64)> {
        spans(self.start.max(self.clip.start), self.end.min(self.clip.end), &self.struck)
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

/// What to render for `pieces`: each one's spans, from its clip's file (whose
/// time 0 is the clip's start). Fails if a clip's file isn't made.
pub fn render_pieces(pieces: &[EditPiece]) -> Result<Vec<Piece>> {
    let mut out = Vec::new();
    for p in pieces {
        let file = match (&p.clip.status, &p.clip.file_path) {
            (ClipStatus::Ready, Some(file)) => PathBuf::from(file),
            _ => bail!("The clip “{}” isn't made yet.", p.clip.title),
        };
        for (a, b) in p.spans() {
            out.push(Piece { source: file.clone(), start: a - p.clip.start, end: b - p.clip.start });
        }
    }
    if out.is_empty() {
        bail!("There's nothing in this edit to render.");
    }
    Ok(out)
}

const EDIT_COLUMNS: &str = "id, title, status, file_path, error, version, rendered_version";

fn piece_row(r: &Row) -> rusqlite::Result<(i64, i64, i64, f64, f64, String)> {
    Ok((r.get("id")?, r.get("edit_id")?, r.get("clip_id")?, r.get("start")?, r.get("end")?, r.get("struck")?))
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
        let length = pieces.iter().flat_map(|p| p.spans()).map(|(a, b)| b - a).sum();
        let mut vod_ids: Vec<i64> = pieces.iter().map(|p| p.clip.vod_id).collect();
        vod_ids.sort_unstable();
        vod_ids.dedup();
        let mut clip_ids: Vec<i64> = pieces.iter().map(|p| p.clip.id).collect();
        clip_ids.sort_unstable();
        clip_ids.dedup();
        Ok(Some(Edit {
            id,
            title,
            status: ClipStatus::parse(&status),
            file_path,
            error,
            current: rendered == Some(version),
            pieces: pieces.len(),
            length,
            vod_ids,
            clip_ids,
            first_clip: pieces.first().map(|p| p.clip.id),
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

    /// Edit `id`'s pieces, in order, with their clips.
    pub fn edit_pieces(&self, id: i64) -> Result<Vec<EditPiece>> {
        let mut stmt = self.conn.prepare("SELECT * FROM edit_pieces WHERE edit_id = ?1 ORDER BY position, id")?;
        let rows: Vec<_> = stmt.query_map([id], piece_row)?.collect::<rusqlite::Result<_>>()?;
        let clip_sql = format!("SELECT {CLIP_COLUMNS} FROM clips WHERE id = ?1");
        let mut out = Vec::new();
        for (id, edit_id, clip_id, start, end, struck) in rows {
            let clip = self.conn.query_row(&clip_sql, [clip_id], clip_from_row)?;
            let vod_title = self.conn.query_row("SELECT title FROM vods WHERE id = ?1", [clip.vod_id], |r| r.get(0))?;
            let struck = serde_json::from_str(&struck).unwrap_or_default();
            out.push(EditPiece { id, edit_id, start, end, struck, clip, vod_title });
        }
        Ok(out)
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

    /// Puts all of clip `clip_id` in edit `edit_id`, at `index` (or last).
    pub fn add_piece(&self, edit_id: i64, clip_id: i64, index: Option<usize>) -> Result<()> {
        let (start, end): (f64, f64) =
            self.conn
                .query_row("SELECT start, end FROM clips WHERE id = ?1", [clip_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        self.atomically(|db| {
            db.conn.execute(
                "INSERT INTO edit_pieces (edit_id, clip_id, position, start, end) VALUES (?1, ?2, 0, ?3, ?4)",
                params![edit_id, clip_id, start, end],
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

    /// Trims piece `id` to `start`..`end` of its VOD, within its clip.
    pub fn set_piece_range(&self, id: i64, start: f64, end: f64) -> Result<()> {
        let Some(edit_id) = self.piece_edit(id)? else { return Ok(()) };
        self.conn.execute(
            "UPDATE edit_pieces SET
               start = max(?2, (SELECT start FROM clips WHERE id = clip_id)),
               end = min(?3, (SELECT end FROM clips WHERE id = clip_id))
             WHERE id = ?1",
            params![id, start, end],
        )?;
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
        let Some((edit_id, clip_id, start, end, struck)) = self
            .conn
            .query_row("SELECT * FROM edit_pieces WHERE id = ?1", [id], piece_row)
            .optional()?
            .map(|(_, edit_id, clip_id, start, end, struck)| (edit_id, clip_id, start, end, struck))
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
                "INSERT INTO edit_pieces (edit_id, clip_id, position, start, end, struck) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
                params![edit_id, clip_id, at, end, struck],
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

    /// The edits clip `clip_id` is in, by id.
    pub fn edits_with_clip(&self, clip_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT edit_id FROM edit_pieces WHERE clip_id = ?1")?;
        Ok(stmt.query_map([clip_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    /// The edit's version now, to say which one a render is of.
    pub fn edit_version(&self, id: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT version FROM edits WHERE id = ?1", [id], |r| r.get(0)).optional()?)
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

    /// Edits left rendering when the app stopped: failed, to render again.
    pub fn reset_renders(&self) -> Result<()> {
        self.conn.execute("UPDATE edits SET status = 'waiting' WHERE status = 'cutting'", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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

    fn info(id: &str) -> VodInfo {
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
    fn edits_hold_pieces_of_clips_in_order() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let a = db.add_clip(vod, 10.0, 20.0, "a", true).unwrap();
        let b = db.add_clip(vod, 30.0, 35.0, "b", true).unwrap();
        let edit = db.add_edit("Race night").unwrap();
        assert_eq!((edit.pieces, edit.length, edit.current), (0, 0.0, false));
        db.add_piece(edit.id, a.id, None).unwrap();
        db.add_piece(edit.id, b.id, Some(0)).unwrap();
        let titles =
            |db: &Db| db.edit_pieces(edit.id).unwrap().iter().map(|p| p.clip.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles(&db), ["b", "a"]);
        assert_eq!(db.edit(edit.id).unwrap().unwrap().first_clip, Some(b.id));
        assert_eq!(db.edit(edit.id).unwrap().unwrap().vod_ids, [vod]);
        let pieces = db.edit_pieces(edit.id).unwrap();
        db.move_piece(pieces[0].id, 1).unwrap();
        assert_eq!(titles(&db), ["a", "b"]);
        // Trimmed within its clip, and a line struck.
        let a_piece = db.edit_pieces(edit.id).unwrap()[0].id;
        db.set_piece_range(a_piece, 5.0, 18.0).unwrap();
        db.set_piece_struck(a_piece, &[(12.0, 14.0)]).unwrap();
        let p = &db.edit_pieces(edit.id).unwrap()[0];
        assert_eq!((p.start, p.end), (10.0, 18.0), "not before the clip");
        assert_eq!(p.spans(), [(10.0, 12.0), (14.0, 18.0)]);
        assert_eq!(db.edit(edit.id).unwrap().unwrap().length, 6.0 + 5.0);
        // Split in two where it's asked, the second right after the first.
        let before = db.edit_pieces(edit.id).unwrap();
        assert!(db.split_piece(before[0].id, 16.0).unwrap());
        assert!(!db.split_piece(before[0].id, 3.0).unwrap(), "not inside it");
        let split = db.edit_pieces(edit.id).unwrap();
        assert_eq!(split.len(), 3);
        assert_eq!(((split[0].start, split[0].end), (split[1].start, split[1].end)), ((10.0, 16.0), (16.0, 18.0)));
        assert_eq!(split[1].struck, [(12.0, 14.0)]);
        db.remove_piece(split[1].id).unwrap();
        db.set_piece_range(split[0].id, 10.0, 18.0).unwrap();
        // Rendered files say whether they're of the edit as it is.
        let version = db.edit_version(edit.id).unwrap();
        db.set_edit_render(edit.id, ClipStatus::Ready, Some("/v/e.mp4"), None, version).unwrap();
        assert!(db.edit(edit.id).unwrap().unwrap().current);
        db.remove_piece(p.id).unwrap();
        let after = db.edit(edit.id).unwrap().unwrap();
        assert!(!after.current);
        assert_eq!(after.file_path.as_deref(), Some("/v/e.mp4"));
        // Deleting a clip takes it out of edits.
        assert_eq!(db.edits_with_clip(b.id).unwrap(), [edit.id]);
        db.delete_clip(b.id).unwrap();
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
    fn renders_need_the_clips_made() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let clip = db.add_clip(vod, 10.0, 20.0, "a", true).unwrap();
        let edit = db.add_edit("e").unwrap();
        db.add_piece(edit.id, clip.id, None).unwrap();
        let err = render_pieces(&db.edit_pieces(edit.id).unwrap()).unwrap_err();
        assert!(err.to_string().contains("isn't made yet"), "{err}");
        db.set_clip_file(clip.id, 10.0, 20.0, ClipStatus::Ready, Some("/c/a.mp4"), None).unwrap();
        let piece = db.edit_pieces(edit.id).unwrap()[0].id;
        db.set_piece_struck(piece, &[(12.0, 13.0)]).unwrap();
        let pieces = render_pieces(&db.edit_pieces(edit.id).unwrap()).unwrap();
        assert_eq!(
            pieces,
            [
                Piece { source: "/c/a.mp4".into(), start: 0.0, end: 2.0 },
                Piece { source: "/c/a.mp4".into(), start: 3.0, end: 10.0 },
            ]
        );
    }
}
