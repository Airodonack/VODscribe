//! Media files as the fake programs make and read them: a few lines of JSON
//! saying what a real file would hold (its length, picture and sound) and
//! what it was cut from, so a test can ask of a clip's file what it would of
//! a video, without decoding one. Real ffmpeg can't read them, and the
//! fakes read nothing else.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// How a fake media file begins.
const MAGIC: &str = "FAKE MEDIA\n";

/// What a fake media file holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Media {
    /// Its length in seconds.
    pub duration: f64,
    pub video: Option<Video>,
    /// The sound's codec, as ffmpeg names it (aac, pcm_s16le, opus).
    pub audio: Option<String>,
    /// Its container, as ffmpeg names its output format (mp4, mov, wav,
    /// matroska).
    pub container: String,
    /// What it's a cut of: the first file or link of the chain it was cut
    /// from, and the second of that its own 0 is. None for one made whole.
    pub cut_from: Option<(String, f64)>,
}

/// A fake file's picture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Video {
    /// As ffmpeg names it (h264, hevc, dnxhd, prores, vp9).
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// How it was encoded, as ffmpeg was asked: the quality step (`-crf`),
    /// the profile (`-profile:v`), the pixel format (`-pix_fmt`) and the tag
    /// (`-tag:v`). None where nothing was said: a source the test made, or
    /// one that wasn't asked for.
    #[serde(default)]
    pub crf: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub pix_fmt: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
}

impl Media {
    /// An H.264 and AAC video in an MP4, 1920x1080 at 30 fps, `duration`
    /// seconds long.
    pub fn video(duration: f64) -> Self {
        Media {
            duration,
            video: Some(Video {
                codec: "h264".into(),
                width: 1920,
                height: 1080,
                fps: 30.0,
                crf: None,
                profile: None,
                pix_fmt: None,
                tag: None,
            }),
            audio: Some("aac".into()),
            container: "mp4".into(),
            cut_from: None,
        }
    }

    /// AAC sound alone in an M4A, `duration` seconds long.
    pub fn sound(duration: f64) -> Self {
        Media { video: None, container: "mp4".into(), ..Media::video(duration) }
    }

    /// Writes it to `path`, its folder made if need be.
    pub fn write(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(path, self.to_bytes()).unwrap();
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        format!("{MAGIC}{}\n", serde_json::to_string(self).unwrap()).into_bytes()
    }

    /// The fake media file at `path`; None if there's none, or it isn't one.
    pub fn read(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let text = std::str::from_utf8(&bytes).ok()?.strip_prefix(MAGIC)?;
        serde_json::from_str(text).ok()
    }

    /// What ffmpeg prints of a file it's given and asked nothing of, as
    /// `local::probe` and the like read it.
    pub(crate) fn details(&self, path: &str) -> String {
        let demuxer = match self.container.as_str() {
            "mp4" | "mov" => "mov,mp4,m4a,3gp,3g2,mj2",
            "matroska" => "matroska,webm",
            other => other,
        };
        let total = (self.duration.max(0.0) * 100.0).round() as u64;
        let (h, m, s, cs) = (total / 360_000, total / 6000 % 60, total / 100 % 60, total % 100);
        let mut text = format!(
            "Input #0, {demuxer}, from '{path}':\n  Duration: {h:02}:{m:02}:{s:02}.{cs:02}, start: 0.000000, bitrate: 6192 kb/s\n"
        );
        let mut stream = 0;
        if let Some(v) = &self.video {
            text += &format!(
                "  Stream #0:{stream}[0x1](und): Video: {} (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), {}x{} [SAR 1:1 DAR 16:9], 6000 kb/s, {} fps, {} tbr, 15360 tbn (default)\n",
                v.codec, v.width, v.height, v.fps, v.fps
            );
            stream += 1;
        }
        if let Some(a) = &self.audio {
            text += &format!(
                "  Stream #0:{stream}[0x2](und): Audio: {a} (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 192 kb/s (default)\n"
            );
        }
        text + "At least one output file must be specified\n"
    }
}
