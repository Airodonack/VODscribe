//! The background queue, in two stages on their own threads: one downloads
//! while the other transcribes, so a stretch can download while another is
//! transcribed. Each stage runs one request (`stretches`) at a time, in the
//! order they were asked for. The queue is the library itself, so it
//! survives a quit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::{Db, Status, Vod};
use vodscribe_core::pipeline::{Transcription, WorkerSpec};
use vodscribe_core::settings::{Device, Settings};
use vodscribe_core::stretches::Stretch;
use vodscribe_core::{Paths, gpu, model, pipeline, tools};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// How many downloaded requests may wait for the transcriber before the
/// downloader pauses, so finished audio doesn't pile up on disk.
const MAX_READY: usize = 2;

/// Why a running job was stopped, from the least to the most final.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stop {
    /// Keep the partial download and transcript; resume later.
    Pause,
    /// End the request, keeping the lines it wrote; its audio goes.
    Cancel,
}

struct Job {
    stretch_id: i64,
    vod_id: i64,
    flag: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<Stop>>>,
}

/// Queue state shared by the stages and the commands. Stages pick and
/// register a job under this lock, and commands that stop or change a
/// request take it too, so a command can't slip in between the two.
#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    /// While set, stages start nothing new (see `Worker::hold`).
    held: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Download,
    Transcribe,
}

pub struct Worker {
    wakes: Mutex<[Sender<()>; 2]>,
    queue: Arc<Mutex<Queue>>,
}

/// Keeps the queue held until dropped; see `Worker::hold`.
pub struct Hold<'a>(&'a Worker);

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        self.0.queue.lock().unwrap().held = false;
        self.0.wake();
    }
}

impl Worker {
    pub fn start(app: AppHandle, paths: Paths) -> Result<Self> {
        Db::open(&paths.db())?.requeue_interrupted()?;
        let queue = Arc::new(Mutex::new(Queue::default()));
        let (dl_tx, dl_rx) = mpsc::channel();
        let (tr_tx, tr_rx) = mpsc::channel();
        for (stage, rx) in [(Stage::Download, dl_rx), (Stage::Transcribe, tr_rx)] {
            let ctx = Ctx {
                app: app.clone(),
                db: Db::open(&paths.db())?,
                paths: paths.clone(),
                queue: queue.clone(),
                wake_other: match stage {
                    Stage::Download => tr_tx.clone(),
                    Stage::Transcribe => dl_tx.clone(),
                },
            };
            let name = match stage {
                Stage::Download => "download",
                Stage::Transcribe => "transcribe",
            };
            std::thread::Builder::new().name(name.into()).spawn(move || run(stage, ctx, rx))?;
        }
        Ok(Self { wakes: Mutex::new([dl_tx, tr_tx]), queue })
    }

    /// Tells both stages to look for work.
    pub fn wake(&self) {
        for tx in self.wakes.lock().unwrap().iter() {
            let _ = tx.send(());
        }
    }

    /// Stops new jobs from starting, pauses the running ones, and waits up to
    /// `timeout` for them to exit. The queue stays held until the returned
    /// guard is dropped, so nothing starts while e.g. files are deleted.
    pub fn hold(&self, timeout: Duration) -> Result<Hold<'_>> {
        {
            let mut queue = self.queue.lock().unwrap();
            queue.held = true;
            for job in &queue.jobs {
                job.stop(Stop::Pause);
            }
        }
        let hold = Hold(self);
        let deadline = Instant::now() + timeout;
        while !self.queue.lock().unwrap().jobs.is_empty() {
            if Instant::now() >= deadline {
                anyhow::bail!("Something was still running. Try again in a moment.");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(hold)
    }

    /// Stops new jobs from starting, without touching running ones, until the
    /// guard is dropped. Also returns the requests the stages are working on,
    /// so the caller can check what's running with nothing new slipping in.
    pub fn hold_starts(&self) -> (Hold<'_>, Vec<i64>) {
        let mut queue = self.queue.lock().unwrap();
        queue.held = true;
        let running = queue.jobs.iter().map(|j| j.stretch_id).collect();
        (Hold(self), running)
    }

    /// Applies `reason` to request `stretch_id`. If a stage is running it,
    /// the stage is told to stop and finishes the stop itself; otherwise
    /// `idle` runs, with the queue locked so no stage can start it meanwhile.
    /// Returns whether a stage was running it.
    pub fn control(&self, stretch_id: i64, reason: Stop, idle: impl FnOnce() -> Result<()>) -> Result<bool> {
        let queue = self.queue.lock().unwrap();
        if let Some(job) = queue.jobs.iter().find(|j| j.stretch_id == stretch_id) {
            job.stop(reason);
            return Ok(true);
        }
        idle()?;
        Ok(false)
    }

    /// Applies `reason` to every running request of VOD `vod_id` (the
    /// stages finish those stops themselves), then runs `idle` with the
    /// running ones' ids, with the queue locked so no stage can start
    /// another of the VOD's meanwhile: it does to the rest what's wanted.
    /// Returns whether a stage was running one.
    pub fn control_vod(&self, vod_id: i64, reason: Stop, idle: impl FnOnce(&[i64]) -> Result<()>) -> Result<bool> {
        let queue = self.queue.lock().unwrap();
        let mut running = Vec::new();
        for job in queue.jobs.iter().filter(|j| j.vod_id == vod_id) {
            job.stop(reason);
            running.push(job.stretch_id);
        }
        idle(&running)?;
        Ok(!running.is_empty())
    }
}

impl Job {
    /// Stops the job for `reason`, unless it's already stopping for a more
    /// final one: clearing the cache pauses every job, which mustn't turn a
    /// cancel on its way into a pause.
    fn stop(&self, reason: Stop) {
        let mut current = self.reason.lock().unwrap();
        *current = (*current).max(Some(reason));
        self.flag.store(true, Ordering::Relaxed);
    }
}

struct Ctx {
    app: AppHandle,
    db: Db,
    paths: Paths,
    queue: Arc<Mutex<Queue>>,
    wake_other: Sender<()>,
}

impl Ctx {
    fn emit_vod(&self, id: i64) {
        let _ = crate::emit_vod(&self.app, &self.db, id);
    }
}

fn run(stage: Stage, mut ctx: Ctx, rx: Receiver<()>) {
    loop {
        // Pick and register the job in one step; see `Queue`.
        let picked = {
            let mut queue = ctx.queue.lock().unwrap();
            let next = if queue.held { Ok(None) } else { next_job(stage, &ctx) };
            next.unwrap_or_else(|e| {
                eprintln!("queue: {e:#}");
                None
            })
            // The download stage marks a request Ready a moment before it lets
            // go of it; the other stage's wake follows.
            .filter(|(_, stretch)| queue.jobs.iter().all(|j| j.stretch_id != stretch.id))
            .map(|(vod, stretch)| {
                let flag = Arc::new(AtomicBool::new(false));
                let reason = Arc::new(Mutex::new(None));
                let job = Job { stretch_id: stretch.id, vod_id: vod.id, flag: flag.clone(), reason: reason.clone() };
                queue.jobs.push(job);
                (vod, stretch, flag, reason)
            })
        };
        let Some((vod, stretch, flag, reason)) = picked else {
            // Sleep until something is queued, a stage finishes, or setup completes.
            if rx.recv().is_err() {
                return;
            }
            continue;
        };

        let mut progress = 0.0;
        let result = match stage {
            Stage::Download => download(&mut ctx, &vod, &stretch, &flag, &mut progress),
            Stage::Transcribe => transcribe(&mut ctx, &vod, &stretch, &flag, &mut progress),
        };
        // Finish and unregister under the queue lock, so a stop can't land
        // after its reason has been read.
        let finished = {
            let mut queue = ctx.queue.lock().unwrap();
            let stop = *reason.lock().unwrap();
            let finished = finish(stage, &ctx.db, &ctx.paths.work(), stretch.id, result, stop, progress);
            queue.jobs.retain(|j| !Arc::ptr_eq(&j.flag, &flag));
            finished
        };
        if let Err(e) = finished {
            eprintln!("queue: {e:#}");
        }
        ctx.emit_vod(vod.id);
        let _ = ctx.wake_other.send(());
    }
}

fn next_job(stage: Stage, ctx: &Ctx) -> Result<Option<(Vod, Stretch)>> {
    let bin = ctx.paths.bin();
    if tools::find_ffmpeg(&bin).is_none() {
        return Ok(None);
    }
    // Without the video downloader, only local files can be fetched; online VODs wait.
    let has_yt_dlp = tools::find_yt_dlp(&bin).is_some();
    match stage {
        Stage::Download => {
            if ctx.db.count_stretches(Status::Ready)? >= MAX_READY {
                return Ok(None);
            }
            ctx.db.next_stretch(Status::Queued, !has_yt_dlp)
        }
        Stage::Transcribe => {
            if !model::is_installed(&ctx.paths.model_dir(), &model::CPU_MODEL) {
                return Ok(None);
            }
            ctx.db.next_stretch(Status::Ready, false)
        }
    }
}

/// Records how a job ended: success, a stop the user asked for, or an error.
fn finish(
    stage: Stage,
    db: &Db,
    work: &std::path::Path,
    stretch_id: i64,
    result: Result<()>,
    stop: Option<Stop>,
    progress: f64,
) -> Result<()> {
    match (result, stop) {
        // The transcript finished before the stop took effect: keep it.
        (Ok(()), Some(_)) if stage == Stage::Transcribe => {}
        (_, Some(Stop::Pause)) => db.set_stretch_status(stretch_id, Status::Paused, progress, None)?,
        (_, Some(Stop::Cancel)) => cancel(db, work, stretch_id)?,
        (Ok(()), None) => {}
        // Stopped without a recorded reason: keep the work, as a pause would.
        (Err(e), None) if vodscribe_core::is_canceled(&e) => {
            db.set_stretch_status(stretch_id, Status::Paused, progress, None)?
        }
        (Err(e), None) => {
            // The VOD may have been deleted while running; ignore that case.
            let _ = db.set_stretch_status(stretch_id, Status::Error, 0.0, Some(&format!("{e:#}")));
        }
    }
    Ok(())
}

pub const NO_FFMPEG: &str = "ffmpeg isn't installed. VODscribe needs it to read and cut audio and video.";

/// Ends request `stretch_id`, keeping the lines it wrote: it's done as far
/// as it reached (`Db::end_request`). Its section of audio goes; with no
/// other request of its VOD left, the VOD's work files go, and a partly downloaded video where
/// downloading it is part of transcribing (one kept whole, or downloaded on
/// request (videos.rs), stays).
pub fn cancel(db: &Db, work_dir: &std::path::Path, stretch_id: i64) -> Result<()> {
    let Some(stretch) = db.stretch(stretch_id)? else { return Ok(()) };
    let vod_id = stretch.vod_id;
    remove_section(work_dir, &stretch);
    db.end_request(stretch_id)?;
    if !db.requests(vod_id)?.is_empty() {
        return Ok(());
    }
    pipeline::remove_work_files(work_dir, vod_id);
    db.set_audio_path(vod_id, None)?;
    if db.vod(vod_id)?.is_some_and(|v| pipeline::keeps_video(&v.info.platform) && !v.media) {
        pipeline::remove_media(work_dir, vod_id);
    }
    Ok(())
}

/// Deletes request `stretch`'s section of audio, and any part of one.
fn remove_section(work_dir: &std::path::Path, stretch: &Stretch) {
    let stem = pipeline::section_stem(work_dir, stretch.vod_id, stretch.id);
    pipeline::remove_work_prefix(work_dir, &stem.file_name().unwrap_or_default().to_string_lossy());
}

/// A progress reporter that throttles events and remembers the last value.
/// The event says which request of which VOD, and its progress within it.
fn reporter<'a>(
    app: &'a AppHandle,
    vod_id: i64,
    stretch_id: i64,
    progress: &'a mut f64,
) -> impl FnMut(Status, f64) + 'a {
    let mut last = (Status::Queued, Instant::now());
    move |status, p| {
        *progress = p;
        let now = Instant::now();
        if status != last.0 || now - last.1 >= PROGRESS_INTERVAL {
            last = (status, now);
            let _ = app.emit("vod-progress", (vod_id, stretch_id, status, p));
        }
    }
}

fn download(ctx: &mut Ctx, vod: &Vod, stretch: &Stretch, flag: &AtomicBool, progress: &mut f64) -> Result<()> {
    let bin = ctx.paths.bin();
    // Local files don't need the video downloader; pipeline::download asks for it otherwise.
    let yt_dlp = tools::find_yt_dlp(&bin);
    let ffmpeg = tools::find_ffmpeg(&bin).context(NO_FFMPEG)?;
    let tools = pipeline::Tools { yt_dlp: yt_dlp.as_ref(), ffmpeg: &ffmpeg };
    let app = ctx.app.clone();
    // A kept video (sites without embeds) in a format this computer plays, as tall as the settings allow.
    let height = Settings::load(&ctx.paths.settings()).video_height();
    let playable = vodscribe_core::playable::Playable::load(&ctx.paths.playable());
    let found = pipeline::audio_for(
        &mut ctx.db,
        vod,
        stretch,
        &tools,
        &ctx.paths.work(),
        Some((&playable, height)),
        flag,
        reporter(&app, vod.id, stretch.id, progress),
    )?;
    // The waveform, read while the audio is here: a VOD from a site with its
    // own player doesn't keep it once transcribed. A section's levels go at
    // their place; whole audio just downloaded is read anew over them; a
    // file or kept video only if there's none (the page reads theirs).
    // Only a stop fails the stage.
    let peaks = ctx.paths.peaks();
    let (path, at) = match &found {
        pipeline::Source::Section { path, start } => (path, Some(*start)),
        pipeline::Source::Whole { path, downloaded } => {
            let read = *downloaded || !vodscribe_core::peaks::path(&peaks, vod.id).is_file();
            (path, read.then_some(0.0))
        }
    };
    if let Some(at) = at {
        match vodscribe_core::peaks::read(&ffmpeg, path, flag) {
            Ok(levels) => {
                let saved = match found {
                    pipeline::Source::Section { .. } => vodscribe_core::peaks::merge(&peaks, vod.id, at, &levels),
                    pipeline::Source::Whole { .. } => vodscribe_core::peaks::save(&peaks, vod.id, &levels),
                };
                if let Err(e) = saved {
                    eprintln!("VOD {}: save its waveform: {e:#}", vod.id);
                }
            }
            Err(e) if vodscribe_core::is_canceled(&e) => return Err(e),
            Err(e) => eprintln!("VOD {}: read its waveform: {e:#}", vod.id),
        }
    }
    match &found {
        pipeline::Source::Section { path, .. } => {
            ctx.db.set_stretch_audio(stretch.id, Some(&path.to_string_lossy()))?
        }
        pipeline::Source::Whole { path, .. } => ctx.db.set_audio_path(vod.id, Some(&path.to_string_lossy()))?,
    }
    ctx.db.set_stretch_status(stretch.id, Status::Ready, 0.0, None)?;
    Ok(())
}

fn transcribe(ctx: &mut Ctx, vod: &Vod, stretch: &Stretch, flag: &AtomicBool, progress: &mut f64) -> Result<()> {
    // Its own section, its 0 at its start; or else the VOD's whole audio.
    let file = |path: Option<String>| path.map(std::path::PathBuf::from).filter(|p| p.is_file());
    let audio = match file(stretch.audio.clone()) {
        Some(path) => Some(pipeline::Audio { path, offset: stretch.start }),
        None => file(ctx.db.audio_path(vod.id)?).map(|path| pipeline::Audio { path, offset: 0.0 }),
    };
    let Some(audio) = audio else {
        // The download went missing (e.g. the work dir was cleared): fetch it again.
        ctx.db.set_audio_path(vod.id, None)?;
        ctx.db.set_stretch_audio(stretch.id, None)?;
        ctx.db.set_stretch_status(stretch.id, Status::Queued, 0.0, None)?;
        return Ok(());
    };
    let ffmpeg = tools::find_ffmpeg(&ctx.paths.bin()).context(NO_FFMPEG)?;
    let (transcription, note) = plan_transcription(&ctx.paths, bundled_lib_dir(&ctx.app))?;
    ctx.db.set_note(vod.id, note.as_deref())?;
    ctx.emit_vod(vod.id);

    let app = ctx.app.clone();
    let mut report = reporter(&app, vod.id, stretch.id, progress);
    // The pipeline holds the stage's connection, so notices go through a second one.
    let notes = Db::open(&ctx.paths.db())?;
    let mut notice = |notice: String| {
        let _ = notes.set_note(vod.id, Some(&notice));
        let _ = crate::emit_vod(&app, &notes, vod.id);
    };
    pipeline::transcribe(&mut ctx.db, vod, stretch, &ffmpeg, &audio, &transcription, flag, &mut report, &mut notice)?;
    remove_section(&ctx.paths.work(), stretch);
    // The whole audio stays while another request of the VOD may use it.
    if ctx.db.requests(vod.id)?.is_empty() {
        pipeline::remove_work_files(&ctx.paths.work(), vod.id);
        ctx.db.set_audio_path(vod.id, None)?;
    }
    Ok(())
}

/// The `lib` folder the installer ships: the no-tts sherpa-onnx library the GPU
/// worker links (the GPU add-on supplies only ONNX Runtime and the CUDA libraries).
/// None if it's missing; the worker then fails to start and the CPU takes the job.
fn bundled_lib_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().resource_dir().ok()?.join("lib");
    if !dir.is_dir() {
        eprintln!("bundled library folder missing: {}", dir.display());
        return None;
    }
    Some(dir)
}

/// Picks the worker for the next job. GPU is used only when selected, installed
/// and free enough; otherwise the CPU runs it and the note says why. The GPU
/// worker searches `bundled_libs` before the add-on's folder.
fn plan_transcription(
    paths: &Paths,
    bundled_libs: Option<std::path::PathBuf>,
) -> Result<(Transcription, Option<String>)> {
    let cpu = WorkerSpec {
        program: tools::sidecar("vodscribe-worker").context("transcription worker")?,
        model_id: model::CPU_MODEL.id,
        model_dir: paths.model_dir(),
        provider: "cpu",
        batch: 1,
        lib_dirs: Vec::new(),
    };
    let cpu_only = |note: Option<String>| (Transcription { worker: cpu.clone(), fallback: None }, note);
    if Settings::load(&paths.settings()).device != Device::Gpu {
        return Ok(cpu_only(None));
    }
    let pack = paths.gpu_pack();
    let skip = |why: &str| Ok(cpu_only(Some(format!("Transcribed with the processor: {why}."))));
    if !gpu::is_installed(&pack) || !model::is_installed(&paths.gpu_model_dir(), &model::GPU_MODEL) {
        return skip("the GPU add-on isn't installed");
    }
    let Ok(program) = tools::sidecar("vodscribe-worker-cuda") else {
        return skip("this version of VODscribe can't use graphics cards");
    };
    let Some(info) = gpu::detect() else {
        return skip("no NVIDIA graphics card was found");
    };
    let Some(batch) = gpu::batch_for_free_vram(info.free_mb) else {
        return skip("the graphics card was busy with something else");
    };
    let cuda = WorkerSpec {
        program,
        model_id: model::GPU_MODEL.id,
        model_dir: paths.gpu_model_dir(),
        provider: "cuda",
        batch,
        lib_dirs: bundled_libs.into_iter().chain([gpu::lib_dir(&pack)]).collect(),
    };
    Ok((Transcription { worker: cuda, fallback: Some(cpu) }, Some("Transcribed with the graphics card.".into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vodscribe_core::fetch::VodInfo;
    use vodscribe_core::stretches::Transcribed;

    /// A work dir, deleted when the test ends.
    struct Work(std::path::PathBuf);

    impl std::ops::Deref for Work {
        type Target = std::path::Path;
        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Work {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A library with one VOD and a request for all of it in `status`, and
    /// a work dir holding its audio. Returns the request's id.
    fn setup(status: Status) -> (Db, Work, i64) {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
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
        let stretch = db.request(id, 0.0, None).unwrap().id;
        db.set_stretch_status(stretch, status, 0.5, None).unwrap();
        // A folder of its own, so its media folder (beside it) is its own too.
        let work =
            std::env::temp_dir().join(format!("vodscribe-finish-{}-{}", std::process::id(), next_id())).join("work");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join(format!("{id}.m4a")), b"audio").unwrap();
        db.set_audio_path(id, Some(&work.join(format!("{id}.m4a")).to_string_lossy())).unwrap();
        (db, Work(work), stretch)
    }

    /// The VOD of request `stretch`.
    fn vod_of(db: &Db, stretch: i64) -> i64 {
        db.stretch(stretch).unwrap().unwrap().vod_id
    }

    fn next_id() -> u64 {
        use std::sync::atomic::AtomicU64;
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    /// The status and error of request `stretch`'s VOD.
    fn status(db: &Db, stretch: i64) -> (Status, Option<String>) {
        let vod = db.vod(vod_of(db, stretch)).unwrap().unwrap();
        (vod.status, vod.error)
    }

    /// Request `stretch`'s VOD's lines, as (start, text).
    fn lines(db: &Db, vod: i64) -> Vec<(f64, String)> {
        db.lines(vod, None).unwrap().into_iter().map(|(start, _, text)| (start, text)).collect()
    }

    /// Writes a line from 0 to 10 s for request `id`, which reached its end.
    fn write_a_line(db: &mut Db, id: i64) {
        let vod = vod_of(db, id);
        let line = vodscribe_core::transcript::Segment { start: 0.0, end: 10.0, text: "hi".into(), words: Vec::new() };
        db.replace_segments(vod, id, (0.0, Some(10.0)), &[line], 10.0).unwrap();
    }

    #[test]
    fn a_pause_keeps_the_request_and_its_work() {
        let (mut db, work, id) = setup(Status::Transcribing);
        write_a_line(&mut db, id);
        finish(Stage::Transcribe, &db, &work, id, Err(vodscribe_core::Canceled.into()), Some(Stop::Pause), 0.4)
            .unwrap();
        assert_eq!(status(&db, id).0, Status::Paused);
        let stretch = db.stretch(id).unwrap().unwrap();
        assert_eq!((stretch.status, stretch.progress, stretch.reached), (Status::Paused, 0.4, Some(10.0)));
        assert!(work.join(format!("{}.m4a", vod_of(&db, id))).exists());
        assert_eq!(lines(&db, vod_of(&db, id)), [(0.0, "hi".into())]);
    }

    #[test]
    fn a_cancel_keeps_the_lines_so_far_and_lets_the_audio_go() {
        let (mut db, work, id) = setup(Status::Transcribing);
        let vod = vod_of(&db, id);
        write_a_line(&mut db, id);
        finish(Stage::Transcribe, &db, &work, id, Err(vodscribe_core::Canceled.into()), Some(Stop::Cancel), 0.4)
            .unwrap();
        let shown = db.vod(vod).unwrap().unwrap();
        assert_eq!((shown.status, shown.transcribed, shown.error), (Status::None, Transcribed::Some, None));
        let stretches: Vec<_> = db.stretches(vod).unwrap().into_iter().map(|s| (s.start, s.end, s.status)).collect();
        assert_eq!(stretches, [(0.0, Some(10.0), Status::Done)], "done as far as it reached");
        assert_eq!(lines(&db, vod), [(0.0, "hi".into())]);
        assert_eq!(db.audio_path(vod).unwrap(), None);
        assert!(!work.join(format!("{vod}.m4a")).exists());
    }

    #[test]
    fn a_cancel_lets_its_section_go_and_the_whole_audio_while_another_waits() {
        let (db, work, id) = setup(Status::Transcribing);
        let vod = vod_of(&db, id);
        let section = work.join(format!("{vod}.s{id}.m4a"));
        std::fs::write(&section, b"section").unwrap();
        std::fs::write(work.join(format!("{vod}.s{id}.m4a.part")), b"part").unwrap();
        db.set_stretch_audio(id, Some(&section.to_string_lossy())).unwrap();
        let other = db.request(vod, 100.0, Some(110.0)).unwrap().id;
        finish(Stage::Transcribe, &db, &work, id, Err(vodscribe_core::Canceled.into()), Some(Stop::Cancel), 0.4)
            .unwrap();
        let mut left: Vec<_> =
            std::fs::read_dir(&*work).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, [format!("{vod}.m4a")], "the whole audio, for the other request");
        finish(Stage::Download, &db, &work, other, Err(vodscribe_core::Canceled.into()), Some(Stop::Cancel), 0.0)
            .unwrap();
        assert_eq!(std::fs::read_dir(&*work).unwrap().count(), 0);
    }

    #[test]
    fn a_cancel_before_any_line_leaves_nothing_asked() {
        let (db, work, id) = setup(Status::Downloading);
        let vod = vod_of(&db, id);
        finish(Stage::Download, &db, &work, id, Err(vodscribe_core::Canceled.into()), Some(Stop::Cancel), 0.4).unwrap();
        assert_eq!(db.stretches(vod).unwrap(), []);
        assert_eq!(db.vod(vod).unwrap().unwrap().status, Status::None);
    }

    #[test]
    fn a_cancel_keeps_a_video_downloaded_on_request() {
        let (db, work, id) = setup(Status::Transcribing);
        let media = pipeline::media_dir(&work);
        std::fs::create_dir_all(&media).unwrap();
        let vod = vod_of(&db, id);
        let video = pipeline::media_stem(&media, vod, "t").with_extension("mp4");
        std::fs::write(&video, b"").unwrap();
        let video = video.to_string_lossy().into_owned();
        db.set_media_path(vod, Some(&video)).unwrap();
        finish(Stage::Transcribe, &db, &work, id, Err(vodscribe_core::Canceled.into()), Some(Stop::Cancel), 0.4)
            .unwrap();
        assert_eq!(db.vod(vod).unwrap().unwrap().status, Status::None);
        assert!(std::path::Path::new(&video).exists());
        assert_eq!(db.media_path(vod).unwrap(), Some(video));
    }

    #[test]
    fn a_transcript_that_finished_before_the_stop_is_kept() {
        let (db, work, id) = setup(Status::Transcribing);
        let vod = vod_of(&db, id);
        db.finish_stretch(id).unwrap();
        // Written anew as it was merged with the others done.
        let id = db.stretches(vod).unwrap()[0].id;
        for stop in [Stop::Pause, Stop::Cancel] {
            finish(Stage::Transcribe, &db, &work, id, Ok(()), Some(stop), 1.0).unwrap();
            assert_eq!(status(&db, id), (Status::Done, None));
        }
    }

    #[test]
    fn a_stop_with_no_reason_is_a_pause_and_a_failure_is_an_error() {
        let (db, work, id) = setup(Status::Downloading);
        finish(Stage::Download, &db, &work, id, Err(vodscribe_core::Canceled.into()), None, 0.3).unwrap();
        assert_eq!(status(&db, id).0, Status::Paused);
        finish(Stage::Download, &db, &work, id, Err(anyhow::anyhow!("HTTP 404")), None, 0.3).unwrap();
        assert_eq!(status(&db, id), (Status::Error, Some("HTTP 404".into())));
    }

    #[test]
    fn a_vod_deleted_while_running_is_no_error() {
        let (db, work, id) = setup(Status::Downloading);
        let vod = vod_of(&db, id);
        db.delete_vod(vod).unwrap();
        finish(Stage::Download, &db, &work, id, Err(anyhow::anyhow!("gone")), None, 0.3).unwrap();
        assert!(db.vod(vod).unwrap().is_none());
    }

    #[test]
    fn stops_never_become_less_final() {
        let job = Job {
            stretch_id: 1,
            vod_id: 1,
            flag: Arc::new(AtomicBool::new(false)),
            reason: Arc::new(Mutex::new(None)),
        };
        job.stop(Stop::Pause);
        assert_eq!(*job.reason.lock().unwrap(), Some(Stop::Pause));
        job.stop(Stop::Cancel);
        // Clearing the cache pauses every job, after the cancel.
        job.stop(Stop::Pause);
        assert_eq!(*job.reason.lock().unwrap(), Some(Stop::Cancel));
        assert!(job.flag.load(Ordering::Relaxed));
    }
}
