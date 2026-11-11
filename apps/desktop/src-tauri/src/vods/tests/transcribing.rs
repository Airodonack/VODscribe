//! Asking for a VOD's transcription: the commands' promises (handbook
//! plans/sections-074.md, 2A4), each through the command as the page calls
//! it, on the app's own queue, with the transcription worker faked. The
//! queue's own specs (its two stages, the worker process) are 4.1's.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vodscribe_core::fetch::VodInfo;
use vodscribe_core::stretches::Transcribed;
use vodscribe_core::testkit::{Gate, eventually, said};

use super::*;
use crate::testkit::TestApp;

/// The app with the transcription worker it runs faked and its model
/// installed (`TestApp::worker`), so its queue transcribes what's asked:
/// each file says what `hears` says.
struct Transcribing {
    app: TestApp,
}

impl std::ops::Deref for Transcribing {
    type Target = TestApp;
    fn deref(&self) -> &TestApp {
        &self.app
    }
}

fn transcribing() -> Transcribing {
    let app = TestApp::new();
    app.worker();
    Transcribing { app }
}

/// What's said, as (second, text): `(10.0, "The Eagle has landed.")`.
type Said<'a> = &'a [(f64, &'a str)];

impl Transcribing {
    /// From now every file the worker is given says `words`, at the VOD's
    /// own seconds.
    fn hears(&self, words: Said) {
        self.worker().hears(words.iter().flat_map(|(at, text)| said(*at, text)).collect());
    }

    /// A file on this computer, `minutes` long, added; it says what the
    /// worker hears.
    fn file(&self, name: &str, minutes: f64) -> i64 {
        self.add_video_file(name, minutes * 60.0).id
    }

    /// Transcribe pressed for `start`..`end` of VOD `vod` (`end` None: to
    /// its end), as the page asks (api.ts `transcribeStretch`): the request
    /// the command answers.
    fn ask(&self, vod: i64, start: f64, end: Option<f64>) -> Result<Value, Value> {
        self.invoke("transcribe_stretch", json!({ "vodId": vod, "start": start, "end": end }))
    }

    /// Command `cmd` (pause_vod, cancel_vod, cancel_stretch, …) on `id`, as
    /// the page calls it.
    fn on(&self, cmd: &str, id: i64) {
        self.invoke(cmd, json!({ "id": id })).unwrap();
    }

    /// VOD `id` as the library has it.
    fn vod(&self, id: i64) -> Vod {
        self.db().vod(id).unwrap().unwrap()
    }

    /// VOD `id` once nothing of it waits or runs: transcribed, paused,
    /// stopped or failed.
    fn settled(&self, id: i64) -> Vod {
        eventually(&format!("VOD {id} settled"), || !busy(self.vod(id).status));
        self.vod(id)
    }

    /// VOD `id`'s lines, as (start, text).
    fn lines(&self, id: i64) -> Vec<(f64, String)> {
        self.db().lines(id).unwrap().into_iter().map(|l| (l.start, l.text)).collect()
    }

    /// VOD `id`'s stretches as the transcript gets them (`transcript_stretches`):
    /// (start, end, status).
    fn stretches(&self, id: i64) -> Vec<(f64, Option<f64>, Status)> {
        let listed = self.invoke("transcript_stretches", json!({ "vodId": id })).unwrap();
        listed
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["start"].as_f64().unwrap(), s["end"].as_f64(), status(&s["status"])))
            .collect()
    }

    /// VOD `id`'s requests, in the order asked.
    fn requests(&self, id: i64) -> Vec<Stretch> {
        self.db().requests(id).unwrap()
    }

    /// Waits till the worker transcribing VOD `id` is held at `gate`, its
    /// first minute's lines written.
    fn held(&self, id: i64, gate: &Gate) {
        gate.reached();
        eventually("its first minute written", || self.requests(id).iter().any(|s| s.reached.is_some()));
    }

    /// `start`..`end` of VOD `vod` asked for while another of its requests
    /// is held, and, once it has its audio, waiting for the transcriber: no
    /// stage is working on it.
    fn waiting(&self, vod: i64, start: f64, end: f64) {
        let id = self.ask(vod, start, Some(end)).unwrap()["id"].as_i64().unwrap();
        eventually("it waits to be transcribed", || {
            // The download stage marks it ready a moment before it lets go.
            let state = self.state();
            let (_held, running) = state.worker.hold_starts();
            !running.contains(&id) && self.db().stretch(id).unwrap().unwrap().status == Status::Ready
        });
    }

    /// The request of VOD `id` being transcribed, if one is.
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

/// A fresh gate, open: for the worker to hold at from now, so it holds no more.
fn open_gate() -> Gate {
    let gate = Gate::new();
    gate.open();
    gate
}

fn lines(said: Said) -> Vec<(f64, String)> {
    said.iter().map(|(at, text)| (*at, text.to_string())).collect()
}

/// 0.6.0: "Adding a VOD doesn't transcribe it."; e2e vod-actions.mjs's
/// header: "Transcribe (an added VOD isn't)". The queue has run another
/// VOD's request meanwhile, so it had the chance.
#[test]
fn nothing_is_transcribed_till_asked() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed.")]);
    let added = app.file("Day 3.mp4", 2.0);
    let other = app.file("Day 4.mp4", 2.0);
    app.ask(other, 0.0, None).unwrap();
    assert_eq!(app.settled(other).transcribed, Transcribed::All);
    let vod = app.vod(added);
    assert_eq!((vod.status, vod.transcribed), (Status::None, Transcribed::None));
    assert_eq!(app.stretches(added), []);
    assert_eq!(app.lines(added), []);
}

/// vods.rs:867, `transcribe_stretch`: "Asks for `start`..`end` seconds of
/// VOD `vod_id` … to be transcribed"; e2e transcribe-stretch.mjs's header:
/// "2:00–3:00 asked for has lines there and nowhere else".
#[test]
fn a_stretch_asked_for_has_lines_there_and_nowhere_else() {
    let app = transcribing();
    app.hears(&[(30.0, "Said before it."), (150.0, "Said inside it."), (400.0, "Said after it.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    app.ask(vod, 120.0, Some(180.0)).unwrap();
    let done = app.settled(vod);
    assert_eq!(app.lines(vod), lines(&[(150.0, "Said inside it.")]));
    assert_eq!(app.stretches(vod), [(120.0, Some(180.0), Status::Done)]);
    assert_eq!(done.transcribed, Transcribed::Some);
}

/// vods.rs:868: "Its lines replace those there as they come"; e2e
/// transcribe-stretch.mjs's header: "2:30–4:00 after it replaces the lines
/// it covers rather than doubling them, and the two make one stretch,
/// 2:00–4:00".
#[test]
fn a_stretch_asked_for_over_lines_replaces_them_and_the_two_make_one_stretch() {
    let app = transcribing();
    app.hears(&[(160.0, "Said just once."), (200.0, "Said later on.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    app.ask(vod, 120.0, Some(180.0)).unwrap();
    app.settled(vod);
    app.ask(vod, 150.0, Some(240.0)).unwrap();
    app.settled(vod);
    assert_eq!(app.lines(vod), lines(&[(160.0, "Said just once."), (200.0, "Said later on.")]));
    assert_eq!(app.stretches(vod), [(120.0, Some(240.0), Status::Done)]);
}

/// vods.rs:868: "to be transcribed, after the requests already made". A
/// stretch of the VOD being transcribed, asked for after another VOD's,
/// waits for that one.
#[test]
fn a_stretch_is_asked_for_after_the_requests_already_made() {
    let app = transcribing();
    let first = app.file("Day 3.mp4", 10.0);
    let other = app.file("Day 4.mp4", 2.0);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.ask(first, 0.0, Some(180.0)).unwrap();
    app.held(first, &gate);
    app.ask(other, 0.0, None).unwrap();
    app.ask(first, 300.0, Some(360.0)).unwrap();
    let next = Gate::new();
    app.worker().holds(&next);
    gate.open();
    app.held(other, &next);
    assert!(app.transcribing(other), "the other VOD's request, asked for first, is next");
    assert!(!app.transcribing(first));
    app.worker().holds(&open_gate());
    next.open();
    app.settled(first);
    assert_eq!(app.stretches(first), [(0.0, Some(180.0), Status::Done), (300.0, Some(360.0), Status::Done)]);
}

/// vods.rs:887, `check_stretch`: "Why `start`..`end` … can't be
/// transcribed": one that's empty, or past the VOD's end, isn't asked for.
#[test]
fn a_stretch_outside_the_vod_or_of_nothing_is_refused() {
    let app = transcribing();
    let vod = app.file("Ten minutes.mp4", 10.0);
    let refused = [(600.0, None), (700.0, Some(800.0)), (200.0, Some(100.0)), (200.0, Some(200.0)), (-1.0, None)];
    for (start, end) in refused {
        let answer = app.ask(vod, start, end).unwrap_err();
        assert!(answer.as_str().is_some_and(|a| a.starts_with("That stretch is")), "{start}..{end:?}: {answer}");
    }
    let nan = app.run(transcribe_stretch(app.handle().clone(), app.state(), vod, f64::NAN, None));
    assert_eq!(nan.unwrap_err().to_string(), "That stretch is empty.");
    assert_eq!(app.stretches(vod), [], "nothing asked for");
    app.ask(vod, 100.0, Some(200.0)).unwrap();
    app.ask(vod, 599.0, None).unwrap();
}

/// The test of `check_stretch` this one moves up (vods/tests/transcribing.rs
/// at d9073e0), its comment: "A length its site or ffmpeg gave as 0
/// isn't known: all of it can be asked for"; vods.rs:893: "A length of 0
/// (as a site or ffmpeg may give one it doesn't know) isn't one". A site's
/// VOD that gave no length at all is the same.
#[test]
fn a_stretch_of_a_vod_whose_length_is_not_known_can_be_asked_for() {
    let app = transcribing();
    let zero = app.file("Live.mp4", 0.0);
    app.ask(zero, 0.0, None).unwrap();
    app.ask(zero, 100.0, Some(200.0)).unwrap();
    let info = VodInfo {
        platform: "youtube".into(),
        platform_id: "unknown-length".into(),
        url: "https://www.youtube.com/watch?v=unknown-length".into(),
        title: "Still live".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: None,
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let unknown = app.db().add_vod(&info).unwrap().0.id;
    let asked = app.ask(unknown, 3600.0, None).unwrap();
    assert_eq!((asked["start"].as_f64(), asked["end"].as_f64()), (Some(3600.0), None));
}

/// vods.rs:867: "(`end` None: to its end)".
#[test]
fn to_the_end_is_asked_for_with_no_end() {
    let app = transcribing();
    app.hears(&[(100.0, "Said before it."), (500.0, "Said near the end.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    let asked = app.ask(vod, 300.0, None).unwrap();
    assert_eq!(asked["end"], Value::Null);
    app.settled(vod);
    assert_eq!(app.lines(vod), lines(&[(500.0, "Said near the end.")]));
    assert_eq!(app.stretches(vod), [(300.0, None, Status::Done)]);
}

/// VOD `vod`, ten minutes, all of it asked for, saying a line in its first
/// minute and one at 5:00; the worker held at `gate` once it has written
/// the first minute.
fn held_from_start(app: &Transcribing, vod: i64, gate: &Gate) -> i64 {
    app.hears(&[(10.0, "The Eagle has landed."), (300.0, "Said later on.")]);
    app.worker().holds(gate);
    app.ask(vod, 0.0, None).unwrap();
    app.held(vod, gate);
    vod
}

/// `held_from_start`, of a site's VOD, its whole audio downloaded for it.
fn held_site_vod(app: &Transcribing, gate: &Gate) -> i64 {
    held_from_start(app, app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 600.0).id, gate)
}

/// The whole audio downloaded for VOD `vod`, if it's there.
fn downloaded(app: &TestApp, vod: i64) -> Option<PathBuf> {
    app.db().audio_path(vod).unwrap().map(PathBuf::from).filter(|p| p.is_file())
}

/// vods.rs:808, `pause_vod`: "Stops a VOD's requests, keeping their
/// partial downloads and transcripts."; e2e vod-actions.mjs's header:
/// "Pause while it transcribes (it stands)". The one running and the one
/// waiting both stop. Moves up worker.rs's
/// `a_pause_keeps_the_request_and_its_work`.
#[test]
fn a_pause_stops_the_requests_and_keeps_the_download_and_lines_so_far() {
    let app = transcribing();
    let gate = Gate::new();
    let vod = held_site_vod(&app, &gate);
    app.waiting(vod, 300.0, 360.0);
    app.on("pause_vod", vod);
    let paused = app.settled(vod);
    assert_eq!(paused.status, Status::Paused);
    let requests: Vec<_> = app.requests(vod).into_iter().map(|s| (s.start, s.end, s.status)).collect();
    assert_eq!(requests, [(0.0, None, Status::Paused), (300.0, Some(360.0), Status::Paused)], "they stand");
    assert_eq!(app.lines(vod), lines(&[(10.0, "The Eagle has landed.")]));
    assert!(downloaded(&app, vod).is_some(), "its download is kept");
}

/// vods.rs:827, `resume_vod`; e2e vod-actions.mjs's header: "Resume (it
/// finishes)".
#[test]
fn resume_finishes_what_was_paused() {
    let app = transcribing();
    let gate = Gate::new();
    let vod = held_site_vod(&app, &gate);
    app.on("pause_vod", vod);
    app.settled(vod);
    gate.open();
    app.on("resume_vod", vod);
    let done = app.settled(vod);
    assert_eq!((done.status, done.transcribed), (Status::Done, Transcribed::All));
    assert_eq!(app.lines(vod), lines(&[(10.0, "The Eagle has landed."), (300.0, "Said later on.")]));
    assert_eq!(app.requests(vod), []);
}

/// vods.rs:838, `cancel_vod`: "Ends a VOD's requests, keeping the lines
/// they wrote"; 0.6.0 Fixes: "Stopping a transcription keeps its lines."
/// The one running and the one waiting both end.
/// Moves up worker.rs's `a_cancel_keeps_the_lines_so_far_and_lets_the_audio_go`.
#[test]
fn a_cancel_keeps_the_lines_so_far() {
    let app = transcribing();
    let gate = Gate::new();
    let vod = held_site_vod(&app, &gate);
    app.waiting(vod, 300.0, 360.0);
    app.on("cancel_vod", vod);
    let stopped = app.settled(vod);
    assert_eq!((stopped.status, stopped.transcribed, stopped.error), (Status::None, Transcribed::Some, None));
    assert_eq!(app.lines(vod), lines(&[(10.0, "The Eagle has landed.")]));
    assert_eq!(app.requests(vod), [], "nothing is asked any more");
    let [(start, _, done)] = app.stretches(vod).try_into().unwrap();
    assert_eq!((start, done), (0.0, Status::Done), "done as far as it reached");
}

/// e2e vod-actions.mjs's header: "Cancel at once (before a line, it's as
/// if never asked)". Held while it downloads, before any line. Moves up
/// worker.rs's `a_cancel_before_any_line_leaves_nothing_asked`.
#[test]
fn a_cancel_before_any_line_is_as_if_never_asked() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed.")]);
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 600.0).id;
    let gate = Gate::new();
    app.yt_dlp.holds(&gate);
    app.ask(vod, 0.0, None).unwrap();
    gate.reached();
    app.on("cancel_vod", vod);
    let stopped = app.settled(vod);
    assert_eq!((stopped.status, stopped.transcribed, stopped.error), (Status::None, Transcribed::None, None));
    assert_eq!(app.stretches(vod), []);
    assert_eq!(app.lines(vod), []);
}

/// vods.rs:839: "Retry then asks for the rest"; vods.rs:740, `requeue`:
/// "With no request at all and some of it not transcribed, asks for each
/// part that isn't."
#[test]
fn retry_after_a_cancel_asks_for_the_rest() {
    let app = transcribing();
    let gate = Gate::new();
    // A file, read where it is: the worker hears the rest in the VOD's own
    // seconds, where a section downloaded of a site's VOD has its own 0.
    let vod = held_from_start(&app, app.file("Ten minutes.mp4", 10.0), &gate);
    app.on("cancel_vod", vod);
    app.settled(vod);
    gate.open();
    app.on("retry_vod", vod);
    let done = app.settled(vod);
    assert_eq!((done.status, done.transcribed), (Status::Done, Transcribed::All));
    assert_eq!(app.lines(vod), lines(&[(10.0, "The Eagle has landed."), (300.0, "Said later on.")]));
    assert_eq!(app.stretches(vod), [(0.0, None, Status::Done)]);
}

/// vods.rs:740, `requeue` (Retry's): "Puts VOD `id`'s requests in one of
/// `which` (paused, failed) back in their queues, at whichever stage each
/// needs next": a paused one whose audio is downloaded waits to be
/// transcribed, a failed download waits to download again.
#[test]
fn retry_puts_paused_and_failed_requests_back_at_the_stage_each_needs() {
    let app = transcribing();
    let gate = Gate::new();
    let paused = held_site_vod(&app, &gate);
    app.on("pause_vod", paused);
    app.settled(paused);
    let failed = app.add_site_vod("twitch", "2901646294", "Day 4", 600.0);
    app.yt_dlp.refuses(&failed.info.url);
    app.ask(failed.id, 0.0, None).unwrap();
    assert_eq!(app.settled(failed.id).status, Status::Error);
    // Nothing starts meanwhile, so each is seen where Retry put it.
    let state = app.state();
    let (_held, _) = state.worker.hold_starts();
    app.on("retry_vod", paused);
    app.on("retry_vod", failed.id);
    let [paused] = app.requests(paused).try_into().unwrap();
    let [failed] = app.requests(failed.id).try_into().unwrap();
    assert_eq!((paused.status, failed.status), (Status::Ready, Status::Queued));
    assert_eq!(failed.error, None);
}

/// vods.rs:855, `cancel_stretch`: "Ends request `id` alone (the
/// transcript's Stop), keeping the lines it wrote"; 0.6.0: "The transcript
/// marks untranscribed parts." ("… with Stop"). The VOD's other request goes on.
#[test]
fn the_transcripts_stop_ends_its_request_alone() {
    let app = transcribing();
    app.hears(&[(10.0, "In the first minute."), (100.0, "In the second minute."), (320.0, "In the other part.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    app.worker().holds(&gate);
    let stopped = app.ask(vod, 0.0, Some(180.0)).unwrap()["id"].as_i64().unwrap();
    app.held(vod, &gate);
    app.ask(vod, 300.0, Some(360.0)).unwrap();
    app.worker().holds(&open_gate());
    app.on("cancel_stretch", stopped);
    app.settled(vod);
    assert_eq!(app.lines(vod), lines(&[(10.0, "In the first minute."), (320.0, "In the other part.")]));
    let stretches = app.stretches(vod);
    assert_eq!(stretches.len(), 2, "{stretches:?}");
    assert_eq!((stretches[0].0, stretches[0].2), (0.0, Status::Done), "kept as far as it reached");
    assert!(stretches[0].1.is_some_and(|end| end < 100.0), "{stretches:?}");
    assert_eq!(stretches[1], (300.0, Some(360.0), Status::Done));
}

/// worker.rs's `a_cancel_of_a_stretch_already_done_changes_nothing`, its
/// comment: "Only requests are ended: a done stretch is the record of
/// what's transcribed"; worker.rs:359 (`cancel`): "A stretch already done
/// isn't a request: it's left as it is."
#[test]
fn a_cancel_of_a_stretch_already_done_changes_nothing() {
    let app = transcribing();
    app.hears(&[(150.0, "Said inside it.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    app.ask(vod, 120.0, Some(180.0)).unwrap();
    app.settled(vod);
    let done = app.db().stretches(vod).unwrap();
    app.on("cancel_stretch", done[0].id);
    assert_eq!(app.db().stretches(vod).unwrap(), done);
    assert_eq!(app.lines(vod), lines(&[(150.0, "Said inside it.")]));
}

/// vods.rs:772, `retranscribe_vod`: "a request for all of it, whose lines
/// replace the old ones as they come"; e2e vod-actions.mjs's header: "then
/// Transcribe again, after asking (it's transcribed anew)". Held after its
/// first minute, that minute's lines are the new ones and the rest the old.
#[test]
fn transcribe_again_replaces_the_lines_as_they_come() {
    let app = transcribing();
    app.hears(&[(10.0, "The old start."), (300.0, "The old later part.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    app.ask(vod, 0.0, None).unwrap();
    app.settled(vod);
    app.hears(&[(10.0, "The new start."), (300.0, "The new later part.")]);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.on("retranscribe_vod", vod);
    app.held(vod, &gate);
    assert_eq!(app.lines(vod), lines(&[(10.0, "The new start."), (300.0, "The old later part.")]));
    gate.open();
    let done = app.settled(vod);
    assert_eq!((done.status, done.transcribed), (Status::Done, Transcribed::All));
    assert_eq!(app.lines(vod), lines(&[(10.0, "The new start."), (300.0, "The new later part.")]));
    assert_eq!(app.stretches(vod), [(0.0, None, Status::Done)]);
}

/// vods.rs:773: "(a failed request it replaces goes)".
#[test]
fn transcribe_again_takes_the_place_of_a_failed_request() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    app.worker().fails_at(200.0, "Error: the model couldn't be read");
    app.ask(vod, 0.0, None).unwrap();
    let failed = app.settled(vod);
    assert_eq!(failed.status, Status::Error);
    // The worker works again.
    app.worker().works();
    app.on("retranscribe_vod", vod);
    assert!(app.requests(vod).iter().all(|s| s.status != Status::Error), "the failed request is gone");
    let done = app.settled(vod);
    assert_eq!((done.status, done.transcribed, done.error), (Status::Done, Transcribed::All, None));
    assert_eq!(app.stretches(vod), [(0.0, None, Status::Done)]);
}

/// vods.rs:772, `retranscribe_vod`: "Transcribes a finished VOD again",
/// and its answer otherwise: "Only finished VODs can be transcribed
/// again." One being transcribed, or paused, is left as it is.
#[test]
fn only_a_finished_vod_is_transcribed_again() {
    let app = transcribing();
    let gate = Gate::new();
    let vod = held_from_start(&app, app.file("Ten minutes.mp4", 10.0), &gate);
    for (unfinished, cmd) in [(Status::Transcribing, None), (Status::Paused, Some("pause_vod"))] {
        if let Some(cmd) = cmd {
            app.on(cmd, vod);
            app.settled(vod);
        }
        let before = app.requests(vod);
        assert_eq!(app.vod(vod).status, unfinished);
        let answer = app.invoke("retranscribe_vod", json!({ "id": vod })).unwrap_err();
        assert_eq!(answer, "Only finished VODs can be transcribed again.");
        assert_eq!(app.requests(vod), before);
    }
}

/// vods.rs:774: "With `outdated`, only the parts transcribed by a model
/// this version no longer uses, each a request." The first minute was
/// transcribed before models were recorded; the rest by this version's.
#[test]
fn transcribing_whats_outdated_asks_only_for_the_parts_an_older_model_made() {
    let app = transcribing();
    app.hears(&[(10.0, "The old start."), (300.0, "The kept later part.")]);
    let vod = app.file("Ten minutes.mp4", 10.0);
    app.ask(vod, 60.0, None).unwrap();
    app.settled(vod);
    {
        let mut db = app.db();
        vodscribe_core::testkit::library::says(&mut db, vod, said(10.0, "The old start."));
        // Done as it's asked for, in one go, so the queue never sees it waiting.
        db.atomically(|db| db.finish_stretch(db.request(vod, 0.0, Some(60.0))?.id)).unwrap();
    }
    assert!(app.vod(vod).outdated);
    app.hears(&[(10.0, "The new start."), (300.0, "The new later part.")]);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.invoke("retranscribe_vod", json!({ "id": vod, "outdated": true })).unwrap();
    let asked: Vec<_> = app.requests(vod).iter().map(|s| (s.start, s.end)).collect();
    assert_eq!(asked, [(0.0, Some(60.0))]);
    gate.open();
    app.settled(vod);
    assert_eq!(app.lines(vod), lines(&[(10.0, "The new start."), (300.0, "The kept later part.")]));
    assert!(!app.vod(vod).outdated);
}

/// vods.rs:900, `transcript_stretches`: "VOD `vod_id`'s stretches,
/// transcribed and asked for, in time order."
#[test]
fn a_vods_stretches_are_listed_in_time_order() {
    let app = transcribing();
    let vod = app.file("Ten minutes.mp4", 10.0);
    app.ask(vod, 400.0, Some(460.0)).unwrap();
    app.settled(vod);
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.ask(vod, 300.0, Some(420.0)).unwrap();
    app.held(vod, &gate);
    app.ask(vod, 100.0, Some(160.0)).unwrap();
    let listed = app.stretches(vod);
    let starts: Vec<f64> = listed.iter().map(|s| s.0).collect();
    assert_eq!(starts, [100.0, 300.0, 400.0], "{listed:?}");
    assert_eq!(listed[1].2, Status::Transcribing);
}

/// 0.3.0 Fixes: "Stop, Pause and Remove could miss a VOD that had just
/// started transcribing." Its first minute out, the worker is held: Pause
/// and Stop each reach it, and it doesn't carry on. (Remove is 2A3's.)
#[test]
fn a_pause_or_stop_reaches_a_vod_that_just_started_transcribing() {
    for (cmd, ends_as) in [("pause_vod", Status::Paused), ("cancel_vod", Status::None)] {
        let app = transcribing();
        app.hears(&[(10.0, "The Eagle has landed."), (100.0, "Said later on.")]);
        let vod = app.file("Ten minutes.mp4", 10.0);
        let gate = Gate::new();
        app.worker().holds(&gate);
        app.ask(vod, 0.0, None).unwrap();
        app.held(vod, &gate);
        app.on(cmd, vod);
        assert_eq!(app.settled(vod).status, ends_as, "{cmd}");
        assert_eq!(app.lines(vod), lines(&[(10.0, "The Eagle has landed.")]), "{cmd}");
    }
}

/// 0.6.0: "Transcribe works on picked VODs." ("… a click asks for them
/// all, transcribed one after another in the order you picked them"): the
/// page asks for each in that order (bulk.ts), and each is transcribed in
/// turn, the next only once the one before is done.
#[test]
fn transcribed_vods_one_after_another_in_the_order_picked() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed.")]);
    let [a, b, c] = ["A.mp4", "B.mp4", "C.mp4"].map(|name| app.file(name, 2.0));
    let picked = [c, a, b];
    let mut gate = Gate::new();
    app.worker().holds(&gate);
    for vod in picked {
        app.ask(vod, 0.0, None).unwrap();
    }
    for (i, &vod) in picked.iter().enumerate() {
        app.held(vod, &gate);
        let others: Vec<i64> = picked.iter().copied().filter(|v| *v != vod).collect();
        assert!(others.iter().all(|v| !app.transcribing(*v)), "VOD {vod} transcribed alone");
        assert!(picked[..i].iter().all(|v| app.vod(*v).transcribed == Transcribed::All), "those before it done");
        let next = Gate::new();
        app.worker().holds(&next);
        std::mem::replace(&mut gate, next).open();
    }
    gate.open();
    for vod in picked {
        assert_eq!(app.settled(vod).transcribed, Transcribed::All);
    }
}

/// worker.rs:359 (`cancel`): "if it asked for all of the VOD, so does a
/// partly downloaded video where downloading it is part of transcribing; a
/// video kept whole, or downloaded on request (videos.rs), stays."; Keep
/// video (videos.rs `keep_video`) on a site without its own player.
/// Moves up worker.rs's `a_cancel_keeps_a_video_downloaded_on_request`.
#[test]
fn a_cancel_keeps_a_video_downloaded_on_request() {
    let app = transcribing();
    app.hears(&[(10.0, "The Eagle has landed.")]);
    let vod = app.add_site_vod("rumble", "v5abcd", "Day 3", 600.0).id;
    app.on("keep_video", vod);
    eventually("its video downloaded", || app.vod(vod).media);
    let video = app.db().media_path(vod).unwrap().unwrap();
    let gate = Gate::new();
    app.worker().holds(&gate);
    app.ask(vod, 0.0, None).unwrap();
    app.held(vod, &gate);
    app.on("cancel_vod", vod);
    assert_eq!(app.settled(vod).status, Status::None);
    assert!(Path::new(&video).is_file(), "its video stays");
    assert_eq!(app.db().media_path(vod).unwrap(), Some(video));
}
