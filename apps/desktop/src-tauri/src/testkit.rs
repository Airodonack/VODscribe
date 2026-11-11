//! The app in a test (`TestApp`): its real state and queues, as it starts
//! them, on Tauri's mock runtime, over a library in a folder of its own,
//! with ffmpeg and yt-dlp faked (`vodscribe_core::testkit`). The commands
//! are called as the page calls them, by name and with its arguments
//! (`TestApp::invoke`), or as Rust functions; what the page would hear is
//! kept (`TestApp::heard`), and what the system's desktop would be asked to
//! do is heard by `Desktop` (system.rs). A drag out of the window begins
//! no system's drag: `Drags` keeps it, and the test lets it go where it
//! says (drag_source). The app's clock stands still till the test moves it
//! (`Clock`), and the variables it was started with are the test's own
//! (`Env`). The window has no screen for the player to draw on till the
//! test gives it one (`Screen`), and the player's sound goes to no device.
//! The transcription worker, a sidecar, is faked once a test asks for it
//! (`TestApp::worker`), with its model installed. A test holds the app's own
//! code at a gate where a fake can't (`Held`): an erasure between its
//! parts, a plan worked out, a settings file read or saved, a search asked.
//! An app that quit or was killed leaves its folder (`Folder`) to the test,
//! which starts the app on it again.
//!
//! The helpers say what a test does in the words of its spec: "a file
//! `Day 3.mp4` of a minute is added" (`add_video_file`), "a clip of
//! 0:10–0:20 is made" (`make_clip`), "the library has these clips"
//! (`clips`). A command whose arguments change is changed here, once.
//!
//! The guide to writing tests with it is crates/core/src/testkit/README.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{Listener, Manager};
use vodscribe_core::db::{Clip, ClipStatus, Db, Vod};
use vodscribe_core::testkit::tools::{self as fakes, At, Fake, Run};
use vodscribe_core::testkit::{Ffmpeg, Gate, Media, TempDir, Worker, YtDlp, eventually, library, yt_dlp};

use crate::{AppHandle, AppState, CmdResult, Rt, WebviewWindow};

/// The events the page hears from the app (`page::emit`), kept as each
/// came: every event the app sends is listened to.
const EVENTS: &[&str] = &[
    "annotations-found",
    "annotations-imported",
    "chat-progress",
    "chat-saved",
    "clip",
    "clip-progress",
    "clip-removed",
    "drag-ended",
    "drag-making",
    "edit",
    "edit-progress",
    "edit-removed",
    "footage",
    "footage-progress",
    "history",
    "jobs",
    "player-time",
    "received",
    "restored",
    "video-progress",
    "vod",
    "vod-progress",
    "vod-removed",
    "yt-dlp-progress",
];

/// The app as it runs, for one test.
pub(crate) struct TestApp {
    app: tauri::App<Rt>,
    window: WebviewWindow,
    /// The ffmpeg the app finds, faked.
    pub(crate) ffmpeg: Ffmpeg,
    /// The yt-dlp the app finds, faked: it serves each site's VOD added with
    /// `add_site_vod`.
    pub(crate) yt_dlp: YtDlp,
    /// The transcription worker the app runs, faked once asked for (`worker`).
    worker: OnceLock<Worker>,
    heard: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    /// Where the test's own files go (a VOD's file on this computer),
    /// outside the library's folder.
    files: PathBuf,
    /// The drags begun by `drag_out`, the page's number for the last.
    dragged: AtomicU64,
    /// How many drags' ends `drag_ended` has given.
    drags_ended: AtomicUsize,
    /// Where its library keeps the files it couldn't delete (`files`).
    leftovers: PathBuf,
    // Kept for the chat queue, which the tests don't run: what's asked of it waits.
    _chats: std::sync::mpsc::Receiver<i64>,
    // The app's while it runs: removed once `drop` has closed what the app
    // has open in it, unless `quit` or `kill` gave it to the test.
    folder: Option<Folder>,
}

/// The folder an app runs on, for a test: its library, and the test's own
/// files beside it. The app started on it owns it while it runs
/// (`TestApp::on`); once that app has quit or was killed (`TestApp::quit`,
/// `TestApp::kill`), the test does, and starts the app on it again. Removed
/// when it's dropped.
pub(crate) struct Folder(TempDir);

impl Folder {
    /// An empty one.
    pub(crate) fn new() -> Self {
        Folder(TempDir::new("app"))
    }

    /// The library's paths in it.
    pub(crate) fn paths(&self) -> vodscribe_core::Paths {
        vodscribe_core::Paths::new(self.0.join("data"))
    }

    fn files(&self) -> PathBuf {
        self.0.join("files")
    }

    /// The next app started on it holds its transcription back till `gate`
    /// opens (`Worker::run_at`), as a busy start does: the other queues
    /// take up what was left first.
    pub(crate) fn holds_worker_start(&self, gate: &Gate) {
        held().push((Held::WorkerStart, self.paths().clips(), gate.clone()));
    }
}

impl Drop for TestApp {
    /// Ends the app with its test: each queue is stopped and its thread
    /// joined, so no job of this test runs on after it, and the app's state
    /// is dropped, which closes the library before its folder goes.
    fn drop(&mut self) {
        // First, so a job whose fake is held at a gate the test never
        // opened (it failed first) ends, and can be joined; and a drag's
        // receiver waiting for a file stops.
        self.drags().end();
        // The player first: its threads tell the state of each frame, and
        // end once it's closed, while the queues below are joined.
        crate::player::close(self.handle());
        self.ffmpeg.end();
        self.yt_dlp.end();
        if let Some(worker) = self.worker.get() {
            worker.end();
            let model = self.paths().model_dir();
            routes().retain(|(m, _)| *m != model);
        }
        // What the library's own code was held at lets go, as the app's below.
        vodscribe_core::testkit::tools::let_go(&self.paths().root);
        self.state().searches.lets_go();
        no_links().retain(|dir| *dir != self.paths().clips());
        held().retain(|(_, dir, gate)| {
            let ours = *dir == self.paths().clips();
            if ours {
                gate.open();
            }
            !ours
        });
        // The threads its commands started end before the state they read goes.
        self.threads_done();
        let state = self.state();
        state.clips.close();
        state.sweeper.close();
        state.videos.close();
        state.storyboards.close();
        state.footage.close();
        state.worker.close();
        // Last: the queues above tell it of their changes.
        state.job_list.close();
        // With every thread that reads the state joined, nothing holds a
        // reference into it (what makes `unmanage` unsafe in a running app).
        // The media server's thread is left, idle: no request comes in a
        // test, and it holds no file.
        #[allow(deprecated)]
        drop(self.app.unmanage::<AppState>());
        crate::files::forget_leftovers(&self.leftovers);
    }
}

impl TestApp {
    /// The app started on an empty library (`on`).
    pub(crate) fn new() -> Self {
        Self::on(Folder::new())
    }

    /// The app started on `folder`'s library, as `lib.rs`'s `run` starts it
    /// (but with no page in its window, nor the background jobs that reach
    /// the network: thumbnails, streamer cards, chat): what's left from
    /// before tidied, then its queues running, beginning with what was left
    /// unfinished. A folder an app quit or was killed on is started on
    /// again as after that quit (`quit`, `kill`). The fakes start anew, and
    /// the fake site serves each site's VOD the library has.
    pub(crate) fn on(folder: Folder) -> Self {
        let paths = folder.paths();
        paths.ensure().unwrap();
        let files = folder.files();
        std::fs::create_dir_all(&files).unwrap();
        let ffmpeg = Ffmpeg::in_bin(&paths.bin());
        let yt_dlp = YtDlp::in_bin(&paths.bin());
        let app = tauri::test::mock_builder()
            .invoke_handler(crate::commands())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let heard: Arc<Mutex<HashMap<String, Vec<Value>>>> = Arc::default();
        for &event in EVENTS {
            let heard = heard.clone();
            app.listen_any(event, move |e| {
                let payload = serde_json::from_str(e.payload()).unwrap_or(Value::Null);
                heard.lock().unwrap().entry(event.into()).or_default().push(payload);
            });
        }
        app.manage(Desktop::default());
        app.manage(Drags::default());
        app.manage(Clock::default());
        app.manage(Env::default());
        app.manage(crate::choices::Kept::load(paths.choices()));
        app.manage(crate::recovery::Problem::none(paths.db()));
        app.manage(crate::screen::Screen::default());
        app.manage(Screen::default());
        app.manage(Started::default());
        // A library whose model a test installed (`worker`) has its worker
        // from the start, for a transcription its queue takes up again.
        let worker = OnceLock::new();
        if vodscribe_core::model::is_installed(&paths.model_dir(), &vodscribe_core::model::CPU_MODEL) {
            let _ = worker.set(fake_worker(&paths));
        }
        // The player's sound plays out in time on a stand-in, not the
        // computer's speakers (every player opened from now, in any test).
        vodscribe_player::sound::stand_in::choose();
        let db = Db::open_in(&paths).unwrap();
        vodscribe_core::library::settle(&db).unwrap();
        // The site still has the VODs added before, for what's left to make.
        for vod in db.vods().unwrap().iter().filter(|v| v.info.platform != vodscribe_core::local::PLATFORM) {
            yt_dlp.serves(details(vod));
        }
        let store = paths.leftovers();
        let chats = crate::open_library(app.handle(), paths, db).unwrap();
        // The storyboards' sweep, which runs beside the start, has asked for what's left.
        app.state::<AppState>().storyboards.swept();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        TestApp {
            app,
            window,
            ffmpeg,
            yt_dlp,
            worker,
            heard,
            files,
            dragged: AtomicU64::new(0),
            drags_ended: AtomicUsize::new(0),
            leftovers: store,
            _chats: chats,
            folder: Some(folder),
        }
    }

    /// Quits, as closing the window does (`lib.rs`'s `quit`): the library's
    /// part of it (`quit_library`: a cut not pasted goes, then nothing more
    /// is written), the files left to delete tried again, then the app
    /// ends. Its folder is the test's from now, to start the app on again
    /// (`on`).
    pub(crate) fn quit(mut self) -> Folder {
        {
            let state = self.state();
            let db = state.db.lock().unwrap();
            crate::quit_library(self.handle(), &db);
            // The real quit keeps the lock till the process ends. Let go
            // here, so the app can end; what its jobs still write is in the
            // transaction `stop_writes` began, gone as the library closes.
        }
        crate::files::delete_leftovers();
        self.folder.take().unwrap()
    }

    /// Ends the app as if it were killed: none of the quit's work, and
    /// nothing it does from now reaches the library, as when its process
    /// is gone (what its jobs still write is in the transaction
    /// `stop_writes` began, gone as the library closes). Its folder is the
    /// test's from now, to start the app on again (`on`).
    pub(crate) fn kill(mut self) -> Folder {
        self.db().stop_writes().unwrap();
        self.folder.take().unwrap()
    }

    pub(crate) fn handle(&self) -> &AppHandle {
        self.app.handle()
    }

    pub(crate) fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    /// The library's disk takes no hard links, as exFAT and FAT don't: a link
    /// made in its clips folder fails (`clips::hard_link`), as the system's
    /// does there.
    pub(crate) fn takes_no_hard_links(&self) {
        no_links().push(self.paths().clips());
    }

    /// A pasted clip's copy of its file waits at `gate` once it's copied,
    /// before the library is taken to record it (`clipboard::copy_file`), as
    /// on a slow disk the copy itself would take a while.
    pub(crate) fn holds_copies(&self, gate: &Gate) {
        self.holds(Held::Copies, gate);
    }

    /// A clip's cut, or an edit's render, once its encode is over (ffmpeg
    /// done, failed or stopped), waits at `gate` before the library is taken
    /// to record it (`clips::cut`, `edits::render_edit`): the moment between
    /// a clip made and its job's end.
    pub(crate) fn holds_cut_ends(&self, gate: &Gate) {
        self.holds(Held::CutEnds, gate);
    }

    /// Each erasure from now (`vods::erase_unused`) waits at `gate` after
    /// each part of a VOD's chat and transcript it erases, the library let
    /// go of: between its parts, when the commands have their turn.
    pub(crate) fn holds_erasures(&self, gate: &Gate) {
        self.holds(Held::BetweenParts, gate);
    }

    /// Each part of an erasure from now waits at `gate` once it's erased,
    /// the library still held, as a slow disk's delete keeps it: what reads
    /// on a connection of its own (the views, search) goes on meanwhile.
    pub(crate) fn holds_erasure_parts(&self, gate: &Gate) {
        self.holds(Held::InAPart, gate);
    }

    /// Each plan of an edit the page asks for from now (`player_plan`) waits
    /// at `gate` once it's worked out, before it's played: one that takes
    /// longer than the next asked for.
    pub(crate) fn holds_plans(&self, gate: &Gate) {
        self.holds(Held::Planned, gate);
    }

    /// Each search from now (`search`) waits at `gate` once it's asked, a
    /// typed one numbered, before it has its connection: as one does behind
    /// a search of seconds (`vodscribe_core::searcher`).
    pub(crate) fn holds_searches(&self, gate: &Gate) {
        self.state().searches.holds(gate);
    }

    /// Each read of the settings file from now that finds it unreadable
    /// (`Settings::load`) waits at `gate` before it sets the file aside.
    pub(crate) fn holds_settings_reads(&self, gate: &Gate) {
        fakes::hold_at(At::SettingsRead, &self.paths().settings(), gate);
    }

    /// Each save of the settings from now (`Settings::save`) waits at `gate`
    /// before it takes its turn and writes: a save asked later can go first.
    pub(crate) fn holds_settings_saves(&self, gate: &Gate) {
        fakes::hold_at(At::SettingsSave, &self.paths().settings(), gate);
    }

    /// The app's own code waits at `gate` at `at` from now, in place of any
    /// gate it waited at there before (`Gate::opened()` to hold no more).
    fn holds(&self, at: Held, gate: &Gate) {
        let dir = self.paths().clips();
        let mut held = held();
        held.retain(|(h, d, _)| !(*h == at && *d == dir));
        held.push((at, dir, gate.clone()));
    }

    /// Waits for the threads the app's commands started of their own
    /// (`started`: an added file's picture asked for) to end. The app's end
    /// waits for them too, before its state goes.
    pub(crate) fn threads_done(&self) {
        let threads = std::mem::take(&mut *self.app.state::<Started>().lock());
        for thread in threads {
            // One that panicked has said so on its own.
            let _ = thread.join();
        }
    }

    /// How many of the threads the app's commands started (`started`) are
    /// still running.
    pub(crate) fn threads_running(&self) -> usize {
        self.app.state::<Started>().lock().iter().filter(|t| !t.is_finished()).count()
    }

    /// The transcription worker the app runs, faked (`Worker`), and its
    /// model installed: from the first call on, the app's queue transcribes
    /// what's asked, each file saying what the worker `hears`. A start on a
    /// library whose model is installed (`on`) has it from the start,
    /// hearing nothing till told.
    pub(crate) fn worker(&self) -> &Worker {
        self.worker.get_or_init(|| fake_worker(&self.paths()))
    }

    /// `forget_gone_clips`, once it has looked at the clips' files (without
    /// the library held), waits at `gate` before it takes the library to
    /// take those it found gone.
    pub(crate) fn holds_gone_looks(&self, gate: &Gate) {
        self.holds(Held::GoneLooks, gate);
    }

    /// The app's clock (`Clock`): it stands still till the test moves it.
    pub(crate) fn clock(&self) -> &Clock {
        self.app.state::<Clock>().inner()
    }

    /// The app sees variable `name` as `value`, as if it had been started
    /// with it (`system::var`). The test's own: the process's aren't seen.
    pub(crate) fn started_with(&self, name: &str, value: &str) {
        self.app.state::<Env>().vars.lock().unwrap().insert(name.into(), value.into());
    }

    /// The window has a screen from now, as the app's own player needs
    /// (`screen::attached`): what it's shown is kept (`screen`).
    pub(crate) fn with_screen(&self) {
        self.screen().attached.store(true, Ordering::SeqCst);
    }

    /// The window's screen, as the tests have it (`Screen`).
    pub(crate) fn screen(&self) -> &Screen {
        self.app.state::<Screen>().inner()
    }

    /// The library, held until the guard is dropped (as a command holds it).
    pub(crate) fn db(&self) -> MutexGuard<'_, Db> {
        self.state().inner().db.lock().unwrap()
    }

    pub(crate) fn paths(&self) -> vodscribe_core::Paths {
        self.state().paths.clone()
    }

    /// What the system's desktop was asked (`Desktop`).
    pub(crate) fn desktop(&self) -> &Desktop {
        self.app.state::<Desktop>().inner()
    }

    /// The system's drags out of the window (`Drags`).
    pub(crate) fn drags(&self) -> &Drags {
        self.app.state::<Drags>().inner()
    }

    /// Starts dragging clips `ids` out of the window, as the page does with
    /// the mouse button still down (`clips::drag_clip`), with no picture of
    /// its own; the page numbers its drags 1, 2, … The command's answer:
    /// true if the drag began (`drags().last()`), false if the button came
    /// up first.
    pub(crate) fn drag_out(&self, ids: &[i64]) -> Result<Value, Value> {
        let number = self.dragged.fetch_add(1, Ordering::SeqCst) + 1;
        let clips = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert("clip", clips.parse().unwrap());
        headers.insert("drag", number.to_string().parse().unwrap());
        self.invoke_with("drag_clip", tauri::ipc::InvokeBody::Raw(Vec::new()), headers)
    }

    /// How the next drag to end ended, as the page hears it ("drag-ended",
    /// `clips::DragEnded`), once it has: the first call gives the first
    /// drag's end, the next the second's.
    pub(crate) fn drag_ended(&self) -> Value {
        let n = self.drags_ended.fetch_add(1, Ordering::SeqCst);
        eventually(&format!("drag {} ended", n + 1), || self.heard("drag-ended").len() > n);
        self.heard("drag-ended")[n].clone()
    }

    /// Runs a command's future (`clips::add_clip(app.handle().clone(), …)`)
    /// to its end, as the page awaits it.
    pub(crate) fn run<T>(&self, command: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(command)
    }

    /// Calls command `cmd` as the page does, through Tauri's IPC: by name,
    /// with `args` as the page writes them (camelCase), answered as JSON.
    pub(crate) fn invoke(&self, cmd: &str, args: Value) -> Result<Value, Value> {
        self.invoke_with(cmd, tauri::ipc::InvokeBody::Json(args), Default::default())
    }

    /// `invoke` for a command that answers in bytes (`player_still`), as the
    /// page gets them.
    pub(crate) fn invoke_bytes(&self, cmd: &str, args: Value) -> Result<Vec<u8>, Value> {
        self.request(cmd, tauri::ipc::InvokeBody::Json(args), Default::default()).map(|body| match body {
            tauri::ipc::InvokeResponseBody::Raw(bytes) => bytes,
            tauri::ipc::InvokeResponseBody::Json(json) => json.into_bytes(),
        })
    }

    /// `invoke`, with the request's `body` and `headers` as the page sends
    /// them (`drag_clip`'s clips in a header, its picture as bytes).
    fn invoke_with(
        &self,
        cmd: &str,
        body: tauri::ipc::InvokeBody,
        headers: tauri::http::HeaderMap,
    ) -> Result<Value, Value> {
        self.request(cmd, body, headers).map(|body| body.deserialize::<Value>().unwrap())
    }

    /// The command's answer to a request as the page makes it.
    fn request(
        &self,
        cmd: &str,
        body: tauri::ipc::InvokeBody,
        headers: tauri::http::HeaderMap,
    ) -> Result<tauri::ipc::InvokeResponseBody, Value> {
        let request = tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            // The page's own origin, whose calls need no capability. Windows
            // serves the page from http://tauri.localhost and takes a
            // tauri:// call for a remote site's, which the kit grants nothing.
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body,
            headers,
            invoke_key: tauri::test::INVOKE_KEY.into(),
        };
        tauri::test::get_ipc_response(&self.window, request)
    }

    /// The payloads of the events `event` the page has heard so far, in order.
    pub(crate) fn heard(&self, event: &str) -> Vec<Value> {
        assert!(EVENTS.contains(&event), "no event {event} is sent (testkit::EVENTS)");
        self.heard.lock().unwrap().get(event).cloned().unwrap_or_default()
    }

    /// A file on this computer, `name` (with its extension), holding
    /// `media`, added to the library as a VOD (`library::add_file`).
    pub(crate) fn add_file(&self, name: &str, media: &Media) -> Vod {
        library::add_file(&self.db(), &self.files, name, media)
    }

    /// A video file, `name`, `duration` seconds long (`Media::video`), added.
    pub(crate) fn add_video_file(&self, name: &str, duration: f64) -> Vod {
        self.add_file(name, &Media::video(duration))
    }

    /// A VOD of `platform` (youtube, twitch, kick) with `id`, `duration`
    /// seconds long, added to the library; the fake site serves it.
    pub(crate) fn add_site_vod(&self, platform: &str, id: &str, title: &str, duration: f64) -> Vod {
        let vod = library::add_site_vod(&self.db(), platform, id, title, duration);
        self.yt_dlp.serves(details(&vod));
        vod
    }

    /// `add_site_vod`, the VOD streamed on `channel`.
    pub(crate) fn add_site_vod_of(&self, platform: &str, id: &str, title: &str, duration: f64, channel: &str) -> Vod {
        let vod = library::add_site_vod_of(&self.db(), platform, id, title, duration, Some(channel));
        self.yt_dlp.serves(details(&vod));
        vod
    }

    /// `text` is said in VOD `vod_id` from `start` seconds (`library::said`).
    pub(crate) fn says(&self, vod_id: i64, start: f64, text: &str) {
        library::says(&mut self.db(), vod_id, library::said(start, text));
    }

    /// `says`, by `speaker`, as an imported VODscription names who says a line.
    pub(crate) fn says_as(&self, vod_id: i64, start: f64, text: &str, speaker: &str) {
        library::says_as(&mut self.db(), vod_id, library::said(start, text), speaker);
    }

    /// Clip `clip` dragged out of the window and let go on an editor that
    /// takes the file itself (`drag_clip`, through the kit's drag), the drag
    /// waited out: the file it was given.
    pub(crate) fn dragged_out(&self, clip: &Clip) -> PathBuf {
        self.drag_out(&[clip.id]).unwrap();
        let drag = self.drags().last();
        drag.let_go(LetGo::On(Receiver::taking_files("Kdenlive")));
        self.drag_ended();
        let given = drag.given();
        assert_eq!(given.len(), 1, "clip {} not handed out", clip.id);
        given[0].clone()
    }

    /// Waits till the list of jobs (`list_jobs`) shows no VOD being erased,
    /// nor waiting to be.
    pub(crate) fn erasures_done(&self) {
        eventually("the erasures done", || {
            let jobs = self.invoke("list_jobs", serde_json::json!({})).unwrap();
            jobs.as_array().unwrap().iter().all(|job| job["kind"] != "erase")
        });
    }

    /// Makes a clip of VOD `vod_id` from `from` to `to` ("0:10", "1:02:03.5")
    /// as Clip in the clip bar does (`clips::add_clip`), and waits till its
    /// file is made or has failed (`made`).
    pub(crate) fn make_clip(&self, vod_id: i64, from: &str, to: &str) -> Clip {
        let clip = self.add_clip(vod_id, from, to).unwrap();
        self.made(clip.id)
    }

    /// `clips::add_clip`'s answer, its file not waited for.
    pub(crate) fn add_clip(&self, vod_id: i64, from: &str, to: &str) -> CmdResult<Clip> {
        self.run(crate::clips::add_clip(self.handle().clone(), vod_id, at(from), at(to), None))
    }

    /// Clip `id` once the clips' queue is done with it: made, failed or
    /// canceled.
    pub(crate) fn made(&self, id: i64) -> Clip {
        eventually(&format!("clip {id} made"), || {
            let (running, waiting) = self.state().clips.listed();
            // A clip canceled as it was made says so once its job has ended.
            let unmade = self.clip(id).is_some_and(|c| matches!(c.status, ClipStatus::Waiting | ClipStatus::Cutting));
            running != Some(id) && !waiting.contains(&id) && !unmade
        });
        self.clip(id).unwrap_or_else(|| panic!("clip {id} is gone"))
    }

    /// Clip `id` as the library has it.
    pub(crate) fn clip(&self, id: i64) -> Option<Clip> {
        self.db().clip(id).unwrap()
    }

    /// The clips VOD `vod_id`'s clip bar has, in its order.
    pub(crate) fn clips(&self, vod_id: i64) -> Vec<Clip> {
        self.db().clips(vod_id).unwrap()
    }

    /// The fake media clip `clip`'s file holds (`Media`); panics if it
    /// isn't made.
    pub(crate) fn clip_media(&self, clip: &Clip) -> Media {
        assert_eq!(clip.status, ClipStatus::Ready, "clip {} isn't made: {:?}", clip.id, clip.error);
        let path = clip.file_path.as_deref().expect("a made clip's file");
        Media::read(Path::new(path)).unwrap_or_else(|| panic!("no media at {path}"))
    }
}

/// The folders whose disk takes no hard links (`TestApp::takes_no_hard_links`),
/// one per test that says so: the tests run at once, in one process.
static NO_LINKS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn no_links() -> MutexGuard<'static, Vec<PathBuf>> {
    NO_LINKS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether a hard link at `at` fails, its disk taking none.
pub(crate) fn refuses_links(at: &Path) -> bool {
    no_links().iter().any(|dir| at.starts_with(dir))
}

/// The threads the app's commands start of their own and don't wait for
/// (`started`), for the test's app to wait for (`TestApp::threads_done`):
/// one still reading the state as the app ends would read freed memory.
#[derive(Default)]
pub(crate) struct Started(Mutex<Vec<std::thread::JoinHandle<()>>>);

impl Started {
    /// The threads, though a test failed holding them.
    fn lock(&self) -> MutexGuard<'_, Vec<std::thread::JoinHandle<()>>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A command of `app`'s started `thread`, which it doesn't wait for: the
/// test's app waits for it before it ends.
pub(crate) fn started(app: &AppHandle, thread: std::thread::JoinHandle<()>) {
    // An app the kit didn't start has no list: the thread is its test's.
    if let Some(started) = app.try_state::<Started>() {
        started.lock().push(thread);
    }
}

/// The details the fake site serves of site VOD `vod` (`yt_dlp::details`),
/// its channel among them.
fn details(vod: &Vod) -> Value {
    let info = &vod.info;
    let mut details =
        yt_dlp::details(&info.platform, &info.platform_id, &info.url, &info.title, info.duration.unwrap_or_default());
    if let Some(channel) = &info.channel {
        details["channel"] = channel.as_str().into();
    }
    details
}

/// The app's worker, a sidecar beside the app (`tools::sidecar`), is at one
/// path for every test in this process, so one fake there passes each run
/// on to its test's own (`Worker`), found by the model folder the app gives
/// it, which is in its library.
fn routes() -> MutexGuard<'static, Vec<(PathBuf, PathBuf)>> {
    static ROUTES: Mutex<Vec<(PathBuf, PathBuf)>> = Mutex::new(Vec::new());
    ROUTES.lock().unwrap_or_else(|e| e.into_inner())
}

/// A fake worker for the library at `paths`, its model installed and each
/// run of the sidecar for it passed on to it (`relay`).
fn fake_worker(paths: &vodscribe_core::Paths) -> Worker {
    static SIDECAR: OnceLock<Fake> = OnceLock::new();
    SIDECAR.get_or_init(|| {
        let exe = std::env::current_exe().unwrap();
        fakes::fake(exe.with_file_name(format!("vodscribe-worker{}", std::env::consts::EXE_SUFFIX)), relay)
    });
    install_model(&paths.model_dir());
    let worker = Worker::at(paths.root.with_file_name("worker").join("vodscribe-worker"));
    routes().push((paths.model_dir(), worker.path().to_path_buf()));
    worker
}

/// The model the CPU worker reads, as installed (`model::is_installed`
/// goes by each file's size): its files hard links to one set of empty
/// files that size, made once in the system's temporary folder.
fn install_model(dir: &Path) {
    use vodscribe_core::model::{CPU_MODEL, is_installed};
    static FILES: OnceLock<PathBuf> = OnceLock::new();
    if is_installed(dir, &CPU_MODEL) {
        return;
    }
    let files = FILES.get_or_init(|| {
        let files = std::env::temp_dir().join(format!("vodscribe-test-model-{}", std::process::id()));
        std::fs::create_dir_all(&files).unwrap();
        for file in CPU_MODEL.files {
            std::fs::File::create(files.join(file.name)).unwrap().set_len(file.size).unwrap();
        }
        files
    });
    std::fs::create_dir_all(dir).unwrap();
    for file in CPU_MODEL.files {
        std::fs::hard_link(files.join(file.name), dir.join(file.name)).unwrap();
    }
}

/// Runs the test's own worker with the arguments the app gave, passing on
/// what it writes, and stops it when the app stops this run.
fn relay(run: &mut Run) -> i32 {
    let model = run.after("--model").map(PathBuf::from);
    let Some(to) = routes().iter().find(|(m, _)| Some(m) == model.as_ref()).map(|(_, w)| w.clone()) else {
        run.err("Error: no test's worker for this library\n");
        return 1;
    };
    let mut cmd = vodscribe_core::tools::command(&to);
    cmd.args(run.args());
    let mut child = vodscribe_core::tools::spawn(&mut cmd).unwrap();
    let errors = vodscribe_core::tools::drain(child.stderr.take().unwrap());
    let (tx, rx) = mpsc::channel();
    vodscribe_core::tools::send_lines(child.stdout.take().unwrap(), tx);
    loop {
        match rx.recv_timeout(Duration::from_millis(2)) {
            Ok(line) => run.out(&format!("{line}\n")),
            Err(mpsc::RecvTimeoutError::Timeout) if run.stopped() => {
                child.kill_tree();
                return 255;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().unwrap();
    run.err(&errors.join().unwrap_or_default());
    status.code().unwrap_or(255)
}

/// Where in the app's own code a test holds it at a gate.
#[derive(Clone, Copy, PartialEq)]
enum Held {
    /// A pasted clip's copy of its file, written (`TestApp::holds_copies`).
    Copies,
    /// A clip's cut, its encode over (`TestApp::holds_cut_ends`).
    CutEnds,
    /// `forget_gone_clips`, its files looked at (`TestApp::holds_gone_looks`).
    GoneLooks,
    /// An erasure between its parts (`TestApp::holds_erasures`).
    BetweenParts,
    /// An erasure's part erased, the library held (`TestApp::holds_erasure_parts`).
    InAPart,
    /// An edit's plan worked out, not yet played (`TestApp::holds_plans`).
    Planned,
    /// The worker's start (`Folder::holds_worker_start`).
    WorkerStart,
}

/// The gates the app's own code waits at, each with where (`Held`) and the
/// clips folder it's for: the tests run at once, in one process.
static HELD: Mutex<Vec<(Held, PathBuf, Gate)>> = Mutex::new(Vec::new());

fn held() -> MutexGuard<'static, Vec<(Held, PathBuf, Gate)>> {
    HELD.lock().unwrap_or_else(|e| e.into_inner())
}

/// Waits at the gate `at` is held at for file `file`'s clips folder, if its
/// test holds it there.
fn hold(at: Held, file: &Path) {
    let gate = held().iter().find(|(h, dir, _)| *h == at && file.starts_with(dir)).map(|(_, _, gate)| gate.clone());
    if let Some(gate) = gate {
        gate.wait();
    }
}

/// A pasted clip's file was just copied to `to`: it waits there if its
/// test holds copies (`TestApp::holds_copies`).
pub(crate) fn copied(to: &Path) {
    hold(Held::Copies, to);
}

/// A clip's encode into `out` is over: it waits there if its test holds
/// cuts' ends (`TestApp::holds_cut_ends`).
pub(crate) fn cut_ended(out: &Path) {
    hold(Held::CutEnds, out);
}

/// `forget_gone_clips` has looked at the files in `clips`: it waits there if
/// its test holds those looks (`TestApp::holds_gone_looks`).
pub(crate) fn gone_looked(clips: &Path) {
    hold(Held::GoneLooks, clips);
}

/// An erasure has erased a part and let go of the library: it waits there
/// if its test holds erasures (`TestApp::holds_erasures`).
pub(crate) fn between_parts(app: &AppHandle) {
    hold(Held::BetweenParts, &app.state::<AppState>().paths.clips());
}

/// An erasure has erased a part, the library still held: it waits there if
/// its test holds erasures' parts (`TestApp::holds_erasure_parts`).
pub(crate) fn in_a_part(app: &AppHandle) {
    hold(Held::InAPart, &app.state::<AppState>().paths.clips());
}

/// An edit's plan is worked out: it waits there if its test holds plans
/// (`TestApp::holds_plans`).
pub(crate) fn planned(app: &AppHandle) {
    hold(Held::Planned, &app.state::<AppState>().paths.clips());
}

/// The gate the worker's start waits at in the library whose clips folder
/// is `clips`, if its test holds it there (`Folder::holds_worker_start`).
pub(crate) fn worker_start_held(clips: &Path) -> Option<Gate> {
    held().iter().find(|(h, dir, _)| *h == Held::WorkerStart && dir == clips).map(|(_, _, gate)| gate.clone())
}

/// The app's clock in a test (`TestApp::clock`): it stands still till the
/// test moves it, so what goes by the time (a cut's progress, sent at most
/// every quarter second) happens when the test says. Each app's own.
pub(crate) struct Clock(Mutex<Instant>);

impl Default for Clock {
    fn default() -> Self {
        Clock(Mutex::new(Instant::now()))
    }
}

impl Clock {
    /// The time now, as the app reads it.
    pub(crate) fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }

    /// Moves it on by `by`.
    pub(crate) fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

/// The app's `Clock`.
pub(crate) fn clock(app: &AppHandle) -> &Clock {
    app.state::<Clock>().inner()
}

/// The variables the app was started with, in a test (`TestApp::started_with`):
/// the test's own, none of the process's.
#[derive(Default)]
pub(crate) struct Env {
    vars: Mutex<HashMap<String, String>>,
}

impl Env {
    /// Variable `name`, if the test set it.
    pub(crate) fn var(&self, name: &str) -> Option<String> {
        self.vars.lock().unwrap().get(name).cloned()
    }
}

/// The app's `Env` (system.rs).
pub(crate) fn env(app: &AppHandle) -> &Env {
    app.state::<Env>().inner()
}

/// The screen the player draws on, as the tests have it (screen.rs): none
/// till the test gives the window one (`TestApp::with_screen`); then the
/// frame it shows is kept, and once it's closed, for good, it shows nothing
/// more, as the real one.
#[derive(Default)]
pub(crate) struct Screen {
    attached: AtomicBool,
    closed: AtomicBool,
    showing: Mutex<Option<Arc<vodscribe_player::decode::Frame>>>,
    shows: AtomicU64,
}

impl Screen {
    pub(crate) fn attached(&self) -> bool {
        self.attached.load(Ordering::SeqCst)
    }

    /// The frame on it: None while it's black, or before anything's shown.
    pub(crate) fn showing(&self) -> Option<Arc<vodscribe_player::decode::Frame>> {
        self.showing.lock().unwrap().clone()
    }

    /// The number in its file of the fake video frame on it
    /// (`media::frame_number`), once one shows (waited for).
    pub(crate) fn frame_shown(&self) -> u64 {
        eventually("a frame on the screen", || self.showing().is_some());
        frame_number(&self.showing().unwrap())
    }

    /// How many times it's been shown something, a frame or black.
    pub(crate) fn shows(&self) -> u64 {
        self.shows.load(Ordering::SeqCst)
    }

    /// Whether it's been taken away for good (`screen::close`).
    pub(crate) fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub(crate) fn show(&self, frame: Option<Arc<vodscribe_player::decode::Frame>>) {
        // Under the frame's lock, as `close` sets it closed: none comes after.
        let mut showing = self.showing.lock().unwrap();
        if self.attached() && !self.closed() {
            *showing = frame;
            self.shows.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub(crate) fn close(&self) {
        let _showing = self.showing.lock().unwrap();
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// The app's `Screen` (screen.rs).
pub(crate) fn screen(app: &AppHandle) -> &Screen {
    app.state::<Screen>().inner()
}

/// The number in its file of fake video frame `frame`, as the fake ffmpeg
/// decodes it (`media::numbered_frame`).
pub(crate) fn frame_number(frame: &vodscribe_player::decode::Frame) -> u64 {
    vodscribe_core::testkit::media::frame_number(&frame.yuv, frame.width, frame.height, 0)
        .expect("a fake video's frame")
}

/// `stamp` (`0:10`, `1:02:03.5`, `75`) in seconds.
pub(crate) fn at(stamp: &str) -> f64 {
    stamp
        .split(':')
        .try_fold(0.0, |total, part| part.parse::<f64>().map(|n| total * 60.0 + n))
        .expect("a time as 1:02:03")
}

/// The system's desktop as the tests have it (system.rs): what it was asked
/// to show and open.
#[derive(Default)]
pub(crate) struct Desktop {
    shown: Mutex<Vec<PathBuf>>,
    opened: Mutex<Vec<PathBuf>>,
}

impl Desktop {
    /// The files shown in the file manager, in order.
    pub(crate) fn shown(&self) -> Vec<PathBuf> {
        self.shown.lock().unwrap().clone()
    }

    /// The files and folders opened, in order.
    pub(crate) fn opened(&self) -> Vec<PathBuf> {
        self.opened.lock().unwrap().clone()
    }

    pub(crate) fn reveal(&self, path: &Path) -> CmdResult<()> {
        self.shown.lock().unwrap().push(path.into());
        Ok(())
    }

    pub(crate) fn open(&self, path: &Path) -> CmdResult<()> {
        self.opened.lock().unwrap().push(path.into());
        Ok(())
    }
}

/// The app's `Desktop` (system.rs).
pub(crate) fn desktop(app: &AppHandle) -> &Desktop {
    app.state::<Desktop>().inner()
}

/// The system's drags out of the window as the tests have them
/// (drag_source): each begun is kept (`last`) until the test lets it go
/// where it says (`Drag::let_go`). The drag offers the files as the
/// system's do (`dragndrop_source`): each asked after as it's made, the
/// receiver told how it went, and the app hears the end as it would.
#[derive(Default)]
pub(crate) struct Drags {
    /// The next drag is refused: the button came up before it could begin.
    early: AtomicBool,
    begun: Mutex<Vec<Arc<Drag>>>,
    /// The app is ending: receivers waiting for a file stop.
    ended: Arc<AtomicBool>,
}

impl Drags {
    /// The button comes up before the next drag can begin, so it doesn't
    /// (`ReleasedEarly`).
    pub(crate) fn releases_early(&self) {
        self.early.store(true, Ordering::SeqCst);
    }

    /// How many drags have begun.
    pub(crate) fn begun(&self) -> usize {
        self.begun.lock().unwrap().len()
    }

    /// The last drag begun; panics if none has.
    pub(crate) fn last(&self) -> Arc<Drag> {
        self.begun.lock().unwrap().last().cloned().expect("a drag begun")
    }

    /// The app's end: each receiver stops waiting, and is waited for. What
    /// the drags hold of the app goes with them.
    fn end(&self) {
        self.ended.store(true, Ordering::SeqCst);
        let begun = std::mem::take(&mut *self.begun.lock().unwrap());
        for drag in begun {
            if let Some(receiving) = drag.receiving.lock().unwrap().take() {
                let _ = receiving.join();
            }
        }
    }

    fn start(
        &self,
        items: Vec<Box<dyn crate::drag_source::Item>>,
        image: &[u8],
        resolve: Option<Arc<crate::resolve::Watch>>,
        on: impl Fn(crate::drag_source::Event) + Send + Sync + 'static,
    ) -> anyhow::Result<()> {
        if self.early.swap(false, Ordering::SeqCst) {
            return Err(crate::drag_source::ReleasedEarly.into());
        }
        let drag = Drag {
            items,
            image: image.to_vec(),
            resolve,
            on: Box::new(on),
            given: Mutex::default(),
            app_ended: self.ended.clone(),
            receiving: Mutex::default(),
        };
        self.begun.lock().unwrap().push(Arc::new(drag));
        Ok(())
    }
}

/// Begins a drag of `items` out of `window`, showing `image`, as each
/// system's `drag_source::start` does: the tests' (`Drags`). On Linux,
/// `resolve` refuses the files to DaVinci Resolve as it asks.
pub(crate) fn start_drag(
    window: &WebviewWindow,
    items: Vec<Box<dyn crate::drag_source::Item>>,
    image: &[u8],
    #[cfg(target_os = "linux")] resolve: Arc<crate::resolve::Watch>,
    on: impl Fn(crate::drag_source::Event) + Send + Sync + 'static,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    let resolve = Some(resolve);
    #[cfg(not(target_os = "linux"))]
    let resolve = None;
    window.app_handle().state::<Drags>().start(items, image, resolve, on)
}

/// Where a drag out of the window is let go (`Drag::let_go`).
pub(crate) enum LetGo {
    /// Called off (Escape).
    CalledOff,
    /// Back on the app's own window.
    Back,
    /// On nothing, or an app that refused it.
    Nowhere,
    /// On another app, which asks for the files.
    On(Receiver),
}

/// An app a drag is let go on (`LetGo::On`).
pub(crate) struct Receiver {
    /// Its name, where the system says (X11's window class); None where
    /// it doesn't (Wayland).
    pub(crate) name: Option<String>,
    pub(crate) takes: Takes,
}

/// What an app a drag is let go on takes of its files.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Takes {
    /// The files themselves, by their addresses, waiting for those still
    /// being made (a file manager, most editors).
    Files,
    /// Copies of them written for it, waiting for those still being made
    /// (a macOS file promise, Explorer's virtual files): still the app's
    /// own files.
    Copies,
    /// Only files already made: one still being made, it takes none.
    FinishedOnly,
}

impl Receiver {
    /// An app named `name` that takes the files themselves (`Takes::Files`).
    pub(crate) fn taking_files(name: &str) -> Self {
        Receiver { name: Some(name.into()), takes: Takes::Files }
    }

    /// An app named `name` that takes copies written for it (`Takes::Copies`).
    pub(crate) fn taking_copies(name: &str) -> Self {
        Receiver { name: Some(name.into()), takes: Takes::Copies }
    }

    /// An app named `name` that takes only files already made.
    pub(crate) fn finished_only(name: &str) -> Self {
        Receiver { name: Some(name.into()), takes: Takes::FinishedOnly }
    }
}

/// A drag out of the window, begun (`Drags::last`): what it offers, and
/// what the app it was let go on was given.
pub(crate) struct Drag {
    items: Vec<Box<dyn crate::drag_source::Item>>,
    image: Vec<u8>,
    resolve: Option<Arc<crate::resolve::Watch>>,
    on: Box<dyn Fn(crate::drag_source::Event) + Send + Sync>,
    given: Mutex<Vec<PathBuf>>,
    app_ended: Arc<AtomicBool>,
    /// The thread that plays where it was let go, till the drag is over.
    receiving: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Drag {
    /// The names its files go by where they're dropped, in order.
    pub(crate) fn names(&self) -> Vec<String> {
        self.items.iter().map(|item| item.name()).collect()
    }

    /// Its files' types, as macOS names them.
    pub(crate) fn file_types(&self) -> Vec<String> {
        self.items.iter().map(|item| item.file_type()).collect()
    }

    /// The picture shown under the pointer (a PNG).
    pub(crate) fn image(&self) -> &[u8] {
        &self.image
    }

    /// The files the app it was let go on was given, in order: none till
    /// it has every one.
    pub(crate) fn given(&self) -> Vec<PathBuf> {
        self.given.lock().unwrap().clone()
    }

    /// Lets it go `where`. On another app, it's heard as let go there at
    /// once, as the system says on its own thread; that app then asks for
    /// the files, waiting for those still being made if it does
    /// (`Takes`), and the drag is over once it has them or one fails. The
    /// end is heard on a thread of its own, as the system's is
    /// (`TestApp::drag_ended` waits for it).
    pub(crate) fn let_go(self: &Arc<Self>, to: LetGo) {
        use crate::drag_source::Event;
        if let LetGo::On(_) = to {
            (self.on)(Event::LetGo);
        }
        let drag = self.clone();
        let receiving = std::thread::spawn(move || {
            let Some(outcome) = drag.receive(to) else { return };
            (drag.on)(Event::Ended(outcome));
        });
        *self.receiving.lock().unwrap() = Some(receiving);
    }

    /// How the drag let go `to` ends; None if the app ended first.
    fn receive(&self, to: LetGo) -> Option<dragndrop_source::Outcome> {
        use dragndrop_source::{Ending, Failure, Outcome};
        let nothing =
            |ending| Outcome { ending, delivered: false, copied: false, failure: None, made: false, receiver: None };
        let receiver = match to {
            LetGo::CalledOff => return Some(nothing(Ending::CalledOff)),
            LetGo::Back => return Some(nothing(Ending::Back)),
            LetGo::Nowhere => return Some(nothing(Ending::Nowhere)),
            LetGo::On(receiver) => receiver,
        };
        // On Linux, as `drag_source::linux` asks: Resolve, told by its
        // window's class, is refused the files, and refuses the drop.
        if let Some(resolve) = self.resolve.as_ref().filter(|r| r.applies()) {
            match receiver.name.as_deref() {
                Some(class) if resolve.refuses(Some(class), None) => return Some(nothing(Ending::Nowhere)),
                Some(_) => {}
                None => resolve.unnamed(),
            }
        }
        let mut told = false;
        let (given, failure) = loop {
            match self.settled() {
                Some(Ok(paths)) => break (Some(paths), None),
                Some(Err(why)) => break (None, Some(Failure::Failed(why))),
                // Answered with none, as it asked.
                None if receiver.takes == Takes::FinishedOnly => break (None, None),
                None => {
                    if !told {
                        told = true;
                        for item in &self.items {
                            if matches!(item.state(), crate::drag_source::State::Making(_)) {
                                item.waited_for();
                            }
                        }
                    }
                    if self.app_ended.load(Ordering::SeqCst) {
                        return None;
                    }
                    // As the system's hold looks again at each tick.
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        };
        let delivered = given.is_some();
        *self.given.lock().unwrap() = given.unwrap_or_default();
        Some(Outcome {
            ending: Ending::Elsewhere,
            delivered,
            copied: delivered && receiver.takes == Takes::Copies,
            failure,
            made: matches!(self.settled(), Some(Ok(_))),
            receiver: receiver.name,
        })
    }

    /// Its files' paths if all are made; else, if one won't be, why; else
    /// (some still being made) None.
    fn settled(&self) -> Option<Result<Vec<PathBuf>, String>> {
        use crate::drag_source::State;
        let mut paths = Vec::new();
        let mut making = false;
        for item in &self.items {
            match item.state() {
                State::Ready(path) => paths.push(path),
                State::Making(_) => making = true,
                State::Failed(why) => return Some(Err(why)),
            }
        }
        (!making).then_some(Ok(paths))
    }
}

mod tests;
