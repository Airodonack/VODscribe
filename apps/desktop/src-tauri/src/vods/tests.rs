//! The specs of `vods.rs`: the files in `tests/` that hold them, and what
//! they share.

use serde_json::{Value, json};
use vodscribe_core::testkit::{Gate, eventually, said};

use super::*;
use crate::testkit::TestApp;

mod adding;
mod list;
mod reading;
mod removing;
mod transcribing;

/// The app with the transcription worker it runs faked and its model
/// installed (`TestApp::worker`), so its queue transcribes what's asked:
/// each file says what `hears` says.
fn transcribing() -> TestApp {
    let app = TestApp::new();
    app.worker();
    app
}

/// What's said, as (second, text): `(10.0, "The Eagle has landed.")`.
type Said<'a> = &'a [(f64, &'a str)];

/// A VOD's transcription as the page asks for it and sees it, through the
/// commands: what the transcribing and removing specs share.
trait Transcriptions {
    /// From now every file the worker is given says `words`, at the VOD's
    /// own seconds.
    fn hears(&self, words: Said);
    /// A file on this computer, `minutes` long, added; it says what the
    /// worker hears.
    fn file(&self, name: &str, minutes: f64) -> i64;
    /// Transcribe pressed for `start`..`end` of VOD `vod` (`end` None: to
    /// its end), as the page asks (api.ts `transcribeStretch`): the request
    /// the command answers.
    fn ask(&self, vod: i64, start: f64, end: Option<f64>) -> Result<Value, Value>;
    /// `ask`, answered: the request's id.
    fn asked(&self, vod: i64, start: f64, end: Option<f64>) -> i64;
    /// Command `cmd` (pause_vod, cancel_vod, cancel_stretch, …) on `id`, as
    /// the page calls it.
    fn on(&self, cmd: &str, id: i64);
    /// VOD `id` as the library has it.
    fn vod(&self, id: i64) -> Vod;
    /// VOD `id` once nothing of it waits or runs: transcribed, paused,
    /// stopped or failed.
    fn settled(&self, id: i64) -> Vod;
    /// VOD `id`'s lines, as (start, text).
    fn lines(&self, id: i64) -> Vec<(f64, String)>;
    /// VOD `id`'s stretches as the transcript gets them (`transcript_stretches`):
    /// (start, end, status).
    fn stretches(&self, id: i64) -> Vec<(f64, Option<f64>, Status)>;
    /// VOD `id`'s requests, in the order asked.
    fn requests(&self, id: i64) -> Vec<Stretch>;
    /// Waits till the worker transcribing VOD `id` is held at `gate`, its
    /// first minute's lines written.
    fn held(&self, id: i64, gate: &Gate);
    /// Waits till request `id` has its audio and waits for the transcriber,
    /// with no stage working on it.
    fn ready(&self, id: i64);
    /// `start`..`end` of VOD `vod` asked for while another of its requests
    /// is held, and, once it has its audio, waiting for the transcriber
    /// (`ready`).
    fn waiting(&self, vod: i64, start: f64, end: f64);
    /// The request of VOD `id` being transcribed, if one is.
    fn transcribing(&self, id: i64) -> bool;
}

impl Transcriptions for TestApp {
    fn hears(&self, words: Said) {
        self.worker().hears(words.iter().flat_map(|(at, text)| said(*at, text)).collect());
    }

    fn file(&self, name: &str, minutes: f64) -> i64 {
        self.add_video_file(name, minutes * 60.0).id
    }

    fn ask(&self, vod: i64, start: f64, end: Option<f64>) -> Result<Value, Value> {
        self.invoke("transcribe_stretch", json!({ "vodId": vod, "start": start, "end": end }))
    }

    fn asked(&self, vod: i64, start: f64, end: Option<f64>) -> i64 {
        self.ask(vod, start, end).unwrap()["id"].as_i64().unwrap()
    }

    fn on(&self, cmd: &str, id: i64) {
        self.invoke(cmd, json!({ "id": id })).unwrap();
    }

    fn vod(&self, id: i64) -> Vod {
        self.db().vod(id).unwrap().unwrap()
    }

    fn settled(&self, id: i64) -> Vod {
        eventually(&format!("VOD {id} settled"), || !busy(self.vod(id).status));
        self.vod(id)
    }

    fn lines(&self, id: i64) -> Vec<(f64, String)> {
        self.db().lines(id).unwrap().into_iter().map(|l| (l.start, l.text)).collect()
    }

    fn stretches(&self, id: i64) -> Vec<(f64, Option<f64>, Status)> {
        let listed = self.invoke("transcript_stretches", json!({ "vodId": id })).unwrap();
        listed
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["start"].as_f64().unwrap(), s["end"].as_f64(), status(&s["status"])))
            .collect()
    }

    fn requests(&self, id: i64) -> Vec<Stretch> {
        self.db().requests(id).unwrap()
    }

    fn held(&self, id: i64, gate: &Gate) {
        gate.reached();
        eventually("its first minute written", || self.requests(id).iter().any(|s| s.reached.is_some()));
    }

    fn ready(&self, id: i64) {
        eventually("it waits to be transcribed", || {
            // The download stage marks it ready a moment before it lets go.
            let state = self.state();
            let (_held, running) = state.worker.hold_starts();
            !running.contains(&id) && self.db().stretch(id).unwrap().unwrap().status == Status::Ready
        });
    }

    fn waiting(&self, vod: i64, start: f64, end: f64) {
        self.ready(self.asked(vod, start, Some(end)));
    }

    fn transcribing(&self, id: i64) -> bool {
        self.requests(id).iter().any(|s| s.status == Status::Transcribing)
    }
}

/// A stretch's status as the page has it ("done").
fn status(shown: &Value) -> Status {
    use Status::*;
    let all = [Queued, Downloading, Ready, Transcribing, Paused, Done, Error];
    all.into_iter().find(|s| json!(s) == *shown).unwrap_or_else(|| panic!("no status {shown}"))
}

/// What's said, as the lines it makes: (start, text).
fn lines(said: Said) -> Vec<(f64, String)> {
    said.iter().map(|(at, text)| (*at, text.to_string())).collect()
}

/// Runs `command` on a thread of its own, held where it takes the queue's
/// lock (`Worker::holds_next_control`): whether it held the library's lock
/// there already, and its answer once let go. The library's lock comes
/// first everywhere (worker.rs `control`), as an erasure takes them
/// (`erase_unused`): one taking the queue's first could wait for the
/// library while an erasure holding it waits for the queue, for good.
/// The caller's requests wait for the transcriber (`ready`), so nothing but
/// the command uses the library meanwhile; another's hold of a moment could
/// only hide the wrong order, never fail the right one.
fn took_the_library_first<T: Send + 'static>(
    app: &TestApp,
    command: impl FnOnce(AppHandle) -> T + Send + 'static,
) -> (bool, T) {
    app.threads_done();
    let gate = Gate::new();
    app.state().worker.holds_next_control(&gate);
    let handle = app.handle().clone();
    let running = std::thread::spawn(move || command(handle));
    gate.reached();
    // Held at the gate, the command holds what it took before the queue.
    let first = app.state().db.try_lock().is_err();
    gate.open();
    (first, running.join().unwrap())
}
