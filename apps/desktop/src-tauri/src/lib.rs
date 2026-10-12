mod worker;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{Db, SearchHit, Status, Vod};
use vodscribe_core::gpu::{self, GpuInfo};
use vodscribe_core::settings::Settings;
use vodscribe_core::{Paths, fetch, model, pipeline, thumbs, tools};

use worker::{Stop, Worker};

struct AppState {
    paths: Paths,
    db: Mutex<Db>,
    worker: Worker,
    setup_running: Arc<AtomicBool>,
    gpu_installing: Arc<AtomicBool>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl Into<anyhow::Error>) -> String {
    format!("{:#}", e.into())
}

#[derive(Serialize)]
struct SetupStatus {
    yt_dlp: bool,
    ffmpeg: bool,
    model: bool,
    js_runtime: bool,
    running: bool,
}

#[tauri::command]
async fn setup_status(state: State<'_, AppState>) -> CmdResult<SetupStatus> {
    let bin = state.paths.bin();
    Ok(SetupStatus {
        yt_dlp: tools::find_yt_dlp(&bin).is_some(),
        ffmpeg: tools::find_ffmpeg(&bin).is_some(),
        model: model::is_installed(&state.paths.model_dir(), &model::CPU_MODEL),
        js_runtime: tools::find_js_runtime(&bin).is_some(),
        running: state.setup_running.load(Ordering::Relaxed),
    })
}

/// Downloads yt-dlp and the speech model if missing. Progress arrives as
/// `setup-progress` events and completion as `setup-done` (an error message or null).
#[tauri::command]
async fn run_setup(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if state.setup_running.swap(true, Ordering::Relaxed) {
        return Ok(());
    }
    let paths = state.paths.clone();
    let running = state.setup_running.clone();
    std::thread::spawn(move || {
        let never = AtomicBool::new(false);
        let progress = |step: &str, done: u64, total: Option<u64>| {
            let _ = app.emit("setup-progress", (step, done, total));
        };
        let result = (|| -> anyhow::Result<()> {
            if tools::find_yt_dlp(&paths.bin()).is_none() {
                tools::install_yt_dlp(&paths.bin(), &never, |n, t| progress("yt-dlp", n, t))?;
            }
            model::install(&paths.models(), &model::CPU_MODEL, &never, |n, t| progress("model", n, Some(t)))?;
            model::remove_others(&paths.models(), &[&model::CPU_MODEL, &model::GPU_MODEL]);
            Ok(())
        })();
        running.store(false, Ordering::Relaxed);
        let _ = app.emit("setup-done", result.err().map(err));
        app.state::<AppState>().worker.wake();
    });
    Ok(())
}

#[derive(Serialize)]
struct Added {
    vod: Vod,
    created: bool,
}

#[tauri::command]
async fn add_vod(app: AppHandle, url: String) -> CmdResult<Added> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("Paste a VOD link first.".into());
    }
    let task_app = app.clone();
    let added = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Added> {
        let state = task_app.state::<AppState>();
        let yt_dlp = tools::find_yt_dlp(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!("yt-dlp isn't installed yet"))?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let probe_path = state.paths.work().join(format!("probe-{nanos}.json"));
        let info = fetch::probe(&yt_dlp, &url, &probe_path)?;

        let db = state.db.lock().unwrap();
        let (mut vod, created) = db.add_vod(&info)?;
        if created {
            // Hand the resolved info to the worker so it doesn't probe again.
            std::fs::rename(&probe_path, pipeline::info_path(&state.paths.work(), vod.id))?;
        } else {
            let _ = std::fs::remove_file(&probe_path);
            if vod.status == Status::Error {
                requeue(&db, vod.id)?;
                vod = db.vod(vod.id)?.unwrap_or(vod);
            }
        }
        Ok(Added { vod, created })
    })
    .await
    .map_err(err)?
    .map_err(err)?;

    let state = app.state::<AppState>();
    state.worker.wake();
    let _ = app.emit("vod", &added.vod);
    if added.created {
        let (app, id) = (app.clone(), added.vod.id);
        std::thread::spawn(move || cache_thumbnail(&app, id));
    }
    Ok(added)
}

/// A thumbnail's bytes, by the file name in `Vod::thumb`, and nothing else.
#[tauri::command]
async fn thumbnail(state: State<'_, AppState>, name: String) -> CmdResult<tauri::ipc::Response> {
    if thumbs::mime(&name).is_none() {
        return Err("not a thumbnail".into());
    }
    let bytes = std::fs::read(state.paths.thumbs().join(&name)).map_err(err)?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
async fn list_vods(state: State<'_, AppState>) -> CmdResult<Vec<Vod>> {
    state.db.lock().unwrap().vods().map_err(err)
}

#[tauri::command]
async fn reorder_vods(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<()> {
    state.db.lock().unwrap().reorder(&ids).map_err(err)
}

#[tauri::command]
async fn delete_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    // A running stage removes the files once its process has stopped.
    let delete_row = || state.db.lock().unwrap().delete_vod(id);
    let running = state
        .worker
        .control(id, Stop::Delete, || {
            pipeline::remove_work_files(&state.paths.work(), id);
            delete_row()
        })
        .map_err(err)?;
    if running {
        delete_row().map_err(err)?;
    }
    thumbs::remove(&state.paths.thumbs(), id);
    let _ = app.emit("vod-removed", id);
    Ok(())
}

fn emit_vod(app: &AppHandle, db: &Db, id: i64) -> CmdResult<()> {
    if let Some(vod) = db.vod(id).map_err(err)? {
        let _ = app.emit("vod", vod);
    }
    Ok(())
}

/// Requeues a VOD at whichever stage it needs next.
fn requeue(db: &Db, id: i64) -> anyhow::Result<()> {
    let downloaded = db.audio_path(id)?.is_some_and(|p| std::path::Path::new(&p).is_file());
    let progress = db.vod(id)?.map_or(0.0, |v| if v.status == Status::Paused { v.progress } else { 0.0 });
    let status = if downloaded { Status::Ready } else { Status::Queued };
    db.set_status(id, status, progress, None)
}

#[tauri::command]
async fn retry_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    requeue(&db, id).map_err(err)?;
    emit_vod(&app, &db, id)?;
    state.worker.wake();
    Ok(())
}

/// Throws away a finished VOD's transcript and transcribes it again, e.g. with
/// a better model.
#[tauri::command]
async fn retranscribe_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    match db.vod(id).map_err(err)? {
        Some(vod) if matches!(vod.status, Status::Done | Status::Error) => {}
        _ => return Err("Only finished VODs can be re-transcribed.".into()),
    }
    db.clear_segments(id).map_err(err)?;
    db.set_model(id, None).map_err(err)?;
    db.set_note(id, None).map_err(err)?;
    requeue(&db, id).map_err(err)?;
    emit_vod(&app, &db, id)?;
    state.worker.wake();
    Ok(())
}

/// Stops a VOD, keeping its partial download and transcript.
#[tauri::command]
async fn pause_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    // A running job is paused by its stage once its process has stopped.
    state
        .worker
        .control(id, Stop::Pause, || {
            let db = state.db.lock().unwrap();
            if let Some(vod) = db.vod(id)?
                && matches!(vod.status, Status::Queued | Status::Ready)
            {
                db.set_status(id, Status::Paused, vod.progress, None)?;
            }
            Ok(())
        })
        .map_err(err)?;
    emit_vod(&app, &state.db.lock().unwrap(), id)
}

#[tauri::command]
async fn resume_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    if db.vod(id).map_err(err)?.is_some_and(|v| v.status == Status::Paused) {
        requeue(&db, id).map_err(err)?;
        emit_vod(&app, &db, id)?;
        state.worker.wake();
    }
    Ok(())
}

/// Stops a VOD and discards its partial work; Retry then starts over.
#[tauri::command]
async fn cancel_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state
        .worker
        .control(id, Stop::Cancel, || worker::discard(&state.db.lock().unwrap(), &state.paths.work(), id))
        .map_err(err)?;
    emit_vod(&app, &state.db.lock().unwrap(), id)
}

#[derive(Serialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
    truncated: bool,
}

#[tauri::command]
async fn search(state: State<'_, AppState>, query: String, vod_id: Option<i64>) -> CmdResult<SearchResponse> {
    let r = state.db.lock().unwrap().search(&query, vod_id, 500).map_err(err)?;
    Ok(SearchResponse { hits: r.hits, truncated: r.truncated })
}

#[derive(Serialize)]
struct Line {
    start: f64,
    text: String,
}

#[tauri::command]
async fn transcript(state: State<'_, AppState>, vod_id: i64) -> CmdResult<Vec<Line>> {
    let lines = state.db.lock().unwrap().lines(vod_id).map_err(err)?;
    Ok(lines.into_iter().map(|(start, text)| Line { start, text }).collect())
}

#[tauri::command]
async fn update_yt_dlp(state: State<'_, AppState>) -> CmdResult<String> {
    let bin = state.paths.bin();
    tauri::async_runtime::spawn_blocking(move || tools::update_yt_dlp(&bin))
        .await
        .map_err(err)?
        .map_err(err)
}

/// Playable quality levels for a Kick VOD; Kick has no embeddable VOD player.
#[tauri::command]
async fn kick_stream(url: String) -> CmdResult<Vec<vodscribe_core::kick::Variant>> {
    tauri::async_runtime::spawn_blocking(move || vodscribe_core::kick::variants(&url))
        .await
        .map_err(err)?
        .map_err(err)
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(Settings::load(&state.paths.settings()))
}

/// Takes effect from the next VOD; one already transcribing keeps its device.
#[tauri::command]
async fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<()> {
    settings.save(&state.paths.settings()).map_err(err)
}

#[derive(Serialize)]
struct GpuStatus {
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
async fn gpu_status(state: State<'_, AppState>) -> CmdResult<GpuStatus> {
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
async fn install_gpu_pack(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if state.gpu_installing.swap(true, Ordering::Relaxed) {
        return Ok(());
    }
    let paths = state.paths.clone();
    let installing = state.gpu_installing.clone();
    std::thread::spawn(move || {
        let never = AtomicBool::new(false);
        let progress = |step: &str, done: u64, total: u64| {
            let _ = app.emit("gpu-progress", (step, done, total));
        };
        let result = (|| -> anyhow::Result<()> {
            if !gpu::is_installed(&paths.gpu_pack()) {
                gpu::install(&paths.gpu_pack(), &never, progress)?;
            }
            model::install(&paths.models(), &model::GPU_MODEL, &never, |n, t| progress("GPU speech model", n, t))?;
            Ok(())
        })();
        installing.store(false, Ordering::Relaxed);
        let _ = app.emit("gpu-done", result.err().map(err));
    });
    Ok(())
}

#[tauri::command]
async fn remove_gpu_pack(state: State<'_, AppState>) -> CmdResult<()> {
    if state.gpu_installing.load(Ordering::Relaxed) {
        return Err("The GPU pack is still downloading.".into());
    }
    gpu::uninstall(&state.paths.gpu_pack()).map_err(err)?;
    gpu::uninstall(&state.paths.gpu_model_dir()).map_err(err)
}

/// Bytes of downloaded files that `clear_cache` would delete.
#[tauri::command]
async fn cache_size(state: State<'_, AppState>) -> CmdResult<u64> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || paths.cache_size()).await.map_err(err)
}

/// Deletes everything that can be downloaded again (models, tools, the GPU
/// pack, unfinished downloads), pausing running jobs first. The library,
/// transcripts and settings are kept; setup runs again afterwards.
#[tauri::command]
async fn clear_cache(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
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

/// Saves a local copy of VOD `id`'s thumbnail, asking Kick for a poster first
/// if a Kick VOD doesn't have one. A VOD with no usable image gets an empty
/// `thumb` so it isn't tried again; network failures are retried next launch.
fn cache_thumbnail(app: &AppHandle, id: i64) {
    let state = app.state::<AppState>();
    let Ok(Some(vod)) = state.db.lock().unwrap().vod(id) else { return };
    let mut url = vod.info.thumbnail.clone();
    if url.is_none() && vod.info.platform == "kick" {
        let Ok(poster) = vodscribe_core::kick::poster(&vod.info.url) else { return };
        let poster = poster.unwrap_or_default();
        let _ = state.db.lock().unwrap().set_thumbnail(id, &poster);
        url = Some(poster);
    }
    let thumb = match url.filter(|u| !u.is_empty()) {
        None => String::new(),
        Some(url) => match thumbs::fetch(&url, &state.paths.thumbs(), id) {
            Ok(thumb) => thumb.unwrap_or_default(),
            Err(_) => return,
        },
    };
    let db = state.db.lock().unwrap();
    if db.set_thumb(id, &thumb).is_ok() {
        let _ = emit_vod(app, &db, id);
    }
}

/// Fills in local thumbnails for VODs that don't have one yet, such as those
/// added before thumbnails were stored locally.
fn cache_missing_thumbnails(app: AppHandle) {
    let state = app.state::<AppState>();
    let dir = state.paths.thumbs();
    let Ok(vods) = state.db.lock().unwrap().vods() else { return };
    for vod in vods {
        let missing = match vod.thumb.as_deref() {
            None => true,
            Some("") => false,
            Some(name) => !dir.join(name).is_file(),
        };
        if missing {
            cache_thumbnail(&app, vod.id);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // On Windows the data dir roams, so downloads go in the local one.
            let paths = Paths::split(app.path().app_data_dir()?, app.path().app_local_data_dir()?);
            paths.ensure()?;
            // Upgrades: drop models this version no longer uses.
            if model::is_installed(&paths.model_dir(), &model::CPU_MODEL) {
                model::remove_others(&paths.models(), &[&model::CPU_MODEL, &model::GPU_MODEL]);
            }
            let db = Mutex::new(Db::open(&paths.db())?);
            let worker = Worker::start(app.handle().clone(), paths.clone())?;
            app.manage(AppState {
                paths,
                db,
                worker,
                setup_running: Arc::new(AtomicBool::new(false)),
                gpu_installing: Arc::new(AtomicBool::new(false)),
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || cache_missing_thumbnails(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup_status,
            run_setup,
            add_vod,
            list_vods,
            thumbnail,
            reorder_vods,
            delete_vod,
            retry_vod,
            pause_vod,
            retranscribe_vod,
            resume_vod,
            cancel_vod,
            search,
            transcript,
            update_yt_dlp,
            kick_stream,
            get_settings,
            set_settings,
            gpu_status,
            install_gpu_pack,
            remove_gpu_pack,
            cache_size,
            clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
