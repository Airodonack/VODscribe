//! Commands for what the app needs to run: first-run setup, settings, the GPU
//! pack and downloaded files.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Whether this copy can update itself: the bundler stamps each package
/// (.deb, .rpm, .msi, the NSIS installer, the macOS app) with its kind, and the
/// updater looks up that kind's file in latest.json. A binary that was never
/// packaged (a development build, or one built from source) has no stamp, and
/// no file to fetch.
#[tauri::command]
pub(crate) async fn self_updatable() -> CmdResult<bool> {
    Ok(tauri::utils::platform::bundle_type().is_some())
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
    state.setup.spawn(app, "setup-done", move |app, cancel| {
        let progress = |item: &str, progress: Progress| {
            let _ = app.emit("setup-progress", ItemProgress { item, progress });
        };
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
    });
    Ok(())
}

/// A long download's flags: `running` while it's under way (one at a time) and
/// `cancel`, which `cancel()` sets to stop it. Canceling isn't an error: the
/// done event carries null. Tools and model files that finished stay; the GPU
/// pack starts over, since its parts are only kept together.
pub(crate) struct Install {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

impl Install {
    pub(crate) fn new() -> Self {
        Self { running: Arc::new(AtomicBool::new(false)), cancel: Arc::new(AtomicBool::new(false)) }
    }

    pub(crate) fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Stops the download under way; it ends within a moment. With none running
    /// this sets nothing, so a click can't carry over into the next download.
    pub(crate) fn cancel(&self) {
        if self.running() {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Runs `install` on its own thread, unless one is already running, then
    /// clears `running` and emits `done_event` with an error message, or null
    /// when it finished or was canceled.
    pub(crate) fn spawn(
        &self,
        app: AppHandle,
        done_event: &'static str,
        install: impl FnOnce(&AppHandle, &AtomicBool) -> anyhow::Result<()> + Send + 'static,
    ) {
        // Cleared before `running` is set, so a cancel that lands as this
        // starts is kept rather than wiped.
        self.cancel.store(false, Ordering::Relaxed);
        if self.running.swap(true, Ordering::Relaxed) {
            return;
        }
        let (running, cancel) = (self.running.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let result = install(&app, &cancel);
            running.store(false, Ordering::Relaxed);
            let error = result.err().filter(|e| !vodscribe_core::is_canceled(e)).map(err);
            let _ = app.emit(done_event, error);
        });
    }
}

#[tauri::command]
/// Updates yt-dlp, reporting its steps as `yt-dlp-progress` events (`Progress`).
pub(crate) async fn update_yt_dlp(app: AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    // Setup installs yt-dlp too; the two mustn't write the same file at once.
    let Some(_setup) = Claim::take(&state.setup.running) else {
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
    state.gpu.spawn(app, "gpu-done", move |app, cancel| {
        let progress = |p: Progress| {
            let _ = app.emit("gpu-progress", p);
        };
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
    });
    Ok(())
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
    if state.gpu.running() {
        return Err("The GPU add-on is still downloading.".into());
    }
    // A transcription may have the pack's libraries loaded, including one the
    // transcribe stage has picked but not yet marked as transcribing. Nothing
    // new starts until the pack is gone.
    let (_held, running) = state.worker.hold_starts();
    let db = state.db.lock().unwrap();
    for id in running {
        if matches!(db.vod(id).map_err(err)?.map(|v| v.status), Some(Status::Ready | Status::Transcribing)) {
            return Err("Wait for the current transcription to finish, or pause it, first.".into());
        }
    }
    drop(db);
    gpu::uninstall(&state.paths.gpu_pack()).map_err(err)?;
    gpu::uninstall(&state.paths.gpu_model_dir()).map_err(err)
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
    let claims = (Claim::take(&state.setup.running), Claim::take(&state.gpu.running));
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

/// A busy flag (an `Install`'s `running`) held until dropped.
struct Claim(Arc<AtomicBool>);

impl Claim {
    /// Sets `flag`, or returns None if something else already holds it.
    fn take(flag: &Arc<AtomicBool>) -> Option<Self> {
        (!flag.swap(true, Ordering::Relaxed)).then(|| Claim(flag.clone()))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}
