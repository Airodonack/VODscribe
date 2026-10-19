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

/// The heights a download can be capped at (as Settings → Storage offers).
pub(crate) const HEIGHTS: [u32; 3] = [480, 720, 1080];

#[derive(Default)]
struct Queue {
    /// VODs and the height their video is capped at.
    waiting: VecDeque<(i64, u32)>,
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
                let (id, height, stop) = {
                    let mut queue = state.videos.queue.lock().unwrap();
                    let (id, height) = loop {
                        if let Some(next) = queue.waiting.pop_front() {
                            break next;
                        }
                        queue = state.videos.wake.wait(queue).unwrap();
                    };
                    let stop = Arc::new(AtomicBool::new(false));
                    queue.running = Some((id, stop.clone()));
                    (id, height, stop)
                };
                let result = download(&app, id, height, &stop);
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
        queue.running.iter().map(|(id, _)| *id).chain(queue.waiting.iter().map(|(id, _)| *id)).collect()
    }

    /// Stops every download and empties the queue (deleting what each had).
    pub(crate) fn stop_all(&self, app: &AppHandle) {
        let mut queue = self.queue.lock().unwrap();
        for (id, _) in queue.waiting.drain(..) {
            let _ = app.emit("video-progress", VideoProgress { id, progress: None, error: None });
        }
        if let Some((_, stop)) = &queue.running {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

/// Downloads VOD `id`'s whole video, at most `height` tall, into
/// `Paths::media`, resuming any part already there, and records it as the
/// VOD's kept video.
fn download(app: &AppHandle, id: i64, height: u32, stop: &AtomicBool) -> Result<()> {
    let state = app.state::<AppState>();
    let paths = &state.paths;
    let Some(vod) = state.db.lock().unwrap().vod(id)? else { return Ok(()) };
    let bin = paths.bin();
    let yt_dlp = tools::find_yt_dlp(&bin).context(fetch::NO_YT_DLP)?;
    let ffmpeg = tools::find_ffmpeg(&bin).context(crate::worker::NO_FFMPEG)?;
    let work = paths.work();
    std::fs::create_dir_all(&work)?;
    let media = paths.media();
    std::fs::create_dir_all(&media)?;
    let info = work.join(format!("video-{id}.info.json"));
    let playable = Playable::load(&paths.playable());
    let selector = playable.selector(height);
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
        let out = pipeline::media_stem(&media, id, &vod.info.title);
        fetch::download_or_reprobe(&yt_dlp, &vod.info.url, &info, &out, wanted, stop, &mut report)
    })();
    let _ = std::fs::remove_file(&info);
    let file = result?;
    // Sites only claim their codecs: kept only if what arrived plays here.
    let streams = local::codecs(&ffmpeg, &file)?;
    if !playable.plays(&streams) {
        let _ = std::fs::remove_file(&file);
        let named = [
            streams.video.as_ref().map(|v| format!("{v} video")),
            streams.audio.as_ref().map(|a| format!("{a} sound")),
        ];
        let named = named.into_iter().flatten().collect::<Vec<_>>().join(", ");
        anyhow::bail!("This video came in a format that doesn't play here ({named}).");
    }
    let db = state.db.lock().unwrap();
    if stop.load(Ordering::Relaxed) {
        // Stopped while its format was checked (the run loop deletes it):
        // checked under the lock, so "Delete all" never misses it.
        return Err(vodscribe_core::Canceled.into());
    }
    if db.vod(id)?.is_none() {
        // Removed while downloading.
        pipeline::remove_media(&work, id);
        return Ok(());
    }
    db.set_media_path(id, Some(&file.to_string_lossy()))?;
    Ok(())
}

/// Queues the download of VOD `id`'s whole video (YouTube, Twitch and Kick;
/// other sites keep theirs when transcribed), at most `height` tall, or as
/// Settings → Storage says.
#[tauri::command]
pub(crate) async fn keep_video(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    height: Option<u32>,
) -> CmdResult<()> {
    let height = match height {
        Some(h) if HEIGHTS.contains(&h) => h,
        Some(_) => return Err("That isn't one of the video qualities.".into()),
        None => Settings::load(&state.paths.settings()).video_height(),
    };
    let vod = state.db.lock().unwrap().vod(id).map_err(err)?.ok_or("That VOD is gone.")?;
    if vod.source_path.is_some() || vod.media {
        return Ok(());
    }
    if pipeline::keeps_video(&vod.info.platform) {
        return Err("This VOD's video is kept once it's transcribed.".into());
    }
    {
        let mut queue = state.videos.queue.lock().unwrap();
        if queue.running.as_ref().is_some_and(|(r, _)| *r == id) || queue.waiting.iter().any(|(w, _)| *w == id) {
            return Ok(());
        }
        queue.waiting.push_back((id, height));
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
    queue.waiting.retain(|(w, _)| *w != id);
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

/// Deletes every downloaded video, keeping the transcripts: they then play on
/// their sites. Downloads under way stop. Videos still needed to transcribe
/// their VOD stay; returns how many.
#[tauri::command]
pub(crate) async fn delete_all_videos(app: AppHandle, state: State<'_, AppState>) -> CmdResult<usize> {
    state.videos.stop_all(&app);
    let work = state.paths.work();
    let mut kept = 0;
    let mut gone = Vec::new();
    {
        let db = state.db.lock().unwrap();
        for vod in db.vods().map_err(err)? {
            if crate::vods::busy(vod.status) {
                kept += vod.media as usize;
                continue;
            }
            if vod.media {
                crate::vods::forget_video(&db, &work, vod.id).map_err(err)?;
                crate::emit_vod(&app, &db, vod.id)?;
            }
            gone.push(vod.id);
        }
    }
    // Deleted with the library let go: many big files can take a while.
    for id in gone {
        pipeline::remove_media(&work, id);
    }
    Ok(kept)
}

/// Opens the folder of downloaded videos in the system file manager. It
/// takes no path, so the interface can't open anything else with it.
#[tauri::command]
pub(crate) async fn open_videos_dir(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = state.paths.media();
    std::fs::create_dir_all(&dir).map_err(err)?;
    app.opener().open_path(dir.display().to_string(), None::<String>).map_err(err)
}
