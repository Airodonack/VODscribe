//! Downloading a VOD's whole video to this computer, on request. A VOD from
//! YouTube, Twitch or Kick then plays in the app's own player, where marks
//! and clips are exact to the frame, and clips are cut from it at once; one
//! from another site (`pipeline::keeps_video`) plays only that way (or is
//! kept once it's transcribed).
//! Kept like other sites' videos, in `Paths::media`, at the Storage quality,
//! in a format that plays here; "Delete video" removes it.
//!
//! One download at a time, in a queue of its own, beside transcription's.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{Db, Vod};
use vodscribe_core::playable::Playable;
use vodscribe_core::settings::Settings;
use vodscribe_core::stretches::Stretch;
use vodscribe_core::{fetch, local, pipeline, tools};

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
/// while waiting its turn, and `error` is set if it failed.
#[derive(Serialize, Clone)]
struct VideoProgress {
    id: i64,
    progress: Option<f64>,
    error: Option<String>,
}

impl Videos {
    pub(crate) fn new() -> Self {
        Videos(Jobs::new(), Mutex::default())
    }

    /// Held while a VOD's video files are deleted, and by a download as it
    /// begins and as it records its file, so neither happens in the middle of
    /// the other: a delete never takes a download's files, nor leaves a
    /// recorded video behind untracked. Taken before the library's lock.
    pub(crate) fn files(&self) -> MutexGuard<'_, ()> {
        self.1.lock().unwrap()
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
                state.storyboards.push(id);
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
    let Some(vod) = begin(&state.db, &state.videos, id)? else { return Ok(()) };
    let bin = paths.bin();
    let yt_dlp = tools::find_yt_dlp(&bin).context(fetch::NO_YT_DLP)?;
    let ffmpeg = tools::find_ffmpeg(&bin).context(crate::worker::NO_FFMPEG)?;
    let work = paths.work();
    std::fs::create_dir_all(&work)?;
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
                let _ = app.emit("video-progress", VideoProgress { id, progress: Some(progress), error: None });
            }
        };
        match stream_then_keep(app, &vod, duration, height, stop, &report) {
            Ok(()) => return Ok(()),
            Err(e) if vodscribe_core::is_canceled(&e) => return Err(e),
            Err(e) => eprintln!("VOD {id}: its video didn't come as a section, so it downloads whole: {e:#}"),
        }
    }
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
    record(&state.db, &state.videos, &work, id, &file, stop)?;
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
    let out = pipeline::media_stem(&paths.media(), vod.id, &vod.info.title).with_extension("mp4");
    vodscribe_core::clips::remux(&ffmpeg, &section.path, &out, duration, stop, |p| report(0.95 + p * 0.05))?;
    record(&state.db, &state.videos, &paths.work(), vod.id, &out, stop)?;
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

/// Records `file`, just downloaded, as VOD `id`'s kept video.
fn record(db: &Mutex<Db>, videos: &Videos, work: &Path, id: i64, file: &Path, stop: &AtomicBool) -> Result<()> {
    let _files = videos.files();
    let db = db.lock().unwrap();
    if stop.load(Ordering::Relaxed) {
        // Stopped while its format was checked (the run loop deletes it):
        // checked under the lock, so "Delete all" never misses it.
        return Err(vodscribe_core::Canceled.into());
    }
    if db.vod(id)?.is_none() {
        // Removed while downloading.
        pipeline::remove_media(work, id);
        return Ok(());
    }
    db.set_media_path(id, Some(&file.to_string_lossy()))?;
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
    if downloading_to_transcribe(&state.db.lock().unwrap(), &vod).map_err(err)? {
        return Err("Its video is already downloading, to transcribe it.".into());
    }
    let queued = |(v, _): &(i64, u32)| *v == id;
    if !state.videos.0.push_new((id, height), queued) {
        return Ok(());
    }
    let _ = app.emit("video-progress", VideoProgress { id, progress: Some(0.0), error: None });
    Ok(())
}

/// Whether VOD `vod`'s video is downloading already, into the same file: a
/// site without its own player, transcribing all of it (`pipeline::audio_for`).
fn downloading_to_transcribe(db: &Db, vod: &Vod) -> Result<bool> {
    // Only a request for all of it downloads the video (a stretch, its sound
    // alone), whichever of its requests the row shows.
    let downloading = |s: &Stretch| s.is_whole() && s.status == vodscribe_core::db::Status::Downloading;
    Ok(pipeline::keeps_video(&vod.info.platform) && db.requests(vod.id)?.iter().any(downloading))
}

/// Stops VOD `id`'s video download, discarding what's downloaded so far, or
/// takes it out of the queue.
#[tauri::command]
pub(crate) async fn stop_video(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    if !stop(&state.db, &state.videos, &state.paths.work(), id) {
        let _ = app.emit("video-progress", VideoProgress { id, progress: None, error: None });
    }
    Ok(())
}

/// `stop_video`'s work; whether VOD `id`'s download was running (it then
/// deletes what it had itself).
fn stop(db: &Mutex<Db>, videos: &Videos, work: &Path, id: i64) -> bool {
    let queued = |(v, _): &(i64, u32)| *v == id;
    let _files = videos.files();
    // Waiting, it has nothing of its own yet, but the VOD's files may be a
    // video forgotten while it was queued (Delete all leaves those of a VOD
    // queued again meanwhile) or a download's parts a quit left: without a
    // video kept, they'd stay untracked.
    if !videos.0.take_waiting(queued).is_empty() && db.lock().unwrap().media_path(id).ok().flatten().is_none() {
        pipeline::remove_media(work, id);
    }
    videos.0.stop_running(queued)
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
    // No download begins meanwhile (`Videos::files`).
    let _files = state.videos.files();
    for id in gone {
        if !state.videos.wanted(id) {
            crate::vods::remove_kept(&state.storyboards, &work, id);
        }
        crate::storyboards::forget_gone(&app, id);
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;

    use vodscribe_core::fetch::VodInfo;

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);

    /// A library with one finished YouTube VOD, and a work dir of its own
    /// (with the media folder beside it), deleted when the test ends.
    struct Setup {
        db: Arc<Mutex<Db>>,
        videos: Arc<Videos>,
        work: std::path::PathBuf,
        id: i64,
        /// The test's own, not the queue thread's copy.
        owner: bool,
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            if self.owner {
                let _ = std::fs::remove_dir_all(self.work.parent().unwrap());
            }
        }
    }

    impl Setup {
        fn new() -> Self {
            use std::sync::atomic::AtomicU64;
            static N: AtomicU64 = AtomicU64::new(0);
            let db = Db::open(Path::new(":memory:")).unwrap();
            let info = VodInfo {
                platform: "youtube".into(),
                platform_id: "x".into(),
                url: "https://youtu.be/x".into(),
                title: "t".into(),
                channel: None,
                duration: Some(60.0),
                thumbnail: None,
                published: None,
            };
            let id = db.add_vod(&info).unwrap().0.id;
            let whole = db.request(id, 0.0, None).unwrap();
            db.finish_stretch(whole.id).unwrap();
            let n = N.fetch_add(1, Ordering::Relaxed);
            let work = std::env::temp_dir().join(format!("vodscribe-videos-{}-{n}", std::process::id())).join("work");
            std::fs::create_dir_all(pipeline::media_dir(&work)).unwrap();
            Setup { db: Arc::new(Mutex::new(db)), videos: Arc::new(Videos::new()), work, id, owner: true }
        }

        /// Where the VOD's video goes, with `ext`.
        fn file(&self, ext: &str) -> std::path::PathBuf {
            pipeline::media_stem(&pipeline::media_dir(&self.work), self.id, "t").with_extension(ext)
        }

        /// A video kept as a download leaves it: the file, recorded.
        fn kept(&self) -> std::path::PathBuf {
            let file = self.file("mp4");
            std::fs::write(&file, b"video").unwrap();
            self.db.lock().unwrap().set_media_path(self.id, Some(&file.to_string_lossy())).unwrap();
            file
        }

        fn media_path(&self) -> Option<String> {
            self.db.lock().unwrap().media_path(self.id).unwrap()
        }

        /// The queue's thread, running `download` as `download` in this file
        /// does: begun, then whatever it does in between, its stop passed on.
        fn run(&self, download: impl Fn(&Setup, &AtomicBool) + Send + 'static) {
            let me = Setup {
                db: self.db.clone(),
                videos: self.videos.clone(),
                work: self.work.clone(),
                id: self.id,
                owner: false,
            };
            self.videos
                .0
                .run("videos-test", move |_, stop| {
                    if begin(&me.db, &me.videos, me.id).unwrap().is_some() {
                        download(&me, stop);
                    }
                })
                .unwrap();
        }

        /// What "Keep video" does to the queue.
        fn keep_video(&self) {
            let id = self.id;
            self.videos.0.push_new((id, 720), |(v, _)| *v == id);
        }
    }

    /// A download that has recorded its file, but whose job hasn't ended yet,
    /// when "Delete video" comes: the file goes with the record.
    #[test]
    fn a_video_deleted_as_its_download_ends_is_deleted() {
        let s = Setup::new();
        let (recorded, has_recorded) = mpsc::channel();
        let (end, ends) = mpsc::channel::<()>();
        let ends = Mutex::new(ends);
        s.run(move |s, stop| {
            let file = s.file("mp4");
            std::fs::write(&file, b"video").unwrap();
            record(&s.db, &s.videos, &s.work, s.id, &file, stop).unwrap();
            recorded.send(()).unwrap();
            // Still the queue's running job (the pause).
            let _ = ends.lock().unwrap().recv_timeout(WAIT);
        });
        s.keep_video();
        has_recorded.recv_timeout(WAIT).unwrap();
        crate::vods::delete_kept_video(&s.db, &s.videos, &s.work, s.id, pipeline::remove_media).unwrap();
        end.send(()).unwrap();
        assert_eq!(s.media_path(), None);
        assert!(!s.file("mp4").exists(), "deleted, not left untracked");
    }

    /// "Keep video" again while "Delete video" deletes the old one: the new
    /// download's files aren't deleted with it.
    #[test]
    fn a_download_started_during_a_delete_keeps_its_files() {
        let s = Setup::new();
        let old = s.kept();
        let (wrote, has_written) = mpsc::channel();
        s.run(move |s, _| {
            std::fs::write(s.file("mp4.part"), b"new").unwrap();
            wrote.send(()).unwrap();
        });
        crate::vods::delete_kept_video(&s.db, &s.videos, &s.work, s.id, |work, id| {
            // Mid-delete (the pause): Keep video, and its download given the
            // time to start.
            s.keep_video();
            let _ = has_written.recv_timeout(Duration::from_millis(500));
            pipeline::remove_media(work, id);
        })
        .unwrap();
        // The download writes now, if it couldn't during the delete.
        let deadline = Instant::now() + WAIT;
        while !s.file("mp4.part").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!old.exists(), "the old video goes");
        assert!(s.file("mp4.part").exists(), "the new download's part stays");
    }

    /// A site without its own player: Play (or Keep video) is refused only
    /// while transcribing all of it downloads the video already, not while a
    /// stretch's sound downloads, nor because another request shows.
    #[test]
    fn keeping_a_video_waits_only_for_a_transcription_downloading_it() {
        use vodscribe_core::db::Status;
        let db = Db::open(Path::new(":memory:")).unwrap();
        let info = VodInfo {
            platform: "rumble".into(),
            platform_id: "r".into(),
            url: "https://rumble.com/r".into(),
            title: "t".into(),
            channel: None,
            duration: Some(600.0),
            thumbnail: None,
            published: None,
        };
        let id = db.add_vod(&info).unwrap().0.id;
        let busy = |db: &Db| downloading_to_transcribe(db, &db.vod(id).unwrap().unwrap()).unwrap();
        let part = db.request(id, 100.0, Some(130.0)).unwrap().id;
        db.set_stretch_status(part, Status::Downloading, 0.1, None).unwrap();
        assert!(!busy(&db), "a stretch's sound alone");
        db.set_stretch_status(part, Status::Transcribing, 0.1, None).unwrap();
        let whole = db.request(id, 0.0, None).unwrap().id;
        db.set_stretch_status(whole, Status::Downloading, 0.1, None).unwrap();
        assert!(busy(&db), "all of it, though the row shows the other");
    }

    /// "Delete video" with a "Keep video" queued (it lands between the
    /// forget and the check: keep_video checks the record, then queues),
    /// then that download stopped before it began: no file is left behind
    /// with no record of it.
    #[test]
    fn a_redownload_stopped_while_waiting_leaves_no_file() {
        let s = Setup::new();
        let old = s.kept();
        // No queue thread: the download waits.
        s.keep_video();
        crate::vods::delete_kept_video(&s.db, &s.videos, &s.work, s.id, pipeline::remove_media).unwrap();
        assert!(!stop(&s.db, &s.videos, &s.work, s.id), "it was waiting");
        assert_eq!(s.media_path(), None);
        assert!(!old.exists(), "not left untracked");
    }

    /// The same through "Delete all", which leaves the files of a VOD queued
    /// again meanwhile: stopped before it began, its download takes them.
    #[test]
    fn a_download_stopped_while_waiting_takes_its_vods_untracked_files() {
        let s = Setup::new();
        let left = s.file("mp4");
        std::fs::write(&left, b"video").unwrap();
        s.keep_video();
        assert!(!stop(&s.db, &s.videos, &s.work, s.id));
        assert!(!left.exists());
    }
}
