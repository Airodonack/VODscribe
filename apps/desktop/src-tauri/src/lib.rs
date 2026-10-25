//! The Tauri shell: app state and startup. The commands the UI calls live in
//! `vods` (the library), `setup` (setup, settings, GPU pack, storage),
//! `thumbnails`, `chats` and `clips`; `worker` runs the download and transcription
//! queue, and `chats` the chat replay one.

mod annotations;
// `tauri dev` serves the app from its dev server instead; tests still cover it.
#[cfg(any(not(dev), test))]
mod app_server;
mod chats;
mod clips;
#[cfg(feature = "e2e")]
mod e2e;
mod edits;
mod embed;
mod files;
mod footage;
mod history;
mod jobs;
mod loopback;
mod media;
mod player;
mod recovery;
mod screen;
mod setup;
mod storyboards;
mod thumbnails;
mod videos;
mod vods;
#[cfg(target_os = "linux")]
mod webview;
mod worker;

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::Db;
use vodscribe_core::{Paths, model};

use worker::Worker;

struct AppState {
    paths: Paths,
    db: Mutex<Db>,
    /// A read-only connection for what the views ask for (the transcript,
    /// chat, stretches, clips), which never waits for the writes on `db`.
    reads: Mutex<Db>,
    /// Search's own read-only connection, so those reads never wait behind
    /// a search of seconds; the newest search typed wins (`Searcher`).
    searches: vodscribe_core::searcher::Searcher,
    worker: Worker,
    /// First-run setup's downloads.
    setup: setup::Install,
    /// The GPU add-on's download.
    gpu: setup::Install,
    media: media::MediaServer,
    chat: chats::ChatQueue,
    clips: clips::Cutter,
    /// Edits being rendered (`edits`).
    /// Footage for edits being downloaded (`footage`).
    footage: footage::Footage,
    /// Whole videos downloaded on request (`videos`).
    videos: videos::Videos,
    /// Storyboards being made (`storyboards`).
    storyboards: storyboards::Storyboards,
    /// What can be undone and redone (`history`).
    history: history::History,
    /// The app's own player, playing a VOD's file (player.rs).
    player: player::Native,
    /// What a site's player appends, played by `player` (embed.rs).
    embed: embed::Embed,
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
/// Only in a build for those tests (the `e2e` feature): in a release, anything
/// that set the variable could open the page to any program here.
/// The features each disables are merged: of two --disable-features, only the
/// last would count.
#[cfg(any(all(windows, feature = "e2e"), test))]
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

/// Everything that runs on the library: the state the commands use, the
/// queues and the background jobs.
fn start_library(app: &mut tauri::App, paths: Paths, db: Db) -> anyhow::Result<()> {
    // Edits' preview segments, from before edits played in the app's own
    // player (up to 2 GB): nothing reads them now.
    let _ = std::fs::remove_dir_all(paths.local.join("preview"));
    embed::clear(&paths.local);
    // Footage sections a quit (or a crash) left downloading or downloaded in the work folder.
    footage::tidy_work(&paths.work());
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
    let searches = vodscribe_core::searcher::Searcher::new(Db::open_reader(&paths.db())?);
    let worker = Worker::start(app.handle().clone(), paths.clone())?;
    let handle = app.handle().clone();
    let media = media::MediaServer::start(move |asked| match asked {
        media::Served::Vod(id) => media::playable_file(&handle.state::<AppState>().db.lock().unwrap(), id),
        media::Served::Footage(name) => crate::footage::section_file(&handle, &name).map(|p| p.display().to_string()),
    })?;
    let (chat, chat_rx) = chats::ChatQueue::new();
    app.manage(AppState {
        paths,
        db,
        reads,
        searches,
        worker,
        setup: setup::Install::new(),
        gpu: setup::Install::new(),
        media,
        chat,
        clips: clips::Cutter::new(),
        footage: footage::Footage::default(),
        videos: videos::Videos::new(),
        storyboards: storyboards::Storyboards::new(),
        history: history::History::default(),
        player: player::Native::default(),
        embed: embed::Embed::default(),
        adding: Mutex::default(),
    });
    app.state::<AppState>().clips.run(app.handle().clone())?;
    app.state::<AppState>().videos.run(app.handle().clone())?;
    app.state::<AppState>().footage.run(app.handle().clone())?;
    app.state::<AppState>().storyboards.run(app.handle().clone())?;
    let handle = app.handle().clone();
    std::thread::spawn(move || thumbnails::cache_missing_thumbnails(handle));
    let handle = app.handle().clone();
    std::thread::spawn(move || storyboards::sweep(&handle));
    // What quitting didn't get to of the VODs nothing uses any more.
    let handle = app.handle().clone();
    std::thread::spawn(move || vods::sweep(&handle, None));
    let handle = app.handle().clone();
    std::thread::spawn(move || chats::run(handle, chat_rx));
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
            // A library that won't open: the window opens on the recovery
            // screen (recovery.rs) instead of the app failing to start.
            let db_path = paths.db();
            match Db::open(&db_path) {
                Ok(db) => {
                    app.manage(recovery::Problem::none(db_path));
                    start_library(app, paths, db)?;
                }
                Err(e) => {
                    eprintln!("The library couldn't be opened: {e:#}");
                    app.manage(recovery::Problem::new(format!("{e:#}"), db_path));
                }
            }
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
            // In a site's player's frame, what it plays goes to the app's player
            // (embed.ts, embed.rs); it does nothing in other frames. Only where
            // built in (the embed-takeover feature): their frames are otherwise
            // left as they are.
            #[cfg(feature = "embed-takeover")]
            let builder = builder.initialization_script_for_all_frames(include_str!("embed_shim.js"));
            let builder = builder
                // No background behind the page: it paints its own, and where it's
                // clear the player's picture under it shows (a site's player over
                // it). On macOS it's given as the WKWebView is made: wry turns its
                // drawsBackground off only then.
                .background_color(tauri::window::Color(0, 0, 0, 0));
            // Under Wayland the picture lies under the window's own surface, which
            // is see-through there. (Under X11 it never goes under the page, and on
            // macOS and Windows it's under the page inside the window.)
            // Only a site's VOD's picture goes under (embed-takeover).
            #[cfg(all(target_os = "linux", feature = "embed-takeover"))]
            let see_through = vodscribe_player::present::can_go_under();
            #[cfg(all(target_os = "linux", not(feature = "embed-takeover")))]
            let see_through = false;
            #[cfg(not(target_os = "linux"))]
            let see_through = false;
            let builder = builder.transparent(see_through);
            #[cfg(all(windows, feature = "e2e"))]
            let builder = match std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
                Ok(extra) if !extra.trim().is_empty() => builder.additional_browser_args(&webview_args(&extra)),
                _ => builder,
            };
            let main = builder.build()?;
            #[cfg(feature = "e2e")]
            e2e::start(app.handle());
            app.manage(screen::Screen::default());
            // Without a screen, the page plays files itself as before.
            if let Err(e) = screen::attach(app.handle(), &main) {
                eprintln!("The player's screen couldn't be put in the window: {e:#}");
            }
            // Closed before the window goes, while the player may be painting.
            let closing = app.handle().clone();
            main.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    screen::close(&closing);
                }
            });
            // Behind the page, the window's background is the theme's (--bg in
            // app.css), not white; but clear where the window is see-through.
            if !see_through {
                let bg = match main.theme() {
                    Ok(tauri::Theme::Dark) => tauri::window::Color(0x14, 0x14, 0x13, 0xff),
                    _ => tauri::window::Color(0xf4, 0xf4, 0xf2, 0xff),
                };
                // On Linux nothing goes under the page then (X11, NVIDIA's driver):
                // the page's view has it too, as it always did there.
                #[cfg(target_os = "linux")]
                let _ = main.set_background_color(Some(bg));
                #[cfg(not(target_os = "linux"))]
                let _ = main.as_ref().window().set_background_color(Some(bg));
            }
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if !main.is_visible().unwrap_or(true) {
                    let _ = main.show();
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            recovery::library_problem,
            recovery::set_library_aside,
            recovery::show_library,
            setup::setup_status,
            setup::self_updatable,
            setup::data_dir,
            setup::open_data_dir,
            setup::run_setup,
            setup::cancel_setup,
            vods::add_vod,
            annotations::import_annotations,
            annotations::import_annotations_file,
            annotations::check_annotations,
            annotations::annotations_offer,
            annotations::fetch_annotations,
            annotations::vod_annotations,
            vods::cancel_add,
            vods::list_vods,
            vods::add_files,
            vods::media_extensions,
            vods::show_file,
            vods::open_file,
            media::media_url,
            media::peaks,
            media::frame_rate,
            media::frame,
            vods::captions,
            clips::list_clips,
            clips::add_clip,
            clips::add_clips,
            clips::set_clip_range,
            clips::rename_clip,
            clips::keep_clip,
            clips::delete_clip,
            clips::delete_clips,
            clips::forget_gone_clips,
            clips::show_clip,
            clips::drag_clip,
            clips::save_clip,
            clips::list_marks,
            clips::add_mark,
            clips::move_mark,
            clips::set_mark_text,
            clips::resolve_installed,
            edits::list_edits,
            edits::create_edit,
            edits::get_edit,
            edits::edit_captions,
            edits::edit_lines,
            clips::edit_marks,
            clips::add_edit_mark,
            clips::edit_clips,
            clips::add_edit_clip,
            clips::add_edit_clips,
            chats::edit_chat_overview,
            chats::edit_chat_windows,
            edits::remove_stretch,
            edits::rename_edit,
            edits::delete_edit,
            edits::add_to_edit,
            edits::add_range_to_edit,
            edits::put_in_edit,
            edits::edit_thumb,
            storyboards::storyboard_frame,
            footage::vod_frame,
            footage::footage_bytes,
            footage::clear_footage,
            history::set_view,
            history::undo,
            history::redo,
            screen::set_screen,
            screen::set_screen_overlay,
            player::player_available,
            player::player_open,
            player::player_open_edit,
            player::player_plan,
            player::player_close,
            embed::embed_takeover,
            embed::embed_open,
            embed::embed_append,
            embed::embed_close,
            player::player_still,
            player::player_frame,
            player::player_preview,
            player::player_decoders_started,
            #[cfg(feature = "e2e")]
            e2e::e2e_player_state,
            #[cfg(feature = "e2e")]
            e2e::e2e_set_inner_size,
            screen::screen_under,
            player::player_play,
            player::player_pause,
            player::player_seek,
            player::player_step,
            player::player_set_speed,
            player::player_set_volume,
            edits::move_piece,
            edits::trim_piece,
            edits::remove_piece,
            edits::remove_from_edit,
            edits::split_piece,
            edits::render_edit_now,
            edits::stop_render,
            edits::show_edit,
            edits::drag_edit,
            clips::delete_mark,
            clips::cancel_mark,
            videos::keep_video,
            videos::stop_video,
            videos::video_downloads,
            videos::delete_all_videos,
            videos::open_videos_dir,
            thumbnails::thumbnail,
            vods::library_order,
            vods::reorder_library,
            vods::delete_vod,
            vods::retry_vod,
            vods::pause_vod,
            vods::retranscribe_vod,
            vods::resume_vod,
            vods::cancel_vod,
            vods::cancel_stretch,
            vods::search,
            vods::author_suggestions,
            vods::transcript,
            vods::transcript_stretches,
            vods::transcribe_stretch,
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
            setup::reset_settings,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                quit(app);
            }
        });
}

/// As the app quits: the VODs nothing uses any more are erased (for a few
/// seconds at most: `vods::sweep`), and what it started (downloads,
/// transcription, encodes) stops with it rather than running on without it. The library's writes
/// stop first, so the jobs whose processes stop don't record that as a
/// failure: they carry on at the next start as after any quit.
fn quit(app: &AppHandle) {
    // First: the player may be painting, and must stop before the program ends.
    screen::close(app);
    if let Some(state) = app.try_state::<AppState>() {
        let db = state.db.lock().unwrap_or_else(|e| e.into_inner());
        vods::sweep(app, Some(&db));
        let _ = db.stop_writes();
        // Held till the process ends.
        std::mem::forget(db);
    }
    vodscribe_core::tools::stop_tracked();
    files::delete_leftovers();
}

/// Opens `path` as ffmpeg does, for tests: on Windows without sharing its
/// deletion, so the file can't be deleted while it's open (as while the
/// player or a storyboard reads it).
#[cfg(test)]
pub(crate) fn open_as_ffmpeg(path: &std::path::Path) -> std::fs::File {
    let mut options = std::fs::File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE, not FILE_SHARE_DELETE (the C runtime's _wsopen).
        options.share_mode(0x1 | 0x2);
    }
    options.open(path).unwrap()
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
