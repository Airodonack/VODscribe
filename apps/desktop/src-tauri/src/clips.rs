//! Clips (`vodscribe_core::clips`): the commands the interface makes and
//! changes them with, and the queue that cuts their files, one at a time. A
//! clip of a VOD that plays from a file here is cut from it; any other waits.
//!
//! Clip files go in the computer's Videos folder, under VODscribe: they're for
//! taking elsewhere, and video editors link to a file where it is, so a clip
//! whose range changes is made again at the same path.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{Clip, ClipStatus, Db};
use vodscribe_core::{clips, tools};

use crate::{AppState, CmdResult, err, media};

/// The longest clip: longer than any highlight, short of a whole VOD.
const LONGEST: f64 = 3.0 * 3600.0;

/// How often a cut's progress is sent.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The clip being cut, and its stop.
type Running = Arc<Mutex<Option<(i64, Arc<AtomicBool>)>>>;

pub(crate) struct Cutter {
    queue: Sender<i64>,
    /// Taken by `run`.
    jobs: Mutex<Option<Receiver<i64>>>,
    current: Running,
}

impl Cutter {
    pub(crate) fn new() -> Self {
        let (queue, jobs) = mpsc::channel();
        Self { queue, jobs: Mutex::new(Some(jobs)), current: Arc::default() }
    }

    /// Starts cutting, once the app's state is in place, beginning with the
    /// clips left unfinished when the app last stopped.
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        let jobs = self.jobs.lock().unwrap().take().context("the clip queue is already running")?;
        let running = self.current.clone();
        let unfinished = app.state::<AppState>().db.lock().unwrap().unfinished_clips()?;
        std::thread::Builder::new().name("clips".into()).spawn(move || {
            for id in jobs {
                let stop = Arc::new(AtomicBool::new(false));
                *running.lock().unwrap() = Some((id, stop.clone()));
                if let Err(e) = cut(&app, id, &stop) {
                    eprintln!("clip {id}: {e:#}");
                }
                *running.lock().unwrap() = None;
            }
        })?;
        for clip in unfinished {
            self.queue(clip.id);
        }
        Ok(())
    }

    fn queue(&self, id: i64) {
        let _ = self.queue.send(id);
    }

    /// Stops clip `id`'s cut, if it's the one being cut.
    fn stop(&self, id: i64) {
        if let Some((current, stop)) = &*self.current.lock().unwrap()
            && *current == id
        {
            stop.store(true, Ordering::Relaxed);
        }
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

/// Makes clip `id`'s file, if it's waiting and its VOD plays from a file here.
fn cut(app: &AppHandle, id: i64, stop: &AtomicBool) -> Result<()> {
    let state = app.state::<AppState>();
    let (clip, vod, source) = {
        let db = state.db.lock().unwrap();
        let Some(clip) = db.clip(id)?.filter(|c| c.status != ClipStatus::Ready) else { return Ok(()) };
        let Some(vod) = db.vod(clip.vod_id)? else { return Ok(()) };
        // A VOD from a site with its own player: its range is downloaded instead (to come).
        let Some(source) = media::playable_file(&db, vod.id) else { return Ok(()) };
        (clip, vod, source)
    };
    let range = (clip.start, clip.end);
    let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).context(crate::worker::NO_FFMPEG)?;
    let out = match &clip.file_path {
        Some(path) => PathBuf::from(path),
        None => clips::unique_path(&clips_dir(app), &clips::file_name(&vod.info.title, clip.start, vod.audio_only)),
    };
    {
        let db = state.db.lock().unwrap();
        if !db.set_clip_file(id, range.0, range.1, ClipStatus::Cutting, None, None)? {
            return Ok(());
        }
        emit(app, &db, id);
    }
    let mut last = Instant::now();
    let result = clips::cut(&ffmpeg, Path::new(&source), &out, range, vod.audio_only, stop, |progress| {
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            let _ = app.emit("clip-progress", Progress { id, progress });
        }
    });
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
        let _ = std::fs::remove_file(&out);
    }
    if recorded {
        emit(app, &db, id);
    }
    Ok(())
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

/// Makes a clip of VOD `vod_id` from `start` to `end` seconds, titled with what's said in it.
#[tauri::command]
pub(crate) async fn add_clip(app: AppHandle, vod_id: i64, start: f64, end: f64) -> CmdResult<Clip> {
    let state = app.state::<AppState>();
    let clip = {
        let db = state.db.lock().unwrap();
        let vod = db.vod(vod_id).map_err(err)?.ok_or("That VOD is gone.")?;
        let end = vod.info.duration.map_or(end, |d| end.min(d));
        check_range(start, end, vod.info.duration)?;
        let words = db.words_between(vod_id, start, end).map_err(err)?;
        db.add_clip(vod_id, start, end, &clips::title(&words, start)).map_err(err)?
    };
    state.clips.queue(clip.id);
    Ok(clip)
}

/// Moves clip `id` to `start`..`end`; its file is made again, at the same path.
#[tauri::command]
pub(crate) async fn set_clip_range(app: AppHandle, id: i64, start: f64, end: f64) -> CmdResult<Clip> {
    let state = app.state::<AppState>();
    let clip = {
        let db = state.db.lock().unwrap();
        let clip = db.clip(id).map_err(err)?.ok_or("That clip is gone.")?;
        let duration = db.vod(clip.vod_id).map_err(err)?.and_then(|v| v.info.duration);
        let end = duration.map_or(end, |d| end.min(d));
        check_range(start, end, duration)?;
        db.set_clip_range(id, start, end).map_err(err)?;
        db.clip(id).map_err(err)?.ok_or("That clip is gone.")?
    };
    state.clips.stop(id);
    state.clips.queue(id);
    Ok(clip)
}

#[tauri::command]
pub(crate) async fn rename_clip(app: AppHandle, id: i64, title: String) -> CmdResult<Clip> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A clip needs a name.".into());
    }
    let title: String = title.chars().take(200).collect();
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    db.rename_clip(id, &title).map_err(err)?;
    db.clip(id).map_err(err)?.ok_or_else(|| "That clip is gone.".into())
}

/// Deletes clip `id` and its file.
#[tauri::command]
pub(crate) async fn delete_clip(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.clips.stop(id);
    let clip = {
        let db = state.db.lock().unwrap();
        let clip = db.clip(id).map_err(err)?;
        db.delete_clip(id).map_err(err)?;
        clip
    };
    if let Some(path) = clip.and_then(|c| c.file_path) {
        let _ = std::fs::remove_file(path);
    }
    let _ = app.emit("clip-removed", id);
    Ok(())
}

/// Shows clip `id`'s file in the system file manager.
#[tauri::command]
pub(crate) async fn show_clip(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let clip = state.db.lock().unwrap().clip(id).map_err(err)?;
    let path = clip.and_then(|c| c.file_path).filter(|p| Path::new(p).is_file());
    app.opener().reveal_item_in_dir(path.ok_or("That clip's file isn't made yet.")?).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

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
