//! The app's own player (crates/player) playing a VOD's file on the screen
//! (screen.rs), driven by the page's controls (NativePlayer.svelte): one
//! player at a time. The page is told the time and whether it's playing at
//! every change and now and then while playing (`player-time`), and keeps
//! time between by its own clock.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::tools;
use vodscribe_player::clock::{Event, Player};

use crate::{AppState, CmdResult, err};

/// The player open, if one is.
#[derive(Default)]
pub(crate) struct Native {
    open: Mutex<Option<Open>>,
    /// Counted up at every open and close, so an open that takes a while
    /// (the file is probed first) finds out the page has moved on since.
    asked: AtomicU64,
}

struct Open {
    player: Player,
    /// Opened for this VOD; a page that asks for the same one keeps it.
    vod_id: i64,
}

/// What the page is told of the player.
#[derive(Serialize, Clone, Copy)]
pub(crate) struct Status {
    time: f64,
    playing: bool,
}

/// What a player opened says of what it plays.
#[derive(Serialize, Clone, Copy)]
pub(crate) struct Opened {
    /// The frame rate at the start, if known.
    fps: Option<f64>,
    duration: f64,
}

fn opened(player: &Player) -> Opened {
    Opened { fps: player.fps_at(0.0), duration: player.duration() }
}

/// How often the page hears the time while playing, apart from changes.
const TOLD: Duration = Duration::from_millis(250);

fn status(player: &Player) -> Status {
    Status { time: player.time(), playing: player.playing() }
}

/// Whether there's a player here: a screen to draw on.
#[tauri::command]
pub(crate) fn player_available(app: AppHandle) -> bool {
    crate::screen::attached(&app)
}

/// Opens VOD `id`'s file at `at` seconds, playing if `play`; the one open
/// for it already is kept, moved there. Fails if the page closed the
/// player, or opened another, while the file was being probed.
#[tauri::command]
pub(crate) async fn player_open(app: AppHandle, id: i64, at: f64, play: bool) -> CmdResult<Opened> {
    let state = app.state::<AppState>();
    let asked = state.player.asked.fetch_add(1, Ordering::AcqRel) + 1;
    let same = {
        let open = state.player.open.lock().unwrap();
        open.as_ref().filter(|o| o.vod_id == id).map(|o| opened(&o.player))
    };
    let opened = match same {
        Some(opened) => opened,
        None => {
            let file = crate::media::playable_file(&state.db.lock().unwrap(), id)
                .ok_or_else(|| "This VOD has no file here to play.".to_string())?;
            let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).ok_or_else(|| crate::worker::NO_FFMPEG.to_string())?;
            let player = tauri::async_runtime::spawn_blocking(move || Player::open_file(&ffmpeg, file.as_ref()))
                .await
                .map_err(err)?
                .map_err(err)?;
            let opened = opened(&player);
            // Each frame due goes to the screen; the time to the page now and then.
            let (shown, told) = (app.clone(), app.clone());
            let last = Mutex::new(Instant::now() - TOLD);
            player.on_frame(Box::new(move |event| {
                let state = shown.state::<AppState>();
                let open = state.player.open.lock().unwrap();
                let Some(open) = open.as_ref() else { return };
                if event == Event::Frame
                    && let Some(frame) = open.player.frame()
                {
                    crate::screen::show(&shown, Some(frame));
                }
                let mut last = last.lock().unwrap();
                if event == Event::Ended || last.elapsed() >= TOLD {
                    *last = Instant::now();
                    let _ = told.emit("player-time", status(&open.player));
                }
            }));
            let mut open = state.player.open.lock().unwrap();
            if state.player.asked.load(Ordering::Acquire) != asked {
                return Err(CLOSED.into());
            }
            *open = Some(Open { player, vod_id: id });
            opened
        }
    };
    let open = state.player.open.lock().unwrap();
    if state.player.asked.load(Ordering::Acquire) != asked {
        return Err(CLOSED.into());
    }
    if let Some(o) = open.as_ref() {
        o.player.seek(at);
        if play {
            o.player.play();
        } else {
            o.player.pause();
        }
        let _ = app.emit("player-time", status(&o.player));
    }
    Ok(opened)
}

/// What an open that the page moved on from is told.
pub(crate) const CLOSED: &str = "The player was closed while opening.";

/// Closes the player, clearing the screen.
#[tauri::command]
pub(crate) fn player_close(app: AppHandle) {
    let state = app.state::<AppState>();
    state.player.asked.fetch_add(1, Ordering::AcqRel);
    drop(state.player.open.lock().unwrap().take());
    crate::screen::show(&app, None);
}

/// Does `f` with the player open, telling the page the time after.
fn with(app: &AppHandle, f: impl FnOnce(&Player)) -> CmdResult<Status> {
    let state = app.state::<AppState>();
    let open = state.player.open.lock().unwrap();
    let open = open.as_ref().ok_or("No player is open.")?;
    f(&open.player);
    let status = status(&open.player);
    let _ = app.emit("player-time", status);
    Ok(status)
}

#[tauri::command]
pub(crate) fn player_play(app: AppHandle) -> CmdResult<Status> {
    with(&app, |p| p.play())
}

#[tauri::command]
pub(crate) fn player_pause(app: AppHandle) -> CmdResult<Status> {
    with(&app, |p| p.pause())
}

/// Goes to `time`, playing from there if `play`.
#[tauri::command]
pub(crate) fn player_seek(app: AppHandle, time: f64, play: bool) -> CmdResult<Status> {
    with(&app, |p| {
        p.seek(time);
        if play {
            p.play();
        }
    })
}

/// Moves `frames` frames (back, if negative), paused.
#[tauri::command]
pub(crate) fn player_step(app: AppHandle, frames: i64) -> CmdResult<Status> {
    with(&app, |p| p.step(frames))
}

#[tauri::command]
pub(crate) fn player_status(app: AppHandle) -> CmdResult<Status> {
    let state = app.state::<AppState>();
    let open = state.player.open.lock().unwrap();
    let open = open.as_ref().ok_or("No player is open.")?;
    Ok(status(&open.player))
}

/// The loudness, 0 to 1.
#[tauri::command]
pub(crate) fn player_set_volume(state: State<'_, AppState>, volume: f32) -> CmdResult<()> {
    let open = state.player.open.lock().unwrap();
    open.as_ref().ok_or("No player is open.")?.player.set_volume(volume);
    Ok(())
}
