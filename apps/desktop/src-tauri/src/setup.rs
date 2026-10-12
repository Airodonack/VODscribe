//! Commands for what the app needs to run: first-run setup, settings, the GPU
//! pack and downloaded files.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::Status;
use vodscribe_core::gpu::{self, GpuInfo};
use vodscribe_core::settings::Settings;
use vodscribe_core::{deps, model, tools};

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
    running: bool,
}

#[tauri::command]
pub(crate) async fn setup_status(state: State<'_, AppState>) -> CmdResult<SetupStatus> {
    let bin = state.paths.bin();
    Ok(SetupStatus {
        yt_dlp: tools::find_yt_dlp(&bin).is_some(),
        yt_dlp_managed: tools::managed_yt_dlp(&bin).is_file(),
        ffmpeg: tools::find_ffmpeg(&bin).is_some(),
        ffmpeg_managed: bin.join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX)).is_file(),
        model: model::is_installed(&state.paths.model_dir(), &model::CPU_MODEL),
        js_runtime: tools::find_js_runtime(&bin).is_some(),
        js_runtime_managed: bin.join(format!("deno{}", std::env::consts::EXE_SUFFIX)).is_file(),
        can_install_tools: deps::ffmpeg().is_some() && deps::deno().is_some(),
        running: state.setup_running.load(Ordering::Relaxed),
    })
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

/// Downloads yt-dlp and the speech model if missing. Progress arrives as
/// `setup-progress` events and completion as `setup-done` (an error message or null).
#[tauri::command]
pub(crate) async fn run_setup(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let paths = state.paths.clone();
    spawn_install(app, state.setup_running.clone(), "setup-done", move |app, never| {
        let progress = |step: &str, done: u64, total: Option<u64>| {
            let _ = app.emit("setup-progress", (step, done, total));
        };
        let bin = paths.bin();
        if tools::find_yt_dlp(&bin).is_none() {
            tools::install_yt_dlp(&bin, never, |n, t| progress("yt-dlp", n, t))?;
        }
        if tools::find_ffmpeg(&bin).is_none()
            && let Some(dep) = deps::ffmpeg()
        {
            deps::install(&dep, &bin, never, |n| progress("ffmpeg", n, Some(dep.size)))?;
        }
        model::install(&paths.models(), &model::CPU_MODEL, never, |n, t| progress("model", n, Some(t)))?;
        model::remove_others(&paths.models(), &[&model::CPU_MODEL, &model::GPU_MODEL]);
        app.state::<AppState>().worker.wake();
        // Last, since it's optional: only some YouTube VODs need it.
        if tools::find_js_runtime(&bin).is_none()
            && let Some(dep) = deps::deno()
        {
            deps::install(&dep, &bin, never, |n| progress("deno", n, Some(dep.size)))?;
        }
        Ok(())
    });
    Ok(())
}

/// Runs a long download on its own thread, one at a time per `running` flag,
/// then clears the flag and emits `done_event` with an error message or null.
pub(crate) fn spawn_install(
    app: AppHandle,
    running: Arc<AtomicBool>,
    done_event: &'static str,
    install: impl FnOnce(&AppHandle, &AtomicBool) -> anyhow::Result<()> + Send + 'static,
) {
    if running.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        // These downloads can't be canceled.
        let result = install(&app, &AtomicBool::new(false));
        running.store(false, Ordering::Relaxed);
        let _ = app.emit(done_event, result.err().map(err));
    });
}

#[tauri::command]
pub(crate) async fn update_yt_dlp(state: State<'_, AppState>) -> CmdResult<String> {
    let bin = state.paths.bin();
    tauri::async_runtime::spawn_blocking(move || tools::update_yt_dlp(&bin))
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
}

#[tauri::command]
pub(crate) async fn gpu_status(state: State<'_, AppState>) -> CmdResult<GpuStatus> {
    let pack = state.paths.gpu_pack();
    let gpu = tauri::async_runtime::spawn_blocking(gpu::detect).await.map_err(err)?;
    Ok(GpuStatus {
        supported: gpu::supported(),
        installed: gpu::is_installed(&pack) && model::is_installed(&state.paths.gpu_model_dir(), &model::GPU_MODEL),
        installing: state.gpu_installing.load(Ordering::Relaxed),
        worker: tools::sidecar("vodscribe-worker-cuda").is_ok(),
        gpu,
        min_free_mb: gpu::MIN_FREE_MB,
    })
}

/// Downloads the GPU pack. Progress arrives as `gpu-progress` events
/// (step, done, total) and completion as `gpu-done` (an error message or null).
#[tauri::command]
pub(crate) async fn install_gpu_pack(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let paths = state.paths.clone();
    spawn_install(app, state.gpu_installing.clone(), "gpu-done", move |app, never| {
        let progress = |step: &str, done: u64, total: u64| {
            let _ = app.emit("gpu-progress", (step, done, total));
        };
        if !gpu::is_installed(&paths.gpu_pack()) {
            gpu::install(&paths.gpu_pack(), never, progress)?;
        }
        let model_label = "Downloading the GPU speech model (Parakeet TDT 0.6B v3, fp16)";
        model::install(&paths.models(), &model::GPU_MODEL, never, |n, t| progress(model_label, n, t))?;
        progress("Every file matches its SHA-256", 1, 1);
        Ok(())
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_gpu_pack(state: State<'_, AppState>) -> CmdResult<()> {
    if state.gpu_installing.load(Ordering::Relaxed) {
        return Err("The GPU add-on is still downloading.".into());
    }
    // A running transcription may have the pack's libraries loaded.
    if state.db.lock().unwrap().count_with_status(Status::Transcribing).map_err(err)? > 0 {
        return Err("Wait for the current transcription to finish, or pause it, first.".into());
    }
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
    if state.setup_running.load(Ordering::Relaxed) || state.gpu_installing.load(Ordering::Relaxed) {
        return Err("Wait for the current download to finish first.".into());
    }
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
