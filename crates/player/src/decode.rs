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
        bail!("{} has no picture and no sound.", path.display());
    }
    Ok(Info { picture: media.picture, audio: media.audio, duration: media.duration })
}

/// One decoded picture: `width` by `height` pixels of packed 8-bit RGB.
pub struct Frame {
    /// Seconds into the file at which it shows.
    pub pts: f64,
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Samples: interleaved stereo 16-bit, from `pts` on.
pub struct Samples {
    pub pts: f64,
    pub pcm: Vec<i16>,
}

pub const CHANNELS: usize = 2;

/// Frames kept decoded ahead of being taken.
const AHEAD: usize = 4;

/// A stream of frames or samples from one ffmpeg process.
pub struct Stream<T> {
    child: Child,
    frames: Receiver<T>,
    _reader: JoinHandle<()>,
}

impl<T: Send + 'static> Stream<T> {
    /// The next frame out, or None once the file (or ffmpeg) ends.
    pub fn next(&mut self) -> Option<T> {
        self.frames.recv().ok()
    }

    /// The next frame if one is decoded already.
    pub fn try_next(&mut self) -> Option<T> {
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
    let mut child = tools::ffmpeg_allow(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE))
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
        .with_context(|| format!("run {}", ffmpeg.display()))?;
    let stdout = child.stdout.take().context("ffmpeg's output")?;
    let (tx, frames) = mpsc::sync_channel(AHEAD);
    let reader = std::thread::Builder::new().name("decode".into()).spawn(move || read(stdout, tx))?;
    Ok(Stream { child, frames, _reader: reader })
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
    pub fn from(&self, from: f64) -> Result<Stream<Arc<Frame>>> {
        let Picture { width, height, fps } = self.picture;
        let rate = format!("{fps}");
        let output = ["-fps_mode", "cfr", "-r", &rate, "-f", "rawvideo", "-pix_fmt", "rgb24"];
        start(&self.ffmpeg, &self.path, from, "0:v:0", &output, move |mut out, tx| {
            let size = width as usize * height as usize * 3;
            let mut n = 0u64;
            loop {
                let mut rgb = vec![0u8; size];
                if out.read_exact(&mut rgb).is_err() {
                    return;
                }
                let frame = Frame { pts: from + n as f64 / fps, width, height, rgb };
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

impl Audio {
    pub fn new(ffmpeg: &Path, path: &Path) -> Audio {
        Audio { ffmpeg: ffmpeg.into(), path: path.into() }
    }

    /// The sound from `from` seconds on, in chunks, at `rate` samples a second.
    pub fn from(&self, from: f64, rate: u32) -> Result<Stream<Samples>> {
        let rate_ = rate.to_string();
        let output = ["-f", "s16le", "-ac", "2", "-ar", &rate_];
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
                let pcm: Vec<i16> = bytes[..got].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
                let samples = Samples { pts: from + done as f64 / rate as f64, pcm };
                done += (got / (CHANNELS * 2)) as u64;
                if tx.send(samples).is_err() {
                    return;
                }
            }
        })
    }
}
