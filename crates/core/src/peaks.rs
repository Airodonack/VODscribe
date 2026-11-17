//! A VOD's loudness over time, drawn as a waveform under the player. Read
//! once from the audio while it's on this computer (VODs from sites with
//! their own players don't keep it after transcribing), and saved as
//! `<vod id>.peaks`: one byte per tenth of a second, so ten hours is 360 KB.
//! A stretch transcribed from a section of its own adds that section's
//! levels at their place (`merge`), with zeros where nothing was read.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::Result;

use crate::audio::{self, SAMPLE_RATE};

/// Levels per second of audio.
pub const RATE: usize = 10;

/// The quietest level drawn above zero, in dB below full scale.
const FLOOR_DB: f32 = 60.0;

/// One level per `1 / RATE` seconds of samples fed in.
#[derive(Default)]
struct Levels {
    out: Vec<u8>,
    sum: f64,
    count: usize,
}

impl Levels {
    const WINDOW: usize = SAMPLE_RATE / RATE;

    fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            self.sum += (s as f64) * (s as f64);
            self.count += 1;
            if self.count == Self::WINDOW {
                self.flush();
            }
        }
    }

    /// The window so far as one level: its loudness (RMS) on a log scale from
    /// `FLOOR_DB` below full scale (0) to full scale (255), as loudness is heard.
    fn flush(&mut self) {
        if self.count == 0 {
            return;
        }
        let rms = (self.sum / self.count as f64).sqrt() as f32;
        let db = 20.0 * rms.max(1e-9).log10();
        self.out.push(((db + FLOOR_DB) / FLOOR_DB * 255.0).clamp(0.0, 255.0).round() as u8);
        (self.sum, self.count) = (0.0, 0);
    }

    fn finish(mut self) -> Vec<u8> {
        self.flush();
        self.out
    }
}

/// Reads `input`'s levels with ffmpeg: none for a video without sound
/// (saved as such, so it isn't read again each time).
pub fn read(ffmpeg: &Path, input: &Path, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let streams = crate::local::codecs(ffmpeg, input)?;
    if streams.audio.is_none() && streams.video.is_some() {
        return Ok(Vec::new());
    }
    let mut levels = Levels::default();
    audio::decode(ffmpeg, input, 0.0, None, cancel, |samples| {
        levels.push(samples);
        Ok(())
    })?;
    Ok(levels.finish())
}

pub fn path(dir: &Path, vod_id: i64) -> PathBuf {
    dir.join(format!("{vod_id}.peaks"))
}

/// Saves VOD `vod_id`'s levels in `dir`. Written aside and renamed, so a
/// reader never sees half a file, even with two writers at once.
pub fn save(dir: &Path, vod_id: i64, levels: &[u8]) -> Result<()> {
    fs::create_dir_all(dir)?;
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let tag = format!("{}-{}", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed));
    let tmp = dir.join(format!("{vod_id}.peaks.{tag}.tmp"));
    fs::write(&tmp, levels)?;
    fs::rename(&tmp, path(dir, vod_id)).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    Ok(())
}

/// `existing` levels with `levels` written over them from `at` seconds,
/// zeros filling any gap between.
fn merged(mut existing: Vec<u8>, at: f64, levels: &[u8]) -> Vec<u8> {
    let from = (at.max(0.0) * RATE as f64).round() as usize;
    let to = from + levels.len();
    if existing.len() < to {
        existing.resize(to, 0);
    }
    existing[from..to].copy_from_slice(levels);
    existing
}

/// Saves `levels`, read from a section of VOD `vod_id` from `at` seconds,
/// at their place among its saved levels.
pub fn merge(dir: &Path, vod_id: i64, at: f64, levels: &[u8]) -> Result<()> {
    save(dir, vod_id, &merged(load(dir, vod_id).unwrap_or_default(), at, levels))
}

/// VOD `vod_id`'s saved levels, if any.
pub fn load(dir: &Path, vod_id: i64) -> Option<Vec<u8>> {
    fs::read(path(dir, vod_id)).ok()
}

pub fn remove(dir: &Path, vod_id: i64) {
    let _ = fs::remove_file(path(dir, vod_id));
}

/// Removes the files in `dir` of VODs not in `library` (deleted while a
/// download was writing one, say), and any left half-written.
pub fn tidy(dir: &Path, library: &[i64]) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = name.strip_suffix(".peaks").and_then(|id| id.parse::<i64>().ok());
        if id.is_none_or(|id| !library.contains(&id)) && entry.path().is_file() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests;
