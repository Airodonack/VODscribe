//! Stretches: what of a VOD is transcribed, and what is asked to be. One
//! table holds both. A `done` row is a transcribed range; any other row is a
//! request (waiting, running, paused or failed), with `reached` how far it
//! has written lines. The lines themselves stay in `segments`, by time, so
//! search, captions and clips read them as before.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;

use crate::db::{Db, Status, VOD_COLUMNS, Vod, vod_from_row};
use crate::transcript::Segment;

/// Ranges this close count as touching, and a VOD transcribed this close to
/// each end as transcribed to it: a VOD's length from its site and the end
/// of its audio differ by a little.
const EDGE: f64 = 0.5;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Stretch {
    pub id: i64,
    pub vod_id: i64,
    pub start: f64,
    /// None: to the VOD's end.
    pub end: Option<f64>,
    pub status: Status,
    /// 0..1 within the current status, within the stretch.
    pub progress: f64,
    /// How far a request has written lines; None until it starts.
    pub reached: Option<f64>,
    pub error: Option<String>,
    /// The model that made its lines (`model::is_current`).
    pub model: Option<String>,
    /// A section of audio downloaded for it alone, whose 0 is at `start`.
    #[serde(skip)]
    pub audio: Option<String>,
}

impl Stretch {
    /// The whole VOD, from its start to its end.
    pub fn is_whole(&self) -> bool {
        self.start <= 0.0 && self.end.is_none()
    }

    /// Where its transcription carries on from: its start, or as far as it got.
    pub fn from(&self) -> f64 {
        self.reached.unwrap_or(self.start).max(self.start)
    }

    fn is_request(&self) -> bool {
        self.status != Status::Done
    }
}

/// How much of a VOD is transcribed.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Transcribed {
    None,
    Some,
    All,
}

const STRETCH_COLUMNS: &str = "id, vod_id, start, end, status, progress, reached, error, model, audio";

fn stretch_from_row(r: &Row) -> rusqlite::Result<Stretch> {
    Ok(Stretch {
        id: r.get("id")?,
        vod_id: r.get("vod_id")?,
        start: r.get("start")?,
        end: r.get("end")?,
        status: Status::parse(&r.get::<_, String>("status")?),
        progress: r.get("progress")?,
        reached: r.get("reached")?,
        error: r.get("error")?,
        model: r.get("model")?,
        audio: r.get("audio")?,
    })
}

type JsonRow = (i64, i64, f64, Option<f64>, String, f64, Option<f64>, Option<String>, Option<String>);

/// A VOD's stretches from the JSON array `VOD_COLUMNS` gathers them in, in id order.
pub(crate) fn from_json(json: &str) -> serde_json::Result<Vec<Stretch>> {
    let rows: Vec<JsonRow> = serde_json::from_str(json)?;
    let mut out: Vec<Stretch> = rows
        .into_iter()
        .map(|(id, vod_id, start, end, status, progress, reached, error, model)| Stretch {
            id,
            vod_id,
            start,
            end,
            status: Status::parse(&status),
            progress,
            reached,
            error,
            model,
            audio: None,
        })
        .collect();
    out.sort_by_key(|s| s.id);
    Ok(out)
}

/// What a VOD's row shows of its stretches; see `Vod`.
pub(crate) struct Shown {
    pub status: Status,
    pub progress: f64,
    pub error: Option<String>,
    pub stretch: Option<i64>,
    pub transcribed: Transcribed,
    pub waiting: usize,
    pub outdated: bool,
}

/// The row's view of `stretches` (in id order) of a VOD `duration` long.
pub(crate) fn summarize(stretches: &[Stretch], duration: Option<f64>, has_segments: bool) -> Shown {
    let requests: Vec<&Stretch> = stretches.iter().filter(|s| s.is_request()).collect();
    let first = |pick: &dyn Fn(Status) -> bool| requests.iter().find(|s| pick(s.status)).copied();
    let shown = first(&|s| matches!(s, Status::Transcribing | Status::Downloading))
        .or_else(|| first(&|s| matches!(s, Status::Queued | Status::Ready)))
        .or_else(|| first(&|s| s == Status::Paused))
        .or_else(|| requests.iter().rev().find(|s| s.status == Status::Error).copied());
    let transcribed = transcribed(stretches, duration);
    // A done stretch a request covers is being transcribed again.
    let replaced = |d: &Stretch| {
        requests.iter().any(|r| r.start <= d.start && r.end.is_none_or(|e| d.end.is_some_and(|de| de <= e)))
    };
    let outdated = has_segments
        && stretches.iter().any(|s| {
            let has_lines = match s.status {
                Status::Done => !replaced(s),
                _ => s.reached.is_some_and(|r| r > s.start),
            };
            has_lines && !s.model.as_deref().is_some_and(crate::model::is_current)
        });
    let waiting = requests.len() - usize::from(shown.is_some());
    match shown {
        Some(s) => Shown {
            status: s.status,
            progress: s.progress,
            error: s.error.clone().filter(|_| s.status == Status::Error),
            stretch: Some(s.id),
            transcribed,
            waiting,
            outdated,
        },
        None => Shown {
            status: if transcribed == Transcribed::All { Status::Done } else { Status::None },
            progress: if transcribed == Transcribed::All { 1.0 } else { 0.0 },
            error: None,
            stretch: None,
            transcribed,
            waiting,
            outdated,
        },
    }
}

/// The ranges of a VOD that are transcribed, in order and apart: its done
/// stretches, and what its requests have reached. An end of infinity is the
/// VOD's end.
pub fn covered(stretches: &[Stretch]) -> Vec<(f64, f64)> {
    let mut ranges: Vec<(f64, f64)> = stretches
        .iter()
        .filter_map(|s| match s.status {
            Status::Done => Some((s.start, s.end.unwrap_or(f64::INFINITY))),
            _ => s.reached.map(|r| (s.start, r)),
        })
        .filter(|(a, b)| b > a)
        .collect();
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in ranges {
        match out.last_mut() {
            Some(last) if a <= last.1 + EDGE => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// How much of a VOD `duration` long (None: unknown) `stretches` cover.
pub fn transcribed(stretches: &[Stretch], duration: Option<f64>) -> Transcribed {
    let covered = covered(stretches);
    let Some(&(a, b)) = covered.first() else { return Transcribed::None };
    let end = duration.map_or(f64::INFINITY, |d| d - EDGE);
    if a <= EDGE && b >= end { Transcribed::All } else { Transcribed::Some }
}

/// The ranges of a VOD `duration` long that `stretches` don't cover, as
/// (start, end), an end of None being the VOD's end.
pub fn gaps(stretches: &[Stretch], duration: Option<f64>) -> Vec<(f64, Option<f64>)> {
    let end = duration.unwrap_or(f64::INFINITY);
    let mut out = Vec::new();
    let mut at = 0.0;
    for (a, b) in covered(stretches) {
        if a > at + EDGE {
            out.push((at, Some(a)));
        }
        at = at.max(b);
    }
    if at + EDGE < end {
        out.push((at, None));
    }
    out
}

/// `ranges` (start, end, model) without `cut`: what's left of each.
fn without(ranges: Vec<(f64, f64, Option<String>)>, cut: (f64, f64)) -> Vec<(f64, f64, Option<String>)> {
    let mut out = Vec::new();
    for (a, b, model) in ranges {
        if a < cut.0.min(b) {
            out.push((a, cut.0.min(b), model.clone()));
        }
        if cut.1.max(a) < b {
            out.push((cut.1.max(a), b, model));
        }
    }
    out
}

impl Db {
    /// Asks for `start`..`end` of VOD `vod_id` (None: to its end) to be
    /// transcribed: a request at the back of the queue.
    pub fn request(&self, vod_id: i64, start: f64, end: Option<f64>) -> Result<Stretch> {
        self.conn.execute(
            "INSERT INTO stretches (vod_id, start, end, status) VALUES (?1, ?2, ?3, 'queued')",
            params![vod_id, start, end],
        )?;
        Ok(self.stretch(self.conn.last_insert_rowid())?.expect("just inserted"))
    }

    pub fn stretch(&self, id: i64) -> Result<Option<Stretch>> {
        let sql = format!("SELECT {STRETCH_COLUMNS} FROM stretches WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], stretch_from_row).optional()?)
    }

    /// VOD `vod_id`'s stretches, done and asked for, in time order.
    pub fn stretches(&self, vod_id: i64) -> Result<Vec<Stretch>> {
        let sql = format!("SELECT {STRETCH_COLUMNS} FROM stretches WHERE vod_id = ?1 ORDER BY start, id");
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([vod_id], stretch_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// The ranges of VOD `vod_id` transcribed by a model this version no
    /// longer uses (or before models were recorded), as (start, end), an end
    /// of None being the VOD's end: what "Transcribe again" asks for when only
    /// some of it is transcribed.
    pub fn outdated_ranges(&self, vod_id: i64) -> Result<Vec<(f64, Option<f64>)>> {
        Ok(self
            .stretches(vod_id)?
            .into_iter()
            .filter(|s| s.status == Status::Done && !s.model.as_deref().is_some_and(crate::model::is_current))
            .map(|s| (s.start, s.end))
            .collect())
    }

    /// VOD `vod_id`'s requests, in the order they were made.
    pub fn requests(&self, vod_id: i64) -> Result<Vec<Stretch>> {
        let sql = format!("SELECT {STRETCH_COLUMNS} FROM stretches WHERE vod_id = ?1 AND status != 'done' ORDER BY id");
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([vod_id], stretch_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// The first request asked for with `status`, of a VOD in the library's
    /// list (one taken out of it waits), with its VOD: the next one for that
    /// stage. Of files on this computer only, if `local_only`.
    pub fn next_stretch(&self, status: Status, local_only: bool) -> Result<Option<(Vod, Stretch)>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT s.id FROM stretches s JOIN vod_order o ON o.vod_id = s.vod_id JOIN vods v ON v.id = s.vod_id
                 WHERE s.status = ?1 AND (?2 = 0 OR v.source_path IS NOT NULL) ORDER BY s.id LIMIT 1",
                params![status.as_str(), local_only],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stretch) = id.map(|id| self.stretch(id)).transpose()?.flatten() else { return Ok(None) };
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE id = ?1");
        let vod = self.conn.query_row(&sql, [stretch.vod_id], vod_from_row)?;
        Ok(Some((vod, stretch)))
    }

    pub fn count_stretches(&self, status: Status) -> Result<usize> {
        let n: i64 =
            self.conn.query_row("SELECT count(*) FROM stretches WHERE status = ?1", [status.as_str()], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Returns requests left mid-stage by a crash or quit to that stage's
    /// queue. Transcription resumes from where it reached.
    pub fn requeue_interrupted(&self) -> Result<()> {
        self.conn.execute_batch(
            "UPDATE stretches SET status = 'queued' WHERE status = 'downloading';
             UPDATE stretches SET status = 'ready' WHERE status = 'transcribing';",
        )?;
        Ok(())
    }

    /// Forgets every finished download, after the work dir has been cleared.
    /// Requests waiting to transcribe go back to waiting to download.
    pub fn forget_downloads(&self) -> Result<()> {
        self.conn.execute_batch(
            "UPDATE vods SET audio_path = NULL;
             UPDATE stretches SET audio = NULL;
             UPDATE stretches SET status = 'queued', progress = 0 WHERE status = 'ready';",
        )?;
        Ok(())
    }

    pub fn set_stretch_status(&self, id: i64, status: Status, progress: f64, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE stretches SET status = ?2, progress = ?3, error = ?4 WHERE id = ?1",
            params![id, status.as_str(), progress, error],
        )?;
        Ok(())
    }

    /// Records how far request `id` has written lines.
    pub fn set_reached(&self, id: i64, reached: Option<f64>) -> Result<()> {
        self.conn.execute("UPDATE stretches SET reached = ?2 WHERE id = ?1", params![id, reached])?;
        Ok(())
    }

    pub fn set_stretch_model(&self, id: i64, model: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE stretches SET model = ?2 WHERE id = ?1", params![id, model])?;
        Ok(())
    }

    pub fn set_stretch_audio(&self, id: i64, audio: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE stretches SET audio = ?2 WHERE id = ?1", params![id, audio])?;
        Ok(())
    }

    pub fn delete_stretch(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM stretches WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Forgets all of VOD `vod_id`'s stretches, done and asked for.
    pub fn clear_stretches(&self, vod_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM stretches WHERE vod_id = ?1", [vod_id])?;
        Ok(())
    }

    /// Records `start`..`end` of VOD `vod_id` as transcribed by `model`, as
    /// is (`merge_done` makes the done stretches apart again).
    pub(crate) fn insert_done(&self, vod_id: i64, start: f64, end: Option<f64>, model: Option<&str>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO stretches (vod_id, start, end, status, progress, model) VALUES (?1, ?2, ?3, 'done', 1, ?4)",
            params![vod_id, start, end, model],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Widens request `id` outwards to the edges of any line its start or
    /// end cuts through, so a line is transcribed again whole or not at all;
    /// before it starts only (later, its audio is already cut to it).
    pub fn snap_stretch(&self, id: i64) -> Result<Stretch> {
        self.atomically(|db| {
            let stretch = db.stretch(id)?.context("That request is gone.")?;
            if stretch.reached.is_some() {
                return Ok(stretch);
            }
            let start: f64 = db.conn.query_row(
                "SELECT coalesce(min(start), ?2) FROM segments WHERE vod_id = ?1 AND start < ?2 AND end > ?2",
                params![stretch.vod_id, stretch.start],
                |r| r.get(0),
            )?;
            let end: Option<f64> = match stretch.end {
                Some(end) => Some(db.conn.query_row(
                    "SELECT coalesce(max(end), ?2) FROM segments WHERE vod_id = ?1 AND start < ?2 AND end > ?2",
                    params![stretch.vod_id, end],
                    |r| r.get(0),
                )?),
                None => None,
            };
            let start = start.min(stretch.start);
            db.conn.execute("UPDATE stretches SET start = ?2, end = ?3 WHERE id = ?1", params![id, start, end])?;
            Ok(Stretch { start, end, ..stretch })
        })
    }

    /// Writes `segments`, transcribed for request `stretch_id`, in place of
    /// the lines that start from `from` to before `to` (None: on to the
    /// end), which it has transcribed over, and records that it reached
    /// `reached`: all in one transaction, so a stop never leaves both lines
    /// or neither. Returns how many lines it replaced.
    pub fn replace_segments(
        &mut self,
        vod_id: i64,
        stretch_id: i64,
        (from, to): (f64, Option<f64>),
        segments: &[Segment],
        reached: f64,
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let replaced = tx.execute(
            "DELETE FROM segments WHERE vod_id = ?1 AND start >= ?2 AND (?3 IS NULL OR start < ?3)",
            params![vod_id, from, to],
        )?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO segments (vod_id, start, end, text, words) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for s in segments {
                stmt.execute(params![vod_id, s.start, s.end, s.text, serde_json::to_string(&s.words)?])?;
            }
        }
        tx.execute("UPDATE stretches SET reached = ?2 WHERE id = ?1", params![stretch_id, reached])?;
        tx.commit()?;
        Ok(replaced)
    }

    /// Rewrites the transcript's search index without the entries of lines
    /// replaced since (`Db::tidy_index`): after a stretch replaced some.
    pub fn tidy_segments(&self) -> Result<()> {
        self.tidy_index("segments_fts")
    }

    /// Ends request `id`, keeping what it transcribed: done from its start
    /// to where it reached, or gone if it reached nothing.
    pub fn end_request(&self, id: i64) -> Result<()> {
        self.atomically(|db| {
            let Some(stretch) = db.stretch(id)? else { return Ok(()) };
            match stretch.reached.filter(|r| *r > stretch.start) {
                Some(reached) => {
                    db.conn.execute("UPDATE stretches SET end = ?2 WHERE id = ?1", params![id, reached])?;
                    db.finish_stretch(id)
                }
                None => db.delete_stretch(id),
            }
        })
    }

    /// Request `id` is done: its range is transcribed, over whatever was there.
    pub fn finish_stretch(&self, id: i64) -> Result<()> {
        self.atomically(|db| {
            let Some(stretch) = db.stretch(id)? else { return Ok(()) };
            db.conn.execute(
                "UPDATE stretches SET status = 'done', progress = 1, error = NULL, audio = NULL WHERE id = ?1",
                [id],
            )?;
            db.merge_done(stretch.vod_id, Some(id))
        })
    }

    /// Makes VOD `vod_id`'s done stretches apart, in time order: where two
    /// overlap, the later one's lines replaced the other's, so it keeps the
    /// overlap (`newest`, if given, is the latest; it's the one that just
    /// finished, whatever its id). Neighbours made by the same model become
    /// one. The rows are written anew.
    pub fn merge_done(&self, vod_id: i64, newest: Option<i64>) -> Result<()> {
        self.atomically(|db| {
            let mut done: Vec<Stretch> =
                db.stretches(vod_id)?.into_iter().filter(|s| s.status == Status::Done).collect();
            done.sort_by_key(|s| (Some(s.id) == newest, s.id));
            let mut ranges: Vec<(f64, f64, Option<String>)> = Vec::new();
            for s in done {
                let range = (s.start, s.end.unwrap_or(f64::INFINITY));
                ranges = without(ranges, range);
                ranges.push((range.0, range.1, s.model));
            }
            ranges.retain(|(a, b, _)| b - a > 1e-6);
            ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut merged: Vec<(f64, f64, Option<String>)> = Vec::new();
            for (a, b, model) in ranges {
                match merged.last_mut() {
                    Some(last) if a <= last.1 + 1e-6 && last.2 == model => last.1 = last.1.max(b),
                    _ => merged.push((a, b, model)),
                }
            }
            db.conn.execute("DELETE FROM stretches WHERE vod_id = ?1 AND status = 'done'", [vod_id])?;
            for (a, b, model) in merged {
                db.insert_done(vod_id, a, Some(b).filter(|b| b.is_finite()), model.as_deref())?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::VodInfo;

    fn info(id: &str, duration: Option<f64>) -> VodInfo {
        VodInfo {
            platform: "youtube".into(),
            platform_id: id.into(),
            url: format!("https://youtu.be/{id}"),
            title: id.into(),
            channel: None,
            duration,
            thumbnail: None,
            published: None,
        }
    }

    fn stretch(id: i64, start: f64, end: Option<f64>, status: Status, reached: Option<f64>) -> Stretch {
        Stretch {
            id,
            vod_id: 1,
            start,
            end,
            status,
            progress: 0.0,
            reached,
            error: None,
            model: Some(crate::model::CPU_MODEL.id.into()),
            audio: None,
        }
    }

    /// Done ranges as (start, end, model), in time order.
    fn done(db: &Db, vod: i64) -> Vec<(f64, Option<f64>, Option<String>)> {
        db.stretches(vod)
            .unwrap()
            .into_iter()
            .filter(|s| s.status == Status::Done)
            .map(|s| (s.start, s.end, s.model))
            .collect()
    }

    #[test]
    fn done_stretches_merge_apart_and_the_latest_keeps_the_overlap() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("a", Some(600.0))).unwrap().0.id;
        let m = |s: &str| Some(s.to_string());
        db.insert_done(vod, 100.0, Some(200.0), Some("a")).unwrap();
        db.insert_done(vod, 300.0, Some(400.0), Some("a")).unwrap();
        db.merge_done(vod, None).unwrap();
        assert_eq!(done(&db, vod), [(100.0, Some(200.0), m("a")), (300.0, Some(400.0), m("a"))]);
        // One between, by the same model, joins them.
        db.insert_done(vod, 200.0, Some(300.0), Some("a")).unwrap();
        db.merge_done(vod, None).unwrap();
        assert_eq!(done(&db, vod), [(100.0, Some(400.0), m("a"))]);
        // A request older than another, finished last: its range wins, by
        // another model, splitting the one it lands in.
        let old = db.request(vod, 150.0, Some(250.0)).unwrap().id;
        db.conn.execute("UPDATE stretches SET model = 'b' WHERE id = ?1", [old]).unwrap();
        db.insert_done(vod, 140.0, Some(160.0), Some("a")).unwrap();
        db.finish_stretch(old).unwrap();
        assert_eq!(
            done(&db, vod),
            [(100.0, Some(150.0), m("a")), (150.0, Some(250.0), m("b")), (250.0, Some(400.0), m("a"))]
        );
        // To the end, over everything after it.
        db.insert_done(vod, 200.0, None, Some("a")).unwrap();
        db.merge_done(vod, None).unwrap();
        assert_eq!(done(&db, vod), [(100.0, Some(150.0), m("a")), (150.0, Some(200.0), m("b")), (200.0, None, m("a"))]);
        assert_eq!(db.requests(vod).unwrap(), []);
    }

    #[test]
    fn the_ranges_made_by_an_older_model_are_found() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("a", Some(600.0))).unwrap().0.id;
        let old = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";
        db.insert_done(vod, 0.0, Some(100.0), Some(old)).unwrap();
        db.insert_done(vod, 100.0, Some(200.0), Some(crate::model::CPU_MODEL.id)).unwrap();
        db.insert_done(vod, 300.0, None, None).unwrap();
        db.request(vod, 400.0, Some(450.0)).unwrap();
        assert_eq!(db.outdated_ranges(vod).unwrap(), [(0.0, Some(100.0)), (300.0, None)]);
    }

    #[test]
    fn coverage_counts_done_stretches_and_how_far_requests_reached() {
        use Status::{Done, Paused, Queued, Transcribing};
        let s = |id, a, b, st, r| stretch(id, a, b, st, r);
        assert_eq!(transcribed(&[], Some(600.0)), Transcribed::None);
        assert_eq!(transcribed(&[s(1, 0.0, None, Queued, None)], Some(600.0)), Transcribed::None);
        assert_eq!(transcribed(&[s(1, 0.0, None, Done, None)], None), Transcribed::All);
        assert_eq!(transcribed(&[s(1, 0.0, None, Paused, Some(100.0))], Some(600.0)), Transcribed::Some);
        // Pieces that meet cover it all, up to a little short of the end.
        let parts = [s(1, 0.0, Some(300.0), Done, None), s(2, 300.0, None, Transcribing, Some(599.8))];
        assert_eq!(covered(&parts), [(0.0, 599.8)]);
        assert_eq!(transcribed(&parts, Some(600.0)), Transcribed::All);
        assert_eq!(transcribed(&parts, None), Transcribed::Some, "an unknown length needs a stretch to the end");
        let apart = [s(1, 0.0, Some(100.0), Done, None), s(2, 200.0, Some(300.0), Done, None)];
        assert_eq!(transcribed(&apart, Some(600.0)), Transcribed::Some);
        assert_eq!(gaps(&apart, Some(600.0)), [(100.0, Some(200.0)), (300.0, None)]);
        assert_eq!(gaps(&[], None), [(0.0, None)]);
        assert_eq!(gaps(&[s(1, 0.0, None, Done, None)], Some(600.0)), []);
    }

    #[test]
    fn a_vods_status_is_the_stretch_its_row_shows() {
        use Status::{Done, Downloading, Error, Paused, Queued, Ready};
        let shown = |stretches: &[Stretch]| {
            let s = summarize(stretches, Some(600.0), true);
            (s.status, s.stretch, s.transcribed, s.waiting)
        };
        assert_eq!(shown(&[]), (Status::None, None, Transcribed::None, 0));
        assert_eq!(shown(&[stretch(1, 0.0, None, Done, None)]), (Done, None, Transcribed::All, 0));
        let some = stretch(1, 0.0, Some(60.0), Done, None);
        assert_eq!(shown(std::slice::from_ref(&some)), (Status::None, None, Transcribed::Some, 0));
        // Running first, then the first waiting, then paused, then the latest failed.
        let mut failed = stretch(2, 60.0, Some(70.0), Error, None);
        failed.error = Some("HTTP 404".into());
        let mut later = failed.clone();
        later.id = 3;
        later.error = Some("gone".into());
        let paused = stretch(4, 70.0, Some(80.0), Paused, Some(75.0));
        let waiting = stretch(5, 80.0, Some(90.0), Ready, None);
        let queued = stretch(6, 90.0, Some(95.0), Queued, None);
        let running = stretch(7, 95.0, Some(99.0), Downloading, None);
        let all = [some.clone(), failed.clone(), later.clone(), paused.clone(), waiting.clone(), queued.clone()];
        assert_eq!(shown(&[all.as_slice(), &[running]].concat()), (Downloading, Some(7), Transcribed::Some, 5));
        assert_eq!(shown(&all), (Ready, Some(5), Transcribed::Some, 4));
        assert_eq!(shown(&all[..4]), (Paused, Some(4), Transcribed::Some, 2));
        let s = summarize(&all[..3], Some(600.0), true);
        assert_eq!((s.status, s.error.as_deref(), s.waiting), (Error, Some("gone"), 1));
        // Outdated: lines made, or begun, with a model no longer used.
        let mut old = stretch(8, 0.0, Some(10.0), Done, None);
        old.model = Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8".into());
        assert!(summarize(std::slice::from_ref(&old), Some(600.0), true).outdated);
        assert!(!summarize(std::slice::from_ref(&old), Some(600.0), false).outdated, "no lines");
        old.status = Queued;
        assert!(!summarize(std::slice::from_ref(&old), Some(600.0), true).outdated, "not begun");
        old.reached = Some(5.0);
        assert!(summarize(std::slice::from_ref(&old), Some(600.0), true).outdated);
        // Done with an older one, and being transcribed again: not outdated meanwhile.
        old.status = Done;
        let again = stretch(9, 0.0, None, Queued, None);
        assert!(!summarize(&[old, again], Some(600.0), true).outdated);
    }

    #[test]
    fn a_request_stopped_before_any_line_leaves_the_vod_untranscribed() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("a", Some(600.0))).unwrap().0.id;
        for status in [Status::Queued, Status::Downloading, Status::Transcribing, Status::Paused, Status::Error] {
            let asked = db.request(vod, 0.0, None).unwrap().id;
            db.set_stretch_status(asked, status, 0.3, (status == Status::Error).then_some("HTTP 404")).unwrap();
            db.end_request(asked).unwrap();
            let v = db.vod(vod).unwrap().unwrap();
            assert_eq!(
                (v.status, v.error, v.stretch, v.transcribed, v.waiting),
                (Status::None, None, None, Transcribed::None, 0),
                "stopped while {status:?}: like a VOD never asked"
            );
        }
    }

    #[test]
    fn the_queue_runs_in_the_order_asked_and_waits_for_listing() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let [a, b] = ["a", "b"].map(|id| db.add_vod(&info(id, Some(60.0))).unwrap().0.id);
        let first = db.request(b, 0.0, None).unwrap();
        let second = db.request(a, 10.0, Some(20.0)).unwrap();
        let next = |db: &Db| db.next_stretch(Status::Queued, false).unwrap().map(|(v, s)| (v.id, s.id));
        assert_eq!(next(&db), Some((b, first.id)), "asked first, whatever the library's order");
        db.set_stretch_status(first.id, Status::Downloading, 0.0, None).unwrap();
        assert_eq!(next(&db), Some((a, second.id)));
        assert_eq!(db.count_stretches(Status::Downloading).unwrap(), 1);
        assert_eq!(db.vod(b).unwrap().unwrap().status, Status::Downloading);
        db.remove_entry(a).unwrap();
        assert_eq!(next(&db), None, "out of the list, its work waits");
    }
}
