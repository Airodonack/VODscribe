//! Commands for what the app needs to run: first-run setup, settings, the GPU
//! pack and downloaded files.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::Status;
use vodscribe_core::gpu::{self, GpuInfo};
use vodscribe_core::settings::Settings;
use vodscribe_core::{Progress, deps, model, tools};

use crate::{AppState, CmdResult, err};

#[derive(Serialize)]
pub(crate) struct SetupStatus {
    yt_dlp: bool,
    /// yt-dlp is the app's own copy, verified when downloaded, rather than one found on PATH.
    yt_dlp_managed: bool,
    ffmpeg: bool,
    /// ffmpeg is the app's own checked copy (in its bin dir).
    ffmpeg_managed: bool,
    model: bool,
    js_runtime: bool,
    js_runtime_managed: bool,
    /// Setup can download ffmpeg and Deno on this platform.
    can_install_tools: bool,
    /// Bytes of the speech model setup downloads.
    model_bytes: u64,
    running: bool,
}

#[tauri::command]
pub(crate) async fn setup_status(state: State<'_, AppState>) -> CmdResult<SetupStatus> {
    let bin = state.paths.bin();
    Ok(SetupStatus {
        yt_dlp: tools::find_yt_dlp(&bin).is_some(),
        yt_dlp_managed: tools::managed_yt_dlp(&bin).is_file(),
        ffmpeg: tools::find_ffmpeg(&bin).is_some(),
        ffmpeg_managed: deps::managed_path(&bin, "ffmpeg").is_file(),
        model: model::is_installed(&state.paths.model_dir(), &model::CPU_MODEL),
        js_runtime: tools::find_js_runtime(&bin).is_some(),
        js_runtime_managed: deps::managed_path(&bin, "deno").is_file(),
        can_install_tools: deps::ffmpeg().is_some() && deps::deno().is_some(),
        model_bytes: model::CPU_MODEL.total_bytes(),
        running: state.setup.running(),
    })
}

/// Whether this copy can update itself. On Linux the updater looks up the file
/// for the package this copy came from (.deb or .rpm), which the bundler stamps
/// into the binary, and latest.json has no file for anything else. A binary that
/// was never packaged (a development build, or one built from source) has no
/// stamp, and no file to fetch. Windows and macOS have a file for any copy.
///
/// VODSCRIBE_NO_UPDATE_CHECK (set by the end-to-end tests) makes it false, so
/// the app never asks for latest.json: test runs don't count as people using it.
#[tauri::command]
pub(crate) async fn self_updatable() -> CmdResult<bool> {
    if std::env::var_os("VODSCRIBE_NO_UPDATE_CHECK").is_some() {
        return Ok(false);
    }
    Ok(cfg!(not(target_os = "linux")) || tauri::utils::platform::bundle_type().is_some())
}

/// Where the library and settings live, for the privacy section in Settings.
#[tauri::command]
pub(crate) async fn data_dir(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(state.paths.root.display().to_string())
}

/// Opens the data folder in the system file manager. It takes no path, so the
/// UI can't use it to open anything else.
#[tauri::command]
pub(crate) async fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(state.paths.root.display().to_string(), None::<String>).map_err(err)
}

/// A `setup-progress` event: which item, and its `Progress`.
#[derive(Clone, Serialize)]
struct ItemProgress<'a> {
    item: &'a str,
    #[serde(flatten)]
    progress: Progress<'a>,
}

/// Downloads what's missing. Progress arrives as `setup-progress` events
/// (`ItemProgress`), and completion as `setup-done` (an error message or null).
#[tauri::command]
pub(crate) async fn run_setup(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let paths = state.paths.clone();
    let busy = "VODscribe is updating or repairing its tools. Try again in a moment.";
    state.setup.spawn(app, "setup-progress", "setup-done", busy, move |app, cancel, report| {
        let progress = |item: &str, progress: Progress| report.send(ItemProgress { item, progress });
        let bin = paths.bin();
        if tools::find_yt_dlp(&bin).is_none() {
            tools::install_yt_dlp(&bin, cancel, |p| progress("yt-dlp", p))?;
        }
        if tools::find_ffmpeg(&bin).is_none()
            && let Some(dep) = deps::ffmpeg()
        {
            deps::install(&dep, &bin, cancel, |p| progress("ffmpeg", p))?;
        }
        model::install(&paths.models(), &model::CPU_MODEL, cancel, |p| progress("model", p))?;
        model::remove_others(&paths.models(), &[&model::CPU_MODEL, &model::GPU_MODEL]);
        app.state::<AppState>().worker.wake();
        // Last, since it's optional: only some YouTube VODs need it.
        // If it fails, setup still succeeded; YouTube links that need it will say so.
        if tools::find_js_runtime(&bin).is_none()
            && let Some(dep) = deps::deno()
            && let Err(e) = deps::install(&dep, &bin, cancel, |p| progress("deno", p))
        {
            eprintln!("Deno wasn't installed: {e:#}");
        }
        Ok(())
    })
}

/// A long download's flags: `held` while it's under way (one at a time) and
/// `cancel`, which `cancel()` sets to stop it. Canceling isn't an error: the
/// done event carries null. Tools and model files that finished stay; the GPU
/// pack starts over, since its parts are only kept together.
pub(crate) struct Install {
    /// `FREE`, `INSTALLING`, or `CLAIMED`: held by something other than an
    /// install (a yt-dlp update, the add-on being removed), so no done event
    /// follows. One value, so taking it and saying by whom is one step.
    held: Arc<AtomicU8>,
    cancel: Arc<AtomicBool>,
    /// The latest progress event's payload while running (see `Reporter`).
    last: Arc<Mutex<Option<serde_json::Value>>>,
}

/// Sends a running install's progress events, keeping the latest, so a panel
/// opened partway through (Settings closes and reopens) shows where it is
/// rather than waiting for the next event, which can be a while between files.
///
/// Downloads report every chunk read, thousands a second on a fast link, so
/// events go out at most every `INTERVAL`, and at once when the step changes
/// (a new file, a check), which is when it reads differently.
pub(crate) struct Reporter {
    app: AppHandle,
    event: &'static str,
    last: Arc<Mutex<Option<serde_json::Value>>>,
    /// When the last event went out, and its step.
    sent: Mutex<Option<(Instant, Option<String>)>>,
}

impl Reporter {
    const INTERVAL: Duration = Duration::from_millis(250);

    pub(crate) fn new(app: AppHandle, event: &'static str, last: Arc<Mutex<Option<serde_json::Value>>>) -> Self {
        Reporter { app, event, last, sent: Mutex::new(None) }
    }

    pub(crate) fn send(&self, payload: impl Serialize) {
        let Ok(value) = serde_json::to_value(payload) else { return };
        let step = value.get("step").and_then(|s| s.as_str()).map(str::to_string);
        *self.last.lock().unwrap() = Some(value.clone());
        let mut sent = self.sent.lock().unwrap();
        let due = match &*sent {
            Some((at, last_step)) => *last_step != step || at.elapsed() >= Self::INTERVAL,
            None => true,
        };
        if due {
            *sent = Some((Instant::now(), step));
            let _ = self.app.emit(self.event, value);
        }
    }
}

const FREE: u8 = 0;
const INSTALLING: u8 = 1;
const CLAIMED: u8 = 2;

impl Install {
    pub(crate) fn new() -> Self {
        Self {
            held: Arc::new(AtomicU8::new(FREE)),
            cancel: Arc::new(AtomicBool::new(false)),
            last: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn running(&self) -> bool {
        self.held.load(Ordering::Relaxed) != FREE
    }

    /// The latest progress the running install reported, if any.
    pub(crate) fn progress(&self) -> Option<serde_json::Value> {
        self.last.lock().unwrap().clone()
    }

    /// Stops the download under way; it ends within a moment. With none running
    /// this sets nothing, so a click can't carry over into the next download.
    pub(crate) fn cancel(&self) {
        if self.running() {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Runs `install` on its own thread, unless one is already running, with a
    /// `Reporter` for its `progress_event`s. Then frees `held` and emits
    /// `done_event` with an error message, or null when it finished or was canceled.
    /// With one running already its done event comes as for this one; held
    /// by something else (`busy`), none would, so that's an error.
    pub(crate) fn spawn(
        &self,
        app: AppHandle,
        progress_event: &'static str,
        done_event: &'static str,
        busy: &'static str,
        install: impl FnOnce(&AppHandle, &AtomicBool, &Reporter) -> anyhow::Result<()> + Send + 'static,
    ) -> CmdResult<()> {
        // Cleared before `held` is taken, so a cancel that lands as this
        // starts is kept rather than wiped.
        self.cancel.store(false, Ordering::Relaxed);
        match self.held.compare_exchange(FREE, INSTALLING, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => {}
            Err(CLAIMED) => return Err(busy.into()),
            Err(_) => return Ok(()),
        }
        *self.last.lock().unwrap() = None;
        let (held, cancel, last) = (self.held.clone(), self.cancel.clone(), self.last.clone());
        std::thread::spawn(move || {
            let report = Reporter::new(app.clone(), progress_event, last.clone());
            let result = install(&app, &cancel, &report);
            *last.lock().unwrap() = None;
            held.store(FREE, Ordering::Relaxed);
            let error = result.err().filter(|e| !vodscribe_core::is_canceled(e)).map(err);
            let _ = app.emit(done_event, error);
        });
        Ok(())
    }
}

#[tauri::command]
/// Updates yt-dlp, reporting its steps as `yt-dlp-progress` events (`Progress`).
pub(crate) async fn update_yt_dlp(app: AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    // Setup installs yt-dlp too; the two mustn't write the same file at once.
    let Some(_setup) = Claim::take(&state.setup) else {
        return Err("Setup is still downloading. Try again when it's done.".into());
    };
    update_yt_dlp_now(app, state).await
}

async fn update_yt_dlp_now(app: AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    let bin = state.paths.bin();
    tauri::async_runtime::spawn_blocking(move || {
        tools::update_yt_dlp(&bin, |p| {
            let _ = app.emit("yt-dlp-progress", p);
        })
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[tauri::command]
pub(crate) async fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(Settings::load(&state.paths.settings()))
}

/// Puts every setting back as it starts (the settings file goes), keeping
/// everything else; returns them as they are now.
#[tauri::command]
pub(crate) async fn reset_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    let path = state.paths.settings();
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(err(e)),
    }
    Ok(Settings::load(&path))
}

/// Takes effect from the next VOD; one already transcribing keeps its device.
#[tauri::command]
pub(crate) async fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<()> {
    settings.save(&state.paths.settings()).map_err(err)
}

#[derive(Serialize)]
pub(crate) struct GpuStatus {
    /// A GPU pack exists for this OS.
    supported: bool,
    installed: bool,
    installing: bool,
    /// While installing, the latest `gpu-progress` event's payload.
    progress: Option<serde_json::Value>,
    /// The GPU worker binary is part of this build.
    worker: bool,
    gpu: Option<GpuInfo>,
    min_free_mb: u64,
    /// Bytes the add-on downloads: the pack and the GPU speech model.
    download_bytes: u64,
}

#[tauri::command]
pub(crate) async fn gpu_status(state: State<'_, AppState>) -> CmdResult<GpuStatus> {
    let pack = state.paths.gpu_pack();
    let gpu = tauri::async_runtime::spawn_blocking(gpu::detect).await.map_err(err)?;
    Ok(GpuStatus {
        supported: gpu::supported(),
        installed: gpu::is_installed(&pack) && model::is_installed(&state.paths.gpu_model_dir(), &model::GPU_MODEL),
        installing: state.gpu.running(),
        progress: state.gpu.progress(),
        worker: tools::sidecar("vodscribe-worker-cuda").is_ok(),
        gpu,
        min_free_mb: gpu::MIN_FREE_MB,
        // Only what's missing, such as just the pack after an update changes it.
        download_bytes: if gpu::is_installed(&pack) { 0 } else { gpu::download_size() }
            + if model::is_installed(&state.paths.gpu_model_dir(), &model::GPU_MODEL) {
                0
            } else {
                model::GPU_MODEL.total_bytes()
            },
    })
}

/// Downloads the GPU pack and model. Progress arrives as `gpu-progress` events
/// (`Progress`) and completion as `gpu-done` (an error message or null).
#[tauri::command]
pub(crate) async fn install_gpu_pack(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let paths = state.paths.clone();
    let busy = "The GPU add-on is being removed or repaired. Try again in a moment.";
    state.gpu.spawn(app, "gpu-progress", "gpu-done", busy, move |_, cancel, report| {
        let progress = |p: Progress| report.send(p);
        // One count of bytes and of files across the pack and the model, so the
        // bar only moves forward and the files read "(1 of 10)" to "(10 of 10)".
        let pack_needed = !gpu::is_installed(&paths.gpu_pack());
        let (pack_total, pack_parts) = if pack_needed { (gpu::download_size(), gpu::download_parts()) } else { (0, 0) };
        // The model counts only if it's missing too; install returns at once if not.
        let (model_total, model_parts) = if model::is_installed(&paths.gpu_model_dir(), &model::GPU_MODEL) {
            (0, 0)
        } else {
            (model::GPU_MODEL.total_bytes(), model::GPU_MODEL.files.len())
        };
        let total_bytes = pack_total + model_total;
        let total = Some(total_bytes);
        let parts = pack_parts + model_parts;
        if pack_needed {
            gpu::install(&paths.gpu_pack(), cancel, |p| progress(Progress { total, parts, ..p }))?;
        }
        model::install(&paths.models(), &model::GPU_MODEL, cancel, |p| {
            progress(Progress { done: pack_total + p.done, total, part: pack_parts + p.part, parts, ..p })
        })?;
        progress(Progress {
            done: total_bytes,
            total,
            part: parts,
            parts,
            ..Progress::step("Every file matches its SHA‑256")
        });
        Ok(())
    })
}

/// Stops setup's downloads; what already finished stays installed.
#[tauri::command]
pub(crate) async fn cancel_setup(state: State<'_, AppState>) -> CmdResult<()> {
    state.setup.cancel();
    Ok(())
}

/// Stops the GPU add-on's download.
#[tauri::command]
pub(crate) async fn cancel_gpu_install(state: State<'_, AppState>) -> CmdResult<()> {
    state.gpu.cancel();
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_gpu_pack(state: State<'_, AppState>) -> CmdResult<()> {
    // Held till it's gone, so an install can't start on it meanwhile.
    let Some(_removing) = Claim::take(&state.gpu) else {
        return Err("The GPU add-on is still downloading.".into());
    };
    // A transcription may have the pack's libraries loaded, including one the
    // transcribe stage has picked but not yet marked as transcribing. Nothing
    // new starts until the pack is gone.
    let (_held, running) = state.worker.hold_starts();
    let db = state.db.lock().unwrap();
    for id in running {
        if matches!(db.stretch(id).map_err(err)?.map(|s| s.status), Some(Status::Ready | Status::Transcribing)) {
            return Err("Wait for the current transcription to finish, or pause it, first.".into());
        }
    }
    drop(db);
    gpu::uninstall(&state.paths.gpu_pack()).map_err(err)?;
    gpu::uninstall(&state.paths.gpu_model_dir()).map_err(err)
}

/// Bytes of the videos kept from sites without embeds (`Paths::media`).
#[tauri::command]
pub(crate) async fn media_size(state: State<'_, AppState>) -> CmdResult<u64> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || paths.media_size()).await.map_err(err)
}

/// Saves what this computer's webview plays (playable.ts asks it at each
/// start), for the formats of videos kept from other sites.
#[tauri::command]
pub(crate) async fn set_playable(
    state: State<'_, AppState>,
    playable: vodscribe_core::playable::Playable,
) -> CmdResult<()> {
    playable.save(&state.paths.playable()).map_err(err)
}

/// Bytes of downloaded files that `clear_cache` would delete.
#[tauri::command]
pub(crate) async fn cache_size(state: State<'_, AppState>) -> CmdResult<u64> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || paths.cache_size()).await.map_err(err)
}

/// Deletes everything that can be downloaded again (models, tools, the GPU
/// pack, unfinished downloads), pausing running jobs first. The library,
/// transcripts and settings are kept; setup runs again afterwards.
#[tauri::command]
pub(crate) async fn clear_cache(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    // Claimed for the whole clear, so setup, a yt-dlp update or the GPU add-on
    // can't start downloading into folders that are being deleted.
    let claims = (Claim::take(&state.setup), Claim::take(&state.gpu));
    let (Some(_setup), Some(_gpu)) = claims else {
        return Err("Wait for the current download to finish first.".into());
    };
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        // Nothing may start while files are deleted; the hold ends when `_held` drops.
        let _held = state.worker.hold(std::time::Duration::from_secs(15)).map_err(err)?;
        state.paths.clear_cache().map_err(err)?;
        state.db.lock().unwrap().forget_downloads().map_err(err)
    })
    .await
    .map_err(err)??;
    for vod in state.db.lock().unwrap().vods().map_err(err)? {
        let _ = app.emit("vod", vod);
    }
    Ok(())
}

/// An `Install`'s busy flag, held by something other than an install until
/// dropped.
struct Claim(Arc<AtomicU8>);

impl Claim {
    /// Holds `install`'s flag, or returns None if something already does.
    fn take(install: &Install) -> Option<Self> {
        install.held.compare_exchange(FREE, CLAIMED, Ordering::Relaxed, Ordering::Relaxed).ok()?;
        Some(Claim(install.held.clone()))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.0.store(FREE, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claim_holds_the_flag_and_says_it_isnt_an_install() {
        let install = Install::new();
        let claim = Claim::take(&install).expect("free");
        assert!(install.running() && install.held.load(Ordering::Relaxed) == CLAIMED);
        assert!(Claim::take(&install).is_none(), "one holder at a time");
        drop(claim);
        assert!(!install.running());
        // Held by an install (as spawn sets it), it isn't claimed.
        install.held.store(INSTALLING, Ordering::Relaxed);
        assert!(Claim::take(&install).is_none() && install.held.load(Ordering::Relaxed) == INSTALLING);
    }
}
