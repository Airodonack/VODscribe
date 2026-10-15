//! Clips: ranges of a VOD made into video files of their own, to drag into a
//! video editor or keep. They're cut with ffmpeg and encoded again (H.264 and
//! AAC in an MP4, which every editor and webview plays), so they start and end
//! on the exact frame asked for, with a keyframe every second for stepping
//! through them. Clips are short, so encoding takes seconds.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};

use crate::tools;
use crate::transcript::Word;

/// Words a clip's title takes from what's said in it.
const TITLE_WORDS: usize = 8;

/// A clip's title: the first words said in it, or failing that where it starts.
pub fn title(words: &[Word], start: f64) -> String {
    let said: Vec<&str> = words.iter().take(TITLE_WORDS).map(|w| w.text.as_str()).collect();
    if said.is_empty() {
        return format!("Clip at {}", stamp(start, ":"));
    }
    let mut title = said.join(" ");
    if words.len() > TITLE_WORDS {
        title.push('…');
    }
    title
}

/// `seconds` as 1:02:03 (with `sep` ":") or 1h02m03s (with "").
fn stamp(seconds: f64, sep: &str) -> String {
    let s = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    match (sep, h) {
        (":", 0) => format!("{m}:{s:02}"),
        (":", _) => format!("{h}:{m:02}:{s:02}"),
        (_, 0) => format!("{m}m{s:02}s"),
        _ => format!("{h}h{m:02}m{s:02}s"),
    }
}

/// The file name a clip of the VOD titled `vod_title` starting at `start` gets:
/// the title made safe for every system's file names (Windows' being the
/// strictest), shortened, then where it starts.
pub fn file_name(vod_title: &str, start: f64, audio_only: bool) -> String {
    let safe: String =
        vod_title.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { ' ' } else { c }).collect();
    let mut safe = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    if safe.chars().count() > 80 {
        safe = safe.chars().take(80).collect::<String>().trim_end().to_string();
    }
    // Windows refuses names ending in a dot or space, and these device names.
    let safe = safe.trim_end_matches(['.', ' ']);
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&safe.to_ascii_uppercase().as_str())
        || (safe.len() == 4
            && ["COM", "LPT"].iter().any(|p| safe.to_ascii_uppercase().starts_with(p))
            && safe.ends_with(|c: char| c.is_ascii_digit()));
    let safe = if safe.is_empty() || reserved { "Clip" } else { safe };
    let ext = if audio_only { "m4a" } else { "mp4" };
    format!("{safe} {}.{ext}", stamp(start, ""))
}

/// `name` in `dir`, or with " (2)", " (3)"… before its extension if taken.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !p.exists()).expect("a free name")
}

/// Cuts `start`..`end` seconds of `source` into `out` (replacing it, only
/// once the new one is whole), reporting progress from 0 to 1. Sound alone
/// (`audio_only`, and cover art is left out) makes an M4A.
pub fn cut(
    ffmpeg: &Path,
    source: &Path,
    out: &Path,
    (start, end): (f64, f64),
    audio_only: bool,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
) -> Result<()> {
    if end.partial_cmp(&start) != Some(std::cmp::Ordering::Greater) {
        bail!("A clip has to end after it starts.");
    }
    let length = end - start;
    let dir = out.parent().context("no folder for the clip")?;
    fs::create_dir_all(dir)?;
    let name = out.file_name().context("no file name for the clip")?.to_string_lossy();
    let part = dir.join(format!(".{name}.part"));
    let mut cmd = tools::command(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-ss", &format!("{start:.3}")]);
    tools::ffmpeg_allow(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE)).arg("-i").arg(source);
    cmd.args(["-t", &format!("{length:.3}")]);
    if audio_only {
        cmd.args(["-map", "0:a:0", "-vn"]);
    } else {
        cmd.args(["-map", "0:v:0", "-map", "0:a:0?", "-c:v", "libx264", "-preset", "veryfast", "-crf", "18"]);
        cmd.args(["-pix_fmt", "yuv420p", "-force_key_frames", "expr:gte(t,n_forced*1)"]);
    }
    cmd.args(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", "-f", "mp4", "-progress", "pipe:1", "-y"]);
    let mut child = cmd
        .arg(&part)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start ffmpeg")?;
    let errors = tools::drain(child.stderr.take().unwrap());
    let mut canceled = false;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        if cancel.load(Ordering::Relaxed) {
            canceled = true;
            tools::kill_tree(&mut child);
            break;
        }
        if let Some(us) = line?.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
            progress((us / 1e6 / length).clamp(0.0, 1.0));
        }
    }
    let status = child.wait()?;
    let errors = errors.join().unwrap_or_default();
    if canceled {
        let _ = fs::remove_file(&part);
        return Err(crate::Canceled.into());
    }
    if !status.success() {
        let _ = fs::remove_file(&part);
        bail!("ffmpeg couldn't cut the clip: {}", tools::last_lines(errors.trim(), 3));
    }
    fs::rename(&part, out)?;
    progress(1.0);
    Ok(())
}

/// A small picture of `clip`'s first frame, as a PNG at `out`, to show
/// while it's dragged.
pub fn thumbnail(ffmpeg: &Path, clip: &Path, out: &Path) -> Result<()> {
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut cmd = tools::command(ffmpeg);
    let status = tools::ffmpeg_allow(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE))
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(clip)
        .args(["-frames:v", "1", "-vf", "scale=160:-2", "-c:v", "png", "-f", "image2", "-y"])
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .context("run ffmpeg")?;
    if !status.success() {
        bail!("ffmpeg couldn't read the clip's first frame");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_what_is_said() {
        let w = |text: &str| Word { text: text.into(), start: 0.0, end: 0.0 };
        let few: Vec<Word> = ["The", "Eagle", "has", "landed."].map(w).into();
        assert_eq!(title(&few, 0.0), "The Eagle has landed.");
        let many: Vec<Word> = (1..=10).map(|i| w(&i.to_string())).collect();
        assert_eq!(title(&many, 0.0), "1 2 3 4 5 6 7 8…");
        assert_eq!(title(&[], 3723.4), "Clip at 1:02:03");
        assert_eq!(title(&[], 65.0), "Clip at 1:05");
    }

    #[test]
    fn file_names_are_safe_everywhere() {
        assert_eq!(file_name("Day 3: the \"big\" raid?", 3723.0, false), "Day 3 the big raid 1h02m03s.mp4");
        assert_eq!(file_name("a/b\\c\n", 65.0, true), "a b c 1m05s.m4a");
        assert_eq!(file_name("con", 0.0, false), "Clip 0m00s.mp4");
        assert_eq!(file_name("COM1", 0.0, false), "Clip 0m00s.mp4");
        assert_eq!(file_name("...", 0.0, false), "Clip 0m00s.mp4");
        let long = file_name(&"x".repeat(200), 0.0, false);
        assert_eq!(long.len(), 80 + " 0m00s.mp4".len());
    }

    #[test]
    fn names_already_taken_get_a_number() {
        let dir = crate::TempDir::new("clip-names");
        assert_eq!(unique_path(&dir, "a 1m00s.mp4"), dir.join("a 1m00s.mp4"));
        fs::write(dir.join("a 1m00s.mp4"), b"").unwrap();
        fs::write(dir.join("a 1m00s (2).mp4"), b"").unwrap();
        assert_eq!(unique_path(&dir, "a 1m00s.mp4"), dir.join("a 1m00s (3).mp4"));
    }

    /// Cuts made with ffmpeg are as long as asked, H.264 and AAC, with a
    /// keyframe every second. Skipped where ffmpeg isn't installed.
    #[test]
    fn cuts_exactly() {
        let Ok(ffmpeg) = which::which("ffmpeg") else {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        };
        let dir = crate::TempDir::new("clip-cut");
        let source = dir.join("source.mkv");
        // VP9 and Opus with keyframes 4 s apart, so a cut that only copied would start early.
        let ok = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=10"])
            .args(["-f", "lavfi", "-i", "sine=duration=10", "-c:v", "libvpx-vp9", "-g", "120", "-c:a", "libopus"])
            .arg(&source)
            .status()
            .unwrap();
        assert!(ok.success());
        let out = dir.join("clips").join("clip.mp4");
        let mut seen = Vec::new();
        cut(&ffmpeg, &source, &out, (2.5, 5.0), false, &AtomicBool::new(false), |p| seen.push(p)).unwrap();
        assert_eq!(seen.last(), Some(&1.0));
        let probe = |args: &[&str]| {
            let out = std::process::Command::new(which::which("ffprobe").unwrap())
                .args(["-v", "error"])
                .args(args)
                .arg(&out)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        let duration: f64 = probe(&["-show_entries", "format=duration", "-of", "csv=p=0"]).trim().parse().unwrap();
        assert!((duration - 2.5).abs() < 0.05, "{duration}");
        let codecs = probe(&["-show_entries", "stream=codec_name", "-of", "csv=p=0"]);
        assert_eq!(codecs.split_whitespace().collect::<Vec<_>>(), ["h264", "aac"]);
        let keyframes = probe(&[
            "-select_streams",
            "v",
            "-skip_frame",
            "nokey",
            "-show_entries",
            "frame=pts_time",
            "-of",
            "csv=p=0",
        ]);
        assert!(keyframes.lines().count() >= 3, "a keyframe a second: {keyframes}");
        assert!(!dir.join("clips").join(".clip.mp4.part").exists());

        let png = dir.join("thumb.png");
        thumbnail(&ffmpeg, &out, &png).unwrap();
        assert!(fs::read(&png).unwrap().starts_with(b"\x89PNG"));

        // Sound alone, and a stop partway leaves nothing behind.
        let audio = dir.join("clips").join("clip.m4a");
        cut(&ffmpeg, &source, &audio, (0.0, 1.0), true, &AtomicBool::new(false), |_| {}).unwrap();
        assert!(audio.is_file());
        let stop = AtomicBool::new(true);
        let err = cut(&ffmpeg, &source, &dir.join("stopped.mp4"), (0.0, 9.0), false, &stop, |_| {}).unwrap_err();
        assert!(crate::is_canceled(&err));
        assert!(fs::read_dir(&*dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("stopped")));
    }
}
