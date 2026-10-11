use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::db::{Db, Status, Vod};
use crate::fetch;
use crate::tools::{self, YtDlp};
use crate::transcript::{Segmenter, Word};

/// Stream URLs in a saved info JSON expire (YouTube's after about 6 hours).
const INFO_MAX_AGE: Duration = Duration::from_secs(30 * 60);

pub struct Tools<'a> {
    pub yt_dlp: &'a YtDlp,
    pub ffmpeg: &'a Path,
}

/// How to launch a `vodscribe-worker` process.
#[derive(Clone, Debug)]
pub struct WorkerSpec {
    pub program: PathBuf,
    /// Recorded on the VOD, so transcripts from older models can be flagged.
    pub model_id: &'static str,
    pub model_dir: PathBuf,
    /// "cpu" or "cuda".
    pub provider: &'static str,
    /// Chunks per decode call; see `asr::Parallelism::batch`.
    pub batch: usize,
    /// Extra directories for the dynamic linker (the GPU pack's libraries).
    pub lib_dirs: Vec<PathBuf>,
}

pub struct Transcription {
    pub worker: WorkerSpec,
    /// Used if `worker` fails, e.g. the GPU runs out of memory.
    pub fallback: Option<WorkerSpec>,
}

/// Something worth telling the user that isn't an error.
pub type Notice<'a> = &'a mut dyn FnMut(String);

pub fn info_path(work_dir: &Path, vod_id: i64) -> PathBuf {
    work_dir.join(format!("{vod_id}.info.json"))
}

/// Downloads a VOD's audio, resuming a partial download. Returns its path.
pub fn download(
    db: &mut Db,
    vod: &Vod,
    tools: &Tools,
    work_dir: &Path,
    cancel: &AtomicBool,
    mut report: impl FnMut(Status, f64),
) -> Result<PathBuf> {
    fs::create_dir_all(work_dir)?;
    let info = info_path(work_dir, vod.id);
    let fresh = fs::metadata(&info)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < INFO_MAX_AGE);

    db.set_status(vod.id, Status::Downloading, 0.0, None)?;
    report(Status::Downloading, 0.0);
    if !fresh {
        fetch::probe(tools.yt_dlp, &vod.info.url, &info)?;
    }
    fetch::download_audio(tools.yt_dlp, &info, &work_dir.join(vod.id.to_string()), cancel, |f| {
        report(Status::Downloading, f)
    })
}

/// Transcribes downloaded audio, writing segments to the index as they are
/// produced so partial results are searchable during a long run. Picks up
/// after the last indexed segment, so a paused or interrupted job resumes.
#[allow(clippy::too_many_arguments)]
pub fn transcribe(
    db: &mut Db,
    vod: &Vod,
    ffmpeg: &Path,
    audio_path: &Path,
    transcription: &Transcription,
    cancel: &AtomicBool,
    report: &mut impl FnMut(Status, f64),
    notice: Notice,
) -> Result<()> {
    let attempt = |spec: &WorkerSpec, db: &mut Db, report: &mut dyn FnMut(Status, f64)| {
        run_worker(db, vod, spec, ffmpeg, audio_path, cancel, report)
    };
    // A paused or interrupted job resumes after its last segment, but never on
    // top of segments from an older model (or from before models were recorded).
    if db.transcribed_until(vod.id)? > 0.0 && !db.model(vod.id)?.is_some_and(|m| crate::model::is_current(&m)) {
        db.clear_segments(vod.id)?;
    }
    let mut used = &transcription.worker;
    db.set_model(vod.id, Some(used.model_id))?;
    if let Err(e) = attempt(used, db, report) {
        let Some(fallback) = &transcription.fallback else { return Err(e) };
        if cancel.load(Ordering::Relaxed) {
            return Err(e);
        }
        notice(format!("GPU transcription failed, so the rest of this VOD was transcribed on the CPU: {e:#}"));
        used = fallback;
        db.set_model(vod.id, Some(used.model_id))?;
        attempt(used, db, report)?;
    }
    db.set_status(vod.id, Status::Done, 1.0, None)?;
    report(Status::Done, 1.0);
    Ok(())
}

/// Deletes everything in `work_dir` belonging to a VOD: its info JSON, the
/// audio, and any partial download fragments.
pub fn remove_work_files(work_dir: &Path, vod_id: i64) {
    let prefix = format!("{vod_id}.");
    let Ok(entries) = fs::read_dir(work_dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[derive(Deserialize)]
struct WorkerLine {
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    words: Vec<Word>,
}

/// Runs one worker over the whole file, indexing segments as they stream in.
#[allow(clippy::too_many_arguments)]
fn run_worker(
    db: &mut Db,
    vod: &Vod,
    spec: &WorkerSpec,
    ffmpeg: &Path,
    audio_path: &Path,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Status, f64),
) -> Result<()> {
    let duration = vod.info.duration.filter(|d| *d > 0.0);
    let fraction = |t: f64| duration.map_or(0.0, |d| (t / d).min(1.0));
    let start = db.transcribed_until(vod.id)?;
    db.set_status(vod.id, Status::Transcribing, fraction(start), None)?;
    report(Status::Transcribing, fraction(start));

    let mut cmd = tools::command(&spec.program);
    cmd.arg("--model").arg(&spec.model_dir).args(["--provider", spec.provider]);
    cmd.args(["--batch", &spec.batch.to_string()]);
    cmd.arg("--ffmpeg").arg(ffmpeg).args(["--start", &format!("{start:.3}")]).arg(audio_path);
    if !spec.lib_dirs.is_empty() {
        let var = if cfg!(windows) { "PATH" } else { "LD_LIBRARY_PATH" };
        let existing = std::env::var_os(var).unwrap_or_default();
        let paths = spec.lib_dirs.iter().cloned().chain(std::env::split_paths(&existing));
        cmd.env(var, std::env::join_paths(paths)?);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("start {}", spec.program.display()))?;

    // sherpa-onnx logs to stderr; keep the tail for error messages.
    let mut stderr = child.stderr.take().unwrap();
    let stderr_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let mut segmenter = Segmenter::new();
    let result = (|| -> Result<()> {
        for line in BufReader::new(child.stdout.take().unwrap()).lines() {
            if cancel.load(Ordering::Relaxed) {
                bail!("canceled");
            }
            let line: WorkerLine = serde_json::from_str(&line?).context("unreadable worker output")?;
            if let Some(end) = line.end {
                db.insert_segments(vod.id, &segmenter.push(line.words))?;
                report(Status::Transcribing, fraction(end));
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let stderr = stderr_thread.join().unwrap_or_default();
    result?;
    if !status.success() {
        let tail: Vec<&str> = stderr.lines().rev().filter(|l| !l.trim().is_empty()).take(3).collect();
        let msg = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        bail!("{}", if msg.is_empty() { format!("worker exited with {status}") } else { msg });
    }
    if let Some(last) = segmenter.finish() {
        db.insert_segments(vod.id, &[last])?;
    }
    Ok(())
}
