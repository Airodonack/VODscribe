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

/// Reads a file's details with ffmpeg, refusing anything without an audio track.
/// The file's absolute path is its id, so adding it twice finds the first one.
pub fn probe(ffmpeg: &Path, path: &Path) -> Result<VodInfo> {
    if is_remote(path) {
        bail!("{} is on another computer. Copy it to this one first.", path.display());
    }
    if !is_media(path) {
        bail!("{} isn't a video or audio file VODscribe can use.", path.display());
    }
    // dunce gives plain C:\ paths on Windows, not \\?\C:\.
    let path = dunce::canonicalize(path).with_context(|| format!("{} can't be opened.", path.display()))?;
    if !is_media(&path) {
        bail!("{} isn't a video or audio file VODscribe can use.", path.display());
    }
    if !path.is_file() {
        bail!("{} isn't a file.", path.display());
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
    if !streams.audio {
        bail!("{name} has no audio to transcribe.");
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
}

/// What's in `path` (`Media`), as ffmpeg reads it.
pub fn media(ffmpeg: &Path, path: &Path) -> Result<Media> {
    let mut cmd = tools::command(ffmpeg);
    let out = tools::ffmpeg_allow(&mut cmd, tools::FFMPEG_AUDIO)
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
        Some(Picture { width: size.0, height: size.1, fps: parse_frame_rate(line).unwrap_or(30.0) })
    });
    Media { picture, audio: streams().any(|l| l.contains(": Audio: ")), duration: parse_streams(details).duration }
}

fn parse_frame_rate(details: &str) -> Option<f64> {
    let line = details
        .lines()
        .find(|l| l.trim_start().starts_with("Stream #") && l.contains(": Video: ") && !l.contains("(attached pic)"))?;
    // "…, 1920x1080, 6000 kb/s, 29.97 fps, 29.97 tbr, …"
    line.split(", ").find_map(|part| part.strip_suffix(" fps")?.parse::<f64>().ok()).filter(|f| *f > 0.0)
}

/// The frame showing at `time` seconds into `path`, as a JPEG `width` pixels wide.
pub fn frame(ffmpeg: &Path, path: &Path, time: f64, width: u32) -> Result<Vec<u8>> {
    let mut cmd = tools::command(ffmpeg);
    let out = tools::ffmpeg_allow(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE))
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
}

fn parse_streams(details: &str) -> Streams {
    let duration = details.lines().find_map(|l| {
        let rest = l.trim().strip_prefix("Duration: ")?;
        let hms = rest.split(',').next()?;
        let mut parts = hms.split(':').map(|p| p.parse::<f64>().ok());
        let (h, m, s) = (parts.next()??, parts.next()??, parts.next()??);
        Some(h * 3600.0 + m * 60.0 + s)
    });
    let audio = details.lines().any(|l| l.trim_start().starts_with("Stream #") && l.contains(": Audio:"));
    Streams { duration, audio }
}

/// A local VOD's file, if it's still where it was added.
pub fn source(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        bail!("The file is no longer at {}.", path.display());
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
        assert_eq!(media.picture, Some(Picture { width: 1920, height: 1080, fps: 29.97 }));
        assert!(media.audio);
        let cover = "  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 320 kb/s
  Stream #0:1: Video: mjpeg (Baseline), yuvj420p(pc, bt470bg/unknown/unknown), 600x600 [SAR 1:1 DAR 1:1], 90k tbr, 90k tbn (attached pic)";
        assert_eq!(parse_media(cover), Media { picture: None, audio: true, duration: None });
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
        assert_eq!(parse_frame_rate(film), Some(29.97));
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
        let jpeg = frame(&ffmpeg, &video, 1.0, 80).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8]), "a JPEG");
        // Real audio with a program's extension is still refused.
        let program = dir.join("clip.mp4.exe");
        fs::copy(&video, &program).unwrap();
        assert!(probe(&ffmpeg, &program).unwrap_err().to_string().contains("isn't a video or audio file"));

        let info = probe(&ffmpeg, &video).unwrap();
        assert_eq!((info.platform.as_str(), info.title.as_str()), (PLATFORM, "clip"));
        assert!((info.duration.unwrap() - 2.0).abs() < 0.2, "{:?}", info.duration);
        assert!(probe(&ffmpeg, &silent).unwrap_err().to_string().contains("no audio"));

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
