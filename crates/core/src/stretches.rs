//! Stretches: what of a VOD is transcribed, and what is asked to be. One
//! table holds both. A `done` row is a transcribed range; any other row is a
//! request (waiting, running, paused or failed), with `reached` how far it
//! has written lines. The lines themselves stay in `segments`, by time, so
//! search, captions and clips read them as before.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;

use crate::asked::Queue;
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
    /// Its audio is here, its own section or the VOD's whole audio, and so
    /// its waveform: the levels are read as the audio comes.
    #[serde(rename = "audio")]
    pub audio_here: bool,
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

const STRETCH_COLUMNS: &str = "id, vod_id, start, end, status, progress, reached, error, model, audio_ext,
    (SELECT audio_ext FROM vods WHERE vods.id = stretches.vod_id) AS vod_audio_ext";

fn stretch_from_row(r: &Row, files: &crate::Paths) -> rusqlite::Result<Stretch> {
    let (id, vod_id) = (r.get("id")?, r.get("vod_id")?);
    let audio = crate::db::ext_of(r.get("audio_ext")?);
    let audio_here = audio.is_some() || crate::db::ext_of(r.get("vod_audio_ext")?).is_some();
    Ok(Stretch {
        id,
        vod_id,
        start: r.get("start")?,
        end: r.get("end")?,
        status: Status::parse(&r.get::<_, String>("status")?),
        progress: r.get("progress")?,
        reached: r.get("reached")?,
        error: r.get("error")?,
        model: r.get("model")?,
        audio: audio.map(|ext| crate::db::shown(&files.source_section(vod_id, id, &ext))),
        audio_here,
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
            audio_here: false,
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
pub(crate) fn covered(stretches: &[Stretch]) -> Vec<(f64, f64)> {
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
pub(crate) fn transcribed(stretches: &[Stretch], duration: Option<f64>) -> Transcribed {
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
    /// transcribed: a request at the back of the queue. The same stretch
    /// asked for again while it waits or runs is that request (Transcribe
    /// pressed twice mustn't transcribe it twice), in its place in the line;
    /// one paused or failed is asked for anew.
    pub fn request(&self, vod_id: i64, start: f64, end: Option<f64>) -> Result<Stretch> {
        let waiting: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM stretches WHERE vod_id = ?1 AND start = ?2 AND end IS ?3
                 AND status IN ('queued', 'downloading', 'ready', 'transcribing') ORDER BY id LIMIT 1",
                params![vod_id, start, end],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = waiting {
            return Ok(self.stretch(id)?.expect("just found"));
        }
        self.atomically(|db| {
            db.conn.execute(
                "INSERT INTO stretches (vod_id, start, end, status) VALUES (?1, ?2, ?3, 'queued')",
                params![vod_id, start, end],
            )?;
            let id = db.conn.last_insert_rowid();
            db.keep_place(Queue::Transcribe, id);
            Ok(db.stretch(id)?.expect("just inserted"))
        })
    }

    pub fn stretch(&self, id: i64) -> Result<Option<Stretch>> {
        let sql = format!("SELECT {STRETCH_COLUMNS} FROM stretches WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], |r| stretch_from_row(r, &self.files)).optional()?)
    }

    /// VOD `vod_id`'s stretches, done and asked for, in time order.
    pub fn stretches(&self, vod_id: i64) -> Result<Vec<Stretch>> {
        let sql = format!("SELECT {STRETCH_COLUMNS} FROM stretches WHERE vod_id = ?1 ORDER BY start, id");
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([vod_id], |r| stretch_from_row(r, &self.files))?.collect::<rusqlite::Result<_>>()?)
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
        Ok(stmt.query_map([vod_id], |r| stretch_from_row(r, &self.files))?.collect::<rusqlite::Result<_>>()?)
    }

    /// The first request asked for with `status`, of a VOD in the library's
    /// list, with its VOD: the next one for that stage. First asked is when
    /// it was last asked for (`asked`: Resume and Retry ask again); one with
    /// no place there (an older version's) after, by id. Of files on this
    /// computer only, if `local_only`. Taking a VOD out of the list ends its
    /// requests; one left on it anyway isn't taken up.
    pub fn next_stretch(&self, status: Status, local_only: bool) -> Result<Option<(Vod, Stretch)>> {
        self.next_stretch_where(status, local_only, |_, _| true)
    }

    /// `next_stretch`, the first of them that `take` takes, in the same
    /// order: one it passes over keeps its place for a later look.
    pub fn next_stretch_where(
        &self,
        status: Status,
        local_only: bool,
        mut take: impl FnMut(&Vod, &Stretch) -> bool,
    ) -> Result<Option<(Vod, Stretch)>> {
        let mut ids = self.conn.prepare(
            "SELECT s.id FROM stretches s JOIN vod_order o ON o.vod_id = s.vod_id JOIN vods v ON v.id = s.vod_id
             LEFT JOIN asked a ON a.queue = ?3 AND a.job = s.id
             WHERE s.status = ?1 AND (?2 = 0 OR v.source_path IS NOT NULL OR v.received_ext IS NOT NULL)
             ORDER BY a.at IS NULL, a.at, s.id",
        )?;
        let ids =
            ids.query_map(params![status.as_str(), local_only, Queue::Transcribe.as_str()], |r| r.get::<_, i64>(0))?;
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE id = ?1");
        for id in ids {
            let Some(stretch) = self.stretch(id?)? else { continue };
            let vod = self.conn.query_row(&sql, [stretch.vod_id], |r| vod_from_row(r, &self.files))?;
            if take(&vod, &stretch) {
                return Ok(Some((vod, stretch)));
            }
        }
        Ok(None)
    }

    pub fn count_stretches(&self, status: Status) -> Result<usize> {
        let n: i64 =
            self.conn.query_row("SELECT count(*) FROM stretches WHERE status = ?1", [status.as_str()], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Returns requests left mid-stage by a crash or quit to that stage's
    /// queue. Transcription resumes from where it reached, in the order the
    /// requests were asked (`Db::in_asked_order`: those an older version
    /// asked after, by id, and the places of requests gone forgotten).
    pub fn requeue_interrupted(&self) -> Result<()> {
        self.atomically(|db| {
            db.conn.execute_batch(
                "UPDATE stretches SET status = 'queued' WHERE status = 'downloading';
                 UPDATE stretches SET status = 'ready' WHERE status = 'transcribing';",
            )?;
            let mut stmt = db.conn.prepare("SELECT id FROM stretches WHERE status != 'done' ORDER BY id")?;
            let requests: Vec<i64> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            db.ordered_as_asked(Queue::Transcribe, requests, |&id| id);
            Ok(())
        })
    }

    /// Forgets every finished download, after the work dir has been cleared.
    /// Requests waiting to transcribe go back to waiting to download.
    pub fn forget_downloads(&self) -> Result<()> {
        self.conn.execute_batch(
            "UPDATE vods SET audio_ext = NULL, audio_path = NULL;
             UPDATE stretches SET audio_ext = NULL, audio = NULL;
             UPDATE stretches SET status = 'queued', progress = 0 WHERE status = 'ready';",
        )?;
        Ok(())
    }

    /// Puts request `id` (paused or failed) back in its stage's queue at
    /// `status`, `progress` along: at the back of the line, as anything
    /// asked for now (decided for 0.7.4: Resume and Retry go to the back),
    /// its place kept with it (`Db::keep_place`).
    pub fn requeue_request(&self, id: i64, status: Status, progress: f64) -> Result<()> {
        self.atomically(|db| {
            db.set_stretch_status(id, status, progress, None)?;
            db.keep_place(Queue::Transcribe, id);
            Ok(())
        })
    }

    pub fn set_stretch_status(&self, id: i64, status: Status, progress: f64, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE stretches SET status = ?2, progress = ?3, error = ?4 WHERE id = ?1",
            params![id, status.as_str(), progress, error],
        )?;
        Ok(())
    }

    /// Records how far request `id` has written lines.
    pub(crate) fn set_reached(&self, id: i64, reached: Option<f64>) -> Result<()> {
        self.conn.execute("UPDATE stretches SET reached = ?2 WHERE id = ?1", params![id, reached])?;
        Ok(())
    }

    pub(crate) fn set_stretch_model(&self, id: i64, model: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE stretches SET model = ?2 WHERE id = ?1", params![id, model])?;
        Ok(())
    }

    /// Records request `id`'s section of audio, at `audio`
    /// (`Paths::source_section`), or that it has none.
    pub fn set_stretch_audio(&self, id: i64, audio: Option<&str>) -> Result<()> {
        let vod_id: i64 = self.conn.query_row("SELECT vod_id FROM stretches WHERE id = ?1", [id], |r| r.get(0))?;
        let ext =
            audio.map(|a| crate::db::built_ext(a, |ext| self.files.source_section(vod_id, id, ext))).transpose()?;
        self.conn.execute("UPDATE stretches SET audio_ext = ?2, audio = ?3 WHERE id = ?1", params![id, ext, audio])?;
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
    pub(crate) fn snap_stretch(&self, id: i64) -> Result<Stretch> {
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
            let mut stmt = tx.prepare(&format!(
                "INSERT INTO segments ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                crate::db::SEGMENT_COLUMNS
            ))?;
            for s in segments {
                // A transcription names no speaker.
                let speaker: Option<&str> = None;
                stmt.execute(params![vod_id, s.start, s.end, s.text, serde_json::to_string(&s.words)?, speaker])?;
            }
        }
        tx.execute("UPDATE stretches SET reached = ?2 WHERE id = ?1", params![stretch_id, reached])?;
        tx.commit()?;
        Ok(replaced)
    }

    /// Rewrites the transcript's search index without the entries of lines
    /// replaced since (`Db::tidy_index`): after a stretch replaced some.
    pub(crate) fn tidy_segments(&self) -> Result<()> {
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
                "UPDATE stretches SET status = 'done', progress = 1, error = NULL, audio_ext = NULL, audio = NULL WHERE id = ?1",
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
    pub(crate) fn merge_done(&self, vod_id: i64, newest: Option<i64>) -> Result<()> {
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
mod tests;
