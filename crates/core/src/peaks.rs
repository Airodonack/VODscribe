//! A VOD's loudness over time, drawn as a waveform under the player. Read
//! once from the audio while it's on this computer (VODs from sites with
//! their own players don't keep it after transcribing), and saved as
//! `<vod id>.peaks`: one byte per tenth of a second, so ten hours is 360 KB.

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
mod tests {
    use super::*;

    #[test]
    fn levels_are_loudness_per_tenth_of_a_second() {
        let mut levels = Levels::default();
        // Silence, then a full-scale square wave, then a 60 dB quieter one,
        // fed in uneven pieces, then half a window.
        let mut samples = vec![0.0; Levels::WINDOW];
        samples.extend((0..Levels::WINDOW).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }));
        samples.extend((0..Levels::WINDOW).map(|i| if i % 2 == 0 { 0.001 } else { -0.001 }));
        samples.extend((0..Levels::WINDOW / 2).map(|_| 0.1));
        for piece in samples.chunks(777) {
            levels.push(piece);
        }
        assert_eq!(levels.finish(), [0, 255, 0, 170], "-20 dB is two thirds of the way up");
    }

    #[test]
    fn saves_and_tidies() {
        let dir = crate::TempDir::new("peaks");
        save(&dir, 3, &[1, 2, 3]).unwrap();
        save(&dir, 4, &[4]).unwrap();
        save(&dir, 3, &[5, 6]).unwrap();
        assert_eq!(load(&dir, 3).unwrap(), [5, 6]);
        fs::write(dir.join("4.peaks.123.tmp"), b"half").unwrap();
        tidy(&dir, &[3]);
        let mut left: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        left.sort();
        assert_eq!(left, ["3.peaks"]);
        remove(&dir, 3);
        assert!(load(&dir, 3).is_none());
    }

    /// Ten levels a second from a real file. Skipped where ffmpeg isn't installed.
    #[test]
    fn reads_a_file() {
        let Ok(ffmpeg) = which::which("ffmpeg") else {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        };
        let dir = crate::TempDir::new("peaks-read");
        let file = dir.join("a.m4a");
        // Two seconds of tone, then a second of silence.
        let ok = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
            .args(["-af", "apad=pad_dur=1", "-c:a", "aac"])
            .arg(&file)
            .status()
            .unwrap();
        assert!(ok.success());
        let levels = read(&ffmpeg, &file, &AtomicBool::new(false)).unwrap();
        assert!((29..=31).contains(&levels.len()), "{}", levels.len());
        assert!(levels[5..15].iter().all(|&l| l > 150), "{levels:?}");
        assert!(levels[24..28].iter().all(|&l| l < 20), "{levels:?}");

        // A video without sound has no levels, rather than failing.
        let silent = dir.join("silent.mkv");
        let ok = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=black:s=64x36:d=1", "-c:v", "png"])
            .arg(&silent)
            .status()
            .unwrap();
        assert!(ok.success());
        assert_eq!(read(&ffmpeg, &silent, &AtomicBool::new(false)).unwrap(), Vec::<u8>::new());
        // A file that isn't there still fails.
        assert!(read(&ffmpeg, &dir.join("gone.mkv"), &AtomicBool::new(false)).is_err());
    }
}
