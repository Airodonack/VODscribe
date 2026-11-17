//! The specs of `worker.rs`: the transcription queue, through the commands
//! as the page calls them and the state they leave, on the app's own queue
//! with the transcription worker faked (`TestApp::worker`). The commands'
//! own promises (Pause, Stop, Retry, Transcribe again) are vods.rs's
//! (vods/tests/transcribing.rs).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vodscribe_core::stretches::Transcribed;
use vodscribe_core::testkit::{Gate, eventually, said};

use super::*;
use crate::testkit::TestApp;

/// The app with the transcription worker faked and its model installed, so
/// its queue transcribes what's asked: each file says what `hears` says.
fn transcribing() -> TestApp {
    let app = TestApp::new();
    app.worker();
    app
}

/// From now every file `app`'s worker is given says `words`, (second, text)
/// each, at the VOD's own seconds.
fn hears(app: &TestApp, words: &[(f64, &str)]) {
    app.worker().hears(words.iter().flat_map(|(at, text)| said(*at, text)).collect());
}

/// A file on this computer, `minutes` long, added: its id.
fn file(app: &TestApp, name: &str, minutes: f64) -> i64 {
    app.add_video_file(name, minutes * 60.0).id
}

/// A YouTube VOD ten minutes long, added: its id.
fn site_vod(app: &TestApp, id: &str) -> i64 {
    app.add_site_vod("youtube", id, "A talk", 600.0).id
}

/// Transcribe asked for `start`..`end` of VOD `vod` (`end` None: to its
/// end), as the page asks: the request's id.
fn ask(app: &TestApp, vod: i64, start: f64, end: Option<f64>) -> i64 {
    let asked = app.invoke("transcribe_stretch", json!({ "vodId": vod, "start": start, "end": end })).unwrap();
    asked["id"].as_i64().unwrap()
}

/// Command `cmd` (pause_vod, cancel_vod, cancel_stretch, …) on `id`, as the
/// page calls it.
fn on(app: &TestApp, cmd: &str, id: i64) {
    app.invoke(cmd, json!({ "id": id })).unwrap();
}

/// VOD `id` as the library has it.
fn vod(app: &TestApp, id: i64) -> Vod {
    app.db().vod(id).unwrap().unwrap()
}

/// VOD `id` once nothing of it waits or runs: transcribed, paused, stopped
/// or failed, and no stage still at work on a request of it (a stage says
/// how a request ended before it's done with the files: `cancel`).
fn settled(app: &TestApp, id: i64) -> Vod {
    eventually(&format!("VOD {id} settled"), || {
        // A request a stage has just finished is written anew, its row gone.
        let others = |stretch: i64| app.db().stretch(stretch).unwrap().is_some_and(|s| s.vod_id != id);
        !crate::vods::busy(vod(app, id).status) && running(app).into_iter().all(others)
    });
    vod(app, id)
}

/// VOD `id`'s lines, as (start, text).
fn lines(app: &TestApp, id: i64) -> Vec<(f64, String)> {
    app.db().lines(id).unwrap().into_iter().map(|l| (l.start, l.text)).collect()
}

/// Request `id` as the library has it.
fn request(app: &TestApp, id: i64) -> Stretch {
    app.db().stretch(id).unwrap().unwrap()
}

/// Waits till the worker transcribing VOD `id` is held at `gate`, its
/// first minute's lines written.
fn held(app: &TestApp, id: i64, gate: &Gate) {
    gate.reached();
    eventually("its first minute written", || app.db().requests(id).unwrap().iter().any(|s| s.reached.is_some()));
}

/// VOD `vod`, all of it asked for, its worker held at `gate` once it has
/// written its first minute.
fn held_from_start(app: &TestApp, vod: i64, gate: &Gate) {
    app.worker().holds(gate);
    ask(app, vod, 0.0, None);
    held(app, vod, gate);
}

/// The requests a stage is working on now.
fn running(app: &TestApp) -> Vec<i64> {
    let state = app.state();
    let (_held, running) = state.worker.hold_starts();
    running
}

/// Waits till request `id` has its audio and waits for the transcriber,
/// with no stage working on it.
fn ready(app: &TestApp, id: i64) {
    eventually("it waits to be transcribed", || {
        // The download stage marks it ready a moment before it lets go.
        !running(app).contains(&id) && request(app, id).status == Status::Ready
    });
}

/// The whole audio downloaded for VOD `id`, if it's there.
fn downloaded(app: &TestApp, id: i64) -> Option<PathBuf> {
    app.db().audio_path(id).unwrap().map(PathBuf::from).filter(|p| p.is_file())
}

/// Whether the library's work folder holds a file of VOD `id`'s.
fn has_work_files(app: &TestApp, id: i64) -> bool {
    pipeline::file_owners(&app.paths().work()).contains(&id)
}

/// What's said, as the lines it makes: (start, text).
fn said_lines(said: &[(f64, &str)]) -> Vec<(f64, String)> {
    said.iter().map(|(at, text)| (*at, text.to_string())).collect()
}

// Filling in.

/// 0.3.0: "Transcripts fill in steadily while they're made". Held after its
/// first minute, a transcription has written that minute's lines and none
/// of the rest yet.
#[test]
fn a_transcript_fills_in_while_it_is_made() {
    let app = transcribing();
    hears(&app, &[(10.0, "The Eagle has landed."), (300.0, "Said later on.")]);
    let vod = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, vod, &gate);
    assert_eq!(lines(&app, vod), said_lines(&[(10.0, "The Eagle has landed.")]));
    assert_eq!(app.db().vod(vod).unwrap().unwrap().status, Status::Transcribing);
    gate.open();
    settled(&app, vod);
    assert_eq!(lines(&app, vod), said_lines(&[(10.0, "The Eagle has landed."), (300.0, "Said later on.")]));
}

/// 0.3.0: "pausing or quitting loses at most half a minute of work";
/// core pipeline.rs `transcribe`: "Picks up where it reached, so a paused or
/// interrupted job resumes." What the worker had said before a pause, or a
/// quit, is kept, not transcribed again: the worker now hears other words
/// there, and they don't come. (How much the worker says at a time is the
/// worker's own spec.)
#[test]
fn pausing_or_quitting_keeps_whats_transcribed_and_goes_on_after_it() {
    let before = [(10.0, "Said before the stop."), (40.0, "Said just after."), (300.0, "Said later on.")];
    let again = [(10.0, "Not said again."), (40.0, "Said just after."), (300.0, "Said later on.")];
    // Paused, then resumed.
    let app = transcribing();
    hears(&app, &before);
    let vod = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, vod, &gate);
    on(&app, "pause_vod", vod);
    assert_eq!(settled(&app, vod).status, Status::Paused);
    hears(&app, &again);
    app.worker().works();
    on(&app, "resume_vod", vod);
    assert_eq!(settled(&app, vod).transcribed, Transcribed::All);
    let done = lines(&app, vod);
    assert_eq!(done.first(), Some(&(10.0, "Said before the stop.".into())), "paused: {done:?}");
    assert_eq!(done.last(), Some(&(300.0, "Said later on.".into())), "paused: {done:?}");
    // Quit, then started again.
    let app = transcribing();
    hears(&app, &before);
    let vod = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, vod, &gate);
    let folder = app.quit();
    let start = Gate::new();
    folder.holds_worker_start(&start);
    let app = TestApp::on(folder);
    hears(&app, &again);
    start.open();
    assert_eq!(settled(&app, vod).transcribed, Transcribed::All);
    let done = lines(&app, vod);
    assert_eq!(done.first(), Some(&(10.0, "Said before the stop.".into())), "quit: {done:?}");
    assert_eq!(done.last(), Some(&(300.0, "Said later on.".into())), "quit: {done:?}");
}

/// worker.rs `reporter`: "The event says which request of which VOD, and
/// its progress within it." A stretch 2:00–4:00, paused after its first
/// minute, is half done: its progress is within it, not the VOD. The page
/// heard it start, naming the VOD and the request.
#[test]
fn a_requests_progress_is_within_it_and_the_page_hears_whose_it_is() {
    let app = transcribing();
    let vod = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    app.worker().holds(&gate);
    let id = ask(&app, vod, 120.0, Some(240.0));
    held(&app, vod, &gate);
    on(&app, "pause_vod", vod);
    settled(&app, vod);
    assert_eq!(request(&app, id).progress, 0.5);
    let heard = app.heard("vod-progress");
    assert_eq!(heard.first(), Some(&json!([vod, id, Status::Transcribing, 0.0])), "{heard:?}");
}

/// 0.6.0: "Adding a VOD doesn't transcribe it." ("its row says "Not
/// transcribed" until you click Transcribe there. One partly transcribed
/// says how much"): once a request is done, the page hears the VOD's row as
/// the library now has it, partly transcribed.
#[test]
fn the_page_hears_a_vods_row_once_its_request_is_done() {
    let app = transcribing();
    let vod = file(&app, "Ten minutes.mp4", 10.0);
    ask(&app, vod, 0.0, Some(60.0));
    let done = settled(&app, vod);
    assert_eq!(done.transcribed, Transcribed::Some);
    eventually("the page told of the row", || {
        let rows: Vec<Value> = app.heard("vod").into_iter().filter(|row| row["id"] == vod).collect();
        rows.last()
            .is_some_and(|row| row["status"] == json!(done.status) && row["transcribed"] == json!(done.transcribed))
    });
}

// The two stages.

/// worker.rs: "one downloads while the other transcribes, so a stretch can
/// download while another is transcribed." A site's VOD's audio comes while
/// a file is transcribed.
#[test]
fn a_stretch_downloads_while_another_is_transcribed() {
    let app = transcribing();
    let first = file(&app, "Ten minutes.mp4", 10.0);
    let other = site_vod(&app, "noa29qsTc4g");
    let gate = Gate::new();
    held_from_start(&app, first, &gate);
    let asked = ask(&app, other, 0.0, None);
    ready(&app, asked);
    assert!(downloaded(&app, other).is_some(), "its audio came");
    assert_eq!(vod(&app, first).status, Status::Transcribing, "still transcribed meanwhile");
    gate.open();
    assert_eq!(settled(&app, first).transcribed, Transcribed::All);
    assert_eq!(settled(&app, other).transcribed, Transcribed::All);
}

/// worker.rs: "Each stage runs one request (`stretches`) at a time". With
/// one request being transcribed, another whose audio is here waits; with
/// one downloading, another to download waits.
#[test]
fn each_stage_runs_one_request_at_a_time() {
    let app = transcribing();
    let first = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, first, &gate);
    let second = ask(&app, file(&app, "Two minutes.mp4", 2.0), 0.0, None);
    ready(&app, second);
    let downloading = Gate::new();
    app.yt_dlp.holds(&downloading);
    let third = ask(&app, site_vod(&app, "noa29qsTc4g"), 0.0, None);
    downloading.reached();
    let fourth = ask(&app, app.add_site_vod("twitch", "2901646294", "Day 4", 600.0).id, 0.0, None);
    let statuses = [second, third, fourth].map(|id| request(&app, id).status);
    assert_eq!(statuses, [Status::Ready, Status::Downloading, Status::Queued]);
    assert!(!running(&app).contains(&fourth), "two downloads at once");
    downloading.open();
    gate.open();
}

/// worker.rs: "Each stage runs one request … at a time, in the order they
/// were asked for", not the library's: the VOD added last, asked for first,
/// downloads first. (The transcribing stage's order is the commands' too:
/// vods/tests/transcribing.rs `transcribed_vods_one_after_another_in_the_order_picked`.)
#[test]
fn downloads_go_in_the_order_they_were_asked_for() {
    let app = transcribing();
    let added_first = site_vod(&app, "noa29qsTc4g");
    let added_last = app.add_site_vod("twitch", "2901646294", "Day 4", 600.0).id;
    let downloading = Gate::new();
    app.yt_dlp.holds(&downloading);
    let (first, last) = {
        // Both asked before the queue looks.
        let state = app.state();
        let (_held, _) = state.worker.hold_starts();
        (ask(&app, added_last, 0.0, None), ask(&app, added_first, 0.0, None))
    };
    downloading.reached();
    assert_eq!([first, last].map(|id| request(&app, id).status), [Status::Downloading, Status::Queued]);
    downloading.open();
    assert_eq!(settled(&app, added_first).transcribed, Transcribed::All);
}

/// worker.rs `MAX_READY`: "How many downloaded requests may wait for the
/// transcriber before the downloader pauses, so finished audio doesn't pile
/// up on disk." Two wait; the third to download waits its turn till the
/// transcriber takes one.
#[test]
fn at_most_two_downloaded_requests_wait_for_the_transcriber() {
    let app = transcribing();
    let first = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, first, &gate);
    let asked = ["A.mp4", "B.mp4", "C.mp4"].map(|name| ask(&app, file(&app, name, 2.0), 0.0, None));
    ready(&app, asked[0]);
    ready(&app, asked[1]);
    assert_eq!(request(&app, asked[2]).status, Status::Queued);
    assert!(!running(&app).contains(&asked[2]), "a third downloaded");
    gate.open();
    for id in asked {
        let vod_id = request(&app, id).vod_id;
        assert_eq!(settled(&app, vod_id).transcribed, Transcribed::All);
    }
}

// What survives.

/// worker.rs: "The queue is the library itself, so it survives a quit."; e2e
/// quit-mid-job.mjs's header: "at the next start the transcription carries
/// on to the end". A request being transcribed and one waiting, whether the
/// app quit or was killed, are both done after the next start, with no
/// failure.
#[test]
fn the_queue_survives_a_quit_or_a_crash() {
    for quit in [true, false] {
        let app = transcribing();
        hears(&app, &[(10.0, "The Eagle has landed."), (100.0, "Said later on.")]);
        let underway = file(&app, "Ten minutes.mp4", 10.0);
        let waiting = file(&app, "Two minutes.mp4", 2.0);
        let gate = Gate::new();
        held_from_start(&app, underway, &gate);
        ask(&app, waiting, 0.0, None);
        let folder = if quit { app.quit() } else { app.kill() };
        let start = Gate::new();
        folder.holds_worker_start(&start);
        let app = TestApp::on(folder);
        hears(&app, &[(10.0, "The Eagle has landed."), (100.0, "Said later on.")]);
        start.open();
        for id in [underway, waiting] {
            let done = settled(&app, id);
            assert_eq!((done.status, done.transcribed, done.error), (Status::Done, Transcribed::All, None), "{quit}");
        }
        assert_eq!(lines(&app, waiting), said_lines(&[(10.0, "The Eagle has landed."), (100.0, "Said later on.")]));
    }
}

// How a request ends.

/// Opens its gate when dropped, as a test ends, passing or failing.
struct OpensWhenDropped(Gate);

impl Drop for OpensWhenDropped {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// Pause (`pause_vod`) or Stop (`cancel_vod`) on VOD `vod`, as the page
/// calls it, on a thread of its own: its answer once it's let go.
fn stopping(app: &TestApp, cmd: &'static str, vod: i64) -> std::thread::JoinHandle<crate::CmdResult<()>> {
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        let state = handle.state::<crate::AppState>();
        match cmd {
            "pause_vod" => tauri::async_runtime::block_on(crate::vods::pause_vod(handle.clone(), state, vod)),
            _ => tauri::async_runtime::block_on(crate::vods::cancel_vod(handle.clone(), state, vod)),
        }
    })
}

/// worker.rs `finish`: "The transcript finished before the stop took
/// effect: keep it." The stage has written all of it and is about to say
/// so when Pause, or Stop, reaches it (held where it takes the queue,
/// `Worker::holds_next_control`): it stays transcribed.
#[test]
fn a_transcript_finished_before_a_stop_took_effect_is_kept() {
    for cmd in ["pause_vod", "cancel_vod"] {
        let app = transcribing();
        hears(&app, &[(10.0, "The Eagle has landed."), (100.0, "Said later on.")]);
        let vod = file(&app, "Two minutes.mp4", 2.0);
        let gate = Gate::new();
        held_from_start(&app, vod, &gate);
        let control = Gate::new();
        app.state().worker.holds_next_control(&control);
        // Let go however the test ends: held, the stop keeps the library.
        let _let_go = OpensWhenDropped(control.clone());
        let stopping = stopping(&app, cmd, vod);
        // The stop holds the library and the queue: the stage writes the
        // rest on its own connection, then waits for the queue.
        control.reached();
        gate.open();
        let own = Db::open_in(&app.paths()).unwrap();
        eventually("all of it written", || own.requests(vod).unwrap().is_empty());
        control.open();
        assert_eq!(stopping.join().unwrap(), Ok(()), "{cmd}");
        let done = settled(&app, vod);
        assert_eq!((done.status, done.transcribed), (Status::Done, Transcribed::All), "{cmd}");
        assert_eq!(lines(&app, vod).len(), 2, "{cmd}");
    }
}

/// worker.rs `finish`: only "The transcript finished before the stop took
/// effect" is kept; vods.rs `pause_vod`: "Stops a VOD's requests, keeping
/// their partial downloads"; `cancel_vod`: "Ends a VOD's requests". A
/// download that has just come in as Pause, or Stop, reaches it (held where
/// it takes the queue, `Worker::holds_next_control`) is paused, its audio
/// kept, or ended, its audio gone: it isn't transcribed.
#[test]
fn a_download_just_finished_as_a_stop_reaches_it_is_stopped() {
    for cmd in ["pause_vod", "cancel_vod"] {
        let app = transcribing();
        let vod = site_vod(&app, "noa29qsTc4g");
        let downloading = Gate::new();
        app.yt_dlp.holds(&downloading);
        let id = ask(&app, vod, 0.0, None);
        downloading.reached();
        let control = Gate::new();
        app.state().worker.holds_next_control(&control);
        let _let_go = OpensWhenDropped(control.clone());
        let stopping = stopping(&app, cmd, vod);
        control.reached();
        downloading.open();
        let own = Db::open_in(&app.paths()).unwrap();
        eventually("its audio in", || own.stretch(id).unwrap().is_some_and(|s| s.status == Status::Ready));
        control.open();
        assert_eq!(stopping.join().unwrap(), Ok(()), "{cmd}");
        let stopped = settled(&app, vod);
        assert_eq!(stopped.transcribed, Transcribed::None, "{cmd}");
        if cmd == "pause_vod" {
            assert_eq!(request(&app, id).status, Status::Paused);
            assert!(downloaded(&app, vod).is_some(), "its download went");
        } else {
            assert_eq!(app.db().requests(vod).unwrap(), []);
            assert!(!has_work_files(&app, vod), "its download kept");
        }
    }
}

/// worker.rs `finish`: "Records how a job ended: success, a stop the user
/// asked for, or an error": a request whose worker fails, or whose download
/// the site refuses, says why.
#[test]
fn a_request_that_fails_says_why() {
    let app = transcribing();
    let vod = file(&app, "Ten minutes.mp4", 10.0);
    app.worker().fails_at(30.0, "Error: the model couldn't be read");
    ask(&app, vod, 0.0, None);
    let failed = settled(&app, vod);
    assert_eq!(failed.status, Status::Error);
    assert!(failed.error.as_deref().is_some_and(|e| e.contains("the model couldn't be read")), "{:?}", failed.error);
    let refused = app.add_site_vod("twitch", "2901646294", "Day 4", 600.0);
    app.yt_dlp.refuses(&refused.info.url);
    let id = ask(&app, refused.id, 0.0, None);
    assert_eq!(settled(&app, refused.id).status, Status::Error);
    assert!(request(&app, id).error.is_some_and(|e| !e.is_empty()));
}

/// worker.rs `finish`: "The VOD may have been deleted while running;
/// ignore that case." Its row gone from the library while its worker is
/// held, the queue goes on to the next request, and nothing of it is left.
#[test]
fn a_vod_deleted_while_its_request_runs_leaves_the_queue_going() {
    let app = transcribing();
    hears(&app, &[(10.0, "The Eagle has landed."), (100.0, "Said later on.")]);
    let deleted = file(&app, "Ten minutes.mp4", 10.0);
    let next = file(&app, "Two minutes.mp4", 2.0);
    let gate = Gate::new();
    held_from_start(&app, deleted, &gate);
    ask(&app, next, 0.0, None);
    app.db().delete_vod(deleted).unwrap();
    gate.open();
    assert_eq!(settled(&app, next).transcribed, Transcribed::All);
    assert!(app.db().vod(deleted).unwrap().is_none());
    assert_eq!(app.db().stretches(deleted).unwrap(), []);
    assert_eq!(lines(&app, deleted), [], "its worker's last lines kept");
}

// A stopped request's files.

/// worker.rs `Stop::Cancel`: "End the request, keeping the lines it wrote;
/// its audio goes."; `cancel`: "Its section of audio goes. With no other
/// request of its VOD left, the VOD's work files go too". A site's VOD's
/// stretch, downloaded alone, then its whole audio downloaded for two
/// requests, each stopped in turn from the transcript.
#[test]
fn a_stopped_requests_audio_goes_and_the_vods_once_none_of_its_requests_is_left() {
    let app = transcribing();
    let busy = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, busy, &gate);
    let vod = site_vod(&app, "noa29qsTc4g");
    let section = ask(&app, vod, 100.0, Some(160.0));
    ready(&app, section);
    let audio = PathBuf::from(request(&app, section).audio.expect("its own section"));
    assert!(audio.is_file());
    on(&app, "cancel_stretch", section);
    assert!(!audio.exists(), "its section kept");
    let whole = ask(&app, vod, 0.0, None);
    ready(&app, whole);
    let other = ask(&app, vod, 300.0, Some(360.0));
    ready(&app, other);
    on(&app, "cancel_stretch", whole);
    assert!(downloaded(&app, vod).is_some(), "the whole audio went while a request of it waits");
    on(&app, "cancel_stretch", other);
    assert_eq!(app.db().requests(vod).unwrap(), []);
    assert_eq!(app.db().audio_path(vod).unwrap(), None);
    assert!(!has_work_files(&app, vod), "its work files kept");
    gate.open();
}

/// worker.rs `cancel`: "A stretch's own section never downloads the video:
/// a download there is "Keep video"'s, or Play's." A VOD from a site
/// without its own player, its video downloading on request: a stretch of
/// it stopped leaves what that download has written be.
#[test]
fn a_stopped_stretch_leaves_a_video_downloading_on_request() {
    let app = transcribing();
    let busy = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, busy, &gate);
    let vod = app.add_site_vod("rumble", "v5abcd", "Day 3", 600.0).id;
    let video = Gate::new();
    app.yt_dlp.holds(&video);
    app.invoke("keep_video", json!({ "id": vod })).unwrap();
    video.reached();
    let written: Vec<PathBuf> = [app.paths().work(), app.paths().media()]
        .iter()
        .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()))
        .collect();
    assert!(!written.is_empty(), "the video's download has written nothing");
    app.yt_dlp.holds(&Gate::opened());
    let stretch = ask(&app, vod, 100.0, Some(130.0));
    ready(&app, stretch);
    on(&app, "cancel_stretch", stretch);
    assert_eq!(app.db().requests(vod).unwrap(), []);
    let gone: Vec<&PathBuf> = written.iter().filter(|p| !p.exists()).collect();
    assert_eq!(gone, Vec::<&PathBuf>::new(), "the video's download lost them with the stretch");
    video.open();
    gate.open();
}

/// worker.rs `cancel`: "if it asked for all of the VOD, so does a partly
/// downloaded video where downloading it is part of transcribing"; 0.3.0
/// Fixes: "Stopping or removing a download could leave partial files
/// behind." A VOD from a site without its own player, all of it asked for,
/// stopped halfway through its download.
#[test]
fn a_whole_request_stopped_while_its_video_downloads_leaves_no_part_of_it() {
    let app = transcribing();
    let vod = app.add_site_vod("rumble", "v5abcd", "Day 3", 600.0).id;
    let halfway = Gate::new();
    app.yt_dlp.holds_at(0.5, &halfway);
    ask(&app, vod, 0.0, None);
    halfway.reached();
    let parts = || vodscribe_core::fetch::part_files(&app.paths().source_video_stem(vod));
    assert!(!parts().is_empty(), "the download has its part");
    on(&app, "cancel_vod", vod);
    let stopped = settled(&app, vod);
    assert_eq!((stopped.status, stopped.media), (Status::None, false));
    assert_eq!(parts(), [], "a part of the video left");
    assert!(!has_work_files(&app, vod));
}

/// worker.rs `transcribe`: "The whole audio stays while another request of
/// the VOD may use it." Once the last is done, it goes.
#[test]
fn a_vods_audio_stays_while_another_request_may_use_it() {
    let app = transcribing();
    let vod = site_vod(&app, "noa29qsTc4g");
    let first = Gate::new();
    held_from_start(&app, vod, &first);
    ask(&app, vod, 300.0, Some(360.0));
    let second = Gate::new();
    app.worker().holds(&second);
    first.open();
    second.reached();
    assert!(downloaded(&app, vod).is_some(), "the whole audio went while a request of it was transcribed");
    second.open();
    assert_eq!(settled(&app, vod).transcribed, Transcribed::All);
    assert_eq!(app.db().audio_path(vod).unwrap(), None);
    assert!(!has_work_files(&app, vod), "its work files kept once all is done");
}

/// worker.rs `transcribe`: "The download went missing (e.g. the work dir
/// was cleared): fetch it again." A site's VOD's audio, downloaded and
/// waiting for the transcriber, deleted meanwhile, is downloaded again and
/// transcribed.
#[test]
fn a_request_whose_download_went_missing_is_downloaded_again() {
    let app = transcribing();
    hears(&app, &[(10.0, "The Eagle has landed.")]);
    let busy = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, busy, &gate);
    let vod = site_vod(&app, "noa29qsTc4g");
    let id = ask(&app, vod, 0.0, None);
    ready(&app, id);
    std::fs::remove_file(downloaded(&app, vod).unwrap()).unwrap();
    gate.open();
    let done = settled(&app, vod);
    assert_eq!((done.status, done.transcribed, done.error), (Status::Done, Transcribed::All, None));
    assert_eq!(lines(&app, vod), said_lines(&[(10.0, "The Eagle has landed.")]));
}

/// VOD `id`'s waveform as the page gets it (media.rs `peaks`): ten levels a
/// second.
fn waveform(app: &TestApp, id: i64) -> Vec<u8> {
    app.invoke_bytes("peaks", json!({ "id": id })).unwrap()
}

/// worker.rs `download`: "The waveform, read while the audio is here … A
/// section's levels go at their place; whole audio just downloaded is read
/// anew over them"; 0.7.4 Fixes: "Paused requests show their whole
/// waveform." A site's VOD's stretch 1:40–2:40, paused once its audio came,
/// has its levels to its end; all of it, once its whole audio comes, to the
/// VOD's.
#[test]
fn a_vods_waveform_is_read_as_its_audio_comes() {
    let app = transcribing();
    let busy = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, busy, &gate);
    let vod = site_vod(&app, "noa29qsTc4g");
    assert!(waveform(&app, vod).is_empty(), "a waveform before any audio");
    let section = ask(&app, vod, 100.0, Some(160.0));
    ready(&app, section);
    on(&app, "pause_vod", vod);
    assert_eq!(request(&app, section).status, Status::Paused);
    assert_eq!(waveform(&app, vod).len(), 1600, "levels to the stretch's end");
    let listed = app.invoke("transcript_stretches", json!({ "vodId": vod })).unwrap();
    assert_eq!(listed[0]["audio"], Value::Bool(true), "{listed}");
    let whole = ask(&app, vod, 0.0, None);
    ready(&app, whole);
    assert_eq!(waveform(&app, vod).len(), 6000, "levels to the VOD's end");
    gate.open();
}

/// worker.rs `download`: "a file or kept video only if there's none (the
/// page reads theirs)". A file with no waveform has its own once it's
/// transcribed; one whose waveform the page already has keeps it.
#[test]
fn a_files_waveform_is_read_as_it_is_transcribed_unless_it_has_one() {
    let app = transcribing();
    let peaks = app.paths().peaks();
    let fresh = file(&app, "Two minutes.mp4", 2.0);
    let read = file(&app, "Read before.mp4", 2.0);
    let before = vec![9; 1200];
    vodscribe_core::peaks::save(&peaks, read, &before).unwrap();
    for vod in [fresh, read] {
        ask(&app, vod, 0.0, None);
        settled(&app, vod);
    }
    assert_eq!(vodscribe_core::peaks::load(&peaks, fresh).map(|l| l.len()), Some(1200));
    assert_eq!(vodscribe_core::peaks::load(&peaks, read), Some(before), "read again over the page's");
}

// What the queue needs.

/// worker.rs `next_job`: "Without the video downloader, only local files
/// can be fetched; online VODs wait." Without yt-dlp, a file asked for after
/// a site's VOD is transcribed, and the site's VOD waits, failing nothing,
/// till the app starts again with it.
#[test]
fn without_yt_dlp_files_are_transcribed_and_site_vods_wait() {
    let app = transcribing();
    hears(&app, &[(10.0, "The Eagle has landed.")]);
    app.lacks_yt_dlp();
    let online = site_vod(&app, "noa29qsTc4g");
    let waits = ask(&app, online, 0.0, None);
    let local = file(&app, "Two minutes.mp4", 2.0);
    ask(&app, local, 0.0, None);
    assert_eq!(settled(&app, local).transcribed, Transcribed::All);
    let waiting = request(&app, waits);
    assert_eq!((waiting.status, waiting.error), (Status::Queued, None));
    let start = Gate::new();
    let folder = app.quit();
    folder.holds_worker_start(&start);
    let app = TestApp::on(folder);
    hears(&app, &[(10.0, "The Eagle has landed.")]);
    start.open();
    assert_eq!(settled(&app, online).transcribed, Transcribed::All);
}

/// Settings → Transcription's device set to `device` ("cpu", "gpu"), as
/// the page saves it (`set_settings`).
fn transcribes_with(app: &TestApp, device: &str) {
    let mut settings = app.invoke("get_settings", json!({})).unwrap();
    settings["device"] = device.into();
    app.invoke("set_settings", json!({ "settings": settings })).unwrap();
}

/// worker.rs `plan_transcription`: "GPU is used only when selected,
/// installed and free enough; otherwise the CPU runs it and the note says
/// why." Not selected, the processor transcribes, and the VOD has no note.
#[test]
fn the_processor_transcribes_unless_the_graphics_card_is_chosen() {
    let app = transcribing();
    transcribes_with(&app, "cpu");
    let vod = file(&app, "Two minutes.mp4", 2.0);
    ask(&app, vod, 0.0, None);
    let done = settled(&app, vod);
    assert_eq!((done.transcribed, done.note), (Transcribed::All, None));
}

/// worker.rs `plan_transcription`: "otherwise the CPU runs it and the note
/// says why": the graphics card chosen without its add-on installed, the
/// processor transcribes, saying so.
#[test]
fn the_graphics_card_chosen_without_its_add_on_the_processor_transcribes_saying_why() {
    let app = transcribing();
    transcribes_with(&app, "gpu");
    let vod = file(&app, "Two minutes.mp4", 2.0);
    ask(&app, vod, 0.0, None);
    let done = settled(&app, vod);
    assert_eq!(done.transcribed, Transcribed::All);
    assert_eq!(done.note.as_deref(), Some("Transcribed with the processor: the GPU add-on isn't installed."));
}

/// worker.rs `Worker::hold`: "Stops new jobs from starting, pauses the
/// running ones, and waits … for them to exit"; setup.rs `clear_cache`:
/// "Deletes everything that can be downloaded again …, pausing running jobs
/// first. The library, transcripts and settings are kept". Repair VODscribe
/// (Settings → Help) while a VOD is transcribed pauses it before it deletes
/// anything, its lines kept.
#[test]
fn repairing_vodscribe_pauses_whats_transcribed_keeping_its_lines() {
    let app = transcribing();
    hears(&app, &[(10.0, "The Eagle has landed."), (300.0, "Said later on.")]);
    let vod = file(&app, "Ten minutes.mp4", 10.0);
    let gate = Gate::new();
    held_from_start(&app, vod, &gate);
    app.invoke("clear_cache", json!({})).unwrap();
    // Paused by the time it's done: it waited for the transcription to stop.
    let [paused] = app.db().requests(vod).unwrap().try_into().unwrap();
    assert_eq!(paused.status, Status::Paused);
    assert_eq!(lines(&app, vod), said_lines(&[(10.0, "The Eagle has landed.")]));
}

// Below the seam, till a stage can be held once its run is over, before it
// records how it ended: what's recorded then can't be reached through the
// commands in a set order.

/// A library's work dir, which it reads as, and its `paths`; the
/// library's folder goes when the test ends.
struct Work {
    paths: Paths,
    dir: PathBuf,
    _root: vodscribe_core::testkit::TempDir,
}

impl std::ops::Deref for Work {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.dir
    }
}

/// A library with one VOD and a request for all of it in `status`, and a
/// work dir holding its audio. Returns the request's id.
fn setup(status: Status) -> (Db, Work, i64) {
    let root = vodscribe_core::testkit::TempDir::new("worker");
    let paths = Paths::new(root.join("library"));
    let work = paths.work();
    std::fs::create_dir_all(&work).unwrap();
    let db = Db::open(&paths.db()).unwrap();
    let id = vodscribe_core::testkit::library::add_site_vod(&db, "youtube", "x", "t", 60.0).id;
    let stretch = db.request(id, 0.0, None).unwrap().id;
    db.set_stretch_status(stretch, status, 0.5, None).unwrap();
    std::fs::write(work.join(format!("{id}.m4a")), b"audio").unwrap();
    db.set_audio_path(id, Some(&work.join(format!("{id}.m4a")).to_string_lossy())).unwrap();
    (db, Work { paths, dir: work, _root: root }, stretch)
}

/// The VOD of request `stretch`.
fn vod_of(db: &Db, stretch: i64) -> i64 {
    db.stretch(stretch).unwrap().unwrap().vod_id
}

/// Writes a line from 0 to 10 s for request `id`, which reached its end.
fn write_a_line(db: &mut Db, id: i64) {
    let vod = vod_of(db, id);
    let line = vodscribe_core::transcript::Segment { start: 0.0, end: 10.0, text: "hi".into(), words: Vec::new() };
    db.replace_segments(vod, id, (0.0, Some(10.0)), &[line], 10.0).unwrap();
}

/// worker.rs `forget_if_erased`: "if it was erased meanwhile (out of the
/// list and gone, its work slow to stop as it was removed with its video:
/// `vods::delete_vod`), what the job wrote as it stopped goes too, its last
/// lines and its stretch done over lines erased". Whether it was canceled,
/// paused or had just finished. (Through the commands, vods/tests/removing.rs
/// has the removal; a stage still running after Remove's wait can't be
/// made there yet.)
#[test]
fn a_stage_ending_on_a_vod_erased_meanwhile_leaves_nothing_of_it() {
    for stop in [Some(Stop::Cancel), Some(Stop::Pause), None] {
        let (mut db, work, id) = setup(Status::Transcribing);
        let vod = vod_of(&db, id);
        db.remove_entry(vod).unwrap();
        db.set_gone(vod).unwrap();
        // Its last words, written as it stopped, after the erasure.
        write_a_line(&mut db, id);
        // Or it finished just then, done.
        let result = if stop.is_some() { Err(vodscribe_core::Canceled.into()) } else { Ok(()) };
        if stop.is_none() {
            db.finish_stretch(id).unwrap();
        }
        finish(Stage::Transcribe, &db, &work.paths, id, result, stop, 0.4).unwrap();
        forget_if_erased(&db, vod).unwrap();
        assert_eq!(db.stretches(vod).unwrap(), [], "{stop:?}");
        assert_eq!(db.lines(vod).unwrap(), [], "{stop:?}");
    }
}

/// worker.rs `Job::stop`: "Stops the job for `reason`, unless it's already
/// stopping for a more final one: clearing the cache pauses every job,
/// which mustn't turn a cancel on its way into a pause."
#[test]
fn stops_never_become_less_final() {
    let job =
        Job { stretch_id: 1, vod_id: 1, flag: Arc::new(AtomicBool::new(false)), reason: Arc::new(Mutex::new(None)) };
    job.stop(Stop::Pause);
    assert_eq!(*job.reason.lock().unwrap(), Some(Stop::Pause));
    job.stop(Stop::Cancel);
    // Clearing the cache pauses every job, after the cancel.
    job.stop(Stop::Pause);
    assert_eq!(*job.reason.lock().unwrap(), Some(Stop::Cancel));
    assert!(job.flag.load(Ordering::Relaxed));
}

/// worker.rs `finish`: "Stopped without a recorded reason: keep the work,
/// as a pause would."
#[test]
fn a_stop_with_no_reason_is_a_pause() {
    let (db, work, id) = setup(Status::Downloading);
    finish(Stage::Download, &db, &work.paths, id, Err(vodscribe_core::Canceled.into()), None, 0.3).unwrap();
    let stretch = db.stretch(id).unwrap().unwrap();
    assert_eq!((stretch.status, stretch.progress), (Status::Paused, 0.3));
}
