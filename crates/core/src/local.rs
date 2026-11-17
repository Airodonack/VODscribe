//! Video and audio files on this computer, added by dropping them on the app.
//! They're transcribed where they are: the app reads them but never copies,
//! moves or deletes them.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::fetch::VodInfo;
use crate::tools;

/// The `platform` of a local file's VOD.
pub const PLATFORM: &str = "local";

/// The file types VODscribe adds and plays, by extension, with the media type
/// the player is told. Anything else is refused, so a program can't be added
/// (and later "opened", which would run it). The app's file picker and its
/// media server use this list too.
pub const MEDIA_TYPES: &[(&str, &str)] = &[
    ("mp4", "video/mp4"),
    ("m4v", "video/mp4"),
    ("mkv", "video/x-matroska"),
    ("webm", "video/webm"),
    ("mov", "video/quicktime"),
    ("avi", "video/x-msvideo"),
    ("ts", "video/mp2t"),
    ("flv", "video/x-flv"),
    ("mp3", "audio/mpeg"),
    ("m4a", "audio/mp4"),
    ("aac", "audio/mp4"),
    ("wav", "audio/wav"),
    ("flac", "audio/flac"),
    ("ogg", "audio/ogg"),
    ("opus", "audio/ogg"),
];

/// The media type of `path` by its extension, if it's one of `MEDIA_TYPES`.
pub fn media_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    MEDIA_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, mime)| *mime)
}

/// Whether `path` has one of the `MEDIA_TYPES` extensions.
pub fn is_media(path: &Path) -> bool {
    media_type(path).is_some()
}

/// Network shares and device paths (\\server\share, \\?\UNC\, \\.\): opening
/// one can make Windows sign in to someone else's server with the user's credentials.
fn is_remote(path: &Path) -> bool {
    let p = path.to_string_lossy();
    p.starts_with("\\\\") || p.starts_with("//")
}

/// Reads a file's details with ffmpeg, refusing anything with neither a
/// picture nor sound (a video without sound is kept to play and edit, with
/// nothing to transcribe).
/// The file's absolute path is its id, so adding it twice finds the first one.
pub fn probe(ffmpeg: &Path, path: &Path) -> Result<VodInfo> {
    if is_remote(path) {
        bail!("{} is on another computer. Copy it to this one first.", tools::shown(path));
    }
    if !is_media(path) {
        bail!("{} isn't a video or audio file VODscribe can use.", tools::shown(path));
    }
    // dunce gives plain C:\ paths on Windows, not \\?\C:\.
    let path = dunce::canonicalize(path).with_context(|| format!("{} can't be opened.", tools::shown(path)))?;
    if !is_media(&path) {
        bail!("{} isn't a video or audio file VODscribe can use.", tools::shown(&path));
    }
    if !path.is_file() {
        bail!("{} isn't a file.", tools::shown(&path));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // With no output, ffmpeg prints the file's details to stderr and exits with an error.
    let mut cmd = tools::command(ffmpeg);
    tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_AUDIO)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(&path)
        .stdin(Stdio::null());
    let out = tools::output(&mut cmd).context("run ffmpeg")?;
    let details = String::from_utf8_lossy(&out.stderr);
    let streams = parse_streams(&details);
    if !streams.audio && !streams.picture {
        bail!("{name} has no picture and no sound.");
    }
    let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name);
    Ok(VodInfo {
        platform: PLATFORM.into(),
        platform_id: path.to_string_lossy().into_owned(),
        url: path.to_string_lossy().into_owned(),
        title,
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: streams.duration,
        thumbnail: None,
        published: None,
        started_at: None,
    })
}

/// The codecs of `path`'s first video and audio streams, as ffmpeg names them
/// (cover art, an "attached pic", isn't video).
pub fn codecs(ffmpeg: &Path, path: &Path) -> Result<crate::playable::Streams> {
    let mut cmd = tools::command(ffmpeg);
    tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_AUDIO)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(path)
        .stdin(Stdio::null());
    let out = tools::output(&mut cmd).context("run ffmpeg")?;
    Ok(parse_codecs(&String::from_utf8_lossy(&out.stderr)))
}

fn parse_codecs(details: &str) -> crate::playable::Streams {
    let first = |kind: &str| {
        details
            .lines()
            .filter(|l| l.trim_start().starts_with("Stream #") && !l.contains("(attached pic)"))
            .find_map(|l| l.split_once(kind)?.1.split([' ', ',', '(']).next().map(str::to_string))
            .filter(|c| !c.is_empty())
    };
    crate::playable::Streams { video: first(": Video: "), audio: first(": Audio: ") }
}

/// The frame rate of `path`'s first video stream, as ffmpeg reads it; None
/// for sound alone.
pub fn frame_rate(ffmpeg: &Path, path: &Path) -> Result<Option<f64>> {
    let mut cmd = tools::command(ffmpeg);
    tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_AUDIO)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(path)
        .stdin(Stdio::null());
    let out = tools::output(&mut cmd).context("run ffmpeg")?;
    Ok(parse_frame_rate(&String::from_utf8_lossy(&out.stderr)))
}

/// What's in a media file, for putting it in a sequence (render.rs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Media {
    /// Its first video stream's size and frame rate, if it has a picture.
    pub picture: Option<Picture>,
    pub audio: bool,
    /// Its length in seconds, as ffmpeg reads it.
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// How its YUV is turned into RGB, as the stream says.
    pub colour: Colour,
}

/// How a picture's YUV values are turned into RGB: the matrix its stream
/// declares, and whether its values take the full 0-255 (JPEG's, "pc") or
/// the limited 16-235 of video ("tv").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour {
    pub matrix: Matrix,
    pub full_range: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Colour {
    /// What a stream that declares nothing is read as: limited BT.601,
    /// as ffmpeg's own conversion to RGB reads it.
    pub const UNTAGGED: Colour = Colour { matrix: Matrix::Bt601, full_range: false };

    /// The colour ffmpeg's details give in a video stream's line: the pixel
    /// format and what's in brackets after it, as in "yuv420p(tv, bt709,
    /// progressive)" or "yuvj420p(pc, bt470bg/unknown/unknown)" (the first
    /// of three slashed is the matrix). An RGB stream (a PNG's "rgb24(pc,
    /// gbr/…)") is turned into YUV by ffmpeg with BT.601, as untagged.
    fn of(line: &str) -> Colour {
        let Some(format) = fields(line).nth(1) else { return Colour::UNTAGGED };
        let (name, tags) = format.split_once('(').unwrap_or((format, ""));
        let tags: Vec<&str> = tags.trim_end_matches(')').split(',').map(str::trim).collect();
        let matrix = tags
            .iter()
            .find_map(|t| match t.split('/').next() {
                Some("bt709") => Some(Matrix::Bt709),
                Some("bt2020nc" | "bt2020c") => Some(Matrix::Bt2020),
                _ => None,
            })
            .unwrap_or(Matrix::Bt601);
        Colour { matrix, full_range: tags.contains(&"pc") || name.trim().starts_with("yuvj") }
    }
}

/// A stream line's fields after "Video: ", split at the commas that aren't
/// in brackets: the codec, the pixel format, the size, …
fn fields(line: &str) -> impl Iterator<Item = &str> {
    let rest = line.split_once(": Video: ").map_or("", |(_, rest)| rest);
    let mut depth = 0i32;
    rest.split(move |c: char| {
        depth += (c == '(') as i32 - (c == ')') as i32;
        c == ',' && depth == 0
    })
    .map(str::trim)
}

/// What's in `path` (`Media`), as ffmpeg reads it.
pub fn media(ffmpeg: &Path, path: &Path) -> Result<Media> {
    let mut cmd = tools::command(ffmpeg);
    tools::ffmpeg_allow_for(&mut cmd, tools::FFMPEG_AUDIO, path)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(path)
        .stdin(Stdio::null());
    let out = tools::output(&mut cmd).context("run ffmpeg")?;
    Ok(parse_media(&String::from_utf8_lossy(&out.stderr)))
}

fn parse_media(details: &str) -> Media {
    let streams = || details.lines().filter(|l| l.trim_start().starts_with("Stream #"));
    let picture = streams().find(|l| l.contains(": Video: ") && !l.contains("(attached pic)")).and_then(|line| {
        // "…, yuv420p(tv, bt709), 1920x1080 [SAR 1:1 DAR 16:9], 6000 kb/s, 30 fps, …"
        let size = line.split([',', ' ']).find_map(|w| {
            let (w, h) = w.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)).filter(|(w, h)| *w > 0 && *h > 0)
        })?;
        Some(Picture {
            width: size.0,
            height: size.1,
            fps: parse_frame_rate(line).unwrap_or(30.0),
            colour: Colour::of(line),
        })
    });
    Media { picture, audio: streams().any(|l| l.contains(": Audio: ")), duration: parse_streams(details).duration }
}

fn parse_frame_rate(details: &str) -> Option<f64> {
    let line = details
        .lines()
        .find(|l| l.trim_start().starts_with("Stream #") && l.contains(": Video: ") && !l.contains("(attached pic)"))?;
    // "…, 1920x1080, 6000 kb/s, 29.97 fps, 29.97 tbr, …". Rates ffmpeg shows
    // as "120k" are a timebase, not a picture rate: unknown.
    let shown = line.split(", ").find_map(|part| part.strip_suffix(" fps")?.parse::<f64>().ok())?;
    (shown > 0.0).then(|| exact_rate(shown))
}

/// The exact rate behind ffmpeg's two decimals: 23.98, 29.97 and 59.94 are
/// 24000/1001, 30000/1001 and 60000/1001, and stepping or decoding at the
/// rounded rate would repeat a frame every few minutes.
fn exact_rate(shown: f64) -> f64 {
    let whole = (shown * 1.001).round();
    let ntsc = whole * 1000.0 / 1001.0;
    if shown.fract() != 0.0 && (shown - ntsc).abs() < 0.006 { ntsc } else { shown }
}

/// The frame showing at `time` seconds into `path`, as a JPEG `width` pixels wide.
pub fn frame(ffmpeg: &Path, path: &Path, time: f64, width: u32) -> Result<Vec<u8>> {
    let mut cmd = tools::command(ffmpeg);
    tools::ffmpeg_allow_for(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE), path)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-ss", &format!("{:.3}", time.max(0.0)), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={width}:-2"), "-f", "image2pipe", "-c:v", "mjpeg", "pipe:1"])
        .stdin(Stdio::null());
    let out = tools::output(&mut cmd).context("run ffmpeg")?;
    if !out.status.success() || out.stdout.is_empty() {
        bail!("ffmpeg couldn't read a frame: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// Whether `path` is inside `dir`, such as a file in the app's own data folder.
pub fn is_inside(path: &Path, dir: &Path) -> bool {
    let (Ok(path), Ok(dir)) = (dunce::canonicalize(path), dunce::canonicalize(dir)) else { return false };
    path.starts_with(dir)
}

struct Streams {
    duration: Option<f64>,
    audio: bool,
    /// A video stream that isn't cover art.
    picture: bool,
}

fn parse_streams(details: &str) -> Streams {
    let duration = details.lines().find_map(|l| {
        let rest = l.trim().strip_prefix("Duration: ")?;
        let hms = rest.split(',').next()?;
        let mut parts = hms.split(':').map(|p| p.parse::<f64>().ok());
        let (h, m, s) = (parts.next()??, parts.next()??, parts.next()??);
        Some(h * 3600.0 + m * 60.0 + s)
    });
    let stream = |kind: &str| {
        details
            .lines()
            .any(|l| l.trim_start().starts_with("Stream #") && l.contains(kind) && !l.contains("(attached pic)"))
    };
    Streams { duration, audio: stream(": Audio:"), picture: stream(": Video:") }
}

/// A local VOD's file, if it's still where it was added.
pub fn source(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        bail!("The file is no longer at {}.", tools::shown(&path));
    }
    Ok(path)
}

#[cfg(test)]
mod tests;
