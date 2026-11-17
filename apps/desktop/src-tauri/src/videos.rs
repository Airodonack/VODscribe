//! Downloading a VOD's whole video to this computer, on request. A VOD from
//! YouTube, Twitch or Kick then plays in the app's own player, where marks
//! and clips are exact to the frame, and clips are cut from it at once; one
//! from another site (`pipeline::keeps_video`) plays only that way (or is
//! kept once it's transcribed).
//! Kept like other sites' videos, in `Paths::media`, at the Storage quality,
//! in a format that plays here; "Delete video" removes it.
//!
//! One download at a time, in a queue of its own, beside transcription's.
//! Transcribing all of a VOD from a site without its own player downloads
//! the same file: whichever of the two would start second waits in its
//! queue till the other is done (`try_turn`), as others go ahead of it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{Manager, State};
use vodscribe_core::asked::Queue;
use vodscribe_core::db::{Db, Vod};
use vodscribe_core::playable::Playable;
use vodscribe_core::settings::Settings;
use vodscribe_core::stretches::Stretch;
use vodscribe_core::{Paths, fetch, local, pipeline, tools};

use crate::AppHandle;
use crate::jobs::Jobs;
use crate::{AppState, CmdResult, err};

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The heights a download can be capped at (as Settings → Storage offers).
pub(crate) const HEIGHTS: [u32; 3] = [480, 720, 1080];

/// VODs' whole videos downloaded one at a time: each VOD's id, and the
/// height its video is capped at. Then the lock on the kept videos' files
/// (`files`).
pub(crate) struct Videos(Jobs<(i64, u32)>, Mutex<()>);

/// Where a VOD's video download is, for the interface: `progress` is None
/// once it's over (done, stopped or failed), and `error` is set if it failed.
/// `streamed`: it comes in as a section of all of it, which plays as far as
/// it's in, the timeline darker past that (`footage::ensure_as` tells how
/// far); else it plays only once it's all in, so the page gives the percent.
#[derive(Serialize, Clone)]
struct VideoProgress {
    id: i64,
    progress: Option<f64>,
    error: Option<String>,
    streamed: bool,
}

impl VideoProgress {
    /// VOD `id`'s download is over: done, stopped, or failed with `error`.
    fn over(id: i64, error: Option<String>) -> Self {
        VideoProgress { id, progress: None, error, streamed: false }
    }
}

impl Videos {
    pub(crate) fn new() -> Self {
        Videos(Jobs::new(), Mutex::default())
    }

    /// Ends the queue for good (`Jobs::close`).
    #[cfg(test)]
    pub(crate) fn close(&self) {
        self.0.close();
    }

    /// Has the queue look again for a job that can begin (`Jobs::wake`).
    fn wake(&self) {
        self.0.wake();
    }

    /// Held while a VOD's video files are deleted, and by a download as it
    /// begins and as it records its file, so neither happens in the middle of
    /// the other: a delete never takes a download's files, nor leaves a
    /// recorded video behind untracked. Taken before the library's lock.
    pub(crate) fn files(&self) -> MutexGuard<'_, ()> {
        self.1.lock().unwrap()
    }

    /// Starts the queue's thread, once the app's state is in place, with
    /// the downloads the app was making when it last stopped (`left_unfinished`).
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        let unfinished = left_unfinished(&app);
        // A download begins with its turn at its VOD's kept video file; one
        // whose turn a transcription has waits in its place, the next going
        // ahead (`Jobs::run_when`), and begins once that's done (`Turn`'s drop).
        let (taker, teller) = (app.clone(), app.clone());
        let newly_passed: Arc<Mutex<Vec<i64>>> = Arc::default();
        let passed = newly_passed.clone();
        let take = move |(id, _): &(i64, u32)| match try_turn(&taker, *id, Writer::Play) {
            Ok(turn) => Some(turn),
            Err(newly) => {
                if newly {
                    passed.lock().unwrap().push(*id);
                }
                None
            }
        };
        // Each passed over is shown downloading, to play once it's in.
        let told = move || {
            for id in std::mem::take(&mut *newly_passed.lock().unwrap()) {
                waiting_progress(&teller, id, 0.0);
            }
            #[cfg(test)]
            crate::testkit::video_waits(&teller);
        };
        self.0.run_when("videos", take, told, move |(id, height), turn: Turn, stop| {
            let state = app.state::<AppState>();
            // Its turn held till what it downloaded is dealt with.
            turn.begin();
            let result = download(&app, id, height, stop);
            // Over, whatever came of it: Play asked for again from here (the
            // page is told it's over below) is a new download (`record`).
            stop.store(true, Ordering::Relaxed);
            let error = match result {
                Ok(()) => None,
                // Stopped: what was downloaded goes, rather than lie unseen in the kept videos.
                // Unless the app is quitting: left as after a crash, for the
                // next start to carry on from (`left_unfinished`).
                Err(e) if vodscribe_core::is_canceled(&e) => {
                    let (kept, quitting) = {
                        let db = state.db.lock().unwrap();
                        (db.media_path(id).ok().flatten().is_some(), db.writes_stopped())
                    };
                    if !kept && !quitting {
                        pipeline::remove_media(&state.paths, id);
                    }
                    None
                }
                Err(e) => {
                    eprintln!("VOD {id}: its video: {e:#}");
                    Some(format!("{e:#}"))
                }
            };
            // Over: no longer one for the next start to carry on, unless
            // it's asked for again meanwhile (`keep_video`, under the same
            // lock). A quit's stopped writes keep it asked.
            {
                let db = state.db.lock().unwrap();
                if !state.videos.0.is_waiting(|(v, _)| *v == id) {
                    let _ = db.set_video_wanted(id, None);
                }
            }
            drop(turn);
            // Done (the VOD now says it has its video), stopped or failed.
            // The VOD first: told the download's over while the VOD doesn't
            // say it has its video yet, the page of one from a site without a
            // player would take it for unplayable, and take down its player.
            if error.is_none() {
                let _ = crate::emit_vod(&app, &state.db.lock().unwrap(), id);
            }
            crate::page::emit(&app, "video-progress", VideoProgress::over(id, error.clone()));
            if error.is_none() {
                state.storyboards.push(id);
            }
        })?;
        for (id, height) in unfinished {
            self.0.push_new((id, height), |(v, _)| *v == id);
        }
        Ok(())
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
        // No longer asked, waiting or under way: a quit before the stopped
        // download ends doesn't bring it back at the next start. Under the
        // library's lock with the queue's take, as `keep_video` asks.
        let state = app.state::<AppState>();
        let taken = {
            let db = state.db.lock().unwrap();
            for (id, _) in self.0.listed() {
                let _ = db.set_video_wanted(id, None);
            }
            self.0.take_waiting(|_| true)
        };
        for (id, _) in taken {
            unpass(&state.paths, id);
            crate::page::emit(app, "video-progress", VideoProgress::over(id, None));
        }
        self.0.stop_running(|_| true);
    }
}

/// The video downloads a quit (or a crash) stopped, carried on at the
/// start in the order they were asked (`Db::in_asked_order`), each at the
/// height it was asked at (`Db::wanted_videos`): yt-dlp
/// carries on from the part files it left (`download`); a section of all
/// of a VOD from a site without its own player starts again. Not one that
/// transcribing all of it will download into the same file: the
/// transcription carries that on, and keeps the video.
fn left_unfinished(app: &AppHandle) -> Vec<(i64, u32)> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let Ok(wanted) = db.wanted_videos() else { return Vec::new() };
    let left: Vec<(i64, u32)> = wanted
        .into_iter()
        .filter(|&(id, _)| db.vod(id).ok().flatten().is_some_and(|vod| !transcribing_all(&db, &vod).unwrap_or(true)))
        .collect();
    db.ordered_as_asked(Queue::Video, left, |&(id, _)| id)
}

/// Downloads VOD `id`'s whole video, at most `height` tall, into
/// `Paths::media`, resuming any part already there, and records it as the
/// VOD's kept video.
fn download(app: &AppHandle, id: i64, height: u32, stop: &AtomicBool) -> Result<()> {
    let state = app.state::<AppState>();
    let paths = &state.paths;
    let Some(vod) = begin(&state.db, &state.videos, id)? else {
        // Kept meanwhile, by a transcription of all of it that had the file
        // first (`try_turn`): a player waiting for it plays it now.
        refile_kept(app, id);
        return Ok(());
    };
    let bin = paths.bin();
    let yt_dlp = tools::find_yt_dlp(&bin).context(fetch::NO_YT_DLP)?;
    let ffmpeg = tools::find_ffmpeg(&bin).context(crate::worker::NO_FFMPEG)?;
    std::fs::create_dir_all(paths.work())?;
    let media = paths.media();
    std::fs::create_dir_all(&media)?;
    // A site without its own player: its video comes as a section of all
    // of it, which plays as it comes in (player.rs `streamed`), then is kept.
    // Where the site's streams can't be read in sections (ffmpeg reads only
    // HTTPS and HLS: fetch.rs), it downloads whole, as for the other sites,
    // and plays once it's in.
    if pipeline::keeps_video(&vod.info.platform)
        && let Some(duration) = vod.info.duration.filter(|d| *d > 0.0)
    {
        let report = Mutex::new(Instant::now());
        let report = |progress: f64| {
            let mut last = report.lock().unwrap();
            if last.elapsed() >= PROGRESS_EVERY {
                *last = Instant::now();
                let streamed = VideoProgress { id, progress: Some(progress), error: None, streamed: true };
                crate::page::emit(app, "video-progress", streamed);
            }
        };
        match stream_then_keep(app, &vod, duration, height, stop, &report) {
            Ok(()) => return Ok(()),
            Err(e) if vodscribe_core::is_canceled(&e) => return Err(e),
            Err(e) => {
                eprintln!("VOD {id}: its video didn't come as a section, so it downloads whole: {e:#}");
                // It plays only once it's in now: the page says how far, from the start.
                let whole = VideoProgress { id, progress: Some(0.0), error: None, streamed: false };
                crate::page::emit(app, "video-progress", whole);
            }
        }
    }
    let info = paths.source_video_info(id);
    let playable = Playable::load(&paths.playable());
    let selector = playable.selector(height);
    let wanted = fetch::Wanted::Video { selector: &selector, ffmpeg: &ffmpeg };
    let mut last = Instant::now();
    let mut report = |progress: f64| {
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            let whole = VideoProgress { id, progress: Some(progress), error: None, streamed: false };
            crate::page::emit(app, "video-progress", whole);
        }
    };
    let result = (|| {
        fetch::probe(&yt_dlp, &vod.info.url, &info, stop)?;
        let out = paths.source_video_stem(id);
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
    record(&state.db, &state.videos, paths, id, &file, stop)?;
    // A player waiting for it plays it now.
    if let Some(duration) = vod.info.duration {
        crate::player::refile(app, id, &file, duration);
    }
    Ok(())
}

/// Downloads all of VOD `vod`'s video (`duration` long, as its site says)
/// as a Matroska section through the footage queue (`footage::ensure_as`),
/// which plays as far as it's in while it comes; then copies it, as it is,
/// into an MP4 kept as its video (`clips::remux`), which the player
/// waiting on it plays from then on. Fails, with nothing kept, where the
/// site's streams can't be read in sections. The footage queue downloads
/// one section at a time: an edit's footage waits behind it meanwhile.
fn stream_then_keep(
    app: &AppHandle,
    vod: &Vod,
    duration: f64,
    height: u32,
    stop: &AtomicBool,
    report: &(dyn Fn(f64) + Sync),
) -> Result<()> {
    let state = app.state::<AppState>();
    let paths = &state.paths;
    let ffmpeg = tools::find_ffmpeg(&paths.bin()).context(crate::worker::NO_FFMPEG)?;
    let playable = Playable::load(&paths.playable());
    let selector = playable.selector(height);
    crate::footage::ensure_as(app, vod.id, (0.0, duration), Some(&selector), stop, &|p| report(p * 0.95))?;
    let section = {
        let db = state.db.lock().unwrap();
        crate::footage::find(app, &db, vod.id, 0.0, duration).context("Its video didn't download.")?
    };
    // Kept already (it was found as its own file): nothing more to do.
    if section.section.is_none() {
        return Ok(());
    }
    let streams = local::codecs(&ffmpeg, &section.path)?;
    if !playable.plays(&streams) {
        anyhow::bail!("its section came in a format that doesn't play here ({streams:?})");
    }
    let out = paths.source_video(vod.id, "mp4");
    vodscribe_core::clips::remux(&ffmpeg, &section.path, &out, duration, stop, |p| report(0.95 + p * 0.05))?;
    record(&state.db, &state.videos, paths, vod.id, &out, stop)?;
    crate::player::refile(app, vod.id, &out, duration);
    // It's the kept video now.
    crate::footage::drop_section(&section.path);
    Ok(())
}

/// The VOD a download of VOD `id`'s video is for, as it begins: after any
/// delete of its files under way. None if it's gone, or has its video already
/// (so a video recorded is never one a download is still writing).
fn begin(db: &Mutex<Db>, videos: &Videos, id: i64) -> Result<Option<Vod>> {
    let _files = videos.files();
    Ok(db.lock().unwrap().vod(id)?.filter(|vod| !vod.media))
}

/// A player playing VOD `id` as its video downloads plays the video kept,
/// if it is (`player::refile`).
fn refile_kept(app: &AppHandle, id: i64) {
    let state = app.state::<AppState>();
    let kept = {
        let db = state.db.lock().unwrap();
        let vod = db.vod(id).ok().flatten().filter(|v| v.media);
        vod.and_then(|v| Some((db.media_path(id).ok().flatten()?, v.info.duration?)))
    };
    if let Some((path, duration)) = kept {
        crate::player::refile(app, id, Path::new(&path), duration);
    }
}

/// Records `file`, just downloaded, as VOD `id`'s kept video.
fn record(db: &Mutex<Db>, videos: &Videos, paths: &Paths, id: i64, file: &Path, stop: &AtomicBool) -> Result<()> {
    let _files = videos.files();
    let db = db.lock().unwrap();
    if stop.load(Ordering::Relaxed) {
        // Stopped while its format was checked (the run loop deletes it):
        // checked under the lock, so "Delete all" never misses it.
        return Err(vodscribe_core::Canceled.into());
    }
    if db.vod(id)?.is_none() {
        // Removed while downloading.
        pipeline::remove_media(paths, id);
        return Ok(());
    }
    db.set_media_path(id, Some(&file.to_string_lossy()))?;
    // Recorded, the download is only ending: Play asked for again, once
    // Delete video has forgotten it, is a new download (`Jobs::push_new`
    // takes a running job stopped as ending), not this one, which wouldn't
    // download it again. Under the library's lock, with `keep_video`'s push.
    stop.store(true, Ordering::Relaxed);
    Ok(())
}

/// Queues the download of VOD `id`'s whole video, at most `height` tall, or
/// as Settings → Storage says.
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
    let queued = |(v, _): &(i64, u32)| *v == id;
    {
        // Recorded, for a start after a quit to carry it on at this height.
        let db = state.db.lock().unwrap();
        if !state.videos.0.push_new((id, height), queued) {
            return Ok(());
        }
        db.atomically(|db| {
            db.set_video_wanted(id, Some(height))?;
            db.keep_place(Queue::Video, id);
            Ok(())
        })
        .map_err(err)?;
    }
    // As `download` begins: a site without its own player streams it, if it
    // says how long it is. Unless transcribing all of it has the file: then
    // it waits for that (`try_turn`), and plays once it's in. A transcription
    // taking the file after this look has it shown so as it's passed over.
    let streamed = pipeline::keeps_video(&vod.info.platform)
        && vod.info.duration.is_some_and(|d| d > 0.0)
        && !transcribing(&state.paths, id);
    crate::page::emit(&app, "video-progress", VideoProgress { id, progress: Some(0.0), error: None, streamed });
    Ok(())
}

/// Whether transcribing all of VOD `vod` will download its video into the
/// kept videos' folder (a site without its own player): its request is
/// waiting to download, or downloading. A start leaves that file to the
/// transcription, whose download sets its request back to queued
/// (`Db::requeue_interrupted`). One failed or paused downloads nothing.
fn transcribing_all(db: &Db, vod: &Vod) -> Result<bool> {
    use vodscribe_core::db::Status;
    let downloads = |s: &Stretch| s.is_whole() && matches!(s.status, Status::Queued | Status::Downloading);
    Ok(pipeline::keeps_video(&vod.info.platform) && db.requests(vod.id)?.iter().any(downloads))
}

/// Who writes each VOD's kept video file (`Paths::source_video_stem`) now,
/// by the media folder it's in (`Paths::media`): Play's download (or Keep
/// video's), or transcribing all of a VOD from a site without its own
/// player, which downloads the same file (`pipeline::audio_for`); and which
/// of them were passed over for it, waiting. One for the process, since the
/// tests open many libraries at once.
static TURNS: Mutex<Turns> = Mutex::new(Turns { writing: Vec::new(), passed: Vec::new() });

/// `TURNS`: each a media folder, a VOD, and what writes its file or waits to.
struct Turns {
    writing: Vec<(PathBuf, i64, Writer)>,
    passed: Vec<(PathBuf, i64, Writer)>,
}

fn turns() -> MutexGuard<'static, Turns> {
    TURNS.lock().unwrap_or_else(|e| e.into_inner())
}

/// What writes a VOD's kept video file (`TURNS`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Writer {
    Play,
    Transcription,
}

/// A VOD's turn to write its kept video file, till it's dropped; its queues
/// then look again for what waited for it.
pub(crate) struct Turn {
    app: AppHandle,
    media: PathBuf,
    id: i64,
    /// It waited for the other's turn (`try_turn`).
    after_other: bool,
}

impl Turn {
    /// The download with this turn begins: having waited for the other,
    /// done, it finds the video kept (`begin`, `audio_for` then use it), or
    /// the other was stopped, failed or removed, and what it left (a failed
    /// or paused download's part file) goes, so this one downloads the
    /// video itself, whole, as with no other. Unless the app is quitting:
    /// those are left for the next start, as after a crash.
    pub(crate) fn begin(&self) {
        if !self.after_other {
            return;
        }
        let state = self.app.state::<AppState>();
        let _files = state.videos.files();
        let db = state.db.lock().unwrap();
        if !db.writes_stopped() && db.media_path(self.id).ok().flatten().is_none() {
            pipeline::remove_media(&state.paths, self.id);
        }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        turns().writing.retain(|(media, id, _)| !(*media == self.media && *id == self.id));
        if let Some(state) = self.app.try_state::<AppState>() {
            state.videos.wake();
            state.worker.wake();
        }
    }
}

/// Takes VOD `id`'s turn to write its kept video file, as `who`, if the
/// other isn't writing it. Decided for 0.7.4: whichever download of the
/// file starts second waits for the first and uses its file. One that
/// can't is passed over, waiting in its queue as others go ahead, and
/// begins once the other's turn is done (`Turn`'s drop wakes both queues):
/// `Err`, saying whether it's newly passed over.
fn try_turn(app: &AppHandle, id: i64, who: Writer) -> std::result::Result<Turn, bool> {
    let media = app.state::<AppState>().paths.media();
    let mut turns = turns();
    let here = |(m, v, w): &(PathBuf, i64, Writer)| *m == media && *v == id && *w == who;
    if turns.writing.iter().any(|(m, v, _)| *m == media && *v == id) {
        let newly = !turns.passed.iter().any(here);
        if newly {
            turns.passed.push((media.clone(), id, who));
        }
        return Err(newly);
    }
    let after_other = turns.passed.iter().any(here);
    turns.passed.retain(|t| !here(t));
    turns.writing.push((media.clone(), id, who));
    Ok(Turn { app: app.clone(), media, id, after_other })
}

/// Transcribing all of VOD `id`, from a site without its own player,
/// downloads its video into the kept video's file (`pipeline::audio_for`):
/// its turn at that file (`try_turn`), or None while Play's download writes
/// it. Done, the transcription takes its sound from the video kept. The turn
/// is held till the transcription's download is dealt with (worker.rs
/// `cancel` among it).
pub(crate) fn turn_to_transcribe(app: &AppHandle, id: i64) -> Option<Turn> {
    try_turn(app, id, Writer::Transcription).ok()
}

/// Whether `who` writes VOD `id`'s kept video file, in the library at `paths`.
fn writes(paths: &Paths, id: i64, who: Writer) -> bool {
    let media = paths.media();
    turns().writing.iter().any(|(m, v, w)| *m == media && *v == id && *w == who)
}

/// Whether Play's download (or Keep video's) is writing VOD `id`'s kept
/// video file, in the library at `paths`: the files there are its own.
pub(crate) fn played(paths: &Paths, id: i64) -> bool {
    writes(paths, id, Writer::Play)
}

/// Whether transcribing all of VOD `id` is writing its kept video file, in
/// the library at `paths`: the files there are its own.
fn transcribing(paths: &Paths, id: i64) -> bool {
    writes(paths, id, Writer::Transcription)
}

/// Whether Play of VOD `id` waits for transcribing all of it to download
/// its video (`try_turn`), in the library at `paths`.
pub(crate) fn play_waits(paths: &Paths, id: i64) -> bool {
    let media = paths.media();
    turns().passed.iter().any(|(m, v, w)| *m == media && *v == id && *w == Writer::Play)
}

/// Forgets the turns of the library at `paths` (`TURNS`), its queues
/// closed: an app started on its folder again starts with none, as a new
/// process does.
#[cfg(test)]
pub(crate) fn forget_turns(paths: &Paths) {
    let media = paths.media();
    let mut turns = turns();
    turns.writing.retain(|(m, _, _)| *m != media);
    turns.passed.retain(|(m, _, _)| *m != media);
}

/// Play of VOD `id` no longer waits: taken out of the queue.
fn unpass(paths: &Paths, id: i64) {
    let media = paths.media();
    turns().passed.retain(|(m, v, w)| !(*m == media && *v == id && *w == Writer::Play));
}

/// Play of VOD `id`, waiting for transcribing all of it to download its
/// video, is shown that download's `progress`, to play once it's in.
pub(crate) fn waiting_progress(app: &AppHandle, id: i64, progress: f64) {
    let waiting = VideoProgress { id, progress: Some(progress), error: None, streamed: false };
    crate::page::emit(app, "video-progress", waiting);
}

/// Stops VOD `id`'s video download, discarding what's downloaded so far, or
/// takes it out of the queue.
#[tauri::command]
pub(crate) async fn stop_video(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    if !stop(&state.db, &state.videos, &state.paths, id) {
        crate::page::emit(&app, "video-progress", VideoProgress::over(id, None));
    }
    Ok(())
}

/// `stop_video`'s work; whether VOD `id`'s download was running (it then
/// deletes what it had itself).
fn stop(db: &Mutex<Db>, videos: &Videos, paths: &Paths, id: i64) -> bool {
    let queued = |(v, _): &(i64, u32)| *v == id;
    let _files = videos.files();
    // No longer asked, waiting or under way: a quit before the stopped
    // download ends doesn't bring it back at the next start. Under the
    // library's lock with the queue's take, as `keep_video` asks.
    let taken = {
        let db = db.lock().unwrap();
        let _ = db.set_video_wanted(id, None);
        !videos.0.take_waiting(queued).is_empty()
    };
    // Waiting, it has nothing of its own yet, but the VOD's files may be a
    // video forgotten while it was queued (Delete all leaves those of a VOD
    // queued again meanwhile) or a download's parts a quit left: without a
    // video kept, they'd stay untracked.
    // Not while transcribing all of it downloads them, though: they're its.
    if taken {
        unpass(paths, id);
    }
    if taken && db.lock().unwrap().media_path(id).ok().flatten().is_none() && !transcribing(paths, id) {
        pipeline::remove_media(paths, id);
    }
    videos.0.stop_running(queued)
}

/// The VODs whose video is downloading or waiting to (the interface asks at start).
#[tauri::command]
pub(crate) async fn video_downloads(state: State<'_, AppState>) -> CmdResult<Vec<i64>> {
    Ok(state.videos.listed())
}

/// Deletes every downloaded video, keeping the transcripts: they then play on
/// their sites. Downloads under way stop, and so does transcribing from a
/// video, keeping the lines so far, as the row's Delete video does
/// (`vods::cancel_and_wait`). A video whose transcription is still stopping
/// stays; returns how many.
#[tauri::command]
pub(crate) async fn delete_all_videos(app: AppHandle, state: State<'_, AppState>) -> CmdResult<usize> {
    state.videos.stop_all(&app);
    let transcribing: Vec<i64> = {
        let vods = state.db.lock().unwrap().vods().map_err(err)?;
        vods.into_iter().filter(|v| v.media && crate::vods::busy(v.status)).map(|v| v.id).collect()
    };
    for id in transcribing {
        let handle = app.clone();
        // Still stopping, it's kept, below.
        let _ = tauri::async_runtime::spawn_blocking(move || crate::vods::cancel_and_wait(&handle, id)).await;
    }
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
                crate::vods::forget_video(&db, vod.id).map_err(err)?;
                crate::emit_vod(&app, &db, vod.id)?;
            }
            gone.push(vod.id);
        }
    }
    // Deleted with the library let go: many big files can take a while.
    // Meanwhile a "Keep video" may have queued one of them again (it no
    // longer has its video): that download's parts stay. Those of VODs without
    // a kept video go too (a download paused by quitting leaves its parts).
    // No download begins meanwhile (`Videos::files`).
    let _files = state.videos.files();
    for id in gone {
        if !state.videos.wanted(id) {
            crate::vods::remove_kept(&state.storyboards, &state.paths, id);
        }
        crate::storyboards::forget_gone(&app, id);
    }
    Ok(kept)
}

/// Opens the folder of downloaded videos in the system file manager. It
/// takes no path, so the interface can't open anything else with it.
#[tauri::command]
pub(crate) async fn open_videos_dir(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let dir = state.paths.media();
    std::fs::create_dir_all(&dir).map_err(err)?;
    crate::system::open(&app, &dir)
}

#[cfg(test)]
mod tests;
