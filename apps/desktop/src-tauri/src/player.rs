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
use vodscribe_core::db::Db;
use vodscribe_core::tools;
use vodscribe_player::clock::{Event, Player};
use vodscribe_player::decode::{Frame, Layout};
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

impl Native {
    /// Whether a player is playing something other than `doc`: work in the
    /// background for `doc` waits meanwhile, so playing stays smooth.
    pub(crate) fn playing_other(&self, doc: Doc) -> bool {
        self.open.lock().unwrap().as_ref().is_some_and(|o| o.doc != doc && o.player.playing())
    }
}

/// What the app's player is at, for the end-to-end tests' page dump
/// (tests/e2e/lib/dump.mjs, through `e2e::e2e_player_state`): whether one
/// is open, for what, and what its threads are at, so a stall carries its
/// cause. `open` is None if the lock on it couldn't be had in a moment (a
/// thread holds it: stuck, or a slow open).
#[cfg(any(feature = "e2e", test))]
#[derive(Serialize)]
pub(crate) struct Seen {
    open: Option<bool>,
    doc: Option<Doc>,
    embed: Option<u64>,
    player: Option<vodscribe_player::clock::Diagnosis>,
}

#[cfg(any(feature = "e2e", test))]
impl Native {
    pub(crate) fn seen(&self) -> Seen {
        let end = std::time::Instant::now() + std::time::Duration::from_millis(500);
        let open = loop {
            match self.open.try_lock() {
                Ok(open) => break Some(open),
                Err(std::sync::TryLockError::Poisoned(poisoned)) => break Some(poisoned.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) if std::time::Instant::now() < end => {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                Err(std::sync::TryLockError::WouldBlock) => break None,
            }
        };
        let Some(open) = open else { return Seen { open: None, doc: None, embed: None, player: None } };
        Seen {
            open: Some(open.is_some()),
            doc: open.as_ref().map(|o| o.doc),
            embed: open.as_ref().and_then(|o| o.embed),
            player: open.as_ref().map(|o| o.player.diagnosis()),
        }
    }
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
    /// The frame rate of what plays at the frame shown (else at the time), if
    /// known: frames are counted by it.
    fps: Option<f64>,
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
        fps: player.fps_at(player.shown().unwrap_or_else(|| player.time())),
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
///
/// A VOD from a site without its own player whose video is still
/// downloading (videos.rs) plays as far as it's in (`streamed`).
#[tauri::command]
pub(crate) async fn player_open(app: AppHandle, id: i64, at: f64, play: bool) -> CmdResult<Opened> {
    let make = move |app: &AppHandle| {
        let state = app.state::<AppState>();
        let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!(crate::worker::NO_FFMPEG))?;
        let file = crate::media::playable_file(&state.db.lock().unwrap(), id);
        match file {
            Some(file) => Player::open_file(&ffmpeg, file.as_ref()),
            None => streamed(app, &ffmpeg, id),
        }
    };
    let opened = open_with(app.clone(), Doc::Vod(id), at, play, None, make).await?;
    let state = app.state::<AppState>();
    catch_up(&state.player, &state.db, id);
    Ok(opened)
}

/// The player just opened for VOD `id`, if it plays the VOD as its video
/// downloads (`streamed`), plays the video kept, if it was kept meanwhile:
/// `refile` found no player open then (it was being probed), and the
/// section it plays goes once the video is kept.
fn catch_up(native: &Native, db: &Mutex<Db>, id: i64) {
    // Read after the open is in place: a video recorded since the open read
    // the library was, so its `refile` either found this player or comes
    // before this.
    let kept = {
        let db = db.lock().unwrap();
        let vod = db.vod(id).ok().flatten().filter(|v| v.media && v.source_path.is_none());
        vod.and_then(|v| Some((crate::media::playable_file(&db, id)?, v.info.duration?)))
    };
    if let Some((path, duration)) = kept {
        refile_open(native, id, std::path::Path::new(&path), duration);
    }
}

/// A player for VOD `id`, from a site without its own player, on the
/// footage of its video as far as it has downloaded (`footage::whole_timeline`),
/// as long as its site says it is: where the download hasn't reached, the
/// time stands (the picture holding its last frame) and the page says it's
/// waiting, till more comes in (`footage_grew`) and it plays on. A seek past
/// what's in waits the same way. Once the download is done, it plays the
/// video kept (`refile`).
///
/// The risk: the length is the site's, not the file's (not known till it's
/// all in); a video that turns out shorter ends early in the wait, and one
/// longer is cut short till it's kept. What's in is read through a growing
/// Matroska file, up to its last whole cluster less a second (`footage::Growing`).
fn streamed(app: &AppHandle, ffmpeg: &std::path::Path, id: i64) -> anyhow::Result<Player> {
    let state = app.state::<AppState>();
    let timeline = {
        let db = state.db.lock().unwrap();
        let vod = db.vod(id)?.ok_or_else(|| anyhow::anyhow!("That VOD is gone."))?;
        let duration = vod.info.duration.filter(|d| *d > 0.0);
        let Some(duration) = duration.filter(|_| vodscribe_core::pipeline::keeps_video(&vod.info.platform)) else {
            anyhow::bail!("This VOD has no file here to play.");
        };
        crate::footage::whole_timeline(app, &db, id, duration)
    };
    let player = Player::open(ffmpeg, timeline)?;
    player.set_waits_for_footage(true);
    Ok(player)
}

/// More of VOD `vod_id`'s footage is in (or it's moved): if the player
/// open plays it as it downloads (`streamed`: not a site's player, and no
/// file of its own yet), it plays on into what came.
pub(crate) fn footage_grew(app: &AppHandle, vod_id: i64) {
    let state = app.state::<AppState>();
    let streaming = |open: &Option<Open>| open.as_ref().is_some_and(|o| o.doc == Doc::Vod(vod_id) && o.embed.is_none());
    if !streaming(&state.player.open.lock().unwrap()) {
        return;
    }
    let timeline = {
        let db = state.db.lock().unwrap();
        let Ok(Some(vod)) = db.vod(vod_id) else { return };
        let Some(duration) = vod.info.duration.filter(|_| !vod.media && vod.source_path.is_none()) else { return };
        crate::footage::whole_timeline(app, &db, vod_id, duration)
    };
    let open = state.player.open.lock().unwrap();
    if let Some(o) = open.as_ref().filter(|_| streaming(&open))
        && o.player.timeline() != timeline
    {
        o.player.set_timeline(timeline);
    }
}

/// VOD `vod_id`'s video is kept now, at `path` (`duration` long): the
/// player open playing it as it downloaded plays that from here on, where
/// it is, waiting for nothing.
pub(crate) fn refile(app: &AppHandle, vod_id: i64, path: &std::path::Path, duration: f64) {
    refile_open(&app.state::<AppState>().player, vod_id, path, duration);
}

/// `refile`'s work, on the player open in `native`: only one playing as the
/// video downloads (a player opened on the video kept has it already).
fn refile_open(native: &Native, vod_id: i64, path: &std::path::Path, duration: f64) {
    let open = native.open.lock().unwrap();
    let streaming = |o: &&Open| o.doc == Doc::Vod(vod_id) && o.embed.is_none() && o.player.waits_for_footage();
    if let Some(o) = open.as_ref().filter(streaming) {
        o.player.set_timeline(Timeline::file(path, duration));
        o.player.set_waits_for_footage(false);
    }
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
        // The frame is YUV; the page takes RGB, turned here on the CPU.
        let mut rgb = Vec::new();
        f.to_rgb(Layout::Rgb, &mut rgb);
        let mut out = Vec::with_capacity(8 + rgb.len());
        out.extend_from_slice(&f.width.to_le_bytes());
        out.extend_from_slice(&f.height.to_le_bytes());
        out.extend_from_slice(&rgb);
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
    let (w, h) = fit(&f, width, height);
    let mut out = Vec::with_capacity(16 + (w * h * 4) as usize);
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&f.id.to_le_bytes());
    rgba(&f, w, h, &mut out);
    tauri::ipc::Response::new(out)
}

/// `f`'s size scaled down to fit in `width` by `height` (never up).
fn fit(f: &Frame, width: u32, height: u32) -> (u32, u32) {
    let k = (width.max(1) as f64 / f.width as f64).min(height.max(1) as f64 / f.height as f64).min(1.0);
    (((f.width as f64 * k).round() as u32).max(1), ((f.height as f64 * k).round() as u32).max(1))
}

/// `f` as RGBA `w` by `h`, onto `out`. The frame is YUV: turned into RGB
/// here, on the CPU, then scaled.
fn rgba(f: &Frame, w: u32, h: u32, out: &mut Vec<u8>) {
    let mut rgb = Vec::new();
    f.to_rgb(Layout::Rgb, &mut rgb);
    scale_rgba(&rgb, f.width, f.height, w, h, out);
}

/// The frame a seek to `time` of `doc` would show, for the timeline's
/// thumbnail, if `doc` is what the player has open (a site's VOD played
/// from its player has none): its width and height (4 bytes each), the
/// time it shows at (an f64) and whether it's exactly the frame there (4
/// bytes, 1 or 0), all little-endian, then RGBA pixels scaled down to fit
/// in `width` by `height`. Empty if there's none. The player isn't moved.
#[tauri::command]
pub(crate) async fn player_preview(
    app: AppHandle,
    doc: Doc,
    time: f64,
    width: u32,
    height: u32,
) -> CmdResult<tauri::ipc::Response> {
    let previewer = {
        let state = app.state::<AppState>();
        let open = state.player.open.lock().unwrap();
        open.as_ref().filter(|o| o.doc == doc && o.embed.is_none()).map(|o| o.player.previewer())
    };
    let Some(previewer) = previewer else { return Ok(tauri::ipc::Response::new(Vec::new())) };
    // Off the main thread, and without the player held: a frame not held
    // yet is decoded first.
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let Some(preview) = previewer.preview(time) else { return Vec::new() };
        let f = &preview.frame;
        let (w, h) = fit(f, width, height);
        let mut out = Vec::with_capacity(20 + (w * h * 4) as usize);
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&preview.time.to_le_bytes());
        out.extend_from_slice(&(preview.exact as u32).to_le_bytes());
        rgba(f, w, h, &mut out);
        out
    })
    .await
    .map_err(err)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// How many video decoders the player has started since the app did: for
/// the end-to-end tests, that play where the pointer rested started none.
#[tauri::command]
pub(crate) fn player_decoders_started() -> u64 {
    vodscribe_player::decode::video_starts()
}

/// `rgb` (`fw` by `fh`) scaled to `w` by `h` as RGBA onto `out`: each pixel
/// the bilinear mix at its centre (at half size, the mean of four); more
/// than halved, the mean of all the pixels it covers, which bilinear's four
/// would skip most of (fine lines flicker, edges alias).
fn scale_rgba(rgb: &[u8], fw: u32, fh: u32, w: u32, h: u32, out: &mut Vec<u8>) {
    let (sx, sy) = (fw as f32 / w as f32, fh as f32 / h as f32);
    if sx > 2.0 || sy > 2.0 {
        return area_rgba(rgb, fw as usize, fh as usize, w as usize, h as usize, out);
    }
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

/// `scale_rgba` by area: each pixel the mean of the source pixels under it.
fn area_rgba(rgb: &[u8], fw: usize, fh: usize, w: usize, h: usize, out: &mut Vec<u8>) {
    let span = |i: usize, n: usize, of: usize| {
        let (a, b) = (i * of / n, ((i + 1) * of / n).max(i * of / n + 1).min(of));
        a..b
    };
    for y in 0..h {
        let rows = span(y, h, fh);
        for x in 0..w {
            let cols = span(x, w, fw);
            let mut sum = [0u32; 3];
            for yy in rows.clone() {
                for xx in cols.clone() {
                    let at = (yy * fw + xx) * 3;
                    for c in 0..3 {
                        sum[c] += rgb[at + c] as u32;
                    }
                }
            }
            let n = (rows.len() * cols.len()) as u32;
            out.extend(sum.map(|s| ((s + n / 2) / n) as u8));
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

#[cfg(test)]
mod tests {
    use super::*;
    use vodscribe_core::fetch::VodInfo;

    /// The tests' account of the player: none open, then one, as JSON the dump reads.
    #[test]
    fn the_player_seen_says_whether_one_is_open_and_what_its_threads_are_at() {
        let native = Native::default();
        let none = serde_json::to_value(native.seen()).unwrap();
        assert_eq!(none["open"], false);
        assert!(none["player"].is_null());
        let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::default()).unwrap();
        *native.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(7), embed: None });
        let seen = serde_json::to_value(native.seen()).unwrap();
        assert_eq!(seen["open"], true);
        assert_eq!(seen["doc"], serde_json::json!({ "kind": "vod", "id": 7 }));
        assert_eq!(seen["player"]["thread_alive"], true);
        assert_eq!(seen["player"]["decoding"], false);
        assert_eq!(seen["player"]["state"]["playing"], false);
        // Held by a stuck thread: said so, not waited on.
        let held = native.open.lock().unwrap();
        let begun = std::time::Instant::now();
        let stuck = std::thread::scope(|s| s.spawn(|| serde_json::to_value(native.seen()).unwrap()).join().unwrap());
        assert!(stuck["open"].is_null());
        assert!(begun.elapsed() < std::time::Duration::from_secs(2));
        drop(held);
    }

    /// A VOD from a site without its own player, its video downloading
    /// (videos.rs `stream_then_keep`), opened while the download ends: the
    /// open reads the library before the video is recorded, and the
    /// recording's `refile` comes before the open is in place.
    #[test]
    fn a_vod_opened_as_its_video_is_kept_plays_the_video_kept() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let info = VodInfo {
            platform: "vimeo".into(),
            platform_id: "x".into(),
            url: "https://vimeo.com/x".into(),
            title: "t".into(),
            channel: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let id = db.add_vod(&info).unwrap().0.id;
        let db = Mutex::new(db);
        let native = Native::default();
        // player_open's `streamed`: what's in of the section (none, here), waiting for the rest.
        assert!(!db.lock().unwrap().vod(id).unwrap().unwrap().media, "no video kept yet");
        let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::covering(60.0, Vec::new())).unwrap();
        player.set_waits_for_footage(true);
        // Meanwhile (the probe), the download ends: recorded, then refiled, with no player open yet.
        let kept = std::env::temp_dir().join(format!("vodscribe-refile-{}.mp4", std::process::id()));
        db.lock().unwrap().set_media_path(id, Some(&kept.to_string_lossy())).unwrap();
        refile_open(&native, id, &kept, 60.0);
        // The open is in place (open_with), and catches up.
        *native.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(id), embed: None });
        catch_up(&native, &db, id);
        let open = native.open.lock().unwrap();
        let player = &open.as_ref().unwrap().player;
        assert_eq!(player.timeline(), Timeline::file(&kept, 60.0), "the video kept, not the section it replaced");
        assert!(!player.waits_for_footage());
    }

    #[test]
    fn frames_scale_down_by_mixing_or_by_area() {
        // 4 by 2: a white column, then black ones, over two rows.
        let row = [255u8, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let rgb = [row, row].concat();
        // Halved: each pixel the mean of the two under it.
        let mut out = Vec::new();
        scale_rgba(&rgb, 4, 2, 2, 1, &mut out);
        assert_eq!(out, [128, 128, 128, 255, 0, 0, 0, 255]);
        // To one pixel, a quarter: the mean of all four columns, not two of them.
        let mut out = Vec::new();
        scale_rgba(&rgb, 4, 2, 1, 1, &mut out);
        assert_eq!(out, [64, 64, 64, 255]);
        // A thin line in a 12x reduction still counts, as bilinear's taps would miss it.
        let mut wide = vec![0u8; 24 * 3];
        wide[13 * 3..14 * 3].copy_from_slice(&[240, 240, 240]);
        let mut out = Vec::new();
        scale_rgba(&wide, 24, 1, 2, 1, &mut out);
        assert_eq!(out, [0, 0, 0, 255, 20, 20, 20, 255]);
    }
}
