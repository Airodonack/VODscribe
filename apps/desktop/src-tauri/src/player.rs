//! The app's own player (crates/player) playing a VOD's file, an edit, or
//! what a site's player fetches (embed.rs), on the screen (screen.rs), driven
//! by the page's controls (NativePlayer.svelte) or the site's player
//! (embed.ts): one player at a time. The page is told the time and whether
//! it's playing at every change and now and then while playing
//! (`player-time`), and keeps time between by its own clock.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::tools;
use vodscribe_player::clock::{Event, Player};
use vodscribe_player::timeline::Timeline;

use crate::history::Doc;
use crate::{AppState, CmdResult, err};

/// The player open, if one is.
#[derive(Default)]
pub(crate) struct Native {
    open: Mutex<Option<Open>>,
    /// Counted up at every open and close, so an open that takes a while
    /// (the file is probed first) finds out the page has moved on since.
    asked: AtomicU64,
    /// Counted up at every plan asked for, and the newest one applied: plans
    /// are made off the main thread, so an older one can be done last.
    plans: AtomicU64,
    planned: Mutex<u64>,
}

struct Open {
    player: Player,
    /// Opened for this VOD or edit; a page that asks for the same one keeps it.
    doc: Doc,
    /// For a VOD played from what its site's player fetches: its session's
    /// token (embed.rs), so only that session changes or closes it.
    embed: Option<u64>,
}

/// What the page is told of the player: `running` once the time moves
/// (playing, it stands until the picture and sound start), for the page's
/// clock to run from then rather than from the play, and at `speed`; and
/// `shown`, where the frame on screen is (none: no frame shows), which the
/// timeline's playhead follows. Told at every frame and every change.
#[derive(Serialize, Clone, Copy)]
pub(crate) struct Status {
    /// Counts up with each status made: a command's reply and the
    /// `player-time` events travel apart, so the page keeps the newest.
    seq: u64,
    time: f64,
    playing: bool,
    running: bool,
    speed: f64,
    shown: Option<f64>,
    /// The time stands for footage still to come (a VOD from a site).
    waiting: bool,
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

fn status(player: &Player) -> Status {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    Status {
        seq: SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        time: player.time(),
        playing: player.playing(),
        running: player.running(),
        speed: player.speed(),
        shown: player.shown(),
        waiting: player.waiting_for_footage(),
    }
}

/// Whether there's a player here: a screen to draw on.
#[tauri::command]
pub(crate) fn player_available(app: AppHandle) -> bool {
    crate::screen::attached(&app)
}

/// Opens VOD `id`'s file at `at` seconds, playing if `play`. The one open
/// for it already is kept, moved there. Fails if the page closed the player,
/// or opened another, while the file was being probed.
#[tauri::command]
pub(crate) async fn player_open(app: AppHandle, id: i64, at: f64, play: bool) -> CmdResult<Opened> {
    let make = move |app: &AppHandle| {
        let state = app.state::<AppState>();
        let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!(crate::worker::NO_FFMPEG))?;
        let file = crate::media::playable_file(&state.db.lock().unwrap(), id)
            .ok_or_else(|| anyhow::anyhow!("This VOD has no file here to play."))?;
        Player::open_file(&ffmpeg, file.as_ref())
    };
    open_with(app, Doc::Vod(id), at, play, None, make).await
}

/// Opens edit `id`, its pieces end to end from their footage, at `at`
/// seconds, playing if `play`. Footage not here yet is fetched, and the
/// page asks for a plan again (`player_plan`) as it comes in.
#[tauri::command]
pub(crate) async fn player_open_edit(app: AppHandle, id: i64, at: f64, play: bool) -> CmdResult<Opened> {
    let make = move |app: &AppHandle| {
        let state = app.state::<AppState>();
        let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!(crate::worker::NO_FFMPEG))?;
        let (timeline, _) = edit_timeline(app, id)?;
        Player::open(&ffmpeg, timeline)
    };
    open_with(app, Doc::Edit(id), at, play, None, make).await
}

/// Edit `id`'s timeline as its pieces and the footage here are now, and
/// its stretches for the page. The footage missing is asked for.
fn edit_timeline(app: &AppHandle, id: i64) -> anyhow::Result<(Timeline, Vec<PlannedStretch>)> {
    let state = app.state::<AppState>();
    let missing = {
        let db = state.db.lock().unwrap();
        let pieces = crate::edits::pieces(app, &db, id)?;
        crate::edits::missing(app, &db, &pieces)
    };
    state.footage.ask(missing);
    clip_timeline(app, id)
}

/// Clip `id`'s timeline (an edit's) as its pieces and the footage here are
/// now, and its stretches for the page.
fn clip_timeline(app: &AppHandle, id: i64) -> anyhow::Result<(Timeline, Vec<PlannedStretch>)> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let pieces = crate::edits::pieces(app, &db, id)?;
    let timeline = Timeline::of(&pieces, |vod, a, b| crate::footage::cover(app, &db, vod, a, b));
    drop(db);
    let stretches = timeline
        .stretches()
        .iter()
        .map(|s| PlannedStretch { at: s.at, length: s.length, piece_id: s.piece_id, footage: s.source.is_some() })
        .collect();
    Ok((timeline, stretches))
}

/// A stretch of an edit's timeline, as the page sees it.
#[derive(Serialize, Clone, Copy)]
pub(crate) struct PlannedStretch {
    at: f64,
    length: f64,
    piece_id: Option<i64>,
    /// Its footage is here (else it waits, playing nothing).
    footage: bool,
}

/// Edit `id`'s stretches as its pieces and footage are now; the player open
/// for it plays them from here on (without a break where the playhead's
/// stretch is unchanged).
#[tauri::command]
pub(crate) async fn player_plan(app: AppHandle, id: i64) -> CmdResult<Vec<PlannedStretch>> {
    let handle = app.clone();
    let plan = app.state::<AppState>().player.plans.fetch_add(1, Ordering::AcqRel) + 1;
    let (timeline, stretches) =
        tauri::async_runtime::spawn_blocking(move || edit_timeline(&handle, id)).await.map_err(err)?.map_err(err)?;
    let state = app.state::<AppState>();
    let mut planned = state.player.planned.lock().unwrap();
    if plan > *planned {
        *planned = plan;
        if let Some(o) = state.player.open.lock().unwrap().as_ref().filter(|o| o.doc == Doc::Edit(id)) {
            o.player.set_timeline(timeline);
        }
    }
    Ok(stretches)
}

/// Opens `doc` with `make` (off the main thread), unless the one open is
/// for it already; then moves to `at`, playing if `play`.
async fn open_with(
    app: AppHandle,
    doc: Doc,
    at: f64,
    play: bool,
    embed: Option<u64>,
    make: impl FnOnce(&AppHandle) -> anyhow::Result<Player> + Send + 'static,
) -> CmdResult<Opened> {
    let state = app.state::<AppState>();
    let asked = state.player.asked.fetch_add(1, Ordering::AcqRel) + 1;
    let same = {
        let open = state.player.open.lock().unwrap();
        open.as_ref().filter(|o| o.doc == doc && o.embed == embed).map(|o| opened(&o.player))
    };
    let opened = match same {
        Some(opened) => opened,
        None => {
            let handle = app.clone();
            let player =
                tauri::async_runtime::spawn_blocking(move || make(&handle)).await.map_err(err)?.map_err(err)?;
            let opened = opened(&player);
            // Each frame due goes to the screen; the time to the page now and
            // then, and at once when it starts to move or the end is reached.
            let (shown, told) = (app.clone(), app.clone());
            player.on_frame(Box::new(move |event| {
                let state = shown.state::<AppState>();
                let open = state.player.open.lock().unwrap();
                let Some(open) = open.as_ref() else { return };
                if event == Event::Frame {
                    // None when there's nothing to play: black.
                    crate::screen::show(&shown, open.player.frame());
                }
                let _ = told.emit("player-time", status(&open.player));
            }));
            let mut open = state.player.open.lock().unwrap();
            if state.player.asked.load(Ordering::Acquire) != asked {
                return Err(CLOSED.into());
            }
            *open = Some(Open { player, doc, embed });
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

/// Opens VOD `vod_id` for embed session `token` on `timeline` (what its
/// site's player has appended: embed.rs), waiting where there's nothing yet,
/// at `at`, paused.
pub(crate) async fn open_embed(
    app: AppHandle,
    vod_id: i64,
    token: u64,
    timeline: Timeline,
    at: f64,
) -> CmdResult<Opened> {
    let make = move |app: &AppHandle| {
        let state = app.state::<AppState>();
        let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!(crate::worker::NO_FFMPEG))?;
        let player = Player::open(&ffmpeg, timeline)?;
        player.set_waits_for_footage(true);
        Ok(player)
    };
    // A new one each time: what's kept starts over.
    close(&app);
    open_with(app, Doc::Vod(vod_id), at, false, Some(token), make).await
}

/// Embed session `token`'s player, if it's the one open, plays `timeline`
/// from here on.
pub(crate) fn set_embed_timeline(app: &AppHandle, token: u64, timeline: Timeline) {
    let state = app.state::<AppState>();
    let open = state.player.open.lock().unwrap();
    if let Some(o) = open.as_ref().filter(|o| o.embed == Some(token))
        && o.player.timeline() != timeline
    {
        o.player.set_timeline(timeline);
    }
}

/// Closes embed session `token`'s player, if it's the one open (a late
/// close of an earlier VOD's leaves the next one's be).
pub(crate) fn close_embed(app: &AppHandle, token: u64) {
    let state = app.state::<AppState>();
    let closed = {
        let mut open = state.player.open.lock().unwrap();
        // Checked and taken under one lock: nothing opened in between goes.
        open.as_ref().is_some_and(|o| o.embed == Some(token)).then(|| {
            state.player.asked.fetch_add(1, Ordering::AcqRel);
            open.take()
        })
    };
    if closed.is_some() {
        drop(closed);
        crate::screen::show(app, None);
    }
}

/// The frame showing now, for the page to show while something covers
/// the screen (which then hides): its width and height (each 4 bytes,
/// little-endian), then its pixels as RGB. Empty if none shows.
#[tauri::command]
pub(crate) fn player_still(app: AppHandle) -> tauri::ipc::Response {
    let state = app.state::<AppState>();
    let frame = state.player.open.lock().unwrap().as_ref().and_then(|o| o.player.frame());
    let bytes = frame.map_or_else(Vec::new, |f| {
        let mut out = Vec::with_capacity(8 + f.rgb.len());
        out.extend_from_slice(&f.width.to_le_bytes());
        out.extend_from_slice(&f.height.to_le_bytes());
        out.extend_from_slice(&f.rgb);
        out
    });
    tauri::ipc::Response::new(bytes)
}

/// The frame showing now, for the page to draw itself where the picture
/// can't go under the page (X11; screen_under): empty if it's frame `since`
/// (or none shows), else its width and height (4 bytes each, little-endian)
/// and number (8 bytes), then RGBA pixels, scaled down to fit in `width` by
/// `height` (never up). Async, so the scaling is off the main thread.
#[tauri::command]
pub(crate) async fn player_frame(app: AppHandle, since: Option<u64>, width: u32, height: u32) -> tauri::ipc::Response {
    let state = app.state::<AppState>();
    let frame = state.player.open.lock().unwrap().as_ref().and_then(|o| o.player.frame());
    let Some(f) = frame.filter(|f| Some(f.id) != since && f.width > 0 && f.height > 0) else {
        return tauri::ipc::Response::new(Vec::new());
    };
    let k = (width.max(1) as f64 / f.width as f64).min(height.max(1) as f64 / f.height as f64).min(1.0);
    let (w, h) = (((f.width as f64 * k).round() as u32).max(1), ((f.height as f64 * k).round() as u32).max(1));
    let mut out = Vec::with_capacity(16 + (w * h * 4) as usize);
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&f.id.to_le_bytes());
    scale_rgba(&f.rgb, f.width, f.height, w, h, &mut out);
    tauri::ipc::Response::new(out)
}

/// `rgb` (`fw` by `fh`) scaled to `w` by `h` as RGBA onto `out`: each pixel
/// the bilinear mix at its centre (at half size, the mean of four).
fn scale_rgba(rgb: &[u8], fw: u32, fh: u32, w: u32, h: u32, out: &mut Vec<u8>) {
    let (sx, sy) = (fw as f32 / w as f32, fh as f32 / h as f32);
    let (fw, fh) = (fw as usize, fh as usize);
    for y in 0..h {
        let v = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
        let (y0, ty) = (v as usize, v.fract());
        let y1 = (y0 + 1).min(fh - 1);
        for x in 0..w {
            let u = ((x as f32 + 0.5) * sx - 0.5).max(0.0);
            let (x0, tx) = (u as usize, u.fract());
            let x1 = (x0 + 1).min(fw - 1);
            let at = |xx: usize, yy: usize, c: usize| rgb[(yy * fw + xx) * 3 + c] as f32;
            for c in 0..3 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
                let bottom = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
                out.push((top * (1.0 - ty) + bottom * ty + 0.5) as u8);
            }
            out.push(255);
        }
    }
}

/// What an open that the page moved on from is told.
pub(crate) const CLOSED: &str = "The player was closed while opening.";

/// Closes the player, clearing the screen; not one an embed session has
/// open (only that session closes it: `close_embed`), so a page that closes
/// late leaves the next VOD's be.
#[tauri::command]
pub(crate) fn player_close(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.player.open.lock().unwrap().as_ref().is_some_and(|o| o.embed.is_some()) {
        return;
    }
    close(&app);
}

/// Closes whatever player is open, clearing the screen.
fn close(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.player.asked.fetch_add(1, Ordering::AcqRel);
    drop(state.player.open.lock().unwrap().take());
    crate::screen::show(app, None);
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

/// Plays `speed` times as fast, the sound at the same pitch.
#[tauri::command]
pub(crate) fn player_set_speed(app: AppHandle, speed: f64) -> CmdResult<Status> {
    with(&app, |p| p.set_speed(speed))
}

/// The loudness, 0 to 1.
#[tauri::command]
pub(crate) fn player_set_volume(state: State<'_, AppState>, volume: f32) -> CmdResult<()> {
    let open = state.player.open.lock().unwrap();
    open.as_ref().ok_or("No player is open.")?.player.set_volume(volume);
    Ok(())
}
