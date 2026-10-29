//! Sources: the VODs clips' pieces are stretches of. A source stays while
//! any piece uses it (`Db::uses`). Removing a VOD and deleting its video
//! first *realizes* each stretch other clips use: the stretch, exactly, is
//! made into a source of its own (a file in the data folder, with that
//! stretch's transcript and chat, in its own time), and the piece moves to
//! it, keeping its id and its origin (the old source's time its file starts
//! at). A source deleted without that (`Db::set_gone`) leaves holes.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use crate::db::{CHAT_COLUMNS, Db, SEGMENT_COLUMNS, now};
use crate::edits::{PIECE_COLUMNS, PieceRow, piece_from_row, struck_json};
use crate::time::Flicks;
use crate::transcript::Word;

impl Db {
    /// The pieces of VOD `vod_id` in clips other than its own and its clip
    /// bar's: what removing it leaves, to realize.
    pub fn pieces_using(&self, vod_id: i64) -> Result<Vec<PieceRow>> {
        let sql = format!(
            "SELECT {PIECE_COLUMNS} FROM items WHERE kind = 'piece' AND vod_id = ?1
               AND clip_id NOT IN (SELECT id FROM clips WHERE whole_of = ?1
                                   OR bar_of IN (SELECT id FROM clips WHERE whole_of = ?1))
             ORDER BY clip_id, id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([vod_id], piece_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// A new source for `start`..`end` seconds of VOD `vod_id`, which is to
    /// be made into a file (`set_media_path`, once it is): the VOD's title,
    /// link and channel, and its transcript and chat in that stretch, moved
    /// to the new source's time (from 0). Not listed. Its id.
    pub fn add_realized(&self, vod_id: i64, start: f64, end: f64) -> Result<i64> {
        self.atomically(|db| {
            db.conn.execute(
                "INSERT INTO vods (id, platform, platform_id, url, title, channel, duration, thumbnail, published,
                                   status, progress, added_at, model, chat)
                 SELECT (SELECT max(next, (SELECT coalesce(max(id), 0) + 1 FROM vods)) FROM vod_ids),
                        'realized', ?1 || ':' || ?2 || '-' || ?3 || ':' || ?5, url, title, channel, ?4, thumbnail,
                        published, 'done', 1.0, ?5, model, chat
                 FROM vods WHERE id = ?1",
                params![vod_id, start, end, end - start, now()],
            )?;
            let id = db.conn.last_insert_rowid();
            let segments: Vec<(f64, f64, String, String, Option<String>)> = {
                let mut stmt = db.conn.prepare(
                    "SELECT start, end, text, words, speaker FROM segments
                     WHERE vod_id = ?1 AND end > ?2 AND start < ?3 ORDER BY start",
                )?;
                stmt.query_map(params![vod_id, start, end], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?
                .collect::<rusqlite::Result<_>>()?
            };
            for (a, b, text, words, speaker) in segments {
                let words: Vec<Word> = serde_json::from_str::<Vec<Word>>(&words)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|w| w.end > start && w.start < end)
                    .map(|w| Word { start: (w.start - start).max(0.0), end: (w.end - start).min(end - start), ..w })
                    .collect();
                let text = if words.is_empty() {
                    text
                } else {
                    words.iter().map(|w| w.text.as_str()).collect::<String>().trim().to_string()
                };
                db.conn.execute(
                    &format!("INSERT INTO segments ({SEGMENT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"),
                    params![
                        id,
                        (a - start).max(0.0),
                        (b - start).min(end - start),
                        text,
                        serde_json::to_string(&words)?,
                        speaker
                    ],
                )?;
            }
            // What of the stretch was transcribed, in the new source's time.
            for done in db.stretches(vod_id)?.into_iter().filter(|s| s.status == crate::db::Status::Done) {
                let (a, b) = (done.start.max(start), done.end.unwrap_or(f64::INFINITY).min(end));
                if b > a {
                    db.insert_done(id, a - start, Some(b - start), done.model.as_deref())?;
                }
            }
            // Every column, in the new source's time (`CHAT_COLUMNS`).
            db.conn.execute(
                &format!(
                    "INSERT INTO chat ({CHAT_COLUMNS})
                     SELECT ?1, time - ?2, author, color, text, parts, kind, amount, event_time - ?2 FROM chat
                     WHERE vod_id = ?3 AND time >= ?2 AND time < ?4 ORDER BY time, id"
                ),
                params![id, start, vod_id, end],
            )?;
            Ok(id)
        })
    }

    /// Moves piece `piece` to source `to`, which holds its stretch from 0
    /// (`add_realized`): its times in `to`'s, its origin where `to` starts in
    /// what it was of. A change to its clip against revision `base`, so one
    /// made from the clip as it was before (`Stale` otherwise); false if the
    /// piece is gone or no longer the stretch `to` holds.
    pub fn realize_piece(&self, piece: i64, to: i64, base: i64) -> Result<bool> {
        let sql = format!("SELECT {PIECE_COLUMNS} FROM items WHERE id = ?1");
        let Some(row) = self.conn.query_row(&sql, [piece], piece_from_row).optional()? else { return Ok(false) };
        let length: f64 = self
            .conn
            .query_row("SELECT duration FROM vods WHERE id = ?1", [to], |r| r.get::<_, Option<f64>>(0))?
            .context("That source has no length.")?;
        let Some(end) = row.end else { return Ok(false) };
        if ((end - row.start).seconds() - length).abs() > 0.001 {
            return Ok(false);
        }
        let shift = row.start;
        let struck: Vec<(Flicks, Flicks)> = row.struck.iter().map(|&(a, b)| (a - shift, b - shift)).collect();
        let origin = row.origin.unwrap_or(Flicks::ZERO) + shift;
        self.change(row.clip_id, Some(base), |db| {
            db.conn.execute(
                "UPDATE items SET vod_id = ?2, start = 0, end = ?3, struck = ?4, origin = ?5 WHERE id = ?1",
                params![piece, to, (end - shift).0, struck_json(&struck)?, origin.0],
            )?;
            Ok(())
        })?;
        Ok(true)
    }

    /// Whether VOD `vod_id`'s files, transcript and chat are gone
    /// (`set_gone`): its pieces are holes.
    pub fn is_gone(&self, vod_id: i64) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT gone FROM vods WHERE id = ?1", [vod_id], |r| r.get(0))
            .optional()?
            .unwrap_or(false))
    }

    /// Notes that VOD `vod_id`'s files, transcript and chat are gone while
    /// pieces of it stay: holes, till nothing uses it and it goes too.
    pub fn set_gone(&self, vod_id: i64) -> Result<()> {
        self.conn.execute("UPDATE vods SET gone = 1, media_path = NULL WHERE id = ?1", [vod_id])?;
        // Its cast and activity go with its transcript.
        self.conn.execute("DELETE FROM vod_annotations WHERE vod_id = ?1", [vod_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::chat::{ChatMessage, ChatPart, Kind};
    use crate::edits::tests::info;
    use crate::transcript::Segment;

    #[test]
    fn a_realized_stretch_is_a_source_of_its_own() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let word = |text: &str, start: f64, end: f64| Word { text: text.into(), start, end };
        let segments = [
            Segment {
                start: 5.0,
                end: 12.0,
                text: " a b".into(),
                words: vec![word(" a", 5.0, 6.0), word(" b", 11.0, 12.0)],
            },
            Segment { start: 20.0, end: 21.0, text: " c".into(), words: vec![word(" c", 20.0, 21.0)] },
        ];
        db.insert_segments(vod, &segments).unwrap();
        db.add_clip(vod, 1.0, 2.0, "snippet", true).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 15.0), "p", None).unwrap();
        let piece = db.piece_rows(edit).unwrap()[0].id;
        db.set_piece_struck(piece, &[(11.0, 12.0)]).unwrap();
        // Only the edit's piece: the VOD's own clip and its snippet go with it.
        assert_eq!(db.pieces_using(vod).unwrap().iter().map(|p| p.id).collect::<Vec<_>>(), [piece]);

        let new = db.add_realized(vod, 10.0, 15.0).unwrap();
        let source = db.vod(new).unwrap().unwrap();
        assert_eq!((source.info.duration, source.listed, source.info.title.as_str()), (Some(5.0), false, "VOD x"));
        let words = db.words_between(new, 0.0, 5.0).unwrap();
        assert_eq!(words.iter().map(|w| (w.text.as_str(), w.start)).collect::<Vec<_>>(), [(" b", 1.0)]);

        let base = db.revision(edit).unwrap().unwrap();
        db.change(edit, None, |_| Ok(())).unwrap();
        assert!(db.realize_piece(piece, new, base).unwrap_err().downcast_ref::<crate::edits::Stale>().is_some());
        assert!(db.realize_piece(piece, new, base + 1).unwrap());
        let row = db.piece_rows(edit).unwrap().remove(0);
        assert_eq!(
            (row.id, row.vod_id, row.start, row.end),
            (piece, new, Flicks::ZERO, Some(Flicks::from_seconds(5.0)))
        );
        assert_eq!(row.struck, [(Flicks::from_seconds(1.0), Flicks::from_seconds(2.0))]);
        assert_eq!(row.origin, Some(Flicks::from_seconds(10.0)));
        assert_eq!((db.uses(vod).unwrap(), db.uses(new).unwrap()), (2, 1));
        assert!(db.pieces_using(vod).unwrap().is_empty());

        db.set_gone(new).unwrap();
        assert!(db.is_gone(new).unwrap() && !db.is_gone(vod).unwrap());
    }

    #[test]
    fn a_kept_stretch_keeps_who_speaks_and_chat_events() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let word = |text: &str, start: f64, end: f64| Word { text: text.into(), start, end };
        db.insert_segments(
            vod,
            &[Segment { start: 11.0, end: 12.0, text: " hi".into(), words: vec![word(" hi", 11.0, 12.0)] }],
        )
        .unwrap();
        db.conn.execute("UPDATE segments SET speaker = 'Marlowe' WHERE vod_id = ?1", [vod]).unwrap();
        let sub = ChatMessage {
            time: 13.0,
            author: "fan".into(),
            color: Some("#ff0000".into()),
            parts: vec![ChatPart::Text { text: "resubbed".into() }],
            kind: Kind::Sub,
            amount: Some(2.0),
            event_time: Some(12.5),
            at: None,
        };
        crate::db::insert_chat(&db.conn, vod, &[sub]).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 15.0), "p", None).unwrap();
        let piece = db.piece_rows(edit).unwrap()[0].id;

        // Kept, then the VOD deleted with its video.
        let new = db.add_realized(vod, 10.0, 15.0).unwrap();
        let base = db.revision(edit).unwrap().unwrap();
        assert!(db.realize_piece(piece, new, base).unwrap());
        db.delete_vod(vod).unwrap();

        let lines = db.lines(new).unwrap();
        assert_eq!(
            lines.iter().map(|l| (l.start, l.text.as_str(), l.speaker.as_deref())).collect::<Vec<_>>(),
            [(1.0, "hi", Some("Marlowe"))]
        );
        let subs = db.chat_of_kind(new, Kind::Sub).unwrap();
        assert_eq!(
            subs.iter().map(|(_, m)| (m.time, m.author.as_str(), m.amount, m.event_time)).collect::<Vec<_>>(),
            [(3.0, "fan", Some(2.0), Some(2.5))]
        );
    }

    #[test]
    fn stretches_kept_while_their_vod_stays_get_back_what_the_copy_lost() {
        let dir = crate::TempDir::new("migrate-v36");
        let path = dir.join("library.sqlite");
        let (vod, kept) = {
            let mut db = Db::open(&path).unwrap();
            let vod = db.add_vod(&info("x")).unwrap().0.id;
            let word = |text: &str, start: f64, end: f64| Word { text: text.into(), start, end };
            let segments = [
                Segment { start: 9.0, end: 11.0, text: " a".into(), words: vec![word(" a", 10.5, 11.0)] },
                Segment { start: 11.0, end: 12.0, text: " hi".into(), words: vec![word(" hi", 11.0, 12.0)] },
            ];
            db.insert_segments(vod, &segments).unwrap();
            db.conn.execute("UPDATE segments SET speaker = 'S' || id WHERE vod_id = ?1", [vod]).unwrap();
            let message =
                |time: f64, text: &str, kind: Kind, amount: Option<f64>, event_time: Option<f64>| ChatMessage {
                    time,
                    author: "fan".into(),
                    color: None,
                    parts: vec![ChatPart::Text { text: text.into() }],
                    kind,
                    amount,
                    event_time,
                    at: None,
                };
            let chat = [
                message(12.0, "hello", Kind::Message, None, None),
                message(13.0, "resubbed", Kind::Sub, Some(2.0), Some(12.5)),
                message(14.0, "gift", Kind::Gift, Some(5.0), None),
            ];
            crate::db::insert_chat(&db.conn, vod, &chat).unwrap();
            // Kept with the copy that left columns out; its VOD stays, as when
            // keeping the next stretch failed and nothing was removed.
            let kept = db.add_realized(vod, 10.0, 15.0).unwrap();
            db.conn
                .execute_batch(&format!(
                    "UPDATE segments SET speaker = NULL WHERE vod_id = {kept};
                     UPDATE chat SET kind = 'message', amount = NULL, event_time = NULL WHERE vod_id = {kept};"
                ))
                .unwrap();
            // One whose VOD is gone: nothing to get back.
            let other = db.add_vod(&info("y")).unwrap().0.id;
            crate::db::insert_chat(&db.conn, other, &[message(1.0, "tip", Kind::Tip, Some(100.0), None)]).unwrap();
            let orphan = db.add_realized(other, 0.0, 5.0).unwrap();
            db.conn.execute("UPDATE chat SET kind = 'message', amount = NULL WHERE vod_id = ?1", [orphan]).unwrap();
            db.delete_vod(other).unwrap();
            (vod, kept)
        };
        rusqlite::Connection::open(&path).unwrap().execute_batch("PRAGMA user_version = 35;").unwrap();
        let db = Db::open(&path).unwrap();

        let lines = db.lines(kept).unwrap();
        let speakers: Vec<Option<String>> = db.lines(vod).unwrap().into_iter().map(|l| l.speaker).collect();
        assert_eq!(lines.into_iter().map(|l| l.speaker).collect::<Vec<_>>(), speakers);
        let events = |kind| {
            db.chat_of_kind(kept, kind)
                .unwrap()
                .into_iter()
                .map(|(_, m)| (m.time, m.amount, m.event_time))
                .collect::<Vec<_>>()
        };
        assert_eq!(events(Kind::Sub), [(3.0, Some(2.0), Some(2.5))]);
        assert_eq!(events(Kind::Gift), [(4.0, Some(5.0), None)]);
        assert_eq!(events(Kind::Message).len(), 1);
    }
}
