//! Decoding, by the ffmpeg the app ships: one process per stream, writing
//! raw frames or samples down a pipe, read on a thread of its own into a
//! short queue. A seek is a new process (ffmpeg's `-ss` before the input
//! decodes from the keyframe before and drops what's before the time asked,
//! so the first frame out is the one showing there). Frames come at the
//! file's own rate, each stamped with its time in the file; nothing here
//! knows about edits or the clock.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;

use anyhow::{Context, Result, bail};
use vodscribe_core::local::{self, Picture};
use vodscribe_core::tools;

/// A file's picture, sound and length, as ffmpeg reads it.
#[derive(Debug, Clone)]
pub struct Info {
    pub picture: Option<Picture>,
    pub audio: bool,
    pub duration: Option<f64>,
}

/// What `path` holds, with `ffmpeg`.
pub fn probe(ffmpeg: &Path, path: &Path) -> Result<Info> {
    let media = local::media(ffmpeg, path)?;
    if media.picture.is_none() && !media.audio {
        bail!("{} has no picture and no sound.", tools::shown(path));
    }
    Ok(Info { picture: media.picture, audio: media.audio, duration: media.duration })
}

/// One decoded picture: `width` by `height` pixels of packed 8-bit RGB.
pub struct Frame {
    /// Its own number, unlike any other frame's this run: what's drawn is told apart by it.
    pub id: u64,
    /// Seconds into the file at which it shows.
    pub pts: f64,
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Samples: interleaved stereo 16-bit, from `pts` (seconds into the file) on.
pub struct Samples {
    pub pts: f64,
    pub pcm: Vec<i16>,
}

pub const CHANNELS: usize = 2;

/// Frames kept decoded ahead of being taken.
const AHEAD: usize = 4;

/// Frame numbers given out (`Frame::id`).
static FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A stream of frames or samples from one ffmpeg process.
pub struct Stream<T> {
    child: Child,
    frames: Receiver<T>,
    _reader: JoinHandle<()>,
    /// Stopped with the app (`tools::stop_tracked`); untracked once `child` is waited for.
    _tracked: tools::Tracked,
}

impl<T: Send + 'static> Stream<T> {
    /// The next frame out, or None once the file (or ffmpeg) ends.
    pub fn recv(&mut self) -> Option<T> {
        self.frames.recv().ok()
    }

    /// The next frame if one is decoded already.
    pub fn try_recv(&mut self) -> Option<T> {
        self.frames.try_recv().ok()
    }
}

impl<T> Drop for Stream<T> {
    fn drop(&mut self) {
        tools::kill_tree(&mut self.child);
    }
}

/// ffmpeg reading `path` from `from` seconds, the stream `map`ped, writing
/// to its stdout as `output` says; `read` turns what comes down the pipe
/// into frames until it ends.
fn start<T: Send + 'static>(
    ffmpeg: &Path,
    path: &Path,
    from: f64,
    map: &str,
    output: &[&str],
    read: impl FnOnce(std::process::ChildStdout, SyncSender<T>) + Send + 'static,
) -> Result<Stream<T>> {
    let mut cmd = tools::command(ffmpeg);
    let mut child =
        tools::ffmpeg_allow_for(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE), path)
            .args(["-hide_banner", "-nostdin", "-loglevel", "error"])
            // Rotated phone videos are read as stored: the frame size stays the probe's.
            .args(["-noautorotate", "-ss", &format!("{:.6}", from.max(0.0)), "-i"])
            .arg(path)
            .args(["-map", map])
            .args(output)
            .arg("pipe:1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("run {}", tools::shown(ffmpeg)))?;
    let stdout = child.stdout.take().context("ffmpeg's output")?;
    let (tx, frames) = mpsc::sync_channel(AHEAD);
    let reader = std::thread::Builder::new().name("decode".into()).spawn(move || read(stdout, tx))?;
    let tracked = tools::track(&child);
    Ok(Stream { child, frames, _reader: reader, _tracked: tracked })
}

/// The largest frame side the player decodes to: a GPU texture can be no
/// larger than 8192 (wgpu's default limit), and a frame 4096 across is
/// already more than a screen shows.
pub const MAX_SIDE: u32 = 4096;

/// `width` by `height` scaled down, keeping its shape, so neither side is
/// over `max` (and both are even, for ffmpeg's scaler); as it is if neither is.
pub fn within(width: u32, height: u32, max: u32) -> (u32, u32) {
    if width <= max && height <= max {
        return (width, height);
    }
    let k = max as f64 / width.max(height) as f64;
    let side = |n: u32| (((n as f64 * k / 2.0).round() as u32) * 2).clamp(2, max);
    (side(width), side(height))
}

/// A file's frames, decoded by `ffmpeg`.
pub struct Video {
    ffmpeg: PathBuf,
    path: PathBuf,
    picture: Picture,
}

impl Video {
    pub fn new(ffmpeg: &Path, path: &Path, picture: Picture) -> Video {
        Video { ffmpeg: ffmpeg.into(), path: path.into(), picture }
    }

    pub fn picture(&self) -> Picture {
        self.picture
    }

    /// The frame showing at `from` seconds and every one after, at the
    /// file's rate (`Picture::fps`). ffmpeg is told that rate: left to
    /// itself it makes a variable-rate file constant at the stream's nominal
    /// rate, which can differ from the average the probe reads, and the
    /// frames would be stamped at the wrong speed.
    /// Frames are scaled to the probe's size, or down to `MAX_SIDE` where
    /// that's larger, so each is the size read (even where the stream's
    /// size changes partway).
    pub fn from(&self, from: f64) -> Result<Stream<Arc<Frame>>> {
        let Picture { width, height, fps } = self.picture;
        let (width, height) = within(width, height, MAX_SIDE);
        let rate = format!("{fps}");
        let scale = format!("scale={width}:{height}");
        let output = ["-vf", &scale, "-fps_mode", "cfr", "-r", &rate, "-f", "rawvideo", "-pix_fmt", "rgb24"];
        start(&self.ffmpeg, &self.path, from, "0:v:0", &output, move |mut out, tx| {
            let size = width as usize * height as usize * 3;
            let mut n = 0u64;
            loop {
                let mut rgb = vec![0u8; size];
                if out.read_exact(&mut rgb).is_err() {
                    return;
                }
                let id = FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let frame = Frame { id, pts: from + n as f64 / fps, width, height, rgb };
                if tx.send(Arc::new(frame)).is_err() {
                    return;
                }
                n += 1;
            }
        })
    }
}

/// A file's sound, decoded by `ffmpeg`.
pub struct Audio {
    ffmpeg: PathBuf,
    path: PathBuf,
}

/// Samples in one chunk.
const CHUNK: usize = 1024;

/// ffmpeg's filter playing sound `speed` times as fast: atempo takes 0.5 to
/// 100, so slower is two or more halvings and the rest.
fn tempo(speed: f64) -> String {
    let mut parts = Vec::new();
    let mut left = speed;
    while left < 0.5 {
        parts.push("atempo=0.5".to_string());
        left /= 0.5;
    }
    parts.push(format!("atempo={left}"));
    parts.join(",")
}

impl Audio {
    pub fn new(ffmpeg: &Path, path: &Path) -> Audio {
        Audio { ffmpeg: ffmpeg.into(), path: path.into() }
    }

    /// The sound from `from` seconds on, in chunks, at `rate` samples a
    /// second, played `speed` times as fast at the same pitch (ffmpeg's
    /// atempo): each second of it is `speed` seconds of the file.
    pub fn from(&self, from: f64, rate: u32, speed: f64) -> Result<Stream<Samples>> {
        let rate_ = rate.to_string();
        let tempo = tempo(speed);
        let mut output = vec!["-f", "s16le", "-ac", "2", "-ar", &rate_];
        if speed != 1.0 {
            output.extend(["-af", &tempo]);
        }
        start(&self.ffmpeg, &self.path, from, "0:a:0", &output, move |mut out, tx| {
            let mut done = 0u64;
            loop {
                let mut bytes = vec![0u8; CHUNK * CHANNELS * 2];
                let mut got = 0;
                while got < bytes.len() {
                    match out.read(&mut bytes[got..]) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => got += n,
                    }
                }
                let got = got / (CHANNELS * 2) * (CHANNELS * 2);
                if got == 0 {
                    return;
                }
                let pcm: Vec<i16> = bytes[..got].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
                let samples = Samples { pts: from + done as f64 / rate as f64 * speed, pcm };
                done += (got / (CHANNELS * 2)) as u64;
                if tx.send(samples).is_err() {
                    return;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_speeds_chain_atempo_within_its_range() {
        assert_eq!(tempo(2.0), "atempo=2");
        assert_eq!(tempo(0.5), "atempo=0.5");
        assert_eq!(tempo(0.25), "atempo=0.5,atempo=0.5");
        assert_eq!(tempo(0.3), "atempo=0.5,atempo=0.6");
    }

    #[test]
    fn frames_too_large_are_scaled_down_keeping_their_shape() {
        assert_eq!(within(1920, 1080, MAX_SIDE), (1920, 1080));
        assert_eq!(within(4096, 2160, MAX_SIDE), (4096, 2160));
        assert_eq!(within(11520, 5760, MAX_SIDE), (4096, 2048));
        assert_eq!(within(1080, 9000, MAX_SIDE), (492, 4096));
        assert_eq!(within(100_000, 1, MAX_SIDE), (4096, 2));
    }
}
