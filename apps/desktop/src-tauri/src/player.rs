//! The app's own player (crates/player) playing a VOD's file, a VOD from a
//! site, or an edit on the screen (screen.rs), driven by the page's controls
//! (NativePlayer.svelte): one player at a time. The page is told the time and
//! whether it's playing at every change and now and then while playing
//! (`player-time`), and keeps time between by its own clock.
//! A VOD from a site plays from footage fetched as it plays (footage.rs):
//! `feed` asks for what's ahead of the playhead, first in line, and plays the
//! footage as it comes in; where it isn't here yet, the time waits for it.

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
    /// For a VOD from a site: the VOD and its own clip, which plays from
    /// footage fetched as it plays (`feed`).
    fetched: Option<(i64, i64)>,
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

/// Opens VOD `id` at `at` seconds, playing if `play`: its file, or one from
/// a site from its footage, fetched as it plays (`feed`). The one open for
/// it already is kept, moved there. Fails if the page closed the player, or
/// opened another, while the file was being probed.
#[tauri::command]
pub(crate) async fn player_open(app: AppHandle, id: i64, at: f64, play: bool) -> CmdResult<Opened> {
    let make = move |app: &AppHandle| {
        let state = app.state::<AppState>();
        let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!(crate::worker::NO_FFMPEG))?;
        let (file, clip) = {
            let db = state.db.lock().unwrap();
            let file = crate::media::playable_file(&db, id);
            let site =
                db.vod(id)?.is_some_and(|v| vodscribe_core::pipeline::SITE_PLAYERS.contains(&v.info.platform.as_str()));
            (file, if site { db.vod_clip(id)? } else { None })
        };
        if let Some(file) = file {
            return Ok((Player::open_file(&ffmpeg, file.as_ref())?, None));
        }
        let clip = clip.ok_or_else(|| anyhow::anyhow!("This VOD has no file here to play."))?;
        let (timeline, _) = clip_timeline(app, clip)?;
        let player = Player::open(&ffmpeg, timeline)?;
        player.set_waits_for_footage(true);
        Ok((player, Some((id, clip))))
    };
    open_with(app, Doc::Vod(id), at, play, make).await
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
        Ok((Player::open(&ffmpeg, timeline)?, None))
    };
    open_with(app, Doc::Edit(id), at, play, make).await
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

/// Clip `id`'s timeline (an edit's, or a VOD's own) as its pieces and the
/// footage here are now, and its stretches for the page.
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
    make: impl FnOnce(&AppHandle) -> anyhow::Result<(Player, Option<(i64, i64)>)> + Send + 'static,
) -> CmdResult<Opened> {
    let state = app.state::<AppState>();
    let asked = state.player.asked.fetch_add(1, Ordering::AcqRel) + 1;
    let same = {
        let open = state.player.open.lock().unwrap();
        open.as_ref().filter(|o| o.doc == doc).map(|o| opened(&o.player))
    };
    let opened = match same {
        Some(opened) => opened,
        None => {
            let handle = app.clone();
            let (player, fetched) =
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
            *open = Some(Open { player, doc, fetched });
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

/// How far ahead of where a VOD from a site plays its footage is fetched.
const AHEAD: f64 = 90.0;
/// How long a stretch the player asks for at a time (a section holds a
/// margin more either side: `footage::section`).
const ASK: f64 = 120.0;
/// A gap in the footage ahead shorter than this isn't asked for (one section ends where the next begins).
const NEAR: f64 = 0.5;
/// How often `feed` looks.
const FEED_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// While a VOD from a site plays (or is paused) in the player: asks for the
/// footage of what's ahead of the playhead that isn't here or coming, first
/// in line, and plays what's come in since (`set_timeline`, without a break).
/// The page is told the time while it waits for footage.
pub(crate) fn feed(app: AppHandle) {
    let spawned = std::thread::Builder::new().name("player feed".into()).spawn(move || {
        let mut told_waiting = false;
        loop {
            std::thread::sleep(FEED_EVERY);
            let state = app.state::<AppState>();
            let (vod, clip, time) = {
                let open = state.player.open.lock().unwrap();
                match open.as_ref().and_then(|o| Some((o.fetched?, o.player.time()))) {
                    Some(((vod, clip), time)) => (vod, clip, time),
                    None => continue,
                }
            };
            // The first moment ahead that isn't here, if it's near.
            let gap = {
                let db = state.db.lock().unwrap();
                let duration = db.vod(vod).ok().flatten().and_then(|v| v.info.duration).unwrap_or(0.0);
                let until = (time + AHEAD).min(duration);
                let mut at = time;
                for (from, to, _) in crate::footage::cover(&app, &db, vod, time, until) {
                    if from > at + NEAR {
                        break;
                    }
                    at = at.max(to);
                }
                (at < until - NEAR).then_some(at)
            };
            if let Some(at) = gap
                && !state.footage.coming(vod, at)
            {
                state.footage.ask_first(vod, at, at + ASK);
            }
            let Ok((timeline, _)) = clip_timeline(&app, clip) else { continue };
            let open = state.player.open.lock().unwrap();
            let Some(o) = open.as_ref().filter(|o| o.fetched == Some((vod, clip))) else { continue };
            if o.player.timeline() != timeline {
                o.player.set_timeline(timeline);
            }
            // Waiting, no frames come to tell the page the time: told here.
            let waiting = o.player.waiting_for_footage();
            if waiting || told_waiting {
                let _ = app.emit("player-time", status(&o.player));
            }
            told_waiting = waiting;
        }
    });
    if let Err(e) = spawned {
        eprintln!("player feed: {e}");
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
