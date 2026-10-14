//! The Tauri shell: app state and startup. The commands the UI calls live in
//! `vods` (the library), `setup` (setup, settings, GPU pack, storage),
//! `thumbnails` and `chats`; `worker` runs the download and transcription
//! queue, and `chats` the chat replay one.

// `tauri dev` serves the app from its dev server instead; tests still cover it.
#[cfg(any(not(dev), test))]
mod app_server;
mod chats;
mod loopback;
mod media;
mod setup;
mod thumbnails;
mod vods;
mod worker;

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::Db;
use vodscribe_core::{Paths, model};

use worker::Worker;

struct AppState {
    paths: Paths,
    db: Mutex<Db>,
    worker: Worker,
    /// First-run setup's downloads.
    setup: setup::Install,
    /// The GPU add-on's download.
    gpu: setup::Install,
    media: media::MediaServer,
    chat: chats::ChatQueue,
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
        // One copy at a time: two would share the library and the queue, and on Linux
        // a second copy's window stays blank. Launching again focuses the open window.
        // (First, so a second copy exits before setting anything up.)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            let handle = app.handle().clone();
            let media = media::MediaServer::start(move |id| {
                let state = handle.state::<AppState>();
                state.db.lock().unwrap().vod(id).ok().flatten().and_then(|v| v.source_path)
            })?;
            let (chat, chat_rx) = chats::ChatQueue::new();
            app.manage(AppState {
                paths,
                db,
                worker,
                setup: setup::Install::new(),
                gpu: setup::Install::new(),
                media,
                chat,
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || thumbnails::cache_missing_thumbnails(handle));
            let handle = app.handle().clone();
            std::thread::spawn(move || chats::run(handle, chat_rx));
            // The window is created here rather than from the config, so release builds
            // open it only once the app server is listening (see app_server.rs).
            let window = app.config().app.windows.first().cloned().ok_or("no window configured")?;
            #[cfg(not(dev))]
            let window = tauri::utils::config::WindowConfig {
                url: tauri::WebviewUrl::External(app_server::start(app.handle())?.parse()?),
                ..window
            };
            // Hidden until the page has painted its first frame and shows it
            // (App.svelte), so it never opens as a blank white window. If the page
            // never gets that far, it's shown anyway, broken page and all.
            let main = tauri::WebviewWindowBuilder::from_config(app.handle(), &window)?.build()?;
            // Behind the page, the theme's background (--bg in app.css), not white.
            let bg = match main.theme() {
                Ok(tauri::Theme::Dark) => tauri::window::Color(0x14, 0x14, 0x13, 0xff),
                _ => tauri::window::Color(0xf4, 0xf4, 0xf2, 0xff),
            };
            let _ = main.set_background_color(Some(bg));
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if !main.is_visible().unwrap_or(true) {
                    let _ = main.show();
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup::setup_status,
            setup::self_updatable,
            setup::data_dir,
            setup::open_data_dir,
            setup::run_setup,
            setup::cancel_setup,
            vods::add_vod,
            vods::list_vods,
            vods::add_files,
            vods::media_extensions,
            vods::show_file,
            vods::open_file,
            media::media_url,
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
            chats::chat_overview,
            chats::chat_windows,
            chats::chat_emotes,
            chats::retry_chat,
            chats::emote,
            chats::get_timeline,
            chats::set_timeline,
            chats::chat_matches,
            setup::update_yt_dlp,
            vods::kick_stream,
            setup::get_settings,
            setup::set_settings,
            setup::gpu_status,
            setup::install_gpu_pack,
            setup::cancel_gpu_install,
            setup::remove_gpu_pack,
            setup::cache_size,
            setup::clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
