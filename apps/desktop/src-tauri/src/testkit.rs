//! The app in a test (`TestApp`): its real state and queues, as it starts
//! them, on Tauri's mock runtime, over a library in a folder of its own,
//! with ffmpeg and yt-dlp faked (`vodscribe_core::testkit`). The commands
//! are called as the page calls them, by name and with its arguments
//! (`TestApp::invoke`), or as Rust functions; what the page would hear is
//! kept (`TestApp::heard`), and what the system's desktop would be asked to
//! do is heard by `Desktop` (system.rs).
//!
//! The helpers say what a test does in the words of its spec: "a file
//! `Day 3.mp4` of a minute is added" (`add_video_file`), "a clip of
//! 0:10–0:20 is made" (`make_clip`), "the library has these clips"
//! (`clips`). A command whose arguments change is changed here, once.
//!
//! The guide to writing tests with it is crates/core/src/testkit/README.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::Value;
use tauri::{Listener, Manager};
use vodscribe_core::db::{Clip, ClipStatus, Db, Vod};
use vodscribe_core::testkit::{Ffmpeg, Media, TempDir, YtDlp, eventually, library, yt_dlp};

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
    heard: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    /// Where the test's own files go (a VOD's file on this computer),
    /// outside the library's folder.
    files: PathBuf,
    // Kept for the chat queue, which the tests don't run: what's asked of it waits.
    _chats: std::sync::mpsc::Receiver<i64>,
    // Removed once `drop` has closed what the app has open in it.
    _dir: TempDir,
}

impl Drop for TestApp {
    /// Ends the app with its test: each queue is stopped and its thread
    /// joined, so no job of this test runs on after it, and the app's state
    /// is dropped, which closes the library before its folder goes.
    fn drop(&mut self) {
        // First, so a job whose fake is held at a gate the test never
        // opened (it failed first) ends, and can be joined.
        self.ffmpeg.end();
        self.yt_dlp.end();
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
    }
}

impl TestApp {
    /// The app started on an empty library, as `lib.rs`'s `run` starts it
    /// (but with no page in its window, nor the background jobs that reach
    /// the network: thumbnails, streamer cards, chat), its queues running.
    pub(crate) fn new() -> Self {
        let dir = TempDir::new("app");
        let paths = vodscribe_core::Paths::new(dir.join("data"));
        paths.ensure().unwrap();
        let files = dir.join("files");
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
        app.manage(crate::choices::Kept::load(paths.choices()));
        app.manage(crate::recovery::Problem::none(paths.db()));
        app.manage(crate::screen::Screen::default());
        let db = Db::open_in(&paths).unwrap();
        vodscribe_core::library::settle(&db).unwrap();
        let chats = crate::manage_library(app.handle(), paths, db).unwrap();
        crate::start_queues(app.handle()).unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        TestApp { app, window, ffmpeg, yt_dlp, heard, files, _chats: chats, _dir: dir }
    }

    pub(crate) fn handle(&self) -> &AppHandle {
        self.app.handle()
    }

    pub(crate) fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
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

    /// Runs a command's future (`clips::add_clip(app.handle().clone(), …)`)
    /// to its end, as the page awaits it.
    pub(crate) fn run<T>(&self, command: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(command)
    }

    /// Calls command `cmd` as the page does, through Tauri's IPC: by name,
    /// with `args` as the page writes them (camelCase), answered as JSON.
    pub(crate) fn invoke(&self, cmd: &str, args: Value) -> Result<Value, Value> {
        let request = tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        };
        tauri::test::get_ipc_response(&self.window, request).map(|body| body.deserialize::<Value>().unwrap())
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
        self.yt_dlp.serves(yt_dlp::details(platform, id, &vod.info.url, title, duration));
        vod
    }

    /// `text` is said in VOD `vod_id` from `start` seconds (`library::said`).
    pub(crate) fn says(&self, vod_id: i64, start: f64, text: &str) {
        library::says(&mut self.db(), vod_id, library::said(start, text));
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

/// `stamp` (`0:10`, `1:02:03.5`, `75`) in seconds.
pub(crate) fn at(stamp: &str) -> f64 {
    stamp
        .split(':')
        .try_fold(0.0, |total, part| part.parse::<f64>().map(|n| total * 60.0 + n))
        .expect("a time as 1:02:03")
}

/// The system's desktop as the tests have it (system.rs): what it was asked
/// to show and open, and what its save dialog answers.
#[derive(Default)]
pub(crate) struct Desktop {
    shown: Mutex<Vec<PathBuf>>,
    opened: Mutex<Vec<PathBuf>>,
    /// Where the save dialog saves; None: the person cancels it.
    saves_to: Mutex<Option<PathBuf>>,
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

    /// From now the save dialog picks `path` (None: it's canceled).
    pub(crate) fn saves_to(&self, path: Option<PathBuf>) {
        *self.saves_to.lock().unwrap() = path;
    }

    pub(crate) fn reveal(&self, path: &Path) -> CmdResult<()> {
        self.shown.lock().unwrap().push(path.into());
        Ok(())
    }

    pub(crate) fn open(&self, path: &Path) -> CmdResult<()> {
        self.opened.lock().unwrap().push(path.into());
        Ok(())
    }

    pub(crate) fn pick_save(&self, _name: &str) -> Option<PathBuf> {
        self.saves_to.lock().unwrap().clone()
    }
}

/// The app's `Desktop` (system.rs).
pub(crate) fn desktop(app: &AppHandle) -> &Desktop {
    app.state::<Desktop>().inner()
}

mod tests;
