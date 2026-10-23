//! Video and audio files on this computer, added by dropping them on the app.
//! They're transcribed where they are: the app reads them but never copies,
//! moves or deletes them.

use std::fs;
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
    let out = tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_AUDIO)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .context("run ffmpeg")?;
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
        duration: streams.duration,
        thumbnail: None,
        published: None,
    })
}

/// The codecs of `path`'s first video and audio streams, as ffmpeg names them
/// (cover art, an "attached pic", isn't video).
pub fn codecs(ffmpeg: &Path, path: &Path) -> Result<crate::playable::Streams> {
    let mut cmd = tools::command(ffmpeg);
    let out = tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_AUDIO)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("run ffmpeg")?;
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
    let out = tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_AUDIO)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("run ffmpeg")?;
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
    let out = tools::ffmpeg_allow_for(&mut cmd, tools::FFMPEG_AUDIO, path)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("run ffmpeg")?;
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
    let out = tools::ffmpeg_allow_for(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE), path)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-ss", &format!("{:.3}", time.max(0.0)), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={width}:-2"), "-f", "image2pipe", "-c:v", "mjpeg", "pipe:1"])
        .stdin(Stdio::null())
        .output()
        .context("run ffmpeg")?;
    if !out.status.success() || out.stdout.is_empty() {
        bail!("ffmpeg couldn't read a frame: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// Saves a frame from a tenth of the way in as VOD `id`'s thumbnail. `None` for
/// audio-only files.
pub fn thumbnail(ffmpeg: &Path, path: &Path, dir: &Path, id: i64, duration: Option<f64>) -> Result<Option<String>> {
    fs::create_dir_all(dir)?;
    crate::thumbs::remove(dir, id);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let name = format!("{id}-{stamp}.jpg");
    let at = duration.map_or(0.0, |d| d / 10.0);
    let mut cmd = tools::command(ffmpeg);
    let status = tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_PICTURE)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-ss", &format!("{at:.3}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", "scale=480:-2", "-y"])
        .arg(dir.join(&name))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("run ffmpeg")?;
    Ok((status.success() && dir.join(&name).is_file()).then_some(name))
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
mod tests {
    use super::*;

    #[test]
    fn reads_what_a_file_holds() {
        let details = "  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 5000 kb/s, 29.97 fps, 29.97 tbr, 30k tbn (default)
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 192 kb/s (default)";
        let media = parse_media(details);
        let colour = Colour { matrix: Matrix::Bt709, full_range: false };
        assert_eq!(media.picture, Some(Picture { width: 1920, height: 1080, fps: 30000.0 / 1001.0, colour }));
        assert!(media.audio);
        let cover = "  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 320 kb/s
  Stream #0:1: Video: mjpeg (Baseline), yuvj420p(pc, bt470bg/unknown/unknown), 600x600 [SAR 1:1 DAR 1:1], 90k tbr, 90k tbn (attached pic)";
        assert_eq!(parse_media(cover), Media { picture: None, audio: true, duration: None });
    }

    #[test]
    fn reads_the_colour_a_stream_declares() {
        let colour = |format: &str| {
            Colour::of(&format!("  Stream #0:0: Video: h264 (High) (avc1 / 0x31637661), {format}, 1920x1080, 30 fps"))
        };
        let (limited, full) =
            (|matrix| Colour { matrix, full_range: false }, |matrix| Colour { matrix, full_range: true });
        assert_eq!(colour("yuv420p(tv, bt709, progressive)"), limited(Matrix::Bt709));
        assert_eq!(colour("yuv420p(tv, smpte170m/unknown/unknown, progressive)"), limited(Matrix::Bt601));
        assert_eq!(colour("yuv420p10le(tv, bt2020nc/bt2020/smpte2084)"), limited(Matrix::Bt2020));
        assert_eq!(colour("yuvj420p(pc, bt709/unknown/unknown, progressive)"), full(Matrix::Bt709));
        assert_eq!(colour("yuvj420p(progressive)"), full(Matrix::Bt601), "a JPEG format is full range");
        assert_eq!(colour("yuv420p(pc)"), full(Matrix::Bt601));
        assert_eq!(colour("rgb24(pc, gbr/unknown/unknown, progressive)"), full(Matrix::Bt601));
        // Only the matrix counts, not the primaries after it.
        assert_eq!(colour("yuv420p(tv, unknown/bt709/bt709)"), Colour::UNTAGGED);
        assert_eq!(colour("yuv420p(progressive)"), Colour::UNTAGGED);
        assert_eq!(colour("yuv420p"), Colour::UNTAGGED);
    }

    #[test]
    fn reads_codecs_from_ffmpegs_details() {
        let details = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'x.mp4':
  Duration: 00:00:03.00, start: 0.000000, bitrate: 100 kb/s
  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p, 320x240, 25 fps
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp
";
        let s = parse_codecs(details);
        assert_eq!((s.video.as_deref(), s.audio.as_deref()), (Some("h264"), Some("aac")));
        assert_eq!(parse_frame_rate(details), Some(25.0));
        let song = "  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 128 kb/s
  Stream #0:1: Video: mjpeg (Baseline), yuvj420p, 600x600, 90k tbr, 90k tbn (attached pic)
";
        let s = parse_codecs(song);
        assert_eq!((s.video, s.audio.as_deref()), (None, Some("mp3")), "cover art isn't video");
        assert_eq!(parse_frame_rate(song), None);
        let film = "  Stream #0:0: Video: h264 (High), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 29.97 fps, 29.97 tbr, 1k tbn (default)";
        assert_eq!(parse_frame_rate(film), Some(30000.0 / 1001.0), "29.97 is 30000/1001");
        let rate =
            |shown: &str| parse_frame_rate(&format!("  Stream #0:0: Video: h264, yuv420p, 1280x720, {shown} fps"));
        assert_eq!(rate("23.98"), Some(24000.0 / 1001.0));
        assert_eq!(rate("59.94"), Some(60000.0 / 1001.0));
        assert_eq!(rate("119.88"), Some(120000.0 / 1001.0));
        assert_eq!(rate("12.5"), Some(12.5), "not a 1001 rate");
        assert_eq!(rate("60"), Some(60.0));
        assert_eq!(rate("120k"), None, "a timebase, not a frame rate");
    }

    /// Probes real files made with ffmpeg; skipped where ffmpeg isn't installed.
    #[test]
    fn probes_real_files() {
        let Ok(ffmpeg) = which::which("ffmpeg") else { return };
        let dir = crate::TempDir::new("probe");
        let make = |name: &str, args: &[&str]| {
            let out = dir.join(name);
            let ok = std::process::Command::new(&ffmpeg)
                .args(["-hide_banner", "-loglevel", "error", "-y"])
                .args(args)
                .arg(&out)
                .status()
                .unwrap()
                .success();
            assert!(ok, "ffmpeg couldn't make {name}");
            out
        };
        let video = make(
            "clip.mp4",
            &["-f", "lavfi", "-i", "testsrc=d=2:s=160x90", "-f", "lavfi", "-i", "sine=d=2", "-shortest"],
        );
        let silent = make("silent.mp4", &["-f", "lavfi", "-i", "testsrc=d=1:s=160x90"]);
        assert_eq!(frame_rate(&ffmpeg, &video).unwrap(), Some(25.0));
        let film = make("film.mkv", &["-f", "lavfi", "-i", "testsrc=d=1:s=160x90:r=24000/1001"]);
        assert_eq!(frame_rate(&ffmpeg, &film).unwrap(), Some(24000.0 / 1001.0));
        let jpeg = frame(&ffmpeg, &video, 1.0, 80).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8]), "a JPEG");
        // Real audio with a program's extension is still refused.
        let program = dir.join("clip.mp4.exe");
        fs::copy(&video, &program).unwrap();
        assert!(probe(&ffmpeg, &program).unwrap_err().to_string().contains("isn't a video or audio file"));

        let info = probe(&ffmpeg, &video).unwrap();
        assert_eq!((info.platform.as_str(), info.title.as_str()), (PLATFORM, "clip"));
        assert!((info.duration.unwrap() - 2.0).abs() < 0.2, "{:?}", info.duration);
        // A video without sound is taken (to play and edit; nothing to transcribe).
        assert!(probe(&ffmpeg, &silent).is_ok());

        let thumbs = dir.join("thumbs");
        let name = thumbnail(&ffmpeg, &video, &thumbs, 7, info.duration).unwrap().unwrap();
        assert!(name.starts_with("7-") && crate::thumbs::mime(&name) == Some("image/jpeg"));
    }

    #[test]
    fn refuses_programs_and_network_paths() {
        assert!(is_media(Path::new("/v/a.MP4")) && is_media(Path::new("C:\\v\\b.mkv")));
        assert!(
            !is_media(Path::new("/v/clip.mp4.exe"))
                && !is_media(Path::new("/v/run.sh"))
                && !is_media(Path::new("/v/x"))
        );
        assert!(is_remote(Path::new("\\\\attacker\\share\\a.mp4")) && is_remote(Path::new("//host/a.mp4")));
        assert!(!is_remote(Path::new("/home/a.mp4")) && !is_remote(Path::new("C:\\a.mp4")));
        let err = probe(Path::new("/nonexistent/ffmpeg"), Path::new("\\\\attacker\\share\\a.mp4")).unwrap_err();
        assert!(err.to_string().contains("another computer"), "{err}");
    }

    #[test]
    fn reads_duration_and_audio_from_ffmpeg() {
        let details = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mp4':\n  Duration: 01:02:03.50, start: 0.000000, bitrate: 1 kb/s\n  Stream #0:0[0x1](und): Video: h264\n  Stream #0:1[0x2](und): Audio: aac (LC), 48000 Hz, stereo\n";
        let s = parse_streams(details);
        assert_eq!(s.duration, Some(3723.5));
        assert!(s.audio);
        let silent = parse_streams("  Duration: N/A, bitrate: N/A\n  Stream #0:0: Video: png\n");
        assert_eq!(silent.duration, None);
        assert!(!silent.audio);
    }
}
