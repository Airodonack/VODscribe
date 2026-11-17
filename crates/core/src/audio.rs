use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};

use crate::tools;

pub const SAMPLE_RATE: usize = 16_000;

/// Decodes any media file to 16 kHz mono with ffmpeg, from `start` seconds
/// to `end` (None: its end), streaming samples to `on_samples` so a
/// multi-hour VOD never has to fit in memory.
pub fn decode(
    ffmpeg: &Path,
    input: &Path,
    start: f64,
    end: Option<f64>,
    cancel: &AtomicBool,
    mut on_samples: impl FnMut(&[f32]) -> Result<()>,
) -> Result<()> {
    let mut cmd = tools::command(ffmpeg);
    tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_AUDIO).args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
    // A start only when carrying on partway: "-ss 0" makes ffmpeg drop the
    // first second or two of some files (FLV, before its first keyframe).
    if start > 0.0 {
        cmd.args(["-ss", &format!("{start:.3}")]);
    }
    // As long as the stretch asked for, read from the input.
    if let Some(end) = end {
        cmd.args(["-t", &format!("{:.3}", (end - start).max(0.0))]);
    }
    cmd.arg("-i")
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "s16le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = tools::spawn(&mut cmd).context("start ffmpeg")?;
    let _tracked = child.track();

    let stderr_thread = tools::drain(child.stderr.take().unwrap());

    let mut stdout = child.stdout.take().unwrap();
    let mut buf = vec![0u8; 1 << 16];
    let mut pending: Option<u8> = None;
    let mut samples = Vec::with_capacity(buf.len() / 2 + 1);
    let result = (|| -> Result<()> {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::Canceled.into());
            }
            let n = stdout.read(&mut buf)?;
            if n == 0 {
                return Ok(());
            }
            samples.clear();
            let mut bytes = &buf[..n];
            if let Some(lo) = pending.take() {
                samples.push(i16::from_le_bytes([lo, bytes[0]]) as f32 / 32768.0);
                bytes = &bytes[1..];
            }
            let (pairs, rest) = bytes.as_chunks::<2>();
            samples.extend(pairs.iter().map(|p| i16::from_le_bytes(*p) as f32 / 32768.0));
            pending = rest.first().copied();
            on_samples(&samples)?;
        }
    })();

    if result.is_err() {
        child.kill_tree();
    }
    let status = child.wait()?;
    let err_text = stderr_thread.join().unwrap_or_default();
    result?;
    if !status.success() {
        bail!("ffmpeg failed: {}", err_text.trim());
    }
    Ok(())
}

pub struct Chunk {
    /// Offset of the first sample from the start of the audio, in seconds.
    pub start: f64,
    pub samples: Vec<f32>,
}

/// Splits a sample stream into chunks of `min..=max` seconds, cutting at the
/// quietest 20 ms frame in that window so words are rarely split in half.
pub struct Chunker {
    buf: Vec<f32>,
    consumed: usize,
    min: usize,
    max: usize,
}

const FRAME: usize = SAMPLE_RATE / 50;

impl Chunker {
    /// `offset` is the time of the first sample pushed, for audio decoded from partway in.
    pub fn new(min_secs: f64, max_secs: f64, offset: f64) -> Self {
        let min = (min_secs * SAMPLE_RATE as f64) as usize;
        let max = (max_secs * SAMPLE_RATE as f64) as usize;
        assert!(min + FRAME <= max);
        let consumed = (offset * SAMPLE_RATE as f64).round() as usize;
        Self { buf: Vec::with_capacity(max * 2), consumed, min, max }
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Chunk> {
        self.buf.extend_from_slice(samples);
        let mut out = Vec::new();
        while self.buf.len() >= self.max {
            let cut = self.quietest_cut();
            out.push(self.take(cut));
        }
        out
    }

    /// Flushes the remainder. Fragments under 0.1 s are dropped.
    pub fn finish(mut self) -> Option<Chunk> {
        let len = self.buf.len();
        (len >= SAMPLE_RATE / 10).then(|| self.take(len))
    }

    fn quietest_cut(&self) -> usize {
        let mut best = (f32::MAX, self.max);
        let mut at = self.min;
        while at + FRAME <= self.max {
            let energy: f32 = self.buf[at..at + FRAME].iter().map(|s| s * s).sum();
            if energy < best.0 {
                best = (energy, at + FRAME / 2);
            }
            at += FRAME;
        }
        best.1
    }

    fn take(&mut self, n: usize) -> Chunk {
        let samples: Vec<f32> = self.buf.drain(..n).collect();
        let start = self.consumed as f64 / SAMPLE_RATE as f64;
        self.consumed += n;
        Chunk { start, samples }
    }
}

#[cfg(test)]
mod tests;
