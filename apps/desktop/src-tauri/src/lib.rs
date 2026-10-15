//! The Tauri shell: app state and startup. The commands the UI calls live in
//! `vods` (the library), `setup` (setup, settings, GPU pack, storage),
//! `thumbnails`, `chats` and `clips`; `worker` runs the download and transcription
//! queue, and `chats` the chat replay one.

// `tauri dev` serves the app from its dev server instead; tests still cover it.
#[cfg(any(not(dev), test))]
mod app_server;
mod chats;
mod clips;
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
    /// A read-only connection for what the views ask for (search, the
    /// transcript, chat), which never waits for the writes on `db`.
    reads: Mutex<Db>,
    worker: Worker,
    /// First-run setup's downloads.
    setup: setup::Install,
    /// The GPU add-on's download.
    gpu: setup::Install,
    media: media::MediaServer,
    chat: chats::ChatQueue,
    clips: clips::Cutter,
    /// Links being looked up to add, by the interface's ticket, so one can be canceled.
    adding: Mutex<std::collections::HashMap<u64, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

type CmdResult<T> = Result<T, String>;

/// Runs `f` with the library's read-only connection (`AppState::reads`) on a
/// blocking thread, so it waits for neither the async runtime nor writes.
async fn with_reads<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&Db) -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>().reads.lock().unwrap()))
        .await
        .map_err(err)?
        .map_err(err)
}

fn err(e: impl Into<anyhow::Error>) -> String {
    format!("{:#}", e.into())
}

/// The arguments WebView2's browser starts with on Windows: wry's own, plus any
/// in WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS. WebView2 ignores that variable
/// once an app passes arguments itself, which wry always does, and it's how
/// msedgedriver asks for a debugging port (the end-to-end tests, tests/e2e).
/// The features each disables are merged: of two --disable-features, only the
/// last would count.
#[cfg(any(windows, test))]
fn webview_args(extra: &str) -> String {
    const WRY: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";
    let (mut disabled, mut rest) = (Vec::new(), Vec::new());
    for arg in WRY.split_whitespace().chain(extra.split_whitespace()) {
        match arg.strip_prefix("--disable-features=") {
            Some(features) => disabled.extend(features.split(',').filter(|f| !f.is_empty())),
            None => rest.push(arg),
        }
    }
    std::iter::once(format!("--disable-features={}", disabled.join(",")))
        .chain(rest.into_iter().map(str::to_string))
        .collect::<Vec<_>>()
        .join(" ")
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
            let db = Db::open(&paths.db())?;
            // Kept videos' leftovers from VODs no longer in the library; only
            // tidying, so a library that can't be read here isn't a reason not
            // to start.
            if let Ok(vods) = db.vods() {
                let library: Vec<i64> = vods.iter().map(|v| v.id).collect();
                vodscribe_core::pipeline::tidy_media(&paths.work(), &library);
                vodscribe_core::peaks::tidy(&paths.peaks(), &library);
            }
            let db = Mutex::new(db);
            let reads = Mutex::new(Db::open_reader(&paths.db())?);
            let worker = Worker::start(app.handle().clone(), paths.clone())?;
            let handle = app.handle().clone();
            let media = media::MediaServer::start(move |id| {
                let state = handle.state::<AppState>();
                media::playable_file(&state.db.lock().unwrap(), id)
            })?;
            let (chat, chat_rx) = chats::ChatQueue::new();
            app.manage(AppState {
                paths,
                db,
                reads,
                worker,
                setup: setup::Install::new(),
                gpu: setup::Install::new(),
                media,
                chat,
                clips: clips::Cutter::new(),
                adding: Mutex::default(),
            });
            app.state::<AppState>().clips.run(app.handle().clone())?;
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
            // Hidden until App.svelte has mounted and shows it, so it never opens
            // as a blank white window. If the page never gets that far, it's
            // shown anyway, broken page and all.
            let builder = tauri::WebviewWindowBuilder::from_config(app.handle(), &window)?;
            #[cfg(windows)]
            let builder = match std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
                Ok(extra) if !extra.trim().is_empty() => builder.additional_browser_args(&webview_args(&extra)),
                _ => builder,
            };
            let main = builder.build()?;
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
            vods::cancel_add,
            vods::list_vods,
            vods::add_files,
            vods::media_extensions,
            vods::show_file,
            vods::open_file,
            media::media_url,
            media::peaks,
            media::frame_rate,
            vods::captions,
            clips::list_clips,
            clips::add_clip,
            clips::set_clip_range,
            clips::rename_clip,
            clips::delete_clip,
            clips::show_clip,
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
            chats::chat_word_emotes,
            chats::retry_chat,
            chats::emote,
            chats::get_timeline,
            chats::set_timeline,
            chats::chat_matches,
            setup::update_yt_dlp,
            vods::kick_stream,
            vods::twitch_clip_video,
            setup::get_settings,
            setup::set_settings,
            setup::gpu_status,
            setup::install_gpu_pack,
            setup::cancel_gpu_install,
            setup::remove_gpu_pack,
            setup::cache_size,
            setup::media_size,
            setup::set_playable,
            vods::delete_video,
            setup::clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn webview_args_keep_wrys_and_add_the_drivers() {
        assert_eq!(
            super::webview_args(
                "--disable-features=IgnoreDuplicateNavs,Prewarm --remote-debugging-port=0 --enable-logging"
            ),
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IgnoreDuplicateNavs,Prewarm \
             --remote-debugging-port=0 --enable-logging"
        );
        assert_eq!(super::webview_args(""), "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    }
}
