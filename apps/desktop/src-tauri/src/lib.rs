//! The Tauri shell: app state and startup. The commands the UI calls live in
//! `vods` (the library), `setup` (setup, settings, GPU pack, storage) and
//! `thumbnails`; `worker` runs the download and transcription queue.

mod setup;
mod thumbnails;
mod vods;
mod worker;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::Db;
use vodscribe_core::{Paths, model};

use worker::Worker;

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

fn emit_vod(app: &AppHandle, db: &Db, id: i64) -> CmdResult<()> {
    if let Some(vod) = db.vod(id).map_err(err)? {
        let _ = app.emit("vod", vod);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
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
            // Local files the user added stay playable across restarts.
            if let Ok(vods) = app.state::<AppState>().db.lock().unwrap().vods() {
                vods.iter().for_each(|vod| vods::allow_playback(app.handle(), vod));
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || thumbnails::cache_missing_thumbnails(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup::setup_status,
            setup::data_dir,
            setup::open_data_dir,
            setup::run_setup,
            vods::add_vod,
            vods::list_vods,
            vods::add_files,
            vods::show_file,
            thumbnails::thumbnail,
            vods::reorder_vods,
            vods::delete_vod,
            vods::retry_vod,
            vods::pause_vod,
            vods::retranscribe_vod,
            vods::resume_vod,
            vods::cancel_vod,
            vods::search,
            vods::transcript,
            setup::update_yt_dlp,
            vods::kick_stream,
            setup::get_settings,
            setup::set_settings,
            setup::gpu_status,
            setup::install_gpu_pack,
            setup::remove_gpu_pack,
            setup::cache_size,
            setup::clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
