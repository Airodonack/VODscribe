//! The search's own read connection, apart from the one the views read with
//! (a transcript, chat, a VOD's stretches, its clips), so they never wait
//! behind a search: a wide window over common words takes seconds. Searches
//! typed into the box come one per pause in the typing, and only the newest
//! is wanted: a typed search overtaken by a newer one while it waits for the
//! connection doesn't run, and one overtaken while it runs is cut short.
//! Without that, each pause queued another search of seconds, all on the
//! connection the player and the library read with too.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::db::Db;

/// How many SQLite steps between looks at whether a newer search came.
const STEPS: i32 = 1000;

pub struct Searcher {
    db: Mutex<Db>,
    /// The newest typed search, by number.
    newest: Arc<AtomicU64>,
}

impl Searcher {
    pub fn new(db: Db) -> Searcher {
        Searcher { db: Mutex::new(db), newest: Arc::default() }
    }

    /// Runs `f` with the search's connection. One `typed` into the search
    /// box gives way to the next typed one: Ok(None) if that came before it
    /// began, or while it ran. Others (a test's, say) always run whole.
    pub fn run<T>(&self, typed: bool, f: impl FnOnce(&Db) -> Result<T>) -> Result<Option<T>> {
        // Numbered as it's asked, before the wait for the connection.
        let mine = typed.then(|| self.newest.fetch_add(1, Ordering::SeqCst) + 1);
        let db = self.db.lock().unwrap();
        let Some(mine) = mine else { return f(&db).map(Some) };
        let newest = self.newest.clone();
        let overtaken = move || newest.load(Ordering::SeqCst) != mine;
        if overtaken() {
            return Ok(None);
        }
        db.conn.progress_handler(STEPS, Some(overtaken.clone()))?;
        let result = f(&db);
        db.conn.progress_handler(0, None::<fn() -> bool>)?;
        match result {
            Ok(found) => Ok(Some(found)),
            // Interrupted by the handler, or failed on its way out: the newer one's answer counts.
            Err(_) if overtaken() => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// SQL that runs for a long while (seconds), unless interrupted.
    fn long(db: &Db) -> Result<i64> {
        Ok(db.conn.query_row(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 200000000) SELECT count(*) FROM n",
            [],
            |r| r.get(0),
        )?)
    }

    fn searcher() -> Arc<Searcher> {
        Arc::new(Searcher::new(Db::open(std::path::Path::new(":memory:")).unwrap()))
    }

    /// Typed while the one before still ran: that one stops, and the new one runs.
    #[test]
    fn a_typed_search_cuts_short_the_one_running() {
        let s = searcher();
        let (began, running) = mpsc::channel();
        let first = {
            let s = s.clone();
            std::thread::spawn(move || {
                let t = Instant::now();
                let r = s.run(true, |db| {
                    began.send(()).unwrap();
                    long(db)
                });
                (r.unwrap(), t.elapsed())
            })
        };
        // The first is on the connection, running, before the second is typed.
        running.recv().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(s.run(true, |_| Ok(7)).unwrap(), Some(7), "the newest runs");
        let (r, took) = first.join().unwrap();
        assert_eq!(r, None, "the one overtaken has no answer");
        assert!(took < Duration::from_secs(2), "it stopped when overtaken, not after {took:?}");
    }

    /// Three typed while the connection is busy: the middle one, overtaken
    /// while it waited, never runs; the newest does.
    #[test]
    fn a_typed_search_overtaken_while_waiting_never_runs() {
        let s = searcher();
        // An untyped search holds the connection till told: not cut short by those typed.
        let (go, hold) = mpsc::channel::<()>();
        let (held, holding) = mpsc::channel();
        let busy = {
            let s = s.clone();
            std::thread::spawn(move || {
                s.run(false, |_| {
                    held.send(()).unwrap();
                    hold.recv().unwrap();
                    Ok("untyped")
                })
                .unwrap()
            })
        };
        holding.recv().unwrap();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let typed = |n: u32| {
            let (s, ran) = (s.clone(), ran.clone());
            std::thread::spawn(move || {
                s.run(true, |_| {
                    ran.lock().unwrap().push(n);
                    Ok(n)
                })
                .unwrap()
            })
        };
        let middle = typed(1);
        // Numbered before the newest is (it waits on the connection then).
        while s.newest.load(Ordering::SeqCst) < 1 {
            std::thread::yield_now();
        }
        let newest = typed(2);
        while s.newest.load(Ordering::SeqCst) < 2 {
            std::thread::yield_now();
        }
        go.send(()).unwrap();
        assert_eq!(busy.join().unwrap(), Some("untyped"), "an untyped search runs whole");
        assert_eq!(middle.join().unwrap(), None);
        assert_eq!(newest.join().unwrap(), Some(2));
        assert_eq!(*ran.lock().unwrap(), [2], "only the newest ran");
    }
}
