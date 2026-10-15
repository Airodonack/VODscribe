//! Downloading a VOD's whole video to this computer, on request, for a VOD
//! from YouTube, Twitch or Kick (those from other sites keep theirs anyway,
//! `pipeline::keeps_video`). It then plays in the app's own player, where
//! marks and clips are exact to the frame, and clips are cut from it at once.
//! Kept like other sites' videos, in `Paths::media`, at the Storage quality,
//! in a format that plays here; "Delete video" removes it.
//!
//! One download at a time, in a queue of its own, beside transcription's.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::playable::Playable;
use vodscribe_core::settings::Settings;
use vodscribe_core::{fetch, local, pipeline, tools};

use crate::{AppState, CmdResult, err};

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Default)]
struct Queue {
    waiting: VecDeque<i64>,
    /// The VOD whose video is downloading, and its stop.
    running: Option<(i64, Arc<AtomicBool>)>,
}

#[derive(Default)]
pub(crate) struct Videos {
    queue: Mutex<Queue>,
    wake: Condvar,
}

/// Where a VOD's video download is, for the interface: `progress` is None
/// while waiting its turn, and `error` is set if it failed.
#[derive(Serialize, Clone)]
struct VideoProgress {
    id: i64,
    progress: Option<f64>,
    error: Option<String>,
}

impl Videos {
    /// Starts the queue's thread; called once the app's state is in place.
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        std::thread::Builder::new().name("videos".into()).spawn(move || {
            loop {
                let state = app.state::<AppState>();
                let (id, stop) = {
                    let mut queue = state.videos.queue.lock().unwrap();
                    let id = loop {
                        if let Some(id) = queue.waiting.pop_front() {
                            break id;
                        }
                        queue = state.videos.wake.wait(queue).unwrap();
                    };
                    let stop = Arc::new(AtomicBool::new(false));
                    queue.running = Some((id, stop.clone()));
                    (id, stop)
                };
                let result = download(&app, id, &stop);
                state.videos.queue.lock().unwrap().running = None;
                let error = match result {
                    Ok(()) => None,
                    // Stopped: what was downloaded goes, rather than lie unseen in the kept videos.
                    Err(e) if vodscribe_core::is_canceled(&e) => {
                        let kept = state.db.lock().unwrap().media_path(id).ok().flatten().is_some();
                        if !kept {
                            pipeline::remove_media(&state.paths.work(), id);
                        }
                        None
                    }
                    Err(e) => {
                        eprintln!("VOD {id}: its video: {e:#}");
                        Some(format!("{e:#}"))
                    }
                };
                // Done (the VOD now says it has its video), stopped or failed.
                let _ = app.emit("video-progress", VideoProgress { id, progress: None, error: error.clone() });
                if error.is_none() {
                    let _ = crate::emit_vod(&app, &state.db.lock().unwrap(), id);
                }
            }
        })?;
        Ok(())
    }

    /// The VODs whose video is downloading or waiting to, the running one first.
    fn listed(&self) -> Vec<i64> {
        let queue = self.queue.lock().unwrap();
        queue.running.iter().map(|(id, _)| *id).chain(queue.waiting.iter().copied()).collect()
    }
}

/// Downloads VOD `id`'s whole video into `Paths::media`, resuming any part
/// already there, and records it as the VOD's kept video.
fn download(app: &AppHandle, id: i64, stop: &AtomicBool) -> Result<()> {
    let state = app.state::<AppState>();
    let paths = &state.paths;
    let Some(vod) = state.db.lock().unwrap().vod(id)? else { return Ok(()) };
    let bin = paths.bin();
    let yt_dlp = tools::find_yt_dlp(&bin).context("The video downloader isn't installed yet.")?;
    let ffmpeg = tools::find_ffmpeg(&bin).context(crate::worker::NO_FFMPEG)?;
    let work = paths.work();
    std::fs::create_dir_all(&work)?;
    let media = paths.media();
    std::fs::create_dir_all(&media)?;
    let info = work.join(format!("video-{id}.info.json"));
    let playable = Playable::load(&paths.playable());
    let selector = playable.selector(Settings::load(&paths.settings()).video_height());
    let wanted = fetch::Wanted::Video { selector: &selector, ffmpeg: &ffmpeg };
    let mut last = Instant::now();
    let mut report = |progress: f64| {
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            let _ = app.emit("video-progress", VideoProgress { id, progress: Some(progress), error: None });
        }
    };
    let result = (|| {
        fetch::probe(&yt_dlp, &vod.info.url, &info, stop)?;
        let out = media.join(id.to_string());
        let mut get = || fetch::download(&yt_dlp, &info, &out, wanted, stop, &mut report);
        match get() {
            // As for whole downloads (pipeline::download): a fresh probe is a fresh session.
            Err(e) if fetch::is_refused(&e) => {
                fetch::probe(&yt_dlp, &vod.info.url, &info, stop)?;
                get()
            }
            result => result,
        }
    })();
    let _ = std::fs::remove_file(&info);
    let file = result?;
    // Sites only claim their codecs: kept only if what arrived plays here.
    let streams = local::codecs(&ffmpeg, &file)?;
    if !playable.plays(&streams) {
        let _ = std::fs::remove_file(&file);
        anyhow::bail!("This video came in a format that doesn't play here ({streams:?}).");
    }
    let db = state.db.lock().unwrap();
    if db.vod(id)?.is_none() {
        // Removed while downloading.
        pipeline::remove_media(&work, id);
        return Ok(());
    }
    db.set_media_path(id, Some(&file.to_string_lossy()))?;
    Ok(())
}

/// Queues the download of VOD `id`'s whole video (YouTube, Twitch and Kick;
/// other sites keep theirs when transcribed).
#[tauri::command]
pub(crate) async fn keep_video(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let vod = state.db.lock().unwrap().vod(id).map_err(err)?.ok_or("That VOD is gone.")?;
    if vod.source_path.is_some() || vod.media {
        return Ok(());
    }
    if pipeline::keeps_video(&vod.info.platform) {
        return Err("This VOD's video is kept once it's transcribed.".into());
    }
    {
        let mut queue = state.videos.queue.lock().unwrap();
        if queue.running.as_ref().is_some_and(|(r, _)| *r == id) || queue.waiting.contains(&id) {
            return Ok(());
        }
        queue.waiting.push_back(id);
    }
    state.videos.wake.notify_one();
    let _ = app.emit("video-progress", VideoProgress { id, progress: Some(0.0), error: None });
    Ok(())
}

/// Stops VOD `id`'s video download, discarding what's downloaded so far, or
/// takes it out of the queue.
#[tauri::command]
pub(crate) async fn stop_video(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let mut queue = state.videos.queue.lock().unwrap();
    queue.waiting.retain(|w| *w != id);
    match &queue.running {
        Some((running, stop)) if *running == id => stop.store(true, Ordering::Relaxed),
        _ => {
            let _ = app.emit("video-progress", VideoProgress { id, progress: None, error: None });
        }
    }
    Ok(())
}

/// The VODs whose video is downloading or waiting to (the interface asks at start).
#[tauri::command]
pub(crate) async fn video_downloads(state: State<'_, AppState>) -> CmdResult<Vec<i64>> {
    Ok(state.videos.listed())
}
