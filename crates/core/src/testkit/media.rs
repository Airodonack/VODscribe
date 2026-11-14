//! Media files as the fake programs make and read them: a few lines of JSON
//! saying what a real file would hold (its length, picture and sound) and
//! what it was cut from, so a test can ask of a clip's file what it would of
//! a video, without decoding one. Real ffmpeg can't read them, and the
//! fakes read nothing else.
//!
//! Decoded (the player's decoders, its storyboards), a fake video's frames
//! say which they are: frame `n` is grey, its top half's shade and its
//! bottom half's telling `n` (`numbered_frame`, `frame_number`), and a
//! storyboard's picture names its time (`storyboard_time`).

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

    /// What its file holds, as `write` writes it: for a file given another
    /// way (a drop's bytes).
    pub fn to_bytes(&self) -> Vec<u8> {
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

/// Shades a fake frame's halves take: video's luma, 16 to 234.
const SHADES: u64 = 219;

/// Frame `n` of a fake video, `width` by `height`, as the fake ffmpeg
/// decodes it to planar YUV 4:2:0 (the Y plane, then the Cb and Cr planes):
/// grey, its top half's shade `n / 219` (mod 219) and its bottom half's
/// `n % 219`, in video's range (16 is black).
pub fn numbered_frame(n: u64, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let (top, bottom) = (16 + (n / SHADES % SHADES) as u8, 16 + (n % SHADES) as u8);
    let mut yuv = Vec::with_capacity(w * h + 2 * cw * ch);
    yuv.resize(w * (h / 2), top);
    yuv.resize(w * h, bottom);
    yuv.resize(w * h + 2 * cw * ch, 128);
    yuv
}

/// The number of the fake frame (`numbered_frame`) whose pixels are `pixels`,
/// `width` by `height`: YUV 4:2:0 as decoded (`channels` 0), or RGB as the
/// player turns it (`channels` 3, or 4 for RGBA), scaled or not. Read at the
/// middle of each half; None if it's no fake frame there (not grey, or a
/// shade out of range).
pub fn frame_number(pixels: &[u8], width: u32, height: u32, channels: usize) -> Option<u64> {
    let (w, h) = (width as usize, height as usize);
    let shade = |row: usize| -> Option<u64> {
        let at = row * w + w / 2;
        let luma = if channels == 0 {
            *pixels.get(at)?
        } else {
            let px = pixels.get(at * channels..at * channels + 3)?;
            if px[0].abs_diff(px[1]) > 1 || px[1].abs_diff(px[2]) > 1 {
                return None;
            }
            // The player's video range to RGB, undone: 16 to 235 spread to 0 to 255.
            (px[1] as f64 * 219.0 / 255.0 + 16.0).round() as u8
        };
        (16..16 + SHADES as u8).contains(&luma).then(|| (luma - 16) as u64)
    };
    Some(shade(h / 4)? * SHADES + shade(h * 3 / 4)?)
}

/// The picture a fake storyboard frame is (the fake ffmpeg's JPEG of its
/// keyframe at `pts` seconds, `width` by `height`): a JPEG's start and end
/// markers around a comment naming them.
pub(crate) fn storyboard_jpeg(pts: f64, width: u32, height: u32) -> Vec<u8> {
    let said = format!("{STORYBOARD}{pts} {width}x{height}");
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xfe];
    jpeg.extend_from_slice(&(said.len() as u16 + 2).to_be_bytes());
    jpeg.extend_from_slice(said.as_bytes());
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

/// How a fake storyboard picture's comment begins.
const STORYBOARD: &str = "FAKE FRAME at ";

/// The time in its file of the keyframe a fake storyboard picture shows
/// (`storyboard_jpeg`); None if it's no fake one.
pub fn storyboard_time(jpeg: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(jpeg.get(6..jpeg.len().checked_sub(2)?)?).ok()?;
    text.strip_prefix(STORYBOARD)?.split(' ').next()?.parse().ok()
}
