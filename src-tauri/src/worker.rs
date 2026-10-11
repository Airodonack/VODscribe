//! The background queue, in two stages on their own threads: one downloads
//! while the other transcribes, so a VOD can download while another is
//! transcribed. Each stage runs one VOD at a time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tauri::{AppHandle, Emitter};
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

#[derive(Clone, Copy)]
enum Stage {
    Download,
    Transcribe,
}

pub struct Worker {
    wakes: Mutex<[Sender<()>; 2]>,
    jobs: Arc<Mutex<Vec<Job>>>,
}

impl Worker {
    pub fn start(app: AppHandle, paths: Paths) -> Result<Self> {
        Db::open(&paths.db())?.requeue_interrupted()?;
        let jobs = Arc::new(Mutex::new(Vec::new()));
        let (dl_tx, dl_rx) = mpsc::channel();
        let (tr_tx, tr_rx) = mpsc::channel();
        for (stage, rx) in [(Stage::Download, dl_rx), (Stage::Transcribe, tr_rx)] {
            let ctx = Ctx {
                app: app.clone(),
                db: Db::open(&paths.db())?,
                paths: paths.clone(),
                jobs: jobs.clone(),
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
        Ok(Self { wakes: Mutex::new([dl_tx, tr_tx]), jobs })
    }

    /// Tells both stages to look for work.
    pub fn wake(&self) {
        for tx in self.wakes.lock().unwrap().iter() {
            let _ = tx.send(());
        }
    }

    /// Stops the VOD if a stage is running it. Returns whether one was; the
    /// stage then finishes the stop according to `reason`.
    pub fn stop(&self, vod_id: i64, reason: Stop) -> bool {
        let jobs = self.jobs.lock().unwrap();
        let Some(job) = jobs.iter().find(|j| j.vod_id == vod_id) else { return false };
        *job.reason.lock().unwrap() = Some(reason);
        job.flag.store(true, Ordering::Relaxed);
        true
    }
}

struct Ctx {
    app: AppHandle,
    db: Db,
    paths: Paths,
    jobs: Arc<Mutex<Vec<Job>>>,
    wake_other: Sender<()>,
}

impl Ctx {
    fn emit_vod(&self, id: i64) {
        if let Ok(Some(vod)) = self.db.vod(id) {
            let _ = self.app.emit("vod", vod);
        }
    }
}

fn run(stage: Stage, mut ctx: Ctx, rx: Receiver<()>) {
    loop {
        let next = next_job(stage, &ctx).unwrap_or_else(|e| {
            eprintln!("queue: {e:#}");
            None
        });
        let Some(vod) = next else {
            // Sleep until something is queued, a stage finishes, or setup completes.
            if rx.recv().is_err() {
                return;
            }
            continue;
        };

        let flag = Arc::new(AtomicBool::new(false));
        let reason = Arc::new(Mutex::new(None));
        ctx.jobs.lock().unwrap().push(Job { vod_id: vod.id, flag: flag.clone(), reason: reason.clone() });
        let mut progress = 0.0;
        let result = match stage {
            Stage::Download => download(&mut ctx, &vod, &flag, &mut progress),
            Stage::Transcribe => transcribe(&mut ctx, &vod, &flag, &mut progress),
        };
        ctx.jobs.lock().unwrap().retain(|j| j.vod_id != vod.id);

        let stop = *reason.lock().unwrap();
        if let Err(e) = finish(&ctx, &vod, result, stop, progress) {
            eprintln!("queue: {e:#}");
        }
        ctx.emit_vod(vod.id);
        let _ = ctx.wake_other.send(());
    }
}

fn next_job(stage: Stage, ctx: &Ctx) -> Result<Option<Vod>> {
    let bin = ctx.paths.bin();
    if tools::find_yt_dlp(&bin).is_none() || tools::find_ffmpeg(&bin).is_none() {
        return Ok(None);
    }
    match stage {
        Stage::Download => {
            if ctx.db.count_with_status(Status::Ready)? >= MAX_READY {
                return Ok(None);
            }
            ctx.db.next_with_status(Status::Queued)
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
fn finish(ctx: &Ctx, vod: &Vod, result: Result<()>, stop: Option<Stop>, progress: f64) -> Result<()> {
    let work = ctx.paths.work();
    match (result, stop) {
        (_, Some(Stop::Delete)) => pipeline::remove_work_files(&work, vod.id),
        (_, Some(Stop::Pause)) => ctx.db.set_status(vod.id, Status::Paused, progress, None)?,
        (_, Some(Stop::Cancel)) => discard(&ctx.db, &work, vod.id)?,
        (Ok(()), None) => {}
        (Err(e), None) => {
            // The VOD may have been deleted while running; ignore that case.
            let _ = ctx.db.set_status(vod.id, Status::Error, 0.0, Some(&format!("{e:#}")));
        }
    }
    Ok(())
}

/// Throws away a VOD's partial work, leaving it in the library as canceled.
pub fn discard(db: &Db, work_dir: &std::path::Path, vod_id: i64) -> Result<()> {
    pipeline::remove_work_files(work_dir, vod_id);
    db.clear_segments(vod_id)?;
    db.set_audio_path(vod_id, None)?;
    db.set_note(vod_id, None)?;
    db.set_status(vod_id, Status::Error, 0.0, Some("Canceled"))
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
    let yt_dlp = tools::find_yt_dlp(&bin).context("yt-dlp is missing")?;
    let ffmpeg = tools::find_ffmpeg(&bin).context("ffmpeg is missing")?;
    let tools = pipeline::Tools { yt_dlp: &yt_dlp, ffmpeg: &ffmpeg };
    let app = ctx.app.clone();
    let audio = pipeline::download(&mut ctx.db, vod, &tools, &ctx.paths.work(), flag, reporter(&app, vod.id, progress))?;
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
    let ffmpeg = tools::find_ffmpeg(&ctx.paths.bin()).context("ffmpeg is missing")?;
    let (transcription, note) = plan_transcription(&ctx.paths)?;
    ctx.db.set_note(vod.id, note.as_deref())?;
    ctx.emit_vod(vod.id);

    let (app, paths) = (ctx.app.clone(), ctx.paths.clone());
    let mut report = reporter(&app, vod.id, progress);
    pipeline::transcribe(&mut ctx.db, vod, &ffmpeg, &audio, &transcription, flag, &mut report, &mut |notice| {
        let _ = db_note(&paths, vod.id, &notice);
        if let Ok(Some(v)) = Db::open(&paths.db()).and_then(|d| d.vod(vod.id)) {
            let _ = app.emit("vod", v);
        }
    })?;
    pipeline::remove_work_files(&ctx.paths.work(), vod.id);
    ctx.db.set_audio_path(vod.id, None)?;
    Ok(())
}

/// Writes a note through a separate connection, since the pipeline holds the worker's.
fn db_note(paths: &Paths, vod_id: i64, note: &str) -> Result<()> {
    Db::open(&paths.db())?.set_note(vod_id, Some(note))
}

/// Picks the worker for the next job. GPU is used only when selected, installed
/// and free enough; otherwise the CPU runs it and the note says why.
fn plan_transcription(paths: &Paths) -> Result<(Transcription, Option<String>)> {
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
    let skip = |why: &str| Ok(cpu_only(Some(format!("Transcribed on the CPU: {why}."))));
    if !gpu::is_installed(&pack) || !model::is_installed(&paths.gpu_model_dir(), &model::GPU_MODEL) {
        return skip("the GPU pack isn't installed");
    }
    let Ok(program) = tools::sidecar("vodscribe-worker-cuda") else {
        return skip("the GPU worker is missing from this build");
    };
    let Some(info) = gpu::detect() else {
        return skip("no NVIDIA GPU was found");
    };
    let Some(batch) = gpu::batch_for_free_vram(info.free_mb) else {
        return skip(&format!("the GPU was busy ({} MB free, {} MB needed)", info.free_mb, gpu::MIN_FREE_MB));
    };
    let cuda = WorkerSpec {
        program,
        model_id: model::GPU_MODEL.id,
        model_dir: paths.gpu_model_dir(),
        provider: "cuda",
        batch,
        lib_dirs: vec![gpu::lib_dir(&pack)],
    };
    Ok((Transcription { worker: cuda, fallback: Some(cpu) }, Some(format!("Transcribed on {}.", info.name))))
}
