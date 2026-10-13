//! Transcription runs in its own process so a GPU crash or out-of-memory
//! error only fails that one job, and so the GPU build can link the CUDA
//! libraries without the app depending on them.
//!
//! Usage: vodscribe-worker --model <dir> --provider cpu|cuda --ffmpeg <path>
//!                         [--batch <chunks per decode>] [--start <secs>] <audio>
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
    batch: usize,
}

fn parse_args() -> Result<Args> {
    let mut args = std::env::args().skip(1);
    let (mut model, mut provider, mut ffmpeg, mut audio, mut start, mut batch) =
        (None, Provider::Cpu, None, None, 0.0, 1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().map(PathBuf::from),
            "--ffmpeg" => ffmpeg = args.next().map(PathBuf::from),
            "--start" => start = args.next().context("--start needs a value")?.parse().context("--start")?,
            "--batch" => batch = args.next().context("--batch needs a value")?.parse().context("--batch")?,
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
        batch,
    })
}

fn emit(value: serde_json::Value) -> Result<()> {
    // A closed stdout means the app went away; the error ends the process.
    let mut out = std::io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()?;
    Ok(())
}

/// On Windows the GPU worker imports onnxruntime.dll by name, and Windows
/// searches System32 before PATH, where Windows 11 keeps an old copy of its
/// own. That copy has no CUDA provider, so the model would quietly run on the
/// CPU under a "graphics card" note. Fails instead, so the app falls back to
/// the CPU worker and says so.
#[cfg(windows)]
fn check_onnxruntime_is_the_pack_copy() -> Result<()> {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
        fn GetModuleFileNameW(module: *mut std::ffi::c_void, out: *mut u16, len: u32) -> u32;
    }
    let name: Vec<u16> = "onnxruntime.dll".encode_utf16().chain([0]).collect();
    let mut buf = vec![0u16; 32768];
    // SAFETY: a NUL-terminated name and a buffer of the length passed.
    let len = unsafe {
        let module = GetModuleHandleW(name.as_ptr());
        if module.is_null() {
            return Ok(());
        }
        GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32)
    };
    let path = std::ffi::OsString::from_wide(&buf[..len as usize]).to_string_lossy().to_lowercase();
    let windows = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_lowercase();
    if path.starts_with(&windows) {
        anyhow::bail!("Windows' own ONNX Runtime ({path}) was loaded instead of the GPU add-on's");
    }
    Ok(())
}

fn run() -> Result<()> {
    let args = parse_args()?;
    #[cfg(windows)]
    if args.provider == Provider::Cuda {
        check_onnxruntime_is_the_pack_copy()?;
    }
    let parallelism = match args.provider {
        Provider::Cpu => Parallelism { batch: args.batch, ..Parallelism::auto() },
        // One decode at a time keeps the GPU busy; extra CPU threads only feed it.
        Provider::Cuda => Parallelism { workers: 1, threads: 2, batch: args.batch },
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
