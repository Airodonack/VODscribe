//! The order work was asked in (`asked`, db.rs `MIGRATE_V47`), so the
//! queues carry on after a quit in the order they had: one row per job
//! asked for (a clip to make, a request to transcribe, a VOD to erase…),
//! with when it was asked, on one counter across every queue.
//!
//! The rows never decide whether a job carries on: each queue finds its own
//! unfinished work from its own state, as before, and only asks here where
//! each goes in the line (`Db::in_asked_order`). A row left over, or one
//! missing (a job an older version asked), costs order at most, never work.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::db::Db;

/// A queue whose order is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Queue {
    /// Clips made and edits rendered, by the clip's or edit's id.
    Render,
    /// Transcription requests, by the request's id (a stretch's).
    Transcribe,
    /// Removed VODs erased, by the VOD's id.
    Erase,
    /// Whole videos kept, by the VOD's id.
    Video,
    /// Footage downloaded for an edit, by the edit's id.
    Footage,
    /// The GPU add-on, its one job 0.
    Addon,
}

impl Queue {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Queue::Render => "render",
            Queue::Transcribe => "transcribe",
            Queue::Erase => "erase",
            Queue::Video => "video",
            Queue::Footage => "footage",
            Queue::Addon => "addon",
        }
    }
}

impl Db {
    /// Job `job` of `queue` was asked for now: last in its line, after
    /// everything asked before in any queue. A job asked again takes this
    /// place too; asked again while it's still waiting, its caller leaves
    /// it where it is by not asking.
    pub fn ask(&self, queue: Queue, job: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO asked (queue, job, at) VALUES (?1, ?2, (SELECT coalesce(max(at), 0) + 1 FROM asked))
             ON CONFLICT (queue, job) DO UPDATE SET at = excluded.at",
            params![queue.as_str(), job],
        )?;
        Ok(())
    }

    /// `ask`, as each queue saves a job's place: in the transaction (or the
    /// lock's hold) its own state is written in, and a place that can't be
    /// written is logged, never a reason to fail what was asked; a job
    /// with no place carries on all the same, after those with one.
    pub fn keep_place(&self, queue: Queue, job: i64) {
        if let Err(e) = self.ask(queue, job) {
            eprintln!("{queue:?} job {job}'s place in its queue, kept for the next start: {e:#}");
        }
    }

    /// `keep_place` for a job put first (`ask_first`).
    pub fn keep_place_first(&self, queue: Queue, job: i64) {
        if let Err(e) = self.ask_first(queue, job) {
            eprintln!("{queue:?} job {job}'s place first in its queue, kept for the next start: {e:#}");
        }
    }

    /// Job `job` of `queue` was put first in its line (a clip a drag waits
    /// for): asked before any other of that queue's.
    pub fn ask_first(&self, queue: Queue, job: i64) -> Result<()> {
        let first: Option<i64> =
            self.conn.query_row("SELECT min(at) FROM asked WHERE queue = ?1", [queue.as_str()], |r| r.get(0))?;
        match first {
            Some(first) => {
                self.conn.execute(
                    "INSERT INTO asked (queue, job, at) VALUES (?1, ?2, ?3)
                     ON CONFLICT (queue, job) DO UPDATE SET at = excluded.at",
                    params![queue.as_str(), job, first - 1],
                )?;
                Ok(())
            }
            None => self.ask(queue, job),
        }
    }

    /// `jobs` of `queue`, those waiting, are in this order now (moved in
    /// the list of jobs): the places they were asked at, earliest first, go
    /// to them in it. Given the whole line as it is, those `Jobs::reorder`
    /// left in their places keep theirs, and whichever of two moves is
    /// written last writes the line as it is then. A job with no place
    /// keeps none.
    pub fn reorder_asked(&self, queue: Queue, jobs: &[i64]) -> Result<()> {
        self.atomically(|db| {
            let mut placed = Vec::new();
            for &job in jobs {
                if let Some(at) = db.asked_at(queue, job)? {
                    placed.push((job, at));
                }
            }
            let mut places: Vec<i64> = placed.iter().map(|&(_, at)| at).collect();
            places.sort_unstable();
            for ((job, _), at) in placed.into_iter().zip(places) {
                db.conn.execute(
                    "UPDATE asked SET at = ?3 WHERE queue = ?1 AND job = ?2",
                    params![queue.as_str(), job, at],
                )?;
            }
            Ok(())
        })
    }

    fn asked_at(&self, queue: Queue, job: i64) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT at FROM asked WHERE queue = ?1 AND job = ?2", params![queue.as_str(), job], |r| r.get(0))
            .optional()?)
    }

    /// Whether job `job` of `queue` has a place in its line.
    pub fn is_asked(&self, queue: Queue, job: i64) -> Result<bool> {
        Ok(self.asked_at(queue, job)?.is_some())
    }

    /// The jobs of `queue` with a place in its line, in its order.
    pub fn asked(&self, queue: Queue) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT job FROM asked WHERE queue = ?1 ORDER BY at, job")?;
        Ok(stmt.query_map([queue.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Job `job` of `queue` is no longer asked for (the add-on canceled).
    pub fn forget_asked(&self, queue: Queue, job: i64) -> Result<()> {
        self.conn.execute("DELETE FROM asked WHERE queue = ?1 AND job = ?2", params![queue.as_str(), job])?;
        Ok(())
    }

    /// `jobs`, the unfinished work of `queue` as its own state has it at a
    /// start, in the order they were asked; those with no place (from
    /// before this table, or asked by an older version) after, in the order
    /// given. At once, in one transaction: the queue's rows of other jobs go
    /// (ended, or gone), and those with no place get one, in that order. A
    /// job given twice is the first.
    pub fn in_asked_order(&self, queue: Queue, jobs: Vec<i64>) -> Result<Vec<i64>> {
        self.in_asked_order_by(queue, jobs, |&job| job)
    }

    /// `in_asked_order` for jobs that carry more than their id (`key`), as
    /// a queue's start takes them up: out of order, if the library can't
    /// say (logged), is no reason not to carry them on, in the order given.
    pub fn ordered_as_asked<T>(&self, queue: Queue, jobs: Vec<T>, key: impl Fn(&T) -> i64) -> Vec<T> {
        let ids = jobs.iter().map(&key).collect();
        match self.in_asked_order(queue, ids) {
            Ok(order) => {
                let mut left: Vec<Option<T>> = jobs.into_iter().map(Some).collect();
                let take = |id: i64| {
                    let at = left.iter().position(|j| j.as_ref().is_some_and(|j| key(j) == id))?;
                    left[at].take()
                };
                order.into_iter().filter_map(take).collect()
            }
            Err(e) => {
                eprintln!("the order {queue:?}'s unfinished jobs were asked in: {e:#}");
                jobs
            }
        }
    }

    fn in_asked_order_by<T>(&self, queue: Queue, jobs: Vec<T>, key: impl Fn(&T) -> i64) -> Result<Vec<T>> {
        self.atomically(|db| {
            let mut seen = std::collections::HashSet::new();
            let mut placed = Vec::new();
            let mut unplaced = Vec::new();
            for job in jobs {
                let id = key(&job);
                if !seen.insert(id) {
                    continue;
                }
                match db.asked_at(queue, id)? {
                    Some(at) => placed.push((at, job)),
                    None => unplaced.push(job),
                }
            }
            // Stable: two at one place (an older row) keep the order given.
            placed.sort_by_key(|&(at, _)| at);
            let kept: std::collections::HashSet<i64> =
                placed.iter().map(|(_, j)| key(j)).chain(unplaced.iter().map(&key)).collect();
            let mut stmt = db.conn.prepare("SELECT job FROM asked WHERE queue = ?1")?;
            let rows: Vec<i64> = stmt.query_map([queue.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            for job in rows.into_iter().filter(|job| !kept.contains(job)) {
                db.forget_asked(queue, job)?;
            }
            for job in &unplaced {
                db.ask(queue, key(job))?;
            }
            Ok(placed.into_iter().map(|(_, j)| j).chain(unplaced).collect())
        })
    }
}

#[cfg(test)]
mod tests;
