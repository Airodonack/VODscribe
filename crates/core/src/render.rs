//! Rendering an edit: pieces of media files (stretches of clips, say), one
//! after another, as one file in the format clips are made in (`ClipFormat`:
//! MP4, or MOV for editors). Each piece is trimmed to the frame, brought to
//! one picture size and frame rate (scaled to fit, with black bars) and one
//! sound format, then they're joined. A piece without sound gets silence, and
//! one without a picture a black one; one without a file at all (a hole: its
//! footage is gone) both. An edit with no pictures at all makes sound alone
//! (an M4A, or a WAV).

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};

use crate::local::{self, Media, Picture};
use crate::settings::ClipFormat;
use crate::tools;

/// A stretch of a media file, `start` to `end` seconds into it; with no
/// file, a hole as long, black and silent.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub source: Option<PathBuf>,
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

/// ffmpeg's filtergraph for `pieces`, each `(input, start, end)` (no input:
/// a hole), of inputs holding `inputs`, at `picture` (None: sound alone).
/// Its outputs are [v] (with a picture) and [a].
pub fn graph(pieces: &[(Option<usize>, f64, f64)], inputs: &[Media], picture: Option<Picture>) -> String {
    const HOLE: Media = Media { picture: None, audio: false, duration: None };
    let mut g = String::new();
    for (i, &(input, start, end)) in pieces.iter().enumerate() {
        let media = input.map_or(HOLE, |n| inputs[n]);
        let input = input.unwrap_or(0);
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
/// has a picture), at `picture` or the first piece's, with `chapters` (start,
/// title; a WAV has none), reporting progress from 0 to 1. `out` is replaced
/// only once the new one is whole.
#[allow(clippy::too_many_arguments)]
pub fn render(
    ffmpeg: &Path,
    pieces: &[Piece],
    picture: Option<Picture>,
    chapters: &[(f64, String)],
    format: ClipFormat,
    out: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(f64),
) -> Result<PathBuf> {
    encode_pieces(ffmpeg, pieces, picture, chapters, format, out, cancel, progress)
}

/// `chapters` (start, title) of a video `length` seconds long, as an ffmpeg
/// metadata file: each to the next one's start, the last to the end.
pub fn chapters_file(chapters: &[(f64, String)], length: f64) -> String {
    // =, ;, # and \ are escaped with a backslash in a value; a title's
    // control characters (line ends among them, which would end its line
    // there) are spaces.
    let escaped = |text: &str| {
        let mut out = String::new();
        for c in text.chars() {
            if matches!(c, '=' | ';' | '#' | '\\') {
                out.push('\\');
            }
            out.push(if c.is_control() { ' ' } else { c });
        }
        // ffmpeg reads a backslash at a line's end as escaping the line end
        // (an escaped one too), and the next chapter would be lost: a space after.
        if out.ends_with('\\') {
            out.push(' ');
        }
        out
    };
    let ms = |t: f64| (t.clamp(0.0, length) * 1000.0).round() as i64;
    let mut out = String::from(";FFMETADATA1\n");
    for (i, (start, title)) in chapters.iter().enumerate() {
        let end = chapters.get(i + 1).map_or(length, |(next, _)| *next);
        if ms(end) <= ms(*start) {
            continue;
        }
        let _ = write!(
            out,
            "[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={}\n",
            ms(*start),
            ms(end),
            escaped(title)
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn encode_pieces(
    ffmpeg: &Path,
    pieces: &[Piece],
    given: Option<Picture>,
    chapters: &[(f64, String)],
    format: ClipFormat,
    out: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(f64),
) -> Result<PathBuf> {
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
    let mut files = Vec::new();
    let mut at = Vec::new();
    for p in pieces {
        let Some(source) = &p.source else {
            at.push((None, 0.0, p.end - p.start));
            continue;
        };
        let media = match read.iter().find(|(s, _)| *s == source) {
            Some((_, m)) => *m,
            None => {
                let m = local::media(ffmpeg, source).with_context(|| format!("read {}", tools::shown(source)))?;
                read.push((source, m));
                m
            }
        };
        let seek = (p.start - READ_BEFORE).max(0.0);
        at.push((Some(inputs.len()), p.start - seek, p.end - seek));
        inputs.push(media);
        files.push((source, seek));
    }
    let picture = picture_of(&inputs, given);
    let length: f64 = pieces.iter().map(|p| p.end - p.start).sum();
    // Named for what it holds, found only now: sound alone is an .m4a or .wav
    // (beside, not over, a file of that name already there).
    let named = out.with_extension(crate::clips::extension(picture.is_none(), format));
    let out = &match (named != out, named.parent(), named.file_name()) {
        (true, Some(dir), Some(name)) if named.exists() => crate::clips::unique_path(dir, &name.to_string_lossy()),
        _ => named,
    };

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
    let sources = files.len();
    for (source, seek) in files {
        tools::ffmpeg_allow_for(&mut cmd, &decoders, source);
        cmd.args(["-ss", &format!("{seek:.6}")]).arg("-i").arg(source);
    }
    // The chapters, from a metadata file of their own read as the last input
    // (a WAV keeps none).
    let wav = out.extension().is_some_and(|e| e == "wav");
    let marks = dir.join(format!(".{name}.chapters"));
    let with_chapters = !chapters.is_empty() && !wav;
    if with_chapters {
        std::fs::write(&marks, chapters_file(chapters, length))?;
        cmd.args(["-format_whitelist", "ffmetadata", "-protocol_whitelist", "file", "-f", "ffmetadata", "-i"])
            .arg(&marks);
    }
    cmd.args(["-filter_complex", &graph(&at, &inputs, picture)]);
    if picture.is_some() {
        cmd.args(["-map", "[v]"]);
    }
    cmd.args(["-map", "[a]"]);
    // The output's tags and chapters are none of the sources' (`output_args`);
    // with chapters, the metadata file's instead, which has only those (its
    // chapters' titles come only with its metadata, not after a -1).
    let chapters_input = sources.to_string();
    let mut args = crate::clips::output_args(format, picture.is_some());
    if with_chapters {
        for pair in args.chunks_mut(2) {
            if matches!(pair, ["-map_metadata" | "-map_chapters", "-1"]) {
                pair[1] = &chapters_input;
            }
        }
    }
    cmd.args(args);
    cmd.args(["-progress", "pipe:1", "-y"]).arg(&part);
    let made = tools::encode(cmd, &part, out, length, cancel, progress, "the video");
    if with_chapters {
        let _ = std::fs::remove_file(&marks);
    }
    made?;
    Ok(out.clone())
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
    fn chapter_titles_stay_on_their_line() {
        let file = chapters_file(&[(0.0, "a\rb\n[CHAPTER]".into()), (1.0, "ends in \\".into())], 2.0);
        assert_eq!(
            file,
            ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=a b [CHAPTER]\n\
             [CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=ends in \\\\ \n"
        );
    }

    #[test]
    fn the_graph_trims_fits_and_joins_each_piece() {
        let p = picture_of(&[VIDEO, SILENT], None);
        assert_eq!(p, Some(Picture { width: 1920, height: 1080, fps: 30.0 }), "the first one's");
        let g = graph(
            &[(Some(0), 1.0, 2.5), (Some(1), 0.0, 1.0), (Some(2), 3.0, 4.0), (None, 0.0, 2.0)],
            &[VIDEO, SILENT, SOUND],
            p,
        );
        assert!(g.starts_with("[0:v:0]trim=start=0.999500:end=2.499500,setpts=PTS-STARTPTS,scale=1920:1080:"));
        assert!(!g.contains("[1:a:0]"), "no sound to take from the silent one");
        assert!(g.contains("anullsrc=r=48000:cl=stereo,atrim=duration=1.000000"));
        assert!(g.contains("color=c=black:s=1920x1080:r=30:d=1.000000"), "a picture for sound alone");
        assert!(g.contains("color=c=black:s=1920x1080:r=30:d=2.000000,format=yuv420p[v3];anullsrc"), "a hole");
        assert!(g.ends_with("[v0][a0][v1][a1][v2][a2][v3][a3]concat=n=4:v=1:a=1[v][a]"));
        let sound = graph(&[(Some(0), 0.0, 1.0)], &[SOUND], None);
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
        let refused = [
            Piece { source: Some(a.clone()), start: 0.0, end: 1.0 },
            Piece { source: Some(nut), start: 0.0, end: 0.5 },
        ];
        let bad = dir.join("out").join("bad.mp4");
        assert!(render(&ffmpeg, &refused, None, &[], ClipFormat::Mp4, &bad, &AtomicBool::new(false), |_| {}).is_err());
        let pieces = [
            Piece { source: Some(a.clone()), start: 1.0, end: 2.5 },
            Piece { source: Some(b), start: 0.0, end: 2.0 },
            // A hole: black and silent.
            Piece { source: None, start: 0.0, end: 0.5 },
            Piece { source: Some(a), start: 4.0, end: 5.0 },
        ];
        let out = dir.join("out").join("edit.mp4");
        let mut seen = Vec::new();
        let chapters = [(0.0, "Intro".to_string()), (2.0, "Part = two; #2".to_string())];
        render(&ffmpeg, &pieces, None, &chapters, ClipFormat::Mp4, &out, &AtomicBool::new(false), |p| seen.push(p))
            .unwrap();
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
        assert!((seconds("video") - 5.0).abs() < 0.05, "{streams}");
        assert!((seconds("audio") - 5.0).abs() < 0.06, "{streams}");
        assert!(!dir.join("out").join(".edit.mp4.part").exists());
        // Its chapters: each to the next's start, the last to the end, titles as written.
        let marks = probe(&["-show_entries", "chapter=start_time,end_time:chapter_tags=title", "-of", "csv=p=0"]);
        let marks: Vec<Vec<&str>> = marks.lines().map(|l| l.split(',').collect()).collect();
        assert_eq!(marks.len(), 2, "{marks:?}");
        assert_eq!(marks[0][2], "Intro", "{marks:?}");
        assert_eq!(marks[1][2..].join(","), "Part = two; #2", "{marks:?}");
        let time = |s: &str| s.parse::<f64>().unwrap();
        assert!((time(marks[0][1]) - 2.0).abs() < 0.01 && (time(marks[1][0]) - 2.0).abs() < 0.01, "{marks:?}");
        assert!((time(marks[1][1]) - 5.0).abs() < 0.01, "{marks:?}");
        assert!(!dir.join("out").join(".edit.mp4.chapters").exists());

        // Sound alone, asked for as a video file: named for what it holds.
        let tone = make("tone.m4a", &["-f", "lavfi", "-i", "sine=duration=2"]);
        let asked = dir.join("out").join("sound.mov");
        let pieces = [Piece { source: Some(tone), start: 0.0, end: 1.0 }];
        // (A WAV keeps no chapters: they're left out.)
        let chapters = [(0.0, "Tone".to_string())];
        let made = render(&ffmpeg, &pieces, None, &chapters, ClipFormat::Mov, &asked, &AtomicBool::new(false), |_| {})
            .unwrap();
        assert_eq!(made, dir.join("out").join("sound.wav"));
        assert!(made.is_file() && !asked.exists());
    }
}
