//! A fake transcription worker that answers as the worker does
//! (`pipeline`'s `WorkerLine`s): the words the test says are spoken in the
//! file it's given, from `--start` to `--end`, in batches of a minute. It
//! fails as the worker does when the file isn't there, and on request
//! partway; or holds partway at a `Gate`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::media::Media;
use super::tools::{self, Fake, Gate, Run};
use crate::transcript::Word;

/// The fake worker at a path, faked until it's dropped.
pub struct Worker {
    fake: Fake,
    plan: Arc<Mutex<Plan>>,
}

#[derive(Clone, Default)]
struct Plan {
    /// What's said in every file, in the file's own seconds.
    words: Vec<Word>,
    /// Fails once it reaches this second, saying this.
    fail: Option<(f64, String)>,
    /// Waits at the gate after its first batch.
    hold: Option<Gate>,
}

impl Worker {
    /// Fakes the worker at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        let plan = Arc::new(Mutex::new(Plan::default()));
        let planned = plan.clone();
        let fake = tools::fake(path, move |run| {
            let plan = planned.lock().unwrap().clone();
            answer(run, &plan)
        });
        Worker { fake, plan }
    }

    /// Ends its runs, now and to come (`Fake::end`).
    pub fn end(&self) {
        self.fake.end();
    }

    pub fn path(&self) -> &Path {
        self.fake.path()
    }

    /// From now every file it's given says `words` (`said`).
    pub fn hears(&self, words: Vec<Word>) {
        self.plan.lock().unwrap().words = words;
    }

    /// From now it fails once it reaches second `at`, with `message` as the
    /// last of its errors.
    pub fn fails_at(&self, at: f64, message: &str) {
        self.plan.lock().unwrap().fail = Some((at, message.into()));
    }

    /// From now it waits at `gate` after its first batch.
    pub fn holds(&self, gate: &Gate) {
        self.plan.lock().unwrap().hold = Some(gate.clone());
    }
}

/// How much audio each batch covers, in seconds.
const BATCH: f64 = 60.0;

fn answer(run: &mut Run, plan: &Plan) -> i32 {
    let file = run.args().last().cloned().unwrap_or_default();
    let Some(media) = Media::read(Path::new(&file)) else {
        run.err(&format!("Error: couldn't read {file}: No such file or directory\n"));
        return 1;
    };
    let start = run.after("--start").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let end = run.after("--end").and_then(|s| s.parse::<f64>().ok()).unwrap_or(media.duration).min(media.duration);
    let mut at = start;
    let mut first = true;
    while at < end {
        let to = (at + BATCH).min(end);
        if let Some((fail, message)) = &plan.fail
            && *fail < to
        {
            run.err(&format!("{message}\n"));
            return 1;
        }
        let words: Vec<&Word> = plan.words.iter().filter(|w| w.start >= at && w.start < to).collect();
        let line = serde_json::json!({ "end": to, "words": words });
        run.out(&format!("{line}\n"));
        if std::mem::take(&mut first)
            && let Some(gate) = &plan.hold
            && !run.hold(gate)
        {
            return 255;
        }
        if run.stopped() {
            return 255;
        }
        at = to;
    }
    0
}
