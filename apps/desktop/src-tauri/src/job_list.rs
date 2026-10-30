//! The list of jobs the library shows (Jobs.svelte): the clips' queue (clips
//! cut and edits rendered, one at a time: `clips::Cutter`), then the
//! erasures of removed VODs (`vods::Sweeper`), which run beside it, each with
//! how far it is. Sent whole as `jobs` whenever it changes, by a thread of
//! its own: the queues (`Jobs::watch`) and the progress of what's running
//! only send it word, so nothing that changes a queue while holding the
//! library's lock waits on the list's reads.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::Db;

use crate::{AppState, CmdResult};

/// What a job makes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    /// A clip's file (a snippet: cut, or rendered as an edit is once changed in the editor).
    Clip,
    /// A listed edit's video.
    Edit,
    /// A removed VOD's transcript, chat and files, erased.
    Erase,
}

/// A queue whose running job reports its progress.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Queue {
    /// The clips' (`clips::Cutter`): clip and edit ids.
    Render,
    /// The erasures' (`vods::Sweeper`): VOD ids.
    Erase,
}

/// A job as the page shows it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct Job {
    kind: Kind,
    /// The clip's or edit's id; the VOD's, for an erasure.
    id: i64,
    /// The clip's or edit's title; the VOD's, for an erasure.
    label: String,
    /// Begun: the one each queue is at.
    running: bool,
    /// How far it is, 0..1; None while that isn't known (waiting, or just begun).
    progress: Option<f64>,
}

pub(crate) struct JobList {
    word: Mutex<Option<Sender<()>>>,
    /// The progress of each queue's running job, by its id.
    rendering: Mutex<Option<Reported>>,
    erasing: Mutex<Option<Reported>>,
}

/// A running job's id and how far it is (NaN: not known yet).
type Reported = (i64, f64);

impl JobList {
    pub(crate) fn new() -> Self {
        JobList { word: Mutex::default(), rendering: Mutex::default(), erasing: Mutex::default() }
    }

    /// Starts the thread that sends the list as it changes.
    pub(crate) fn run(&self, app: AppHandle) -> anyhow::Result<()> {
        let (word, heard) = mpsc::channel();
        *self.word.lock().unwrap() = Some(word);
        std::thread::Builder::new().name("job-list".into()).spawn(move || send_changes(&app, heard))?;
        Ok(())
    }

    fn progress(&self, queue: Queue) -> &Mutex<Option<Reported>> {
        match queue {
            Queue::Render => &self.rendering,
            Queue::Erase => &self.erasing,
        }
    }
}

/// Word that the list may have changed: a queue's jobs, or which is running.
pub(crate) fn changed(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>()
        && let Some(word) = &*state.job_list.word.lock().unwrap()
    {
        let _ = word.send(());
    }
}

/// The job `id` running on `queue` has begun: no progress known yet.
pub(crate) fn begun(app: &AppHandle, queue: Queue, id: i64) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.job_list.progress(queue).lock().unwrap() = Some((id, f64::NAN));
    }
}

/// The job `id` running on the clips' queue is `progress` (0..1) along.
pub(crate) fn progressed(app: &AppHandle, id: i64, progress: f64) {
    progressed_on(app, Queue::Render, id, progress);
}

/// The job `id` running on `queue` is `progress` (0..1) along.
pub(crate) fn progressed_on(app: &AppHandle, queue: Queue, id: i64, progress: f64) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.job_list.progress(queue).lock().unwrap() = Some((id, progress));
        changed(app);
    }
}

/// Sends the list each time word comes that it may have changed, if it has.
fn send_changes(app: &AppHandle, heard: Receiver<()>) {
    let mut last = None;
    while heard.recv().is_ok() {
        // Word that came meanwhile is answered by this one list.
        while heard.try_recv().is_ok() {}
        let jobs = listed(app);
        if last.as_ref() != Some(&jobs) {
            let _ = app.emit("jobs", &jobs);
            last = Some(jobs);
        }
    }
}

/// The list as it is now.
fn listed(app: &AppHandle) -> Vec<Job> {
    let state = app.state::<AppState>();
    let render = state.clips.listed();
    let erase = state.sweeper.listed();
    let rendering = *state.job_list.rendering.lock().unwrap();
    let erasing = *state.job_list.erasing.lock().unwrap();
    // Read-only: never waits for the library's writes.
    let db = state.reads.lock().unwrap();
    arrange(render, erase, (rendering, erasing), |queue, id| describe(&db, queue, id))
}

/// What job `id` on `queue` makes and what it's called; None if it's gone.
fn describe(db: &Db, queue: Queue, id: i64) -> Option<(Kind, String)> {
    match queue {
        Queue::Render => match db.is_snippet(id).ok()?? {
            true => Some((Kind::Clip, db.clip(id).ok()??.title)),
            false => Some((Kind::Edit, db.edit(id).ok()??.title)),
        },
        Queue::Erase => Some((Kind::Erase, db.vod(id).ok()??.info.title)),
    }
}

/// Running and waiting ids on each queue (`Cutter::listed`, `Sweeper::listed`).
type Listed = (Option<i64>, Vec<i64>);

/// The list: the clips' queue, its running job then those waiting, then the
/// erasures the same way; each job once (one asked for again while it runs
/// is made again after: it's the same job to see), those `describe` doesn't
/// find left out (gone meanwhile). `progress` is each queue's running job's.
fn arrange(
    render: Listed,
    erase: Listed,
    progress: (Option<Reported>, Option<Reported>),
    describe: impl Fn(Queue, i64) -> Option<(Kind, String)>,
) -> Vec<Job> {
    let mut jobs: Vec<Job> = Vec::new();
    for (queue, (running, waiting), progress) in
        [(Queue::Render, render, progress.0), (Queue::Erase, erase, progress.1)]
    {
        let all = running.map(|id| (id, true)).into_iter().chain(waiting.into_iter().map(|id| (id, false)));
        let mut seen = Vec::new();
        for (id, running) in all {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let Some((kind, label)) = describe(queue, id) else { continue };
            let progress =
                progress.filter(|&(of, p)| running && of == id && p.is_finite()).map(|(_, p)| p.clamp(0.0, 1.0));
            jobs.push(Job { kind, id, label, running, progress });
        }
    }
    jobs
}

/// The jobs as they are, for the page's first look (then `jobs` news).
#[tauri::command]
pub(crate) async fn list_jobs(app: AppHandle) -> CmdResult<Vec<Job>> {
    tauri::async_runtime::spawn_blocking(move || listed(&app)).await.map_err(crate::err)
}

/// Puts the clips and edits waiting in the order of `ids`, as dragged in the list.
#[tauri::command]
pub(crate) async fn reorder_jobs(app: AppHandle, ids: Vec<i64>) -> CmdResult<()> {
    app.state::<AppState>().clips.reorder(&ids);
    Ok(())
}

/// Cancels a job from the list: a clip or edit is taken out of the queue or
/// stopped (`Cutter::cancel_job`); an erasure is stopped, what's left of the
/// VOD erased at the next start (`Sweeper::cancel`).
#[tauri::command]
pub(crate) async fn cancel_job(app: AppHandle, kind: Kind, id: i64) -> CmdResult<()> {
    let state = app.state::<AppState>();
    match kind {
        Kind::Clip | Kind::Edit => state.clips.cancel_job(&app, id),
        Kind::Erase => state.sweeper.cancel(id),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(queue: Queue, id: i64) -> Option<(Kind, String)> {
        match (queue, id) {
            (_, 99) => None,
            (Queue::Render, id) if id < 10 => Some((Kind::Clip, format!("clip {id}"))),
            (Queue::Render, id) => Some((Kind::Edit, format!("edit {id}"))),
            (Queue::Erase, id) => Some((Kind::Erase, format!("VOD {id}"))),
        }
    }

    fn seen(jobs: &[Job]) -> Vec<(Kind, i64, bool, Option<f64>)> {
        jobs.iter().map(|j| (j.kind, j.id, j.running, j.progress)).collect()
    }

    #[test]
    fn renders_come_first_running_then_waiting_then_erasures() {
        let jobs = arrange((Some(1), vec![12, 2, 99, 1]), (Some(5), vec![6]), (Some((1, 0.25)), Some((5, 1.5))), named);
        assert_eq!(
            seen(&jobs),
            [
                (Kind::Clip, 1, true, Some(0.25)),
                (Kind::Edit, 12, false, None),
                (Kind::Clip, 2, false, None),
                (Kind::Erase, 5, true, Some(1.0)),
                (Kind::Erase, 6, false, None),
            ],
            "99 gone; 1 asked for again while it runs is seen once"
        );
        assert_eq!(jobs[1].label, "edit 12");
        assert_eq!(jobs[3].label, "VOD 5");
    }

    #[test]
    fn progress_is_the_running_jobs_own_and_none_till_it_reports() {
        // Progress left from the job before; then the new one begun, not yet reported.
        let jobs = arrange((Some(2), vec![]), (None, vec![]), (Some((1, 0.9)), None), named);
        assert_eq!(seen(&jobs), [(Kind::Clip, 2, true, None)]);
        let jobs = arrange((Some(2), vec![]), (None, vec![]), (Some((2, f64::NAN)), None), named);
        assert_eq!(seen(&jobs), [(Kind::Clip, 2, true, None)]);
        assert!(arrange((None, vec![]), (None, vec![]), (None, None), named).is_empty());
    }

    #[test]
    fn a_job_is_sent_as_the_page_reads_it() {
        let job = Job { kind: Kind::Erase, id: 3, label: "A stream".into(), running: true, progress: None };
        assert_eq!(
            serde_json::to_value(&job).unwrap(),
            serde_json::json!({ "kind": "erase", "id": 3, "label": "A stream", "running": true, "progress": null })
        );
        let kind: Kind = serde_json::from_str("\"clip\"").unwrap();
        assert_eq!(kind, Kind::Clip);
    }
}
