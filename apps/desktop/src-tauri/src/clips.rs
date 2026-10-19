//! Clips (`vodscribe_core::clips`): the commands the interface makes and
//! changes them with, and the queue that cuts their files, one at a time. A
//! clip of a VOD that plays from a file here is cut from it; of any other, its
//! range is downloaded (`fetch::Wanted::Section`) and cut from that.
//!
//! Clip files go in the computer's Videos folder, under VODscribe: they're for
//! taking elsewhere. Each is named for its range, so a clip whose range
//! changes is a new file; the one before goes, unless it was dragged out to
//! another app (`handed_out`), which may link to it where it is.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{Clip, ClipStatus, Db, Mark};
use vodscribe_core::settings::{ClipFormat, Settings};
use vodscribe_core::{clips, fetch, tools};

use crate::history::{self, Doc};
use crate::jobs::Jobs;
use crate::{AppState, CmdResult, err, media};

/// The longest clip: longer than any highlight, short of a whole VOD.
const LONGEST: f64 = 3.0 * 3600.0;

/// How often a cut's progress is sent.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Clips cut one at a time, by id (`jobs`).
pub(crate) struct Cutter {
    jobs: Jobs<i64>,
    /// VODs being removed: their clips still queued aren't cut (the rows go
    /// last, after the transcript and chat, which can take a while).
    removing: Arc<Mutex<HashSet<i64>>>,
}

impl Cutter {
    pub(crate) fn new() -> Self {
        Self { jobs: Jobs::new(), removing: Arc::default() }
    }

    /// Starts cutting, once the app's state is in place, beginning with the
    /// clips left unfinished when the app last stopped.
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        let removing = self.removing.clone();
        let unfinished = {
            let state = app.state::<AppState>();
            let db = state.db.lock().unwrap();
            // Clips made only to be dragged out: their files are someone else's now.
            db.forget_hidden_clips()?;
            db.unfinished_clips()?
        };
        {
            // Pictures of clips no longer there (deleted with their VOD).
            let thumbs = thumbs_dir(&app);
            let state = app.state::<AppState>();
            let db = state.db.lock().unwrap();
            for entry in std::fs::read_dir(&thumbs).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let id = name.strip_suffix(".png").and_then(|id| id.parse::<i64>().ok());
                if id.is_none_or(|id| db.clip(id).ok().flatten().is_none()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        self.jobs.run("clips", move |id, stop| {
            if let Err(e) = cut(&app, id, stop, &removing) {
                eprintln!("clip {id}: {e:#}");
                fail(&app, id, &e);
            }
        })?;
        for clip in unfinished {
            self.queue(clip.id);
        }
        Ok(())
    }

    pub(crate) fn queue(&self, id: i64) {
        self.jobs.push(id);
    }

    /// Stops the cut of a clip of VOD `vod_id`, if one's being cut, and cuts
    /// none of its others (its VOD is being deleted: a download from its site
    /// could take a while).
    pub(crate) fn stop_vod(&self, db: &Db, vod_id: i64) {
        self.removing.lock().unwrap().insert(vod_id);
        self.jobs.stop_running(|&id| db.clip(id).ok().flatten().is_some_and(|c| c.vod_id == vod_id));
    }

    /// Stops clip `id`'s cut, if it's the one being cut.
    pub(crate) fn stop(&self, id: i64) {
        self.jobs.stop_running(|&j| j == id);
    }
}

/// Where clip files go: VODscribe in the Videos folder, or in the app's data
/// where there's no Videos folder.
pub(crate) fn clips_dir(app: &AppHandle) -> PathBuf {
    match app.path().video_dir() {
        Ok(videos) => videos.join("VODscribe"),
        Err(_) => app.state::<AppState>().paths.local.join("clips"),
    }
}

/// Each clip's first frame, shown while it's dragged out (`<clip id>.png`).
fn thumbs_dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.local.join("clip-thumbs")
}

fn thumb_path(app: &AppHandle, id: i64) -> PathBuf {
    thumbs_dir(app).join(format!("{id}.png"))
}

#[derive(Serialize, Clone)]
struct Progress {
    id: i64,
    progress: f64,
}

fn emit(app: &AppHandle, db: &Db, id: i64) {
    if let Ok(Some(clip)) = db.clip(id) {
        let _ = app.emit("clip", clip);
    }
}

/// Shows clip `id` as failed with `e`, if it failed before its file was
/// begun (no ffmpeg, say) and so still says it's waiting.
fn fail(app: &AppHandle, id: i64, e: &anyhow::Error) {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    if let Ok(Some(clip)) = db.clip(id)
        && clip.status == ClipStatus::Waiting
        && db.set_clip_file(id, clip.start, clip.end, ClipStatus::Error, None, Some(&format!("{e:#}"))).is_ok()
    {
        emit(app, &db, id);
    }
}

/// Makes clip `id`'s file, if it's waiting and its VOD plays from a file here
/// (and isn't being removed: `removing`).
fn cut(app: &AppHandle, id: i64, stop: &AtomicBool, removing: &Mutex<HashSet<i64>>) -> Result<()> {
    let state = app.state::<AppState>();
    let (clip, vod, source) = {
        let db = state.db.lock().unwrap();
        let removed = |c: &Clip| removing.lock().unwrap().contains(&c.vod_id);
        let Some(clip) = db.clip(id)?.filter(|c| c.status != ClipStatus::Ready && !removed(c)) else { return Ok(()) };
        let Some(vod) = db.vod(clip.vod_id)? else { return Ok(()) };
        let source = media::playable_file(&db, vod.id);
        (clip, vod, source)
    };
    let range = (clip.start, clip.end);
    let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).context(crate::worker::NO_FFMPEG)?;
    let format = Settings::load(&state.paths.settings()).clip_format;
    // Named for its range: one already made for this range is made again in
    // place; a range moved gets a new file, and the old one stays where it is.
    let (folder, name) = clips::file_path(
        vod.info.channel.as_deref(),
        vod.info.published.as_deref(),
        &vod.info.title,
        range,
        vod.audio_only,
        format,
    );
    let dir = folder.map_or_else(|| clips_dir(app), |f| clips_dir(app).join(f));
    let out = match &clip.file_path {
        Some(path) if Path::new(path).file_name().is_some_and(|n| *n == *name) => PathBuf::from(path),
        _ => clips::unique_path(&dir, &name),
    };
    {
        let db = state.db.lock().unwrap();
        if !db.set_clip_file(id, range.0, range.1, ClipStatus::Cutting, None, None)? {
            return Ok(());
        }
        emit(app, &db, id);
    }
    let mut last = Instant::now();
    let mut report = |progress: f64| {
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            let _ = app.emit("clip-progress", Progress { id, progress });
        }
    };
    let result = match &source {
        Some(source) => clips::cut(&ffmpeg, Path::new(source), &out, range, vod.audio_only, format, stop, report),
        None => download_and_cut(app, &vod, id, range, &ffmpeg, &out, format, stop, &mut report),
    };
    let db = state.db.lock().unwrap();
    let path = out.to_string_lossy();
    let recorded = match &result {
        Ok(()) => db.set_clip_file(id, range.0, range.1, ClipStatus::Ready, Some(&path), None)?,
        // Stopped: moved (it's queued again) or deleted.
        Err(e) if vodscribe_core::is_canceled(e) => false,
        Err(e) => db.set_clip_file(id, range.0, range.1, ClipStatus::Error, None, Some(&format!("{e:#}")))?,
    };
    if result.is_ok() && db.clip(id)?.is_none() {
        // Deleted while it was being cut.
        delete_file(&out);
    }
    // Its file for the range before goes, unless another app took it (then
    // it stays there, and the new one is the app's own).
    if let Some(old) = clip.file_path.as_deref().filter(|p| recorded && result.is_ok() && Path::new(p) != out) {
        if db.clip_handed_out(id)? {
            db.take_back_clip(id)?;
        } else {
            delete_file(old);
        }
    }
    if recorded && result.is_ok() && !vod.audio_only {
        drop(db);
        if let Err(e) = clips::thumbnail(&ffmpeg, &out, &thumb_path(app, id)) {
            eprintln!("clip {id}: its picture: {e:#}");
        }
        emit(app, &state.db.lock().unwrap(), id);
    } else if recorded {
        emit(app, &db, id);
    }
    delete_leftovers();
    Ok(())
}

/// Seconds downloaded before a clip of a VOD from a site: the section is
/// copied, not encoded again, so it keeps no picture until its first keyframe
/// (a short clip could have none); this reaches back past one.
const LEAD_IN: f64 = 10.0;

/// Makes a clip of a VOD from a site: downloads its range, from a little
/// before (`LEAD_IN`; the bulk of the time, so most of the progress), then
/// cuts it from that exactly as from a file. A section's time 0 is where it
/// was asked to start.
#[allow(clippy::too_many_arguments)]
fn download_and_cut(
    app: &AppHandle,
    vod: &vodscribe_core::db::Vod,
    id: i64,
    (start, end): (f64, f64),
    ffmpeg: &Path,
    out: &Path,
    format: ClipFormat,
    stop: &AtomicBool,
    report: &mut impl FnMut(f64),
) -> Result<()> {
    let paths = &app.state::<AppState>().paths;
    let yt_dlp = tools::find_yt_dlp(&paths.bin()).context(fetch::NO_YT_DLP)?;
    let work = paths.work();
    std::fs::create_dir_all(&work)?;
    let info = work.join(format!("clip-{id}.info.json"));
    // Named for the range, so a download for an earlier range is never taken for this one.
    let stem = work.join(format!("clip-{id}-{}-{}", (start * 1000.0).round() as i64, (end * 1000.0).round() as i64));
    let selector = clips::selector(Settings::load(&paths.settings()).video_height());
    let from = (start - LEAD_IN).max(0.0);
    let wanted = fetch::Wanted::Section { selector: &selector, ffmpeg, start: from, end, matroska: false };
    let result = (|| {
        fetch::probe(&yt_dlp, &vod.info.url, &info, stop)?;
        let section =
            fetch::download_or_reprobe(&yt_dlp, &vod.info.url, &info, &stem, wanted, stop, |f| report(f * 0.8))?;
        let range = (start - from, end - from);
        clips::cut(ffmpeg, &section, out, range, vod.audio_only, format, stop, |p| report(0.8 + p * 0.2))
    })();
    // Whatever was downloaded goes, done or not.
    vodscribe_core::pipeline::remove_work_prefix(&work, &format!("clip-{id}"));
    result
}

/// A range that can be a clip of a VOD `duration` seconds long (if known).
fn check_range(start: f64, end: f64, duration: Option<f64>) -> CmdResult<()> {
    if !(start.is_finite() && end.is_finite() && start >= 0.0 && end > start) {
        return Err("A clip has to end after it starts.".into());
    }
    if end - start > LONGEST {
        return Err("A clip can be at most 3 hours long.".into());
    }
    if duration.is_some_and(|d| start >= d) {
        return Err("That's past the end of the video.".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn list_clips(app: AppHandle, vod_id: i64) -> CmdResult<Vec<Clip>> {
    crate::with_reads(app, move |db| db.clips(vod_id)).await
}

/// Makes a clip of VOD `vod_id` from `start` to `end` seconds, titled with
/// what's said in it; in the clip bar unless `shown` is false (one only
/// dragged out of the window).
#[tauri::command]
pub(crate) async fn add_clip(
    app: AppHandle,
    vod_id: i64,
    start: f64,
    end: f64,
    shown: Option<bool>,
) -> CmdResult<Clip> {
    let state = app.state::<AppState>();
    let clip = {
        let db = state.db.lock().unwrap();
        let vod = db.vod(vod_id).map_err(err)?.ok_or("That VOD is gone.")?;
        let end = vod.info.duration.map_or(end, |d| end.min(d));
        check_range(start, end, vod.info.duration)?;
        let words = db.words_between(vod_id, start, end).map_err(err)?;
        let doc = Doc::Vod(vod_id);
        let before = history::snapshot(&db, doc).ok().flatten();
        let clip = db.add_clip(vod_id, start, end, &clips::title(&words, start), shown.unwrap_or(true)).map_err(err)?;
        state.history.record(&db, doc, "Clip added", before);
        clip
    };
    state.clips.queue(clip.id);
    Ok(clip)
}

/// Moves clip `id` to `start`..`end`; its file is made again, named for the new range.
#[tauri::command]
pub(crate) async fn set_clip_range(app: AppHandle, id: i64, start: f64, end: f64) -> CmdResult<Clip> {
    let state = app.state::<AppState>();
    let clip = {
        let db = state.db.lock().unwrap();
        let clip = db.clip(id).map_err(err)?.ok_or("That clip is gone.")?;
        let duration = db.vod(clip.vod_id).map_err(err)?.and_then(|v| v.info.duration);
        let end = duration.map_or(end, |d| end.min(d));
        check_range(start, end, duration)?;
        let doc = Doc::Vod(clip.vod_id);
        let before = history::snapshot(&db, doc).ok().flatten();
        db.set_clip_range(id, start, end).map_err(err)?;
        state.history.record(&db, doc, "Clip resized", before);
        db.clip(id).map_err(err)?.ok_or("That clip is gone.")?
    };
    state.clips.stop(id);
    state.clips.queue(id);
    Ok(clip)
}

/// Puts clip `id` (one dragged out of the window) in the clip bar.
#[tauri::command]
pub(crate) async fn keep_clip(app: AppHandle, id: i64) -> CmdResult<Clip> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let doc = Doc::Vod(db.clip(id).map_err(err)?.ok_or("That clip is gone.")?.vod_id);
    let before = history::snapshot(&db, doc).ok().flatten();
    db.show_clip(id).map_err(err)?;
    state.history.record(&db, doc, "Clip added", before);
    // News of it too, in order with the cutter's (the page keeps the newest).
    emit(&app, &db, id);
    db.clip(id).map_err(err)?.ok_or_else(|| "That clip is gone.".into())
}

#[tauri::command]
pub(crate) async fn rename_clip(app: AppHandle, id: i64, title: String) -> CmdResult<Clip> {
    let title = clips::written_title(&title).ok_or("A clip needs a name.")?;
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let doc = Doc::Vod(db.clip(id).map_err(err)?.ok_or("That clip is gone.")?.vod_id);
    let before = history::snapshot(&db, doc).ok().flatten();
    db.rename_clip(id, &title).map_err(err)?;
    state.history.record(&db, doc, "Clip renamed", before);
    emit(&app, &db, id);
    db.clip(id).map_err(err)?.ok_or_else(|| "That clip is gone.".into())
}

/// Deletes clip `id`, and its file unless that was dragged out of the app
/// (it's someone else's then).
#[tauri::command]
pub(crate) async fn delete_clip(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    let Some(clip) = db.clip(id).map_err(err)? else { return Ok(()) };
    let doc = Doc::Vod(clip.vod_id);
    let before = history::snapshot(&db, doc).ok().flatten();
    remove(&app, &db, id).map_err(err)?;
    state.history.record(&db, doc, if clip.shown { "Clip deleted" } else { "Clip dropped" }, before);
    Ok(())
}

/// Takes off VOD `vod_id`'s clip bar the clips whose file was deleted outside
/// the app (in a file manager). Only where the file's folder is still there:
/// a drive that's away (unmounted, unplugged) takes none of them.
#[tauri::command]
pub(crate) async fn forget_gone_clips(app: AppHandle, state: State<'_, AppState>, vod_id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    for clip in db.clips(vod_id).map_err(err)? {
        let Some(path) = clip.file_path.as_deref().filter(|_| clip.shown && clip.status == ClipStatus::Ready) else {
            continue;
        };
        let path = Path::new(path);
        if !path.exists() && path.parent().is_some_and(Path::is_dir) {
            remove(&app, &db, clip.id).map_err(err)?;
        }
    }
    Ok(())
}

/// Deletes clip `id` (stopping its cut), and its file unless that was
/// dragged out of the app, and tells the page (`clip-removed`).
pub(crate) fn remove(app: &AppHandle, db: &Db, id: i64) -> Result<()> {
    let state = app.state::<AppState>();
    state.clips.stop(id);
    let clip = db.clip(id)?;
    let handed_out = db.clip_handed_out(id)?;
    db.delete_clip(id)?;
    if let Some(path) = clip.and_then(|c| c.file_path).filter(|_| !handed_out) {
        delete_file(path);
    }
    let _ = std::fs::remove_file(thumb_path(app, id));
    let _ = app.emit("clip-removed", id);
    Ok(())
}

/// Files that couldn't be deleted when they were to go: on Windows a file
/// can't be while something has it open (a render or the player reading a
/// clip's file). Tried again as renders and cuts end, and as the app quits.
static LEFTOVERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Deletes `path` now, or once nothing has it open (`delete_leftovers`).
pub(crate) fn delete_file(path: impl AsRef<Path>) {
    let path = path.as_ref();
    if std::fs::remove_file(path).is_err() && path.exists() {
        LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner()).push(path.to_path_buf());
    }
}

/// Deletes what couldn't be deleted before, if it can be now.
pub(crate) fn delete_leftovers() {
    LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| std::fs::remove_file(p).is_err() && p.exists());
}

/// Shows clip `id`'s file in the system file manager.
#[tauri::command]
pub(crate) async fn show_clip(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let clip = state.db.lock().unwrap().clip(id).map_err(err)?;
    let path = clip.and_then(|c| c.file_path).filter(|p| Path::new(p).is_file());
    app.opener().reveal_item_in_dir(path.ok_or("That clip's file isn't made yet.")?).map_err(err)
}

/// VOD `vod_id`'s marks on its timeline, in time order.
#[tauri::command]
pub(crate) async fn list_marks(app: AppHandle, vod_id: i64) -> CmdResult<Vec<Mark>> {
    crate::with_reads(app, move |db| db.marks(vod_id)).await
}

/// What a mark can be (`db::Mark`).
const MARK_KINDS: [&str; 3] = ["cut", "chapter", "highlight"];

/// A mark's title or note (`clips::written_title`).
fn mark_text(text: Option<String>) -> Option<String> {
    text.as_deref().and_then(clips::written_title)
}

/// Puts a mark on VOD `vod_id`'s timeline at `time` seconds: a cut (between
/// two frames) unless `kind` says a chapter or a highlight, with its `text`.
#[tauri::command]
pub(crate) async fn add_mark(
    state: State<'_, AppState>,
    vod_id: i64,
    time: f64,
    kind: Option<String>,
    text: Option<String>,
) -> CmdResult<Mark> {
    if !(time.is_finite() && time >= 0.0) {
        return Err("A mark has to be within the video.".into());
    }
    let kind = kind.unwrap_or_else(|| "cut".into());
    if !MARK_KINDS.contains(&kind.as_str()) {
        return Err("That isn't a kind of mark.".into());
    }
    let db = state.db.lock().unwrap();
    db.vod(vod_id).map_err(err)?.ok_or("That VOD is gone.")?;
    let doc = Doc::Vod(vod_id);
    let before = history::snapshot(&db, doc).ok().flatten();
    let mark = db.add_mark(vod_id, &kind, time, mark_text(text).as_deref()).map_err(err)?;
    let label = match kind.as_str() {
        "chapter" => "Chapter added",
        "highlight" => "Highlight added",
        _ => "Cut added",
    };
    state.history.record(&db, doc, label, before);
    Ok(mark)
}

/// Sets mark `id`'s title (a chapter's) or note (a highlight's).
#[tauri::command]
pub(crate) async fn set_mark_text(state: State<'_, AppState>, id: i64, text: Option<String>) -> CmdResult<()> {
    marking(&state, id, "Text changed", |db| db.set_mark_text(id, mark_text(text).as_deref()))
}

/// Runs `change` on mark `id` (false if it's gone), recording it in its VOD's history as `label`.
fn marking(
    state: &AppState,
    id: i64,
    label: &'static str,
    change: impl FnOnce(&Db) -> anyhow::Result<bool>,
) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    let Some(vod_id) = db.mark_vod(id).map_err(err)? else { return Err("That mark is gone.".into()) };
    let doc = Doc::Vod(vod_id);
    let before = history::snapshot(&db, doc).ok().flatten();
    if !change(&db).map_err(err)? {
        return Err("That mark is gone.".into());
    }
    state.history.record(&db, doc, label, before);
    Ok(())
}

#[tauri::command]
pub(crate) async fn move_mark(state: State<'_, AppState>, id: i64, time: f64) -> CmdResult<()> {
    if !(time.is_finite() && time >= 0.0) {
        return Err("A mark has to be within the video.".into());
    }
    marking(&state, id, "Mark moved", |db| db.move_mark(id, time))
}

#[tauri::command]
pub(crate) async fn delete_mark(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    if state.db.lock().unwrap().mark_vod(id).map_err(err)?.is_none() {
        return Ok(());
    }
    marking(&state, id, "Mark removed", |db| db.delete_mark(id).map(|()| true))
}

/// Takes away mark `id`, just added and cancelled (Escape in its title box),
/// as if it had never been made: its adding leaves undo's history too. With
/// anything changed since, it's an ordinary removal.
#[tauri::command]
pub(crate) async fn cancel_mark(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    let Some(vod_id) = db.mark_vod(id).map_err(err)? else { return Ok(()) };
    let doc = Doc::Vod(vod_id);
    let before = history::snapshot(&db, doc).ok().flatten();
    db.delete_mark(id).map_err(err)?;
    if !state.history.unrecord(&db, doc) {
        state.history.record(&db, doc, "Mark removed", before);
    }
    Ok(())
}

/// Whether DaVinci Resolve is installed where its Linux installer puts it:
/// it can't read MP4 clips there, so the interface points to the setting.
#[tauri::command]
pub(crate) fn resolve_installed() -> bool {
    cfg!(target_os = "linux") && Path::new("/opt/resolve/bin/resolve").is_file()
}

/// Clip `id`'s file, if it's made.
fn ready_file(app: &AppHandle, id: i64) -> CmdResult<PathBuf> {
    let clip = app.state::<AppState>().db.lock().unwrap().clip(id).map_err(err)?.ok_or("That clip is gone.")?;
    clip.file_path
        .filter(|_| clip.status == ClipStatus::Ready)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or_else(|| "That clip's file isn't made yet.".into())
}

/// Shown while a clip with no picture (sound alone) is dragged.
const DRAG_ICON: &[u8] = include_bytes!("../icons/64x64.png");

/// Starts dragging clip `id`'s file out of the window, into another app (a
/// video editor, a file manager). Takes the clip's id, not a path, so the
/// interface can't offer any other file. Called while the mouse button is
/// still down; the drag then belongs to the system.
#[tauri::command]
pub(crate) async fn drag_clip(
    app: AppHandle,
    window: tauri::WebviewWindow,
    request: tauri::ipc::Request<'_>,
) -> CmdResult<()> {
    // The clip in a header, its picture as the request's bytes (a PNG is
    // tens of KB: a JSON array of its numbers took a moment to send).
    let id: i64 = request.headers().get("clip").and_then(|v| v.to_str().ok()?.parse().ok()).ok_or("No clip named.")?;
    let image = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) if !bytes.is_empty() => Some(bytes.clone()),
        _ => None,
    };
    let path = ready_file(&app, id)?;
    // The picture the interface drew (the frame over its waveform), if it's a
    // PNG of reasonable size; else the clip's first frame.
    let drawn = image.filter(|png| png.len() < (2 << 20) && png.starts_with(b"\x89PNG"));
    let image = drawn.or_else(|| std::fs::read(thumb_path(&app, id)).ok());
    // Another app's once one takes it (not when the drag begins: called off,
    // or dropped on nothing, the file is still the app's to delete).
    let taken = move || {
        let _ = app.state::<AppState>().db.lock().unwrap().hand_out_clip(id);
    };
    drag_file(window, path, image, taken).await
}

/// Starts the system's drag of the file at `path` (one the app made), showing
/// `image` (a PNG) or the app's icon. When it's over, `taken` runs if it was
/// dropped on something that took it, then the page hears "drag-ended", so
/// what it does next (deleting a clip made only for the drag) knows.
pub(crate) async fn drag_file(
    window: tauri::WebviewWindow,
    path: PathBuf,
    image: Option<Vec<u8>>,
    taken: impl Fn() + Send + Sync + 'static,
) -> CmdResult<()> {
    let image = image.unwrap_or_else(|| DRAG_ICON.to_vec());
    let (tx, rx) = mpsc::channel();
    let on_main = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(start_drag(&on_main, &path, image, taken).map_err(|e| format!("{e:#}")));
        })
        .map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || rx.recv()).await.map_err(err)?.map_err(err)?
}

#[cfg(target_os = "linux")]
fn start_drag(
    window: &tauri::WebviewWindow,
    path: &Path,
    image: Vec<u8>,
    taken: impl Fn() + Send + Sync + 'static,
) -> Result<()> {
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    let source = window.gtk_window()?;
    // Escaped, so names with spaces and other characters arrive whole.
    let uri = glib::filename_to_uri(path, None)?;
    let targets = gtk::TargetList::new(&[]);
    targets.add_uri_targets(0);
    let handlers: Rc<RefCell<Vec<glib::SignalHandlerId>>> = Rc::default();
    // Taken: something asked for the file's address, and the drag didn't
    // fail (a file manager asks as it's dragged over; called off, it fails).
    let (asked, failed) = (Rc::new(std::cell::Cell::new(false)), Rc::new(std::cell::Cell::new(false)));
    let asking = asked.clone();
    let data = source.connect_drag_data_get(move |_, _, data, _, _| {
        data.set_uris(&[uri.as_str()]);
        asking.set(true);
    });
    handlers.borrow_mut().push(data);
    // Dropped where nothing takes it: no animation of it flying back, which
    // holds on to the pointer (losing the next press) until it's done.
    let failing = failed.clone();
    let on_failed = source.connect_drag_failed(move |_, _, _| {
        failing.set(true);
        glib::Propagation::Stop
    });
    handlers.borrow_mut().push(on_failed);
    // Once it's over, dropped or not, the window stops answering for it.
    let ended = handlers.clone();
    let told = window.clone();
    let taken = std::sync::Arc::new(taken);
    let end = source.connect_drag_end(move |source, _| {
        for handler in ended.borrow_mut().drain(..) {
            source.disconnect(handler);
        }
        // Taken (above), then the page told, so a drag of its own coming back
        // in isn't taken for one to add. Off the main thread, for the
        // library's lock.
        let (dropped, taken, told) = (asked.get() && !failed.get(), taken.clone(), told.clone());
        std::thread::spawn(move || {
            if dropped {
                taken();
            }
            let _ = told.emit("drag-ended", ());
        });
        // The press that started the drag never comes back up in the page:
        // the drag took the release. Without one, its next press is lost.
        if let Some(page) = find_webview(source.upcast_ref()) {
            release_button(&page);
        }
    });
    handlers.borrow_mut().push(end);
    // Its picture, set as it begins: else GTK sets its own icon then, loading
    // it from the icon theme through gdk-pixbuf, and aborts the app if no
    // loader can read it (an SVG, where the loaders are sandboxed away).
    // (One that can't be decoded: none to see, rather than a theme icon.)
    let picture =
        drag_picture(&image).or_else(|| gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 1, 1).ok());
    let begin = source.connect_drag_begin(move |_, context| {
        if let Some(surface) = &picture {
            context.drag_set_icon_surface(surface);
        }
    });
    handlers.borrow_mut().push(begin);
    if source.drag_begin_with_coordinates(&targets, gtk::gdk::DragAction::COPY, 1, None, -1, -1).is_none() {
        for handler in handlers.borrow_mut().drain(..) {
            source.disconnect(handler);
        }
        anyhow::bail!("The drag couldn't start.");
    }
    Ok(())
}

/// The PNG `png` as a cairo surface for a drag's picture, decoded here.
#[cfg(target_os = "linux")]
fn drag_picture(png: &[u8]) -> Option<gtk::cairo::ImageSurface> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return None,
    };
    let (width, height) = (i32::try_from(info.width).ok()?, i32::try_from(info.height).ok()?);
    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, width, height).ok()?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().ok()?;
        for (y, row) in pixels.chunks(info.line_size).take(info.height as usize).enumerate() {
            for (x, px) in row.chunks(channels).take(info.width as usize).enumerate() {
                let (r, g, b, a) =
                    if channels == 4 { (px[0], px[1], px[2], px[3]) } else { (px[0], px[0], px[0], px[1]) };
                // Cairo's ARGB32: premultiplied, in the machine's byte order (little-endian here: B, G, R, A).
                let pre = |c: u8| (u16::from(c) * u16::from(a) / 255) as u8;
                let at = y * stride + x * 4;
                data[at..at + 4].copy_from_slice(&[pre(b), pre(g), pre(r), a]);
            }
        }
    }
    Some(surface)
}

/// The page's WebKitGTK view, inside the window.
#[cfg(target_os = "linux")]
fn find_webview(widget: &gtk::Widget) -> Option<gtk::Widget> {
    use gtk::prelude::*;
    if widget.type_().name() == "WebKitWebView" {
        return Some(widget.clone());
    }
    widget.downcast_ref::<gtk::Container>()?.children().iter().find_map(find_webview)
}

/// Sends `page` the release of the first mouse button, where the pointer is.
#[cfg(target_os = "linux")]
fn release_button(page: &gtk::Widget) {
    use gtk::glib::translate::{ToGlibPtr, ToGlibPtrMut};
    use gtk::prelude::*;
    let (Some(window), Some(device)) = (page.window(), page.display().default_seat().and_then(|s| s.pointer())) else {
        return;
    };
    let (_, x, y, _) = window.device_position_double(&device);
    let (_, root_x, root_y) = device.position_double();
    let mut event = gtk::gdk::Event::new(gtk::gdk::EventType::ButtonRelease);
    // SAFETY: a button event just made, of the type its fields are written as;
    // the window is referenced for the event, which unreferences it when freed.
    unsafe {
        let button = ToGlibPtrMut::<*mut gtk::gdk::ffi::GdkEvent>::to_glib_none_mut(&mut event).0
            as *mut gtk::gdk::ffi::GdkEventButton;
        (*button).window = ToGlibPtr::<*mut gtk::gdk::ffi::GdkWindow>::to_glib_full(&window);
        (*button).send_event = 1;
        (*button).time = gtk::gdk::ffi::GDK_CURRENT_TIME as u32;
        (*button).x = x;
        (*button).y = y;
        (*button).x_root = root_x;
        (*button).y_root = root_y;
        (*button).button = 1;
        (*button).state = gtk::gdk::ffi::GDK_BUTTON1_MASK;
    }
    event.set_device(Some(&device));
    gtk::main_do_event(&mut event);
}

#[cfg(not(target_os = "linux"))]
fn start_drag(
    window: &tauri::WebviewWindow,
    path: &Path,
    image: Vec<u8>,
    taken: impl Fn() + Send + Sync + 'static,
) -> Result<()> {
    let told = window.clone();
    let taken = std::sync::Arc::new(taken);
    drag::start_drag(
        window,
        drag::DragItem::Files(vec![path.to_path_buf()]),
        drag::Image::Raw(image),
        move |result, _| {
            // Taken, then the page told (so a drag of its own coming back in
            // isn't taken for one to add), in that order, off the UI thread.
            let (dropped, taken, told) = (matches!(result, drag::DragResult::Dropped), taken.clone(), told.clone());
            std::thread::spawn(move || {
                if dropped {
                    taken();
                }
                let _ = told.emit("drag-ended", ());
            });
        },
        drag::Options::default(),
    )
    .map_err(|e| anyhow::anyhow!("The drag couldn't start: {e}"))
}

/// Saves a copy of clip `id`'s file where the person picks, in the system's
/// save dialog (opened here, so the interface can't name a place to write).
/// False if they canceled.
#[tauri::command]
pub(crate) async fn save_clip(app: AppHandle, window: tauri::WebviewWindow, id: i64) -> CmdResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    let from = ready_file(&app, id)?;
    let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = from.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(&name)
            .add_filter(if matches!(ext.as_str(), "m4a" | "wav") { "Audio" } else { "Video" }, &[ext.as_str()])
            .blocking_save_file();
        let Some(to) = picked else { return Ok(false) };
        let to = to.into_path().map_err(err)?;
        if to != from {
            std::fs::copy(&from, &to).map_err(|e| format!("Couldn't save the clip there: {e}"))?;
        }
        Ok(true)
    })
    .await
    .map_err(err)?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On Windows a file another program has open can't be deleted: it goes
    /// once that program lets go.
    #[cfg(windows)]
    #[test]
    fn a_file_in_use_goes_once_let_go() {
        use std::os::windows::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!("vodscribe-in-use-{}.mp4", std::process::id()));
        std::fs::write(&path, b"video").unwrap();
        // Shared for reading only, as ffmpeg opens what it reads.
        let reader = std::fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        delete_file(&path);
        assert!(path.exists(), "still open");
        delete_leftovers();
        assert!(path.exists(), "still open");
        drop(reader);
        delete_leftovers();
        assert!(!path.exists(), "gone once let go");
    }

    #[test]
    fn ranges_must_be_clips() {
        assert!(check_range(1.0, 2.0, Some(10.0)).is_ok());
        assert!(check_range(2.0, 2.0, None).is_err());
        assert!(check_range(-1.0, 2.0, None).is_err());
        assert!(check_range(f64::NAN, 2.0, None).is_err());
        assert!(check_range(0.0, 4.0 * 3600.0, None).is_err());
        assert!(check_range(10.0, 12.0, Some(10.0)).is_err());
    }
}
