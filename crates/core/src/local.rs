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

/// The file types VODscribe adds and plays. Anything else is refused, so a
/// program can't be added (and later "opened", which would run it).
pub const MEDIA_EXTENSIONS: &[&str] =
    &["mp4", "m4v", "mkv", "webm", "mov", "avi", "ts", "flv", "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus"];

/// Whether `path` has one of the `MEDIA_EXTENSIONS`.
pub fn is_media(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
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
    let out = tools::command(ffmpeg)
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

/// Saves a frame from a tenth of the way in as VOD `id`'s thumbnail. `None` for
/// audio-only files.
pub fn thumbnail(ffmpeg: &Path, path: &Path, dir: &Path, id: i64, duration: Option<f64>) -> Result<Option<String>> {
    fs::create_dir_all(dir)?;
    crate::thumbs::remove(dir, id);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let name = format!("{id}-{stamp}.jpg");
    let at = duration.map_or(0.0, |d| d / 10.0);
    let status = tools::command(ffmpeg)
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

    /// Probes real files made with ffmpeg; skipped where ffmpeg isn't installed.
    #[test]
    fn probes_real_files() {
        let Ok(ffmpeg) = which::which("ffmpeg") else { return };
        let dir = std::env::temp_dir().join(format!("vodscribe-probe-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
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
        let video = make("clip.mp4", &["-f", "lavfi", "-i", "testsrc=d=2:s=160x90", "-f", "lavfi", "-i", "sine=d=2", "-shortest"]);
        let silent = make("silent.mp4", &["-f", "lavfi", "-i", "testsrc=d=1:s=160x90"]);
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
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_programs_and_network_paths() {
        assert!(is_media(Path::new("/v/a.MP4")) && is_media(Path::new("C:\\v\\b.mkv")));
        assert!(!is_media(Path::new("/v/clip.mp4.exe")) && !is_media(Path::new("/v/run.sh")) && !is_media(Path::new("/v/x")));
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
