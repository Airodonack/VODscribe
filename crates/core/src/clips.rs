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

/// The name a clip goes by where it's handed out (dragged to another app,
/// copied, shown in its folder): the stream's date (ISO 8601, when the site
/// says), the VOD's title made safe, and the clip's start and end to the
/// millisecond: `2026-10-15 Title 01h02m03.125s-01h02m30.500s.mp4`. The
/// clip's own file is named by its id (`Paths::clip_file`).
pub fn file_name(
    published: Option<&str>,
    vod_title: &str,
    (start, end): (f64, f64),
    audio_only: bool,
    format: ClipFormat,
) -> String {
    // Only a date as ISO 8601 has it (YYYY-MM-DD), as yt-dlp gives it.
    let date = published.filter(|d| {
        d.len() == 10
            && d.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })
    });
    let ext = extension(audio_only, format);
    let range = format!("{}-{}", file_stamp(start), file_stamp(end));
    match date {
        Some(date) => format!("{date} {} {range}.{ext}", safe_name(vod_title)),
        None => format!("{} {range}.{ext}", safe_name(vod_title)),
    }
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

/// Whether `file`, a clip's or an edit's, was made as clips are made now
/// (`now`): in its format and, with a picture, its codec and quality, as
/// recorded (`made`), else as a file of its format was made before that was
/// (`Settings::clip_encoding`). Sound alone is the same in every codec of its
/// format. The page asks the same (dragout.ts `madeAsWanted`).
pub fn made_as(file: &Path, made: Option<ClipEncoding>, now: ClipEncoding) -> bool {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let format = if matches!(ext.as_str(), "mov" | "wav") { ClipFormat::Mov } else { ClipFormat::Mp4 };
    if format != now.format() {
        return false;
    }
    if matches!(ext.as_str(), "m4a" | "wav") {
        return true;
    }
    let before = || crate::settings::Settings { clip_format: format, ..Default::default() }.clip_encoding();
    made.unwrap_or_else(before) == now
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

/// The ffmpeg encoder a picture in `codec` is made with (`output_args`).
pub fn encoder(codec: ClipCodec) -> &'static str {
    encoder_after(codec, true, "-c:v")
}

/// The ffmpeg encoder sound in `codec`'s format is made with, with a
/// picture (`video`) or alone: AAC for MP4, PCM for MOV (`output_args`).
pub fn sound_encoder(codec: ClipCodec, video: bool) -> &'static str {
    encoder_after(codec, video, "-c:a")
}

fn encoder_after(codec: ClipCodec, video: bool, flag: &str) -> &'static str {
    let args = output_args(ClipEncoding { codec, quality: ClipQuality::Standard }, video);
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1]).expect("an encoder")
}

/// Whether an ffmpeg with encoders `has` can't make a clip in `codec`, with
/// a picture (`video`) or sound alone.
fn lacks(has: &tools::Encoders, codec: ClipCodec, video: bool) -> bool {
    (video && !has.contains(encoder(codec))) || !has.contains(sound_encoder(codec, video))
}

/// The formats (codecs) `ffmpeg` can't make clips with a picture in, as it
/// says itself (`tools::encoders`): its picture's encoder or its sound's is
/// missing (a build without AAC can make no MP4). None when it can't say.
pub fn missing_codecs(ffmpeg: &Path) -> Vec<ClipCodec> {
    let Some(has) = tools::encoders(ffmpeg) else { return Vec::new() };
    ClipCodec::ALL.into_iter().filter(|c| lacks(&has, *c, true)).collect()
}

/// Refuses a clip or an edit in `encoding`, with a picture (`video`) or
/// sound alone, when `ffmpeg` can't make it, saying which format and where
/// to pick another, before anything is fetched for it. The choice stays as
/// it is: made in another format, the file might not open in its editor.
pub fn can_make(ffmpeg: &Path, encoding: ClipEncoding, video: bool) -> Result<()> {
    if tools::encoders(ffmpeg).is_some_and(|has| lacks(&has, encoding.codec, video)) {
        bail!("VODscribe's ffmpeg can't make {}. Pick another format in Settings › Encoding.", encoding.codec.name());
    }
    Ok(())
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
    tools::ffmpeg_allow(&mut cmd, &format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE))
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(clip)
        .args(["-frames:v", "1", "-vf", "scale=160:-2", "-c:v", "png", "-f", "image2", "-y"])
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    let status = tools::status(&mut cmd).context("run ffmpeg")?;
    if !status.success() {
        bail!("ffmpeg couldn't read the clip's first frame");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
