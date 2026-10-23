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
    let mut child = cmd
        .arg("-i")
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "s16le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start ffmpeg")?;
    let _tracked = tools::track(&child);

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
        tools::kill_tree(&mut child);
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
mod tests {
    use super::*;

    /// Every kind of file the app reads decodes through ffmpeg's allowlists
    /// (`tools::ffmpeg_allow`), and a container outside them doesn't. Made with
    /// ffmpeg; skipped where ffmpeg isn't installed.
    #[test]
    fn decodes_what_the_allowlists_allow() {
        let Ok(ffmpeg) = which::which("ffmpeg") else {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        };
        let dir = crate::TempDir::new("allowlist");
        let make = |name: &str, args: &[&str]| {
            let path = dir.join(name);
            let ok = std::process::Command::new(&ffmpeg)
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=3"])
                .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-shortest"])
                .args(args)
                .arg(&path)
                .status()
                .is_ok_and(|s| s.success());
            ok.then_some(path)
        };
        let samples: &[(&str, &[&str])] = &[
            ("a.mp4", &["-c:v", "libx264", "-c:a", "aac"]),
            ("b.mkv", &["-c:v", "libx264", "-c:a", "libopus"]),
            ("c.webm", &["-c:v", "libvpx-vp9", "-c:a", "libopus"]),
            ("d.avi", &["-c:v", "mpeg4", "-c:a", "libmp3lame"]),
            ("e.ts", &["-c:v", "libx264", "-c:a", "aac"]),
            ("f.flv", &["-c:v", "libx264", "-c:a", "aac"]),
            ("g.mov", &["-c:v", "libx264", "-c:a", "aac"]),
            ("h.mp3", &["-vn"]),
            ("i.m4a", &["-vn", "-c:a", "aac"]),
            ("j.wav", &["-vn"]),
            ("k.flac", &["-vn"]),
            ("l.ogg", &["-vn", "-c:a", "libvorbis"]),
            ("m.opus", &["-vn", "-c:a", "libopus"]),
        ];
        let never = AtomicBool::new(false);
        let mut tried = 0;
        for (name, args) in samples {
            // An ffmpeg without that encoder can't make the sample.
            let Some(path) = make(name, args) else { continue };
            let mut samples = 0;
            decode(&ffmpeg, &path, 0.0, None, &never, |s| {
                samples += s.len();
                Ok(())
            })
            .unwrap_or_else(|e| panic!("{name}: {e:#}"));
            // All of it: a file that loses its start (FLV with "-ss 0") falls short.
            assert!(samples > SAMPLE_RATE * 28 / 10, "{name}: {samples} samples of 3 s");
            tried += 1;
        }
        assert!(tried >= 8, "only {tried} samples could be made");
        // NUT, a container ffmpeg reads but VODscribe doesn't take: refused.
        let nut = make("n.nut", &["-c:v", "mpeg4", "-c:a", "pcm_s16le"]).expect("ffmpeg can make NUT");
        assert!(decode(&ffmpeg, &nut, 0.0, None, &never, |_| Ok(())).is_err());
    }

    /// From a start to an end: that second of a three-second tone and no
    /// more. Made with ffmpeg; skipped where ffmpeg isn't installed.
    #[test]
    fn decodes_a_stretch_only() {
        let Ok(ffmpeg) = which::which("ffmpeg") else {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        };
        let dir = crate::TempDir::new("decode-end");
        let path = dir.join("tone.m4a");
        let made = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-c:a", "aac"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        let mut samples = 0;
        decode(&ffmpeg, &path, 1.0, Some(2.0), &AtomicBool::new(false), |s| {
            samples += s.len();
            Ok(())
        })
        .unwrap();
        let secs = samples as f64 / SAMPLE_RATE as f64;
        assert!((secs - 1.0).abs() < 0.05, "{secs} s");
    }

    #[test]
    fn chunker_offsets_resumed_audio() {
        let mut chunker = Chunker::new(2.0, 3.0, 100.0);
        let chunks = chunker.push(&vec![0.1; 4 * SAMPLE_RATE]);
        assert_eq!(chunks[0].start, 100.0);
    }

    #[test]
    fn chunker_cuts_at_silence_and_keeps_offsets() {
        // 1 s of tone, with silence at 2.5 s, over 6 s total.
        let mut audio: Vec<f32> = (0..6 * SAMPLE_RATE).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let silent = (2.5 * SAMPLE_RATE as f64) as usize;
        audio[silent..silent + FRAME * 2].fill(0.0);

        let mut chunker = Chunker::new(2.0, 3.0, 0.0);
        let mut chunks = Vec::new();
        for block in audio.chunks(1000) {
            chunks.extend(chunker.push(block));
        }
        chunks.extend(chunker.finish());

        assert_eq!(chunks[0].start, 0.0);
        let first_len = chunks[0].samples.len() as f64 / SAMPLE_RATE as f64;
        assert!((first_len - 2.51).abs() < 0.02, "cut at {first_len}");
        let total: usize = chunks.iter().map(|c| c.samples.len()).sum();
        assert_eq!(total, audio.len());
        for pair in chunks.windows(2) {
            let end = pair[0].start + pair[0].samples.len() as f64 / SAMPLE_RATE as f64;
            assert!((pair[1].start - end).abs() < 1e-9);
        }
    }
}
