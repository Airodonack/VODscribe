//! Clips: ranges of a VOD made into video files of their own, to drag into a
//! video editor or keep. They're cut with ffmpeg and encoded again (by default
//! H.264 and AAC in an MP4, which every editor plays), so they start and end
//! on the exact frame asked for, with a keyframe every second for stepping
//! through them. Clips are short, so encoding takes seconds.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};

use crate::settings::{ClipCodec, ClipEncoding, ClipFormat, ClipQuality};
use crate::tools;
use crate::transcript::Word;

/// Words a clip's title takes from what's said in it.
const TITLE_WORDS: usize = 8;

/// A clip's title: the first words said in it, or failing that where it starts.
pub fn title(words: &[Word], start: f64) -> String {
    let said: Vec<&str> = words.iter().take(TITLE_WORDS).map(|w| w.text.as_str()).collect();
    if said.is_empty() {
        return format!("Clip at {}", stamp(start));
    }
    let mut title = said.join(" ");
    if words.len() > TITLE_WORDS {
        title.push('…');
    }
    title
}

/// Said of a span whose numbers aren't a start and a later end.
pub const ENDS_BEFORE_START: &str = "A clip has to end after it starts.";
/// Said of a span that starts where the video has ended.
pub const PAST_THE_END: &str = "That's past the end of the video.";

/// Whether `start`..`end` seconds can be a clip or a piece of a video (a
/// VOD, or an edit's timeline) `end_of` seconds long, if that's known: two
/// real times from 0 on, the end after the start, starting before the video
/// ends. Of any length; an end past the video's is cut there by whoever keeps
/// the span. Every span a command is given is checked here, so all of them
/// agree on what's refused and say it the same way.
pub fn check_span(start: f64, end: f64, end_of: Option<f64>) -> Result<(), &'static str> {
    if !(start.is_finite() && end.is_finite() && start >= 0.0 && end > start) {
        return Err(ENDS_BEFORE_START);
    }
    if end_of.is_some_and(|d| start >= d) {
        return Err(PAST_THE_END);
    }
    Ok(())
}

/// A title or note as written for a clip, an edit, a piece or a mark:
/// trimmed, at most 200 characters; None if nothing's left.
pub fn written_title(text: &str) -> Option<String> {
    Some(text.trim().chars().take(200).collect::<String>()).filter(|t| !t.is_empty())
}

/// `seconds` as 1:02:03, for a title.
fn stamp(seconds: f64) -> String {
    let s = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h == 0 { format!("{m}:{s:02}") } else { format!("{h}:{m:02}:{s:02}") }
}

/// `seconds` as 01h02m03.125s, for a file name: to the millisecond (frames
/// apart up to 1000 fps), the hours padded so a VOD's clips sort in order.
fn file_stamp(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let (h, m, s, ms) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    format!("{h:02}h{m:02}m{s:02}.{ms:03}s")
}

/// Where a clip's file goes in the clips folder, and its name: a folder per
/// channel (none for files on this computer), then the stream's date (ISO
/// 8601, when the site says), the VOD's title made safe, and the clip's start
/// and end to the millisecond:
/// `Channel/2026-10-15 Title 01h02m03.125s-01h02m30.500s.mp4`. A range moved
/// later is a new file, so editors linking to the old one keep it.
pub fn file_path(
    channel: Option<&str>,
    published: Option<&str>,
    vod_title: &str,
    (start, end): (f64, f64),
    audio_only: bool,
    format: ClipFormat,
) -> (Option<String>, String) {
    let folder = channel.map(safe_name).filter(|c| c != "Clip");
    // Only a date as ISO 8601 has it (YYYY-MM-DD), as yt-dlp gives it.
    let date = published.filter(|d| {
        d.len() == 10
            && d.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })
    });
    let ext = extension(audio_only, format);
    let range = format!("{}-{}", file_stamp(start), file_stamp(end));
    let name = match date {
        Some(date) => format!("{date} {} {range}.{ext}", safe_name(vod_title)),
        None => format!("{} {range}.{ext}", safe_name(vod_title)),
    };
    (folder, name)
}

/// The file extension of a clip (or rendered edit) in `format`: sound alone is
/// M4A (AAC) or WAV (PCM).
pub fn extension(audio_only: bool, format: ClipFormat) -> &'static str {
    match (audio_only, format) {
        (false, ClipFormat::Mp4) => "mp4",
        (true, ClipFormat::Mp4) => "m4a",
        (false, ClipFormat::Mov) => "mov",
        (true, ClipFormat::Mov) => "wav",
    }
}

/// ffmpeg's output options for `encoding`, after the streams are mapped: with
/// a picture (`video`) or sound alone (AAC for MP4, PCM in a WAV for MOV).
/// H.264 and H.265 have a keyframe a second, so editors seek them quickly;
/// DNxHR and ProRes are all keyframes. Every picture's sides must be even
/// (4:2:0 and 4:2:2 halve them): `cut` and the render make them so.
pub fn output_args(encoding: ClipEncoding, video: bool) -> Vec<&'static str> {
    use ClipQuality::{High, Proxy, Standard};
    // None of the source's tags or chapters: a phone's recording carries
    // where it was shot, a recorder's the account's name, and the file
    // is made to be shared.
    let mut args = vec!["-map_metadata", "-1", "-map_chapters", "-1"];
    let q = encoding.quality;
    let pick = |proxy, standard, high| match q {
        Proxy => proxy,
        Standard => standard,
        High => high,
    };
    if video {
        match encoding.codec {
            ClipCodec::H264 => {
                args.extend(["-c:v", "libx264", "-preset", "veryfast", "-crf", pick("23", "18", "14")]);
                args.extend(["-pix_fmt", "yuv420p"]);
            }
            ClipCodec::H265 => {
                args.extend(["-c:v", "libx265", "-preset", "fast", "-crf", pick("26", "22", "18")]);
                // hvc1: QuickTime and Final Cut refuse H.265 tagged otherwise.
                // x265 writes its own notices unless told not to.
                args.extend(["-pix_fmt", "yuv420p", "-tag:v", "hvc1", "-x265-params", "log-level=error"]);
            }
            ClipCodec::Dnxhr => {
                let profile = pick("dnxhr_lb", "dnxhr_sq", "dnxhr_hq");
                args.extend(["-c:v", "dnxhd", "-profile:v", profile, "-pix_fmt", "yuv422p"]);
            }
            ClipCodec::Prores => {
                // Proxy, LT and HQ; prores_ks takes 10-bit 4:2:2 (8-bit in is fine).
                args.extend(["-c:v", "prores_ks", "-profile:v", pick("0", "1", "3"), "-pix_fmt", "yuv422p10le"]);
            }
        }
    }
    match (encoding.format(), video) {
        (ClipFormat::Mp4, _) => {
            if video {
                args.extend(["-force_key_frames", "expr:gte(t,n_forced*1)"]);
            }
            args.extend(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", "-f", "mp4"]);
        }
        (ClipFormat::Mov, true) => args.extend(["-c:a", "pcm_s16le", "-f", "mov"]),
        (ClipFormat::Mov, false) => args.extend(["-c:a", "pcm_s16le", "-f", "wav"]),
    }
    args
}

/// The longest a name made by `safe_name` is, in bytes.
const MAX_NAME: usize = 100;

/// `title` made safe for every system's file names (Windows' being the
/// strictest) and for yt-dlp's output templates (no `%`), and shortened.
/// Square brackets become round ones: a kept video's name ends with its
/// VOD's id in square brackets (`pipeline::media_stem`).
pub fn safe_name(title: &str) -> String {
    let safe: String = title
        .chars()
        .map(|c| match c {
            '[' => '(',
            ']' => ')',
            c if c.is_control() || r#"<>:"/\|?*%"#.contains(c) => ' ',
            c => c,
        })
        .collect();
    let mut safe = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    // File names may be 255 bytes long (a character can take four), with room
    // left for the clip's date and range, or a download's part suffixes.
    if safe.len() > MAX_NAME {
        let cut = (0..=MAX_NAME).rev().find(|&i| safe.is_char_boundary(i)).unwrap_or(0);
        safe.truncate(cut);
    }
    // Windows refuses names ending in a dot or space, and device names, with
    // any extension.
    let safe = safe.trim_end_matches(['.', ' ']);
    let device = safe.split('.').next().unwrap_or("").trim_end().to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&device.as_str())
        || (device.chars().count() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && device.ends_with(|c: char| c.is_ascii_digit() || "¹²³".contains(c)));
    let safe = if safe.is_empty() || reserved { "Clip" } else { safe };
    safe.to_string()
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

/// yt-dlp's format selection for a clip's section: video at most `height`
/// tall, the best there is; else the smallest taller one (a small channel's
/// Twitch VOD has only its source, often 1080p, besides audio alone); only
/// then audio alone. Any codec will do, as clips are encoded again; only
/// streams fetched over the web.
pub fn selector(height: u32) -> String {
    let choices = [
        format!("bv*[height<={height}]+ba"),
        format!("b[height<={height}]"),
        format!("wv*[height>{height}]+ba"),
        format!("w[height>{height}]"),
        "ba".to_string(),
        "b".to_string(),
    ];
    choices.iter().map(|c| crate::playable::with_web(c)).collect::<Vec<_>>().join("/")
}

/// Cuts `start`..`end` seconds of `source` into `out` (replacing it, only
/// once the new one is whole), reporting progress from 0 to 1. Sound alone
/// (`audio_only`, and cover art is left out) makes an M4A or a WAV.
#[allow(clippy::too_many_arguments)]
pub fn cut(
    ffmpeg: &Path,
    source: &Path,
    out: &Path,
    (start, end): (f64, f64),
    audio_only: bool,
    encoding: ClipEncoding,
    cancel: &AtomicBool,
    progress: impl FnMut(f64),
) -> Result<()> {
    if end.partial_cmp(&start) != Some(std::cmp::Ordering::Greater) {
        bail!(ENDS_BEFORE_START);
    }
    let length = end - start;
    // Cuts are at frames' times: half a millisecond early, so neither rounding
    // nor a container's time base puts the first frame before the seek point
    // (and drops it). That's under half a frame up to 1000 fps.
    let seek = (start - 0.0005).max(0.0);
    let dir = out.parent().context("no folder for the clip")?;
    fs::create_dir_all(dir)?;
    let name = out.file_name().context("no file name for the clip")?.to_string_lossy();
    let part = dir.join(format!(".{name}.part"));
    let mut cmd = tools::command(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-ss", &format!("{seek:.6}")]);
    tools::ffmpeg_allow_for(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE), source)
        .arg("-i")
        .arg(source);
    cmd.args(["-t", &format!("{length:.6}")]);
    if audio_only {
        cmd.args(["-map", "0:a:0", "-vn"]);
    } else {
        cmd.args(["-map", "0:v:0", "-map", "0:a:0?"]);
        cmd.args(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"]);
    }
    cmd.args(output_args(encoding, !audio_only));
    cmd.args(["-progress", "pipe:1", "-y"]);
    cmd.arg(&part);
    tools::encode(cmd, &part, out, length, cancel, progress, "the clip")
}

/// Copies `source`'s picture and sound, `length` seconds, into an MP4 at
/// `out` as they are (nothing encoded again: a minute's work is seconds),
/// reporting progress from 0 to 1: a VOD's video downloaded as a Matroska
/// section, to play while it came in, kept as its video
/// (desktop videos.rs).
pub fn remux(
    ffmpeg: &Path,
    source: &Path,
    out: &Path,
    length: f64,
    cancel: &AtomicBool,
    progress: impl FnMut(f64),
) -> Result<()> {
    let dir = out.parent().context("no folder for the video")?;
    fs::create_dir_all(dir)?;
    let name = out.file_name().context("no file name for the video")?.to_string_lossy();
    let part = dir.join(format!(".{name}.part"));
    let mut cmd = tools::command(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error"]);
    tools::ffmpeg_allow_for(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE), source)
        .arg("-i")
        .arg(source);
    cmd.args(["-map", "0:v:0?", "-map", "0:a:0?", "-c", "copy", "-movflags", "+faststart", "-f", "mp4"]);
    cmd.args(["-progress", "pipe:1", "-y"]);
    cmd.arg(&part);
    tools::encode(cmd, &part, out, length, cancel, progress, "the video")
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

    /// DNxHR LB, as MOV clips were made before quality was a choice.
    const DNXHR: ClipEncoding = ClipEncoding { codec: ClipCodec::Dnxhr, quality: ClipQuality::Proxy };

    /// A Matroska file's streams copied into an MP4, as they were. Made with
    /// ffmpeg; skipped where ffmpeg isn't installed.
    #[test]
    fn remuxes_into_an_mp4() {
        let Ok(ffmpeg) = which::which("ffmpeg") else {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        };
        let dir = crate::TempDir::new("remux");
        let mkv = dir.join("section.mkv");
        let made = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=160x90:rate=25:duration=2"])
            .args(["-f", "lavfi", "-i", "sine=duration=2", "-c:v", "libx264", "-c:a", "aac", "-f", "matroska"])
            .arg(&mkv)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            eprintln!("skipped: this ffmpeg can't make the sample");
            return;
        }
        let out = dir.join("kept.mp4");
        remux(&ffmpeg, &mkv, &out, 2.0, &AtomicBool::new(false), |_| {}).unwrap();
        let streams = crate::local::codecs(&ffmpeg, &out).unwrap();
        assert_eq!((streams.video.as_deref(), streams.audio.as_deref()), (Some("h264"), Some("aac")));
        let length = crate::local::probe(&ffmpeg, &out).unwrap().duration.unwrap();
        assert!((length - 2.0).abs() < 0.1, "{length}");
    }

    #[test]
    fn written_titles_are_trimmed_and_capped() {
        assert_eq!(written_title("  The landing \n").as_deref(), Some("The landing"));
        assert_eq!(written_title(" \t "), None);
        assert_eq!(written_title(&"é".repeat(300)).map(|t| t.chars().count()), Some(200));
    }

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
    fn sections_are_video_even_above_the_height_asked() {
        let s = selector(720);
        let choices: Vec<String> = s.split('/').map(|c| c.replace(crate::playable::WEB, "")).collect();
        assert_eq!(
            choices,
            ["bv*[height<=720]+ba", "b[height<=720]", "wv*[height>720]+ba", "w[height>720]", "ba", "b"],
            "audio alone comes last"
        );
        assert!(s.split('/').all(|c| c.split('+').all(|f| f.ends_with(crate::playable::WEB))));
    }

    #[test]
    fn file_names_are_safe_everywhere() {
        let name =
            |title: &str, start: f64, end: f64| file_path(None, None, title, (start, end), false, ClipFormat::Mp4).1;
        assert_eq!(
            name("Day 3: the \"big\" raid?", 3723.0, 3725.5),
            "Day 3 the big raid 01h02m03.000s-01h02m05.500s.mp4"
        );
        assert_eq!(
            file_path(None, None, "a/b\\c\n", (65.0, 70.0), true, ClipFormat::Mp4).1,
            "a b c 00h01m05.000s-00h01m10.000s.m4a"
        );
        assert_eq!(name("con", 0.0, 1.0), "Clip 00h00m00.000s-00h00m01.000s.mp4");
        assert_eq!(name("COM1", 0.0, 1.0), "Clip 00h00m00.000s-00h00m01.000s.mp4");
        assert_eq!(name("...", 0.0, 1.0), "Clip 00h00m00.000s-00h00m01.000s.mp4");
        assert_eq!(safe_name("100% hype"), "100 hype", "no % for yt-dlp's templates");
        assert_eq!(safe_name("Day [3]"), "Day (3)", "square brackets are for ids");
        assert_eq!(name(&"x".repeat(200), 0.0, 1.0).len(), 100 + " 00h00m00.000s-00h00m01.000s.mp4".len());
        assert_eq!(safe_name(&"配信".repeat(50)), "配信".repeat(16) + "配", "bytes, on a character's edge");
        for device in ["CON.x", "nul.tv", "CONIN$", "Com1 .txt", "LPT¹"] {
            assert_eq!(safe_name(device), "Clip", "{device}");
        }
        assert_eq!(safe_name("Console"), "Console");
        // Frames of a 480 Hz video are told apart.
        assert_ne!(name("a", 1.0 / 480.0, 1.0), name("a", 2.0 / 480.0, 1.0));
        // A channel's folder, and the stream's date when it's a date.
        assert_eq!(
            file_path(Some("Some/One"), Some("2026-10-15"), "Day 3", (61.25, 90.0), false, ClipFormat::Mp4),
            (Some("Some One".into()), "2026-10-15 Day 3 00h01m01.250s-00h01m30.000s.mp4".into())
        );
        assert_eq!(
            file_path(None, Some("20261015"), "x", (0.0, 1.0), false, ClipFormat::Mp4).1,
            "x 00h00m00.000s-00h00m01.000s.mp4"
        );
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
            .args(["-metadata", "artist=Someone", "-metadata", "location=+40.7128-074.0060/"])
            .arg(&source)
            .status()
            .unwrap();
        assert!(ok.success());
        let out = dir.join("clips").join("clip.mp4");
        let mut seen = Vec::new();
        cut(&ffmpeg, &source, &out, (2.5, 5.0), false, ClipEncoding::default(), &AtomicBool::new(false), |p| {
            seen.push(p)
        })
        .unwrap();
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
        // Nothing of where or by whom the source was recorded.
        let tags = probe(&["-show_entries", "format_tags", "-of", "csv=p=0"]);
        assert!(!tags.contains("Someone") && !tags.contains("40.7"), "{tags}");
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
        cut(&ffmpeg, &source, &audio, (0.0, 1.0), true, ClipEncoding::default(), &AtomicBool::new(false), |_| {})
            .unwrap();
        assert!(audio.is_file());
        let stop = AtomicBool::new(true);
        let err =
            cut(&ffmpeg, &source, &dir.join("stopped.mp4"), (0.0, 9.0), false, ClipEncoding::default(), &stop, |_| {})
                .unwrap_err();
        assert!(crate::is_canceled(&err));
        assert!(fs::read_dir(&*dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("stopped")));

        // For editing (DaVinci Resolve on Linux): DNxHR and PCM in MOV, as long,
        // and sound alone as WAV.
        let mov = dir.join("clips").join("clip.mov");
        cut(&ffmpeg, &source, &mov, (2.5, 5.0), false, DNXHR, &AtomicBool::new(false), |_| {}).unwrap();
        let o = std::process::Command::new(which::which("ffprobe").unwrap())
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,profile:format=duration,format_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&mov)
            .output()
            .unwrap();
        let info = String::from_utf8(o.stdout).unwrap();
        assert!(info.contains("dnxhd,DNXHR LB") && info.contains("pcm_s16le") && info.contains("mov"), "{info}");
        let seconds: f64 = info.lines().last().unwrap().rsplit(',').next().unwrap().parse().unwrap();
        assert!((seconds - 2.5).abs() < 0.05, "{info}");
        let wav = dir.join("clips").join("clip.wav");
        cut(&ffmpeg, &source, &wav, (0.0, 1.0), true, DNXHR, &AtomicBool::new(false), |_| {}).unwrap();
        assert!(fs::read(&wav).unwrap().starts_with(b"RIFF"));
        thumbnail(&ffmpeg, &mov, &png).unwrap();
    }

    /// What each codec and quality asks ffmpeg for: the encoder and the one
    /// setting that makes the step, and the container and sound.
    #[test]
    fn encodings_ask_ffmpeg_for_their_codec_and_step() {
        use ClipCodec::*;
        use ClipQuality::*;
        let args = |codec, quality, video| output_args(ClipEncoding { codec, quality }, video).join(" ");
        let has = |codec, quality, wanted: &[&str]| {
            let said = args(codec, quality, true);
            for w in wanted {
                assert!(said.contains(w), "{codec:?} {quality:?}: {said} lacks {w}");
            }
        };
        let crf = |step: &str| format!("-crf {step} ");
        let mp4 = "-c:a aac -b:a 192k -movflags +faststart -f mp4";
        let mov = "-c:a pcm_s16le -f mov";
        for (quality, step) in [(Proxy, "23"), (Standard, "18"), (High, "14")] {
            has(H264, quality, &["-c:v libx264 -preset veryfast", &crf(step), "-pix_fmt yuv420p", "n_forced", mp4]);
        }
        for (quality, step) in [(Proxy, "26"), (Standard, "22"), (High, "18")] {
            has(H265, quality, &["-c:v libx265 -preset fast", &crf(step), "-pix_fmt yuv420p -tag:v hvc1", mp4]);
        }
        for (quality, profile) in [(Proxy, "dnxhr_lb"), (Standard, "dnxhr_sq"), (High, "dnxhr_hq")] {
            has(Dnxhr, quality, &[&format!("-c:v dnxhd -profile:v {profile} -pix_fmt yuv422p "), mov]);
        }
        for (quality, profile) in [(Proxy, "0"), (Standard, "1"), (High, "3")] {
            has(Prores, quality, &[&format!("-c:v prores_ks -profile:v {profile} -pix_fmt yuv422p10le "), mov]);
        }
        // Today's clips as they were: H.264 at CRF 18, DNxHR LB.
        assert_eq!(
            args(H264, Standard, true),
            "-map_metadata -1 -map_chapters -1 -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p \
             -force_key_frames expr:gte(t,n_forced*1) -c:a aac -b:a 192k -movflags +faststart -f mp4"
        );
        // Sound alone: no picture's settings, AAC or a WAV.
        assert_eq!(
            args(H265, High, false),
            "-map_metadata -1 -map_chapters -1 -c:a aac -b:a 192k -movflags +faststart -f mp4"
        );
        assert_eq!(args(Prores, Proxy, false), "-map_metadata -1 -map_chapters -1 -c:a pcm_s16le -f wav");
    }

    /// A two-second clip in each codec, at two steps, as ffprobe reads it:
    /// the codec, its profile, the pixels and (H.265) the tag Apple's
    /// players need. Skipped where ffmpeg lacks an encoder.
    #[test]
    fn encodes_each_codec() {
        let (Ok(ffmpeg), Ok(ffprobe)) = (which::which("ffmpeg"), which::which("ffprobe")) else {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        };
        let dir = crate::TempDir::new("clip-codecs");
        let source = dir.join("source.mkv");
        // Odd sides, which every codec here needs made even.
        let made = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=321x181:rate=30:duration=2"])
            .args(["-f", "lavfi", "-i", "sine=duration=2", "-c:v", "libvpx-vp9", "-c:a", "libopus"])
            .arg(&source)
            .status();
        assert!(made.is_ok_and(|s| s.success()));
        use ClipCodec::*;
        use ClipQuality::*;
        for (codec, quality, want) in [
            (H264, Standard, "h264,High,yuv420p,avc1"),
            (H265, Proxy, "hevc,Main,yuv420p,hvc1"),
            (H265, High, "hevc,Main,yuv420p,hvc1"),
            (Dnxhr, Standard, "dnxhd,DNXHR SQ,yuv422p,AVdh"),
            (Dnxhr, High, "dnxhd,DNXHR HQ,yuv422p,AVdh"),
            (Prores, Proxy, "prores,Proxy,yuv422p10le,apco"),
            (Prores, High, "prores,HQ,yuv422p10le,apch"),
        ] {
            let encoding = ClipEncoding { codec, quality };
            let out = dir.join(format!("{codec:?}-{quality:?}.{}", extension(false, encoding.format())));
            if let Err(e) = cut(&ffmpeg, &source, &out, (0.5, 1.5), false, encoding, &AtomicBool::new(false), |_| {}) {
                assert!(format!("{e:#}").contains("Unknown encoder"), "{codec:?} {quality:?}: {e:#}");
                eprintln!("skipped {codec:?}: this ffmpeg lacks its encoder");
                continue;
            }
            let o = std::process::Command::new(&ffprobe)
                .args(["-v", "error", "-select_streams", "v", "-show_entries"])
                .args(["stream=codec_name,profile,pix_fmt,codec_tag_string,width,height", "-of", "default=nw=1"])
                .arg(&out)
                .output()
                .unwrap();
            let info = String::from_utf8(o.stdout).unwrap();
            let field = |key: &str| {
                let line = info.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='));
                line.unwrap_or_else(|| panic!("no {key} in {info}"))
            };
            let got = ["codec_name", "profile", "pix_fmt", "codec_tag_string"].map(field).join(",");
            assert_eq!(got, want, "{codec:?} {quality:?}");
            assert_eq!((field("width"), field("height")), ("320", "180"), "{codec:?} {quality:?}");
        }
    }

    #[test]
    fn formats_name_their_files() {
        let name = |audio, format| file_path(None, None, "x", (0.0, 1.0), audio, format).1;
        assert!(name(false, ClipFormat::Mov).ends_with(".mov"));
        assert!(name(true, ClipFormat::Mov).ends_with(".wav"));
        assert!(name(false, ClipFormat::Mp4).ends_with(".mp4"));
    }
}
