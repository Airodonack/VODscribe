//! Downloading a VOD's whole video to this computer, on request, for a VOD
//! from YouTube, Twitch or Kick (those from other sites keep theirs anyway,
//! `pipeline::keeps_video`). It then plays in the app's own player, where
//! marks and clips are exact to the frame, and clips are cut from it at once.
//! Kept like other sites' videos, in `Paths::media`, at the Storage quality,
//! in a format that plays here; "Delete video" removes it.
//!
//! One download at a time, in a queue of its own, beside transcription's.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::playable::Playable;
use vodscribe_core::settings::Settings;
use vodscribe_core::{fetch, local, pipeline, tools};

use crate::jobs::Jobs;
use crate::{AppState, CmdResult, err};

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The heights a download can be capped at (as Settings → Storage offers).
pub(crate) const HEIGHTS: [u32; 3] = [480, 720, 1080];

/// VODs' whole videos downloaded one at a time: each VOD's id, and the
/// height its video is capped at.
pub(crate) struct Videos(Jobs<(i64, u32)>);

/// Where a VOD's video download is, for the interface: `progress` is None
/// while waiting its turn, and `error` is set if it failed.
#[derive(Serialize, Clone)]
struct VideoProgress {
    id: i64,
    progress: Option<f64>,
    error: Option<String>,
}

impl Videos {
    pub(crate) fn new() -> Self {
        Videos(Jobs::new())
    }

    /// Starts the queue's thread; called once the app's state is in place.
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        self.0.run("videos", move |(id, height), stop| {
            let state = app.state::<AppState>();
            let result = download(&app, id, height, stop);
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
        })
    }

    /// The VODs whose video is downloading or waiting to, the running one first.
    fn listed(&self) -> Vec<i64> {
        self.0.listed().into_iter().map(|(id, _)| id).collect()
    }

    /// Whether VOD `id`'s video is waiting to download, or downloading and not
    /// stopped: its files are that download's, not to be deleted.
    pub(crate) fn wanted(&self, id: i64) -> bool {
        self.0.is_wanted(|(v, _)| *v == id)
    }

    /// Stops every download and empties the queue (deleting what each had).
    pub(crate) fn stop_all(&self, app: &AppHandle) {
        for (id, _) in self.0.take_waiting(|_| true) {
            let _ = app.emit("video-progress", VideoProgress { id, progress: None, error: None });
        }
        self.0.stop_running(|_| true);
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
    let queued = |(v, _): &(i64, u32)| *v == id;
    if !state.videos.0.push_new((id, height), queued) {
        return Ok(());
    }
    let _ = app.emit("video-progress", VideoProgress { id, progress: Some(0.0), error: None });
    Ok(())
}

/// Stops VOD `id`'s video download, discarding what's downloaded so far, or
/// takes it out of the queue.
#[tauri::command]
pub(crate) async fn stop_video(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let queued = |(v, _): &(i64, u32)| *v == id;
    state.videos.0.take_waiting(queued);
    if !state.videos.0.stop_running(queued) {
        let _ = app.emit("video-progress", VideoProgress { id, progress: None, error: None });
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
    // Meanwhile a "Keep video" may have queued one of them again (it no
    // longer has its video): that download's parts stay. Those of VODs without
    // a kept video go too (a download paused by quitting leaves its parts).
    for id in gone {
        if !state.videos.wanted(id) {
            pipeline::remove_media(&work, id);
        }
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
