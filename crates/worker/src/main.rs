//! Transcription runs in its own process so a GPU crash or out-of-memory
//! error only fails that one job, and so the GPU build can link the CUDA
//! libraries without the app depending on them.
//!
//! Usage: vodscribe-worker --model <dir> --provider cpu|cuda --ffmpeg <path> [--start <secs>] <audio>
//!
//! Prints JSON lines to stdout: `{"ready":true}` once the model has loaded,
//! then `{"end":<secs>,"words":[...]}` per batch of audio. Exits non-zero on error.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};
use serde_json::json;
use vodscribe_core::asr::{Parallelism, Provider, Transcriber};
use vodscribe_core::audio::{self, Chunk, Chunker, SAMPLE_RATE};

const CHUNK_MIN_SECS: f64 = 20.0;
const CHUNK_MAX_SECS: f64 = 30.0;

struct Args {
    model: PathBuf,
    provider: Provider,
    ffmpeg: PathBuf,
    audio: PathBuf,
    /// Resume point, in seconds.
    start: f64,
}

fn parse_args() -> Result<Args> {
    let mut args = std::env::args().skip(1);
    let (mut model, mut provider, mut ffmpeg, mut audio, mut start) = (None, Provider::Cpu, None, None, 0.0);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().map(PathBuf::from),
            "--ffmpeg" => ffmpeg = args.next().map(PathBuf::from),
            "--start" => start = args.next().context("--start needs a value")?.parse().context("--start")?,
            "--provider" => {
                provider = match args.next().as_deref() {
                    Some("cpu") => Provider::Cpu,
                    Some("cuda") => Provider::Cuda,
                    other => bail!("unknown provider {other:?}"),
                }
            }
            _ if audio.is_none() && !a.starts_with("--") => audio = Some(PathBuf::from(a)),
            _ => bail!("unexpected argument {a}"),
        }
    }
    Ok(Args {
        model: model.context("--model is required")?,
        provider,
        ffmpeg: ffmpeg.context("--ffmpeg is required")?,
        audio: audio.context("an audio file is required")?,
        start,
    })
}

fn emit(value: serde_json::Value) -> Result<()> {
    // A closed stdout means the app went away; the error ends the process.
    let mut out = std::io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()?;
    Ok(())
}

fn run() -> Result<()> {
    let args = parse_args()?;
    let parallelism = match args.provider {
        Provider::Cpu => Parallelism::auto(),
        // One decode keeps the GPU busy; extra CPU threads only feed it.
        Provider::Cuda => Parallelism { workers: 1, threads: 2 },
    };
    let transcriber = Transcriber::load(&args.model, args.provider, parallelism)?;
    emit(json!({ "ready": true }))?;

    let never = AtomicBool::new(false);
    let mut chunker = Chunker::new(CHUNK_MIN_SECS, CHUNK_MAX_SECS, args.start);
    let mut pending: Vec<Chunk> = Vec::new();
    let flush = |chunks: &mut Vec<Chunk>| -> Result<()> {
        let Some(last) = chunks.last() else { return Ok(()) };
        let end = last.start + last.samples.len() as f64 / SAMPLE_RATE as f64;
        let words = transcriber.transcribe(chunks);
        chunks.clear();
        emit(json!({ "end": end, "words": words }))
    };
    audio::decode(&args.ffmpeg, &args.audio, args.start, &never, |samples| {
        pending.extend(chunker.push(samples));
        if pending.len() >= transcriber.batch_size() {
            flush(&mut pending)?;
        }
        Ok(())
    })?;
    pending.extend(chunker.finish());
    flush(&mut pending)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
