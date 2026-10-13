//! The background queue, in two stages on their own threads: one downloads
//! while the other transcribes, so a VOD can download while another is
//! transcribed. Each stage runs one VOD at a time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::{Db, Status, Vod};
use vodscribe_core::pipeline::{Transcription, WorkerSpec};
use vodscribe_core::settings::{Device, Settings};
use vodscribe_core::{Paths, gpu, model, pipeline, tools};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// How many downloaded VODs may wait for the transcriber before the
/// downloader pauses, so finished audio doesn't pile up on disk.
const MAX_READY: usize = 2;

/// Why a running job was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// Keep the partial download and transcript; resume later.
    Pause,
    /// Discard partial work; the VOD stays in the library as canceled.
    Cancel,
    /// The VOD was removed; clean up its files.
    Delete,
}

struct Job {
    vod_id: i64,
    flag: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<Stop>>>,
}

/// Queue state shared by the stages and the commands. Stages pick and
/// register a job under this lock, and commands that stop or change a VOD
/// take it too, so a command can't slip in between the two.
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
    /// guard is dropped. Also returns the VODs the stages are working on, so
    /// the caller can check what's running with nothing new slipping in.
    pub fn hold_starts(&self) -> (Hold<'_>, Vec<i64>) {
        let mut queue = self.queue.lock().unwrap();
        queue.held = true;
        let running = queue.jobs.iter().map(|j| j.vod_id).collect();
        (Hold(self), running)
    }

    /// Applies `reason` to a VOD. If a stage is running it, the stage is told
    /// to stop and finishes the stop itself; otherwise `idle` runs, with the
    /// queue locked so no stage can start the VOD meanwhile. Returns whether a
    /// stage was running it.
    pub fn control(&self, vod_id: i64, reason: Stop, idle: impl FnOnce() -> Result<()>) -> Result<bool> {
        let queue = self.queue.lock().unwrap();
        if let Some(job) = queue.jobs.iter().find(|j| j.vod_id == vod_id) {
            job.stop(reason);
            return Ok(true);
        }
        idle()?;
        Ok(false)
    }
}

impl Job {
    fn stop(&self, reason: Stop) {
        *self.reason.lock().unwrap() = Some(reason);
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
            .map(|vod| {
                let flag = Arc::new(AtomicBool::new(false));
                let reason = Arc::new(Mutex::new(None));
                queue.jobs.push(Job { vod_id: vod.id, flag: flag.clone(), reason: reason.clone() });
                (vod, flag, reason)
            })
        };
        let Some((vod, flag, reason)) = picked else {
            // Sleep until something is queued, a stage finishes, or setup completes.
            if rx.recv().is_err() {
                return;
            }
            continue;
        };

        let mut progress = 0.0;
        let result = match stage {
            Stage::Download => download(&mut ctx, &vod, &flag, &mut progress),
            Stage::Transcribe => transcribe(&mut ctx, &vod, &flag, &mut progress),
        };
        // Finish and unregister under the queue lock, so a stop can't land
        // after its reason has been read.
        let finished = {
            let mut queue = ctx.queue.lock().unwrap();
            let stop = *reason.lock().unwrap();
            let finished = finish(stage, &ctx.db, &ctx.paths.work(), vod.id, result, stop, progress);
            queue.jobs.retain(|j| j.vod_id != vod.id);
            finished
        };
        if let Err(e) = finished {
            eprintln!("queue: {e:#}");
        }
        ctx.emit_vod(vod.id);
        let _ = ctx.wake_other.send(());
    }
}

fn next_job(stage: Stage, ctx: &Ctx) -> Result<Option<Vod>> {
    let bin = ctx.paths.bin();
    if tools::find_ffmpeg(&bin).is_none() {
        return Ok(None);
    }
    // Without the video downloader, only local files can be fetched; online VODs wait.
    let has_yt_dlp = tools::find_yt_dlp(&bin).is_some();
    match stage {
        Stage::Download => {
            if ctx.db.count_with_status(Status::Ready)? >= MAX_READY {
                return Ok(None);
            }
            if has_yt_dlp {
                ctx.db.next_with_status(Status::Queued)
            } else {
                ctx.db.next_local_with_status(Status::Queued)
            }
        }
        Stage::Transcribe => {
            if !model::is_installed(&ctx.paths.model_dir(), &model::CPU_MODEL) {
                return Ok(None);
            }
            ctx.db.next_with_status(Status::Ready)
        }
    }
}

/// Records how a job ended: success, a stop the user asked for, or an error.
fn finish(
    stage: Stage,
    db: &Db,
    work: &std::path::Path,
    vod_id: i64,
    result: Result<()>,
    stop: Option<Stop>,
    progress: f64,
) -> Result<()> {
    match (result, stop) {
        (_, Some(Stop::Delete)) => pipeline::remove_work_files(work, vod_id),
        // The transcript finished before the stop took effect: keep it.
        (Ok(()), Some(_)) if stage == Stage::Transcribe => {}
        (_, Some(Stop::Pause)) => db.set_status(vod_id, Status::Paused, progress, None)?,
        (_, Some(Stop::Cancel)) => discard(db, work, vod_id)?,
        (Ok(()), None) => {}
        // Stopped without a recorded reason: keep the work, as a pause would.
        (Err(e), None) if vodscribe_core::is_canceled(&e) => db.set_status(vod_id, Status::Paused, progress, None)?,
        (Err(e), None) => {
            // The VOD may have been deleted while running; ignore that case.
            let _ = db.set_status(vod_id, Status::Error, 0.0, Some(&format!("{e:#}")));
        }
    }
    Ok(())
}

/// The error note of a VOD the user stopped; the UI shows it as "Stopped", not "Failed".
pub const STOPPED: &str = "Stopped";
pub const NO_FFMPEG: &str = "ffmpeg isn't installed. VODscribe needs it to read audio.";

/// Throws away a VOD's partial work, leaving it in the library as canceled.
pub fn discard(db: &Db, work_dir: &std::path::Path, vod_id: i64) -> Result<()> {
    pipeline::remove_work_files(work_dir, vod_id);
    db.atomically(|db| {
        db.clear_segments(vod_id)?;
        db.set_audio_path(vod_id, None)?;
        db.set_note(vod_id, None)?;
        db.set_status(vod_id, Status::Error, 0.0, Some(STOPPED))
    })
}

/// A progress reporter that throttles events and remembers the last value.
fn reporter<'a>(app: &'a AppHandle, vod_id: i64, progress: &'a mut f64) -> impl FnMut(Status, f64) + 'a {
    let mut last = (Status::Queued, Instant::now());
    move |status, p| {
        *progress = p;
        let now = Instant::now();
        if status != last.0 || now - last.1 >= PROGRESS_INTERVAL {
            last = (status, now);
            let _ = app.emit("vod-progress", (vod_id, status, p));
        }
    }
}

fn download(ctx: &mut Ctx, vod: &Vod, flag: &AtomicBool, progress: &mut f64) -> Result<()> {
    let bin = ctx.paths.bin();
    // Local files don't need the video downloader; pipeline::download asks for it otherwise.
    let yt_dlp = tools::find_yt_dlp(&bin);
    let ffmpeg = tools::find_ffmpeg(&bin).context(NO_FFMPEG)?;
    let tools = pipeline::Tools { yt_dlp: yt_dlp.as_ref(), ffmpeg: &ffmpeg };
    let app = ctx.app.clone();
    let audio =
        pipeline::download(&mut ctx.db, vod, &tools, &ctx.paths.work(), flag, reporter(&app, vod.id, progress))?;
    ctx.db.set_audio_path(vod.id, Some(&audio.to_string_lossy()))?;
    ctx.db.set_status(vod.id, Status::Ready, 0.0, None)?;
    Ok(())
}

fn transcribe(ctx: &mut Ctx, vod: &Vod, flag: &AtomicBool, progress: &mut f64) -> Result<()> {
    let audio = ctx.db.audio_path(vod.id)?.map(std::path::PathBuf::from).filter(|p| p.is_file());
    let Some(audio) = audio else {
        // The download went missing (e.g. the work dir was cleared): fetch it again.
        ctx.db.set_audio_path(vod.id, None)?;
        ctx.db.set_status(vod.id, Status::Queued, 0.0, None)?;
        return Ok(());
    };
    let ffmpeg = tools::find_ffmpeg(&ctx.paths.bin()).context(NO_FFMPEG)?;
    let (transcription, note) = plan_transcription(&ctx.paths, bundled_lib_dir(&ctx.app))?;
    ctx.db.set_note(vod.id, note.as_deref())?;
    ctx.emit_vod(vod.id);

    let app = ctx.app.clone();
    let mut report = reporter(&app, vod.id, progress);
    // The pipeline holds the stage's connection, so notices go through a second one.
    let notes = Db::open(&ctx.paths.db())?;
    pipeline::transcribe(&mut ctx.db, vod, &ffmpeg, &audio, &transcription, flag, &mut report, &mut |notice| {
        let _ = notes.set_note(vod.id, Some(&notice));
        let _ = crate::emit_vod(&app, &notes, vod.id);
    })?;
    pipeline::remove_work_files(&ctx.paths.work(), vod.id);
    ctx.db.set_audio_path(vod.id, None)?;
    Ok(())
}

/// The `lib` folder the installer ships: the no-tts sherpa-onnx library the GPU
/// worker links (the GPU add-on supplies only ONNX Runtime and the CUDA libraries).
fn bundled_lib_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().resource_dir().ok()?.join("lib");
    if !dir.is_dir() {
        eprintln!("bundled library folder missing: {}", dir.display());
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

    /// A library with one VOD in `status`, and a work dir holding its audio.
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
        db.set_status(id, status, 0.5, None).unwrap();
        let work = std::env::temp_dir().join(format!("vodscribe-finish-{}-{}", std::process::id(), next_id()));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join(format!("{id}.m4a")), b"audio").unwrap();
        db.set_audio_path(id, Some(&work.join(format!("{id}.m4a")).to_string_lossy())).unwrap();
        (db, Work(work), id)
    }

    fn next_id() -> u64 {
        use std::sync::atomic::AtomicU64;
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    fn status(db: &Db, id: i64) -> (Status, Option<String>) {
        let vod = db.vod(id).unwrap().unwrap();
        (vod.status, vod.error)
    }

    #[test]
    fn a_pause_keeps_the_work() {
        let (db, work, id) = setup(Status::Downloading);
        finish(Stage::Download, &db, &work, id, Err(vodscribe_core::Canceled.into()), Some(Stop::Pause), 0.4).unwrap();
        assert_eq!(status(&db, id).0, Status::Paused);
        assert_eq!(db.vod(id).unwrap().unwrap().progress, 0.4);
        assert!(work.join(format!("{id}.m4a")).exists());
    }

    #[test]
    fn a_cancel_discards_the_work_and_says_stopped() {
        let (db, work, id) = setup(Status::Transcribing);
        finish(Stage::Transcribe, &db, &work, id, Err(vodscribe_core::Canceled.into()), Some(Stop::Cancel), 0.4)
            .unwrap();
        assert_eq!(status(&db, id), (Status::Error, Some(STOPPED.into())));
        assert_eq!(db.audio_path(id).unwrap(), None);
        assert!(!work.join(format!("{id}.m4a")).exists());
    }

    #[test]
    fn a_delete_only_cleans_up_files() {
        let (db, work, id) = setup(Status::Downloading);
        finish(Stage::Download, &db, &work, id, Err(vodscribe_core::Canceled.into()), Some(Stop::Delete), 0.4).unwrap();
        assert!(!work.join(format!("{id}.m4a")).exists());
        // The command that deleted it removes the row; finish leaves it be.
        assert_eq!(status(&db, id).0, Status::Downloading);
    }

    #[test]
    fn a_transcript_that_finished_before_the_stop_is_kept() {
        let (db, work, id) = setup(Status::Done);
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
        db.delete_vod(id).unwrap();
        finish(Stage::Download, &db, &work, id, Err(anyhow::anyhow!("gone")), None, 0.3).unwrap();
        assert!(db.vod(id).unwrap().is_none());
    }
}
