//! Rendering an edit: pieces of media files (stretches of clips, say), one
//! after another, as one MP4. Each piece is trimmed to the frame, brought to
//! one picture size and frame rate (scaled to fit, with black bars) and one
//! sound format, then they're joined. A piece without sound gets silence, and
//! one without a picture a black one; an edit with no pictures at all makes
//! an M4A.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};

use crate::local::{self, Media, Picture};
use crate::settings::ClipFormat;
use crate::tools;

/// A stretch of a media file, `start` to `end` seconds into it.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub source: PathBuf,
    pub start: f64,
    pub end: f64,
}

/// The picture an edit's rendered at: as given, or else its first picture's.
pub fn picture_of(sources: &[Media], given: Option<Picture>) -> Option<Picture> {
    let first = sources.iter().find_map(|m| m.picture)?;
    // Even sizes, which H.264's 4:2:0 needs.
    let even = |n: u32| n.max(2) & !1;
    let p = given.unwrap_or(first);
    Some(Picture { width: even(p.width), height: even(p.height), fps: if p.fps > 0.0 { p.fps } else { 30.0 } })
}

/// How far before a piece its file is read from: the seek there needs no
/// decoding of what's before it, and the trim from there is to the frame.
const READ_BEFORE: f64 = 1.0;

/// Seconds a cut's time is moved earlier, so neither rounding nor a time base
/// puts a frame that starts there before it (as in `clips::cut`).
const EARLY: f64 = 0.0005;

/// ffmpeg's filtergraph for `pieces`, each `(input, start, end)`, of inputs
/// holding `inputs`, at `picture` (None: sound alone). Its outputs are [v]
/// (with a picture) and [a].
pub fn graph(pieces: &[(usize, f64, f64)], inputs: &[Media], picture: Option<Picture>) -> String {
    let mut g = String::new();
    for (i, &(input, start, end)) in pieces.iter().enumerate() {
        let media = inputs[input];
        let (from, to) = ((start - EARLY).max(0.0), end - EARLY);
        let length = end - start;
        if let Some(p) = picture {
            let (w, h, fps) = (p.width, p.height, p.fps);
            if media.picture.is_some() {
                let _ = write!(
                    g,
                    "[{input}:v:0]trim=start={from:.6}:end={to:.6},setpts=PTS-STARTPTS,\
                     scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,\
                     setsar=1,fps={fps},format=yuv420p[v{i}];"
                );
            } else {
                let _ = write!(g, "color=c=black:s={w}x{h}:r={fps}:d={length:.6},format=yuv420p[v{i}];");
            }
        }
        if media.audio {
            let _ = write!(
                g,
                "[{input}:a:0]atrim=start={from:.6}:end={to:.6},asetpts=PTS-STARTPTS,\
                 aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}];"
            );
        } else {
            let _ = write!(
                g,
                "anullsrc=r=48000:cl=stereo,atrim=duration={length:.6},aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}];"
            );
        }
    }
    for i in 0..pieces.len() {
        if picture.is_some() {
            let _ = write!(g, "[v{i}]");
        }
        let _ = write!(g, "[a{i}]");
    }
    let v = picture.is_some() as u8;
    let _ = write!(g, "concat=n={}:v={v}:a=1", pieces.len());
    g.push_str(if picture.is_some() { "[v][a]" } else { "[a]" });
    g
}

/// Renders `pieces` one after another into `out` (an MP4, or an M4A if none
/// has a picture), at `picture` or the first piece's, reporting progress from
/// 0 to 1. `out` is replaced only once the new one is whole.
pub fn render(
    ffmpeg: &Path,
    pieces: &[Piece],
    picture: Option<Picture>,
    format: ClipFormat,
    out: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(f64),
) -> Result<()> {
    encode_pieces(ffmpeg, pieces, Quality::Final(picture, format), out, cancel, progress)
}

/// The picture previews are made at, whatever the edit's: every segment the
/// same, so one player plays them one after another (`preview`).
pub const PREVIEW: Picture = Picture { width: 960, height: 540, fps: 30.0 };

/// The codecs a preview segment is in, as Media Source Extensions name them
/// (H.264 High at level 4.0, AAC-LC).
pub const PREVIEW_CODECS: &str = r#"video/mp4; codecs="avc1.640028, mp4a.40.2""#;

/// Renders `piece` as a preview segment into `out`: at `PREVIEW`'s size and
/// rate, picture and sound both always there (black, or silence, where its
/// file has none), as fragmented MP4 without B-frames, so its first frame is
/// at 0 and segments can be fed one after another to one player.
pub fn preview(ffmpeg: &Path, piece: &Piece, out: &Path, cancel: &AtomicBool) -> Result<()> {
    encode_pieces(ffmpeg, std::slice::from_ref(piece), Quality::Preview, out, cancel, |_| {})
}

/// What a render is for.
enum Quality {
    /// The edit's video: at the picture given (or its first piece's), as good
    /// as it was, in the format clips are made in.
    Final(Option<Picture>, ClipFormat),
    /// A preview segment (`preview`).
    Preview,
}

fn encode_pieces(
    ffmpeg: &Path,
    pieces: &[Piece],
    quality: Quality,
    out: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(f64),
) -> Result<()> {
    if pieces.is_empty() {
        bail!("There's nothing to render.");
    }
    if let Some(p) = pieces.iter().find(|p| p.end.partial_cmp(&p.start) != Some(std::cmp::Ordering::Greater)) {
        bail!("A piece has to end after it starts ({} to {}).", p.start, p.end);
    }
    // Each file read once for what it holds; each piece an input of its own,
    // read from a little before it, so a piece hours into a file starts at once.
    let mut read: Vec<(&Path, Media)> = Vec::new();
    let mut inputs = Vec::new();
    let mut seeks = Vec::new();
    let mut at = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let media = match read.iter().find(|(s, _)| *s == p.source) {
            Some((_, m)) => *m,
            None => {
                let m = local::media(ffmpeg, &p.source).with_context(|| format!("read {}", p.source.display()))?;
                read.push((&p.source, m));
                m
            }
        };
        let seek = (p.start - READ_BEFORE).max(0.0);
        inputs.push(media);
        seeks.push(seek);
        at.push((i, p.start - seek, p.end - seek));
    }
    let picture = match quality {
        Quality::Final(given, _) => picture_of(&inputs, given),
        Quality::Preview => Some(PREVIEW),
    };
    let length: f64 = pieces.iter().map(|p| p.end - p.start).sum();

    let dir = out.parent().context("no folder for the video")?;
    std::fs::create_dir_all(dir)?;
    let name = out.file_name().context("no file name for the video")?.to_string_lossy();
    let part = dir.join(format!(".{name}.part"));
    let mut cmd = tools::command(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error"]);
    // Each input limited on its own: what's allowed is set for the next
    // input only, and a second piece's file would otherwise be read with
    // any format and decoder ffmpeg has.
    let decoders = format!("{},{}", tools::FFMPEG_AUDIO, tools::FFMPEG_PICTURE);
    for (p, seek) in pieces.iter().zip(&seeks) {
        tools::ffmpeg_allow(&mut cmd, &decoders);
        cmd.args(["-ss", &format!("{seek:.6}")]).arg("-i").arg(&p.source);
    }
    cmd.args(["-filter_complex", &graph(&at, &inputs, picture)]);
    match quality {
        Quality::Final(_, format) => {
            if picture.is_some() {
                cmd.args(["-map", "[v]"]);
            }
            cmd.args(["-map", "[a]"]);
            cmd.args(crate::clips::output_args(format, picture.is_some()));
        }
        Quality::Preview => {
            cmd.args(["-map", "[v]", "-c:v", "libx264", "-preset", "veryfast", "-crf", "26", "-bf", "0", "-g", "60"]);
            cmd.args(["-pix_fmt", "yuv420p", "-profile:v", "high", "-level", "4.0"]);
            cmd.args(["-map", "[a]", "-c:a", "aac", "-b:a", "128k"]);
            // Not shifted for the sound encoder's priming (as the muxer would, by a
            // 21 ms AAC frame): the picture's frames then fall on the rate's steps,
            // and the last one ends at the segment's end instead of past it, where
            // the player's cut would drop it and leave a gap between segments.
            cmd.args(["-avoid_negative_ts", "disabled"]);
            cmd.args(["-movflags", "+frag_keyframe+empty_moov+default_base_moof", "-f", "mp4"]);
        }
    }
    cmd.args(["-progress", "pipe:1", "-y"]).arg(&part);
    tools::encode(cmd, &part, out, length, cancel, progress, "the video")
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: Media =
        Media { picture: Some(Picture { width: 1920, height: 1080, fps: 30.0 }), audio: true, duration: None };
    const SILENT: Media =
        Media { picture: Some(Picture { width: 1280, height: 720, fps: 60.0 }), audio: false, duration: None };
    const SOUND: Media = Media { picture: None, audio: true, duration: None };

    #[test]
    fn the_graph_trims_fits_and_joins_each_piece() {
        let p = picture_of(&[VIDEO, SILENT], None);
        assert_eq!(p, Some(Picture { width: 1920, height: 1080, fps: 30.0 }), "the first one's");
        let g = graph(&[(0, 1.0, 2.5), (1, 0.0, 1.0), (2, 3.0, 4.0)], &[VIDEO, SILENT, SOUND], p);
        assert!(g.starts_with("[0:v:0]trim=start=0.999500:end=2.499500,setpts=PTS-STARTPTS,scale=1920:1080:"));
        assert!(!g.contains("[1:a:0]"), "no sound to take from the silent one");
        assert!(g.contains("anullsrc=r=48000:cl=stereo,atrim=duration=1.000000"));
        assert!(g.contains("color=c=black:s=1920x1080:r=30:d=1.000000"), "a picture for sound alone");
        assert!(g.ends_with("[v0][a0][v1][a1][v2][a2]concat=n=3:v=1:a=1[v][a]"));
        let sound = graph(&[(0, 0.0, 1.0)], &[SOUND], None);
        assert_eq!(
            sound,
            "[0:a:0]atrim=start=0.000000:end=0.999500,asetpts=PTS-STARTPTS,aresample=48000,\
             aformat=sample_fmts=fltp:channel_layouts=stereo[a0];[a0]concat=n=1:v=0:a=1[a]"
        );
        let odd = Picture { width: 1081, height: 1921, fps: 0.0 };
        assert_eq!(picture_of(&[VIDEO], Some(odd)), Some(Picture { width: 1080, height: 1920, fps: 30.0 }));
        assert_eq!(picture_of(&[SOUND], Some(odd)), None, "no picture to make");
    }

    /// Two files (one without sound, at another size and rate) joined: as
    /// long as their pieces, at the first's size, with sound throughout.
    /// Skipped where ffmpeg isn't installed.
    #[test]
    fn renders_pieces_of_files_into_one() {
        let (Ok(ffmpeg), Ok(ffprobe)) = (which::which("ffmpeg"), which::which("ffprobe")) else {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        };
        let dir = crate::TempDir::new("render");
        let make = |name: &str, args: &[&str]| {
            let path = dir.join(name);
            let ok = std::process::Command::new(&ffmpeg).args(["-v", "error", "-y"]).args(args).arg(&path).status();
            assert!(ok.unwrap().success());
            path
        };
        let a = make(
            "a.mp4",
            &["-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=6", "-f", "lavfi", "-i", "sine=duration=6"],
        );
        let b = make(
            "b.mp4",
            &["-f", "lavfi", "-i", "testsrc2=size=200x200:rate=25:duration=4", "-an", "-metadata", "artist=Someone"],
        );
        // A second piece in a format the app doesn't read is refused, as a first would be.
        let nut = make("c.nut", &["-f", "lavfi", "-i", "testsrc2=size=64x64:rate=25:duration=1", "-an"]);
        let refused = [Piece { source: a.clone(), start: 0.0, end: 1.0 }, Piece { source: nut, start: 0.0, end: 0.5 }];
        let bad = dir.join("out").join("bad.mp4");
        assert!(render(&ffmpeg, &refused, None, ClipFormat::Mp4, &bad, &AtomicBool::new(false), |_| {}).is_err());
        let pieces = [
            Piece { source: a.clone(), start: 1.0, end: 2.5 },
            Piece { source: b, start: 0.0, end: 2.0 },
            Piece { source: a, start: 4.0, end: 5.0 },
        ];
        let out = dir.join("out").join("edit.mp4");
        let mut seen = Vec::new();
        render(&ffmpeg, &pieces, None, ClipFormat::Mp4, &out, &AtomicBool::new(false), |p| seen.push(p)).unwrap();
        assert_eq!(seen.last(), Some(&1.0));
        let probe = |args: &[&str]| {
            let o = std::process::Command::new(&ffprobe).args(["-v", "error"]).args(args).arg(&out).output().unwrap();
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        let tags = probe(&["-show_entries", "format_tags", "-of", "csv=p=0"]);
        assert!(!tags.contains("Someone"), "{tags}");
        let streams = probe(&["-show_entries", "stream=codec_type,width,height,duration", "-of", "csv=p=0"]);
        let video: Vec<&str> = streams.lines().find(|l| l.starts_with("video")).unwrap().split(',').collect();
        assert_eq!(&video[1..3], ["320", "180"], "{streams}");
        let seconds = |kind: &str| -> f64 {
            streams.lines().find(|l| l.starts_with(kind)).unwrap().rsplit(',').next().unwrap().parse().unwrap()
        };
        assert!((seconds("video") - 4.5).abs() < 0.05, "{streams}");
        assert!((seconds("audio") - 4.5).abs() < 0.06, "{streams}");
        assert!(!dir.join("out").join(".edit.mp4.part").exists());

        // A preview segment of the silent file: at the preview's size, with
        // (silent) sound, starting at 0.
        let seg = dir.join("seg.mp4");
        let silent = Piece { source: dir.join("b.mp4"), start: 1.0, end: 3.0 };
        preview(&ffmpeg, &silent, &seg, &AtomicBool::new(false)).unwrap();
        let o = std::process::Command::new(&ffprobe)
            .args(["-v", "error", "-show_entries", "stream=codec_type,width,height,start_time", "-of", "csv=p=0"])
            .arg(&seg)
            .output()
            .unwrap();
        let streams = String::from_utf8_lossy(&o.stdout).to_string();
        assert!(streams.contains("video,960,540,0.000000"), "{streams}");
        assert!(streams.lines().any(|l| l.starts_with("audio") && l.ends_with("0.000000")), "{streams}");

        // Its frames on the preview's steps, the last ending at its end (none
        // past it, for the player's cut to drop and leave a gap).
        let o = std::process::Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v", "-show_entries", "packet=pts_time", "-of", "csv=p=0"])
            .arg(&seg)
            .output()
            .unwrap();
        let times: Vec<f64> = String::from_utf8_lossy(&o.stdout).lines().map(|l| l.parse().unwrap()).collect();
        let step = 1.0 / PREVIEW.fps;
        assert!(times.iter().enumerate().all(|(i, t)| (t - i as f64 * step).abs() < 0.001), "{times:?}");
        assert!((times.last().unwrap() + step - 2.0).abs() < 0.001, "{times:?}");
    }
}
