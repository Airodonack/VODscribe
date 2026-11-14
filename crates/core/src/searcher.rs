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
    /// Where a test holds each search, asked and not yet on the connection (`holds`).
    #[cfg(any(test, feature = "testkit"))]
    hold: Mutex<Option<crate::testkit::Gate>>,
}

impl Searcher {
    pub fn new(db: Db) -> Searcher {
        Searcher {
            db: Mutex::new(db),
            newest: Arc::default(),
            #[cfg(any(test, feature = "testkit"))]
            hold: Mutex::default(),
        }
    }

    /// Each search from now waits at `gate` once it's asked (a typed one
    /// numbered), before it has the connection, as one does behind a
    /// search of seconds: for a test to type another meanwhile.
    #[cfg(any(test, feature = "testkit"))]
    pub fn holds(&self, gate: &crate::testkit::Gate) {
        *self.hold.lock().unwrap() = Some(gate.clone());
    }

    /// Lets go of the searches held (`holds`), and holds no more: a test's end.
    #[cfg(any(test, feature = "testkit"))]
    pub fn lets_go(&self) {
        if let Some(gate) = self.hold.lock().unwrap().take() {
            gate.open();
        }
    }

    /// Runs `f` with the search's connection. One `typed` into the search
    /// box gives way to the next typed one: Ok(None) if that came before it
    /// began, or while it ran. Others (a test's, say) always run whole.
    pub fn run<T>(&self, typed: bool, f: impl FnOnce(&Db) -> Result<T>) -> Result<Option<T>> {
        // Numbered as it's asked, before the wait for the connection.
        let mine = typed.then(|| self.newest.fetch_add(1, Ordering::SeqCst) + 1);
        #[cfg(any(test, feature = "testkit"))]
        {
            let held = self.hold.lock().unwrap().clone();
            if let Some(gate) = held {
                gate.wait();
            }
        }
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
