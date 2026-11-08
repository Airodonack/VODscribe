//! The specs of clips' titles, names, encodings and cuts: the public
//! functions of `clips.rs`, with the kit's fake ffmpeg. The tests that run
//! the real ffmpeg are the fake's check against it (testing.md, "Fakes, not
//! mocks"); they skip where ffmpeg isn't installed.
//!
//! "A span of any length is a clip" (core clips.rs:42; CHANGELOG.md 0.7.0,
//! "Clips can be any length.") is `every_span_is_checked_the_same_way` in
//! edits.rs, which checks a four-hour span, so it isn't said again here.

use super::*;
use crate::testkit::media::Video;
use crate::testkit::{Ffmpeg, Gate, Media};

// ---------------------------------------------------------------- titles

fn said(text: &[&str]) -> Vec<Word> {
    text.iter().map(|t| Word { text: t.to_string(), start: 0.0, end: 0.0 }).collect()
}

fn numbered(count: usize) -> Vec<Word> {
    let texts: Vec<String> = (1..=count).map(|i| i.to_string()).collect();
    said(&texts.iter().map(String::as_str).collect::<Vec<_>>())
}

/// A clip's title is "the first words said in it", at most `TITLE_WORDS`
/// (eight), "or failing that where it starts" (core clips.rs:18 and :21).
#[test]
fn a_clip_is_titled_with_its_first_eight_words_and_an_ellipsis_past_them() {
    assert_eq!(title(&said(&["The", "Eagle", "has", "landed."]), 0.0), "The Eagle has landed.");
    assert_eq!(title(&numbered(8), 0.0), "1 2 3 4 5 6 7 8", "eight words fit whole");
    assert_eq!(title(&numbered(9), 0.0), "1 2 3 4 5 6 7 8…");
    assert_eq!(title(&numbered(10), 0.0), "1 2 3 4 5 6 7 8…");
}

/// "or failing that where it starts" (core clips.rs:21), as 1:02:03.
#[test]
fn a_clip_with_nothing_said_is_titled_clip_at_where_it_starts() {
    assert_eq!(title(&[], 3723.4), "Clip at 1:02:03");
    assert_eq!(title(&[], 65.0), "Clip at 1:05");
    assert_eq!(title(&[], 0.0), "Clip at 0:00");
}

/// "A span ... two real times from 0 on, the end after the start" (core
/// clips.rs:34 and :39).
#[test]
fn a_span_must_end_after_it_starts_from_zero_on() {
    assert_eq!(check_span(0.0, 0.001, None), Ok(()), "from 0 on");
    assert_eq!(check_span(1.0, 2.0, None), Ok(()));
    for (start, end) in [
        (2.0, 2.0),
        (3.0, 2.0),
        (-0.001, 2.0),
        (-5.0, -1.0),
        (f64::NAN, 2.0),
        (1.0, f64::NAN),
        (1.0, f64::INFINITY),
        (f64::NEG_INFINITY, 2.0),
    ] {
        assert_eq!(check_span(start, end, None), Err(ENDS_BEFORE_START), "{start}..{end}");
        assert_eq!(check_span(start, end, Some(100.0)), Err(ENDS_BEFORE_START), "{start}..{end} in 100 s");
    }
}

/// "starting before the video ends" (core clips.rs:36 and :39); "an end past
/// the video's is cut there by whoever keeps the span".
#[test]
fn a_span_starting_at_or_past_the_video_s_end_is_refused() {
    assert_eq!(check_span(10.0, 12.0, Some(10.0)), Err(PAST_THE_END));
    assert_eq!(check_span(11.0, 12.0, Some(10.0)), Err(PAST_THE_END));
    assert_eq!(check_span(9.999, 12.0, Some(10.0)), Ok(()), "an end past the video's is the keeper's to cut");
    assert_eq!(check_span(11.0, 12.0, None), Ok(()), "no length known, none to be past");
}

/// "trimmed, at most 200 characters" (core clips.rs:55).
#[test]
fn a_written_title_is_trimmed_and_cut_at_200_characters() {
    assert_eq!(written_title("  The landing \n").as_deref(), Some("The landing"));
    assert_eq!(written_title(&"é".repeat(300)).map(|t| t.chars().count()), Some(200));
    assert_eq!(written_title(&"x".repeat(200)).map(|t| t.len()), Some(200));
}

/// "None if nothing's left" (core clips.rs:56).
#[test]
fn a_written_title_of_only_spaces_is_none() {
    assert_eq!(written_title(" \t \n"), None);
    assert_eq!(written_title(""), None);
}

// ----------------------------------------------------------------- names

/// `file_name` of a VOD with no date.
fn named(title: &str, (start, end): (f64, f64)) -> String {
    file_name(None, title, (start, end), false, ClipFormat::Mp4)
}

/// "The name is the stream's date (ISO 8601, when the site says), the VOD's
/// title made safe, and the clip's start and end to the millisecond:
/// `2026-10-15 Title 01h02m03.125s-01h02m30.500s.mp4`" (core clips.rs,
/// `file_name`).
#[test]
fn a_clip_s_name_is_its_vod_s_date_title_and_range_to_the_millisecond() {
    let name = |published, title, range| file_name(published, title, range, false, ClipFormat::Mp4);
    assert_eq!(
        name(Some("2026-10-15"), "Title", (3723.125, 3750.5)),
        "2026-10-15 Title 01h02m03.125s-01h02m30.500s.mp4"
    );
    assert_eq!(name(None, "Title", (3723.125, 3750.5)), "Title 01h02m03.125s-01h02m30.500s.mp4", "no date, none named");
}

/// "ISO 8601, when the site says" (core clips.rs, `file_name`), and the
/// check's comment there: "Only a date as ISO 8601 has it (YYYY-MM-DD), as yt-dlp gives it."
#[test]
fn a_date_not_written_as_iso_8601_is_left_out_of_the_name() {
    let name = |published| file_name(Some(published), "x", (0.0, 1.0), false, ClipFormat::Mp4);
    assert_eq!(name("2026-10-15"), "2026-10-15 x 00h00m00.000s-00h00m01.000s.mp4");
    for not_iso in ["20261015", "2026/10/15", "Oct 15, 2026", "2026-10-1", "2026-1O-15", "2026-10-155", ""] {
        assert_eq!(name(not_iso), "x 00h00m00.000s-00h00m01.000s.mp4", "{not_iso:?}");
    }
}

/// "the hours padded so a VOD's clips sort in order" (core clips.rs:68).
#[test]
fn a_vod_s_clips_sort_in_time_order_by_name() {
    let starts = [0.0, 0.5, 9.0, 9.5, 10.0, 59.999, 60.0, 600.0, 3599.0, 3600.0, 36_000.0, 86_399.0];
    let names: Vec<String> = starts.iter().map(|&s| named("Day 3", (s, s + 1.0))).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

/// "to the millisecond (frames apart up to 1000 fps)" (core clips.rs:68).
#[test]
fn clips_a_frame_apart_at_480_fps_have_different_names() {
    for fps in [24.0, 30.0, 60.0, 120.0, 240.0, 480.0, 1000.0] {
        for frame in 0..40 {
            let (a, b) = (frame as f64 / fps, (frame + 1) as f64 / fps);
            assert_ne!(named("a", (a, 100.0)), named("a", (b, 100.0)), "start, frame {frame} at {fps} fps");
            assert_ne!(named("a", (0.0, a + 1.0)), named("a", (0.0, b + 1.0)), "end, frame {frame} at {fps} fps");
        }
    }
}

/// "made safe for every system's file names (Windows' being the strictest)"
/// (core clips.rs:191), and "Windows refuses names ending in a dot or space,
/// and device names, with any extension" (the comment at :212).
#[test]
fn a_name_is_safe_on_every_system() {
    assert_eq!(safe_name("Day 3: the \"big\" raid?"), "Day 3 the big raid");
    assert_eq!(safe_name("a/b\\c\n|<>*d"), "a b c d", "Windows' characters and control characters become spaces");
    assert_eq!(safe_name("a \t\t b"), "a b", "runs of spaces become one");
    assert_eq!(safe_name("Raid..."), "Raid", "no trailing dot");
    assert_eq!(safe_name("Raid. . "), "Raid", "no trailing dot or space");
    assert_eq!(safe_name("Console"), "Console", "only a whole device name is one");
    assert_eq!(safe_name("Raid.x"), "Raid.x", "a dot inside stays");
}

/// "(no `%`)" for "yt-dlp's output templates" (core clips.rs:191).
#[test]
fn a_name_has_no_percent_sign_for_yt_dlp_s_templates() {
    assert_eq!(safe_name("100% hype"), "100 hype");
    assert!(!safe_name("%(title)s %%").contains('%'));
    assert!(!named("100%", (0.0, 1.0)).contains('%'));
}

/// "Square brackets become round ones: a kept video's name ends with its
/// VOD's id in square brackets" (core clips.rs:193).
#[test]
fn square_brackets_in_a_title_become_round_ones() {
    assert_eq!(safe_name("Day [3]"), "Day (3)");
    assert!(!named("[a] [b]", (0.0, 1.0)).contains(['[', ']']));
}

/// "shortened" to `MAX_NAME`, 100 bytes, "a character can take four"
/// (core clips.rs:188 and :206).
#[test]
fn a_long_title_is_cut_to_100_bytes_on_a_character_s_edge() {
    assert_eq!(safe_name(&"x".repeat(200)), "x".repeat(100));
    assert_eq!(safe_name(&"配信".repeat(50)), "配信".repeat(16) + "配", "3-byte characters: 99 bytes");
    assert_eq!(safe_name(&"😀".repeat(40)), "😀".repeat(25), "4-byte characters: 100 bytes");
    let name = named(&"x".repeat(200), (0.0, 1.0));
    assert_eq!(name.len(), 100 + " 00h00m00.000s-00h00m01.000s.mp4".len(), "the range keeps its room");
}

/// "Windows refuses ... device names, with any extension" (core
/// clips.rs:212); a title with nothing safe left is "Clip".
#[test]
fn a_title_with_nothing_safe_left_or_a_device_name_is_named_clip() {
    for nothing in ["", "   ", "...", "???", "<>:\"/\\|?*%", ". ."] {
        assert_eq!(safe_name(nothing), "Clip", "{nothing:?}");
    }
    for device in ["con", "COM1", "CON.x", "nul.tv", "CONIN$", "conout$", "Com1 .txt", "LPT¹", "Aux", "PRN"] {
        assert_eq!(safe_name(device), "Clip", "{device}");
    }
    for not_a_device in ["COMA", "ABC1", "COM", "LPT", "Console", "NULL"] {
        assert_eq!(safe_name(not_a_device), not_a_device, "only COM1 to COM9 and LPT1 to LPT9 are devices");
    }
    assert_eq!(named("con", (0.0, 1.0)), "Clip 00h00m00.000s-00h00m01.000s.mp4");
}

/// "The file extension of a clip (or rendered edit) in `format`: sound alone
/// is M4A (AAC) or WAV (PCM)" (core clips.rs:107); CHANGELOG.md 0.4.0, "Clips
/// and edits can be made as MOV".
#[test]
fn a_clip_with_a_picture_is_mp4_or_mov_and_sound_alone_m4a_or_wav() {
    assert_eq!(extension(false, ClipFormat::Mp4), "mp4");
    assert_eq!(extension(false, ClipFormat::Mov), "mov");
    assert_eq!(extension(true, ClipFormat::Mp4), "m4a");
    assert_eq!(extension(true, ClipFormat::Mov), "wav");
    for (audio, format, ext) in [
        (false, ClipFormat::Mp4, ".mp4"),
        (false, ClipFormat::Mov, ".mov"),
        (true, ClipFormat::Mp4, ".m4a"),
        (true, ClipFormat::Mov, ".wav"),
    ] {
        assert!(file_name(None, "x", (0.0, 1.0), audio, format).ends_with(ext), "{ext}");
    }
}

/// "`name` in `dir`, or with " (2)", " (3)"… before its extension if taken"
/// (core clips.rs:224).
#[test]
fn a_name_taken_gets_the_next_free_number() {
    let dir = crate::TempDir::new("clip-names");
    assert_eq!(unique_path(&dir, "a 1m00s.mp4"), dir.join("a 1m00s.mp4"));
    fs::write(dir.join("a 1m00s.mp4"), b"").unwrap();
    assert_eq!(unique_path(&dir, "a 1m00s.mp4"), dir.join("a 1m00s (2).mp4"));
    fs::write(dir.join("a 1m00s (2).mp4"), b"").unwrap();
    assert_eq!(unique_path(&dir, "a 1m00s.mp4"), dir.join("a 1m00s (3).mp4"));
}

// ------------------------------------------------------------- encodings

fn encoding(codec: ClipCodec, quality: ClipQuality) -> ClipEncoding {
    ClipEncoding { codec, quality }
}

/// "A codec or quality changed in Settings makes a file made otherwise out
/// of date, as a change of format does" (core clips.rs:118, `made_as`).
/// The 0.7.2 bug, "changing the encoding and remaking a clip from the same
/// slice does nothing" (0.7.2.md, "The owner's walkthrough", Fix before the
/// tag, walk-clips), was fixed in 8896230 ("A clip is made again after a
/// change of codec or quality"); this is its core half.
#[test]
fn a_file_is_as_wanted_only_in_its_format_codec_and_quality() {
    use ClipCodec::*;
    use ClipQuality::*;
    let h264 = encoding(H264, Standard);
    let mp4 = Path::new("/c/1.mp4");
    assert!(made_as(mp4, Some(h264), h264));
    assert!(!made_as(mp4, Some(h264), encoding(H265, Standard)), "another codec");
    assert!(!made_as(mp4, Some(h264), encoding(H264, High)), "another quality");
    assert!(!made_as(mp4, Some(h264), encoding(H264, Proxy)), "another quality");
    assert!(!made_as(mp4, Some(h264), encoding(Dnxhr, Proxy)), "another format");
    let mov = Path::new("/c/1.mov");
    assert!(made_as(mov, Some(encoding(Prores, High)), encoding(Prores, High)));
    assert!(!made_as(mov, Some(encoding(Prores, Standard)), encoding(Dnxhr, Standard)), "ProRes against DNxHR");
    assert!(!made_as(mov, Some(encoding(Prores, Standard)), encoding(Prores, High)), "another quality");
    assert!(!made_as(mov, Some(encoding(Dnxhr, Proxy)), h264), "a MOV where an MP4 is wanted");
}

/// "else as a file of its format was made before that was (`Settings::
/// clip_encoding`)" (core clips.rs:118): an MP4 as H.264 Standard, a MOV as
/// DNxHR Proxy.
#[test]
fn a_file_from_before_codecs_were_recorded_reads_as_its_format_was_made_then() {
    use ClipCodec::*;
    use ClipQuality::*;
    let (mp4, mov) = (Path::new("/c/1.mp4"), Path::new("/c/1.mov"));
    assert!(made_as(mp4, None, encoding(H264, Standard)));
    assert!(!made_as(mp4, None, encoding(H264, High)));
    assert!(!made_as(mp4, None, encoding(H265, Standard)));
    assert!(made_as(mov, None, encoding(Dnxhr, Proxy)));
    assert!(!made_as(mov, None, encoding(Dnxhr, Standard)));
    assert!(!made_as(mov, None, encoding(Prores, Proxy)));
    assert!(!made_as(mp4, None, encoding(Dnxhr, Proxy)), "a MOV is wanted");
}

/// "Sound alone is the same in every codec of its format" (core
/// clips.rs:121).
#[test]
fn sound_alone_is_as_wanted_in_any_codec_of_its_format() {
    use ClipCodec::*;
    use ClipQuality::*;
    let m4a = Path::new("/c/1.m4a");
    for codec in [H264, H265] {
        for quality in [Proxy, Standard, High] {
            let now = encoding(codec, quality);
            assert!(made_as(m4a, Some(encoding(H264, Standard)), now), "{codec:?} {quality:?}");
            assert!(made_as(m4a, None, now), "{codec:?} {quality:?}");
        }
    }
    let wav = Path::new("/c/1.wav");
    for codec in [Dnxhr, Prores] {
        for quality in [Proxy, Standard, High] {
            assert!(made_as(wav, Some(encoding(Dnxhr, Proxy)), encoding(codec, quality)), "{codec:?} {quality:?}");
        }
    }
    assert!(!made_as(m4a, None, encoding(Dnxhr, Proxy)), "sound alone of the other format is not as wanted");
    assert!(!made_as(wav, None, encoding(H264, Standard)), "sound alone of the other format is not as wanted");
}

/// What each codec and quality asks ffmpeg for: "the encoder and the one
/// setting that makes the step, and the container and sound" (core
/// clips.rs:136; plans/clip-encodings.md, "The plan": CRF 23/18/14 for H.264,
/// 26/22/18 for H.265, the DNxHR and ProRes profiles; CHANGELOG.md 0.7.2,
/// "Clips come in more formats.").
#[test]
fn each_codec_and_quality_asks_ffmpeg_for_its_encoder_and_step() {
    use ClipCodec::*;
    use ClipQuality::*;
    let args = |codec, quality, video| output_args(encoding(codec, quality), video).join(" ");
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
        has(H264, quality, &["-c:v libx264 -preset veryfast", &crf(step), "-pix_fmt yuv420p", mp4]);
    }
    for (quality, step) in [(Proxy, "26"), (Standard, "22"), (High, "18")] {
        has(H265, quality, &["-c:v libx265 -preset fast", &crf(step), "-pix_fmt yuv420p", mp4]);
    }
    for (quality, profile) in [(Proxy, "dnxhr_lb"), (Standard, "dnxhr_sq"), (High, "dnxhr_hq")] {
        has(Dnxhr, quality, &[&format!("-c:v dnxhd -profile:v {profile} -pix_fmt yuv422p "), mov]);
    }
    for (quality, profile) in [(Proxy, "0"), (Standard, "1"), (High, "3")] {
        has(Prores, quality, &[&format!("-c:v prores_ks -profile:v {profile} -pix_fmt yuv422p10le "), mov]);
    }
    // Sound alone: no picture's settings, AAC or a WAV.
    assert_eq!(
        args(H265, High, false),
        "-map_metadata -1 -map_chapters -1 -c:a aac -b:a 192k -movflags +faststart -f mp4"
    );
    assert_eq!(args(Prores, Proxy, false), "-map_metadata -1 -map_chapters -1 -c:a pcm_s16le -f wav");
}

/// "hvc1: QuickTime and Final Cut refuse H.265 tagged otherwise" (core
/// clips.rs:161; plans/clip-encodings.md, "The plan").
#[test]
fn h265_is_tagged_hvc1_for_apple_s_players() {
    use ClipCodec::*;
    use ClipQuality::*;
    for quality in [Proxy, Standard, High] {
        let args = output_args(encoding(H265, quality), true).join(" ");
        assert!(args.contains("-tag:v hvc1"), "{quality:?}: {args}");
    }
    for codec in [H264, Dnxhr, Prores] {
        let args = output_args(encoding(codec, Standard), true).join(" ");
        assert!(!args.contains("-tag:v"), "{codec:?}: {args}");
    }
}

/// The real ffmpeg and ffprobe, if they are installed.
struct Real {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

impl Real {
    fn find() -> Option<Self> {
        match (which::which("ffmpeg"), which::which("ffprobe")) {
            (Ok(ffmpeg), Ok(ffprobe)) => Some(Real { ffmpeg, ffprobe }),
            _ => {
                eprintln!("skipped: no ffmpeg on PATH");
                None
            }
        }
    }

    /// Runs ffmpeg with `args`; whether it made its file.
    fn make(&self, args: &[&str]) -> bool {
        let mut cmd = std::process::Command::new(&self.ffmpeg);
        cmd.args(["-v", "error", "-y"]).args(args).status().is_ok_and(|s| s.success())
    }

    /// What ffprobe prints of `file` for `args`.
    fn probe(&self, file: &Path, args: &[&str]) -> String {
        let out =
            std::process::Command::new(&self.ffprobe).args(["-v", "error"]).args(args).arg(file).output().unwrap();
        String::from_utf8(out.stdout).unwrap()
    }

    /// A ten-second VP9 and Opus source with keyframes four seconds apart,
    /// so a cut that only copied would start early. `extra` goes among its
    /// inputs and options.
    fn source(&self, dir: &Path, size: &str, extra: &[&str]) -> PathBuf {
        let path = dir.join("source.mkv");
        let input = format!("testsrc2=size={size}:rate=30:duration=10");
        let mut args = vec!["-f", "lavfi", "-i", &input, "-f", "lavfi", "-i", "sine=duration=10"];
        args.extend(extra);
        args.extend(["-c:v", "libvpx-vp9", "-g", "120", "-c:a", "libopus"]);
        args.push(path.to_str().unwrap());
        assert!(self.make(&args));
        path
    }

    /// Cuts `range` of `source` into `out`; None where this ffmpeg lacks the encoder.
    fn cut(&self, source: &Path, out: &Path, range: (f64, f64), encoding: ClipEncoding) -> Option<()> {
        match cut(&self.ffmpeg, source, out, range, false, encoding, &AtomicBool::new(false), |_| {}) {
            Ok(()) => Some(()),
            Err(e) => {
                assert!(format!("{e:#}").contains("Unknown encoder"), "{e:#}");
                eprintln!("skipped {:?}: this ffmpeg lacks its encoder", encoding.codec);
                None
            }
        }
    }
}

/// "H.264 and H.265 have a keyframe a second, so editors seek them quickly"
/// (core clips.rs:4 and :138). Asked of ffmpeg for each, and read back from
/// the real one's file.
#[test]
fn h264_and_h265_have_a_keyframe_every_second() {
    use ClipCodec::*;
    use ClipQuality::*;
    for (codec, quality) in [(H264, Proxy), (H264, High), (H265, Proxy), (H265, High)] {
        let args = output_args(encoding(codec, quality), true).join(" ");
        assert!(args.contains("-force_key_frames expr:gte(t,n_forced*1)"), "{codec:?}: {args}");
    }
    for codec in [Dnxhr, Prores] {
        let args = output_args(encoding(codec, Standard), true).join(" ");
        assert!(!args.contains("-force_key_frames"), "{codec:?} is all keyframes already: {args}");
    }
    let Some(real) = Real::find() else { return };
    let dir = crate::TempDir::new("clip-keyframes");
    let source = real.source(&dir, "320x180", &[]);
    for codec in [H264, H265] {
        let out = dir.join(format!("{codec:?}.mp4"));
        if real.cut(&source, &out, (2.5, 5.0), encoding(codec, Standard)).is_none() {
            continue;
        }
        let keyframes = real.probe(
            &out,
            &["-select_streams", "v", "-skip_frame", "nokey", "-show_entries", "frame=pts_time", "-of", "csv=p=0"],
        );
        // 2.5 s: keyframes at 0, 1 and 2 s, where a source's own are 4 s apart.
        assert_eq!(keyframes.lines().count(), 3, "a keyframe a second, {codec:?}: {keyframes}");
    }
}

/// "None of the source's tags or chapters: a phone's recording carries where
/// it was shot, a recorder's the account's name" (core clips.rs:143).
#[test]
fn a_clip_keeps_none_of_its_source_s_tags_or_chapters() {
    let start = "-map_metadata -1 -map_chapters -1 ";
    assert!(output_args(ClipEncoding::default(), true).join(" ").starts_with(start));
    assert!(output_args(ClipEncoding::default(), false).join(" ").starts_with(start));
    let Some(real) = Real::find() else { return };
    let dir = crate::TempDir::new("clip-tags");
    let notes = dir.join("notes.txt");
    let chapter = "[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=5000\ntitle=Opening\n";
    fs::write(&notes, format!(";FFMETADATA1\nartist=Someone\nlocation=+40.7128-074.0060/\n{chapter}")).unwrap();
    let source =
        real.source(&dir, "320x180", &["-i", notes.to_str().unwrap(), "-map_metadata", "2", "-map_chapters", "2"]);
    let shown = ["-show_entries", "format_tags:chapter_tags", "-of", "default=nw=1"];
    let before = real.probe(&source, &shown);
    assert!(before.contains("Someone") && before.contains("Opening"), "the source carries them: {before}");
    let out = dir.join("clip.mp4");
    real.cut(&source, &out, (2.5, 5.0), ClipEncoding::default()).unwrap();
    let after = real.probe(&out, &shown);
    assert!(!after.contains("Someone") && !after.contains("40.7") && !after.contains("Opening"), "{after}");
}

/// "video at most `height` tall, the best there is; else the smallest taller
/// one ...; only then audio alone" (core clips.rs:234).
#[test]
fn a_section_is_video_at_most_the_height_asked_else_the_smallest_taller_then_sound() {
    let s = selector(720);
    let choices: Vec<String> = s.split('/').map(|c| c.replace(crate::playable::WEB, "")).collect();
    assert_eq!(
        choices,
        ["bv*[height<=720]+ba", "b[height<=720]", "wv*[height>720]+ba", "w[height>720]", "ba", "b"],
        "audio alone comes last"
    );
    assert!(s.split('/').all(|c| c.split('+').all(|f| f.ends_with(crate::playable::WEB))), "only streams over the web");
}

// ------------------------------------------------------------------ cuts

/// A minute of fake video, and the fake ffmpeg that cuts it.
struct Cuts {
    dir: crate::TempDir,
    ffmpeg: Ffmpeg,
    source: PathBuf,
}

impl Cuts {
    fn new(name: &str) -> Self {
        let dir = crate::TempDir::new(name);
        let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
        let source = dir.join("vod.mp4");
        Media::video(60.0).write(&source);
        Cuts { dir, ffmpeg, source }
    }

    fn out(&self, name: &str) -> PathBuf {
        self.dir.join("clips").join(name)
    }

    /// A cut of `range`, with its progress.
    fn cut_with(
        &self,
        out: &Path,
        range: (f64, f64),
        audio_only: bool,
        encoding: ClipEncoding,
    ) -> (Result<()>, Vec<f64>) {
        let mut seen = Vec::new();
        let never = AtomicBool::new(false);
        let done = cut(self.ffmpeg.path(), &self.source, out, range, audio_only, encoding, &never, |p| seen.push(p));
        (done, seen)
    }

    fn cut(&self, out: &Path, range: (f64, f64)) -> Result<()> {
        self.cut_with(out, range, false, ClipEncoding::default()).0
    }

    /// Every file in `clips/`, hidden ones too.
    fn left_in_clips(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dir.join("clips"))
            .map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }
}

/// "they start and end on the exact frame asked for" (core clips.rs:1), cut
/// from `source` (core clips.rs:251).
#[test]
fn a_cut_is_exactly_its_stretch_of_the_source() {
    let cuts = Cuts::new("cut-exact");
    let out = cuts.out("clip.mp4");
    cuts.cut(&out, (12.345, 20.5)).unwrap();
    let made = Media::read(&out).unwrap();
    assert!((made.duration - (20.5 - 12.345)).abs() < 1e-3, "{}", made.duration);
    let (from, at) = made.cut_from.unwrap();
    assert_eq!(from, cuts.source.to_string_lossy());
    assert!((at - 12.345).abs() < 1e-3, "{at}");
    assert_eq!(cuts.left_in_clips(), ["clip.mp4"]);
}

/// "Cuts are at frames' times: half a millisecond early, so neither rounding
/// nor a container's time base puts the first frame before the seek point
/// (and drops it)" (the comment at core clips.rs:269).
#[test]
fn a_cut_seeks_half_a_millisecond_early_so_its_first_frame_is_kept() {
    let cuts = Cuts::new("cut-early");
    let out = cuts.out("clip.mp4");
    cuts.cut(&out, (12.0, 20.0)).unwrap();
    let made = Media::read(&out).unwrap();
    assert!((made.cut_from.unwrap().1 - 11.9995).abs() < 1e-6);
    assert!((made.duration - 8.0).abs() < 1e-6, "the length is still the span's: {}", made.duration);
    cuts.cut(&out, (0.0002, 1.0)).unwrap();
    assert_eq!(Media::read(&out).unwrap().cut_from.unwrap().1, 0.0, "never before the start of the source");
}

/// "replacing it, only once the new one is whole" (core clips.rs:251).
#[test]
fn a_cut_replaces_its_file_only_once_the_new_one_is_whole() {
    let cuts = Cuts::new("cut-replaces");
    let out = cuts.out("clip.mp4");
    fs::create_dir_all(out.parent().unwrap()).unwrap();
    fs::write(&out, b"the old clip, whole").unwrap();
    cuts.ffmpeg.fails_at(0.5, "Error while decoding stream #0:0");
    cuts.cut(&out, (10.0, 20.0)).unwrap_err();
    assert_eq!(fs::read(&out).unwrap(), b"the old clip, whole", "halfway through, the old one is as it was");
    cuts.ffmpeg.works();
    cuts.cut(&out, (10.0, 20.0)).unwrap();
    assert!((Media::read(&out).unwrap().duration - 10.0).abs() < 1e-3, "whole, it replaces the old one");
}

/// A cut that fails says "ffmpeg couldn't make the clip: " and ffmpeg's last
/// lines (core clips.rs:251, `tools::encode`), and leaves no part file.
#[test]
fn a_cut_that_fails_leaves_no_part_file_and_says_ffmpeg_s_reason() {
    let cuts = Cuts::new("cut-fails");
    cuts.ffmpeg.fails_at(0.5, "Invalid data found when processing input");
    let out = cuts.out("clip.mp4");
    let err = format!("{:#}", cuts.cut(&out, (10.0, 20.0)).unwrap_err());
    assert!(err.contains("ffmpeg couldn't make the clip") && err.contains("Invalid data found"), "{err}");
    assert!(cuts.left_in_clips().is_empty(), "{:?}", cuts.left_in_clips());
    // And ffmpeg's own words when it lacks the encoder.
    cuts.ffmpeg.works();
    cuts.ffmpeg.lacks("libx265");
    let h265 = encoding(ClipCodec::H265, ClipQuality::Standard);
    let err = format!("{:#}", cuts.cut_with(&out, (10.0, 20.0), false, h265).0.unwrap_err());
    assert!(err.contains("Unknown encoder 'libx265'"), "{err}");
    assert!(cuts.left_in_clips().is_empty());
}

/// A stop partway "leaves nothing behind" (core clips.rs:251; `crate::
/// is_canceled`). Stopped while ffmpeg is held halfway.
#[test]
fn a_cut_stopped_partway_leaves_nothing_behind() {
    let cuts = Cuts::new("cut-stopped");
    let halfway = Gate::new();
    cuts.ffmpeg.holds_at(0.5, &halfway);
    let (out, stop) = (cuts.out("clip.mp4"), AtomicBool::new(false));
    let err = std::thread::scope(|s| {
        let running = s.spawn(|| {
            cut(cuts.ffmpeg.path(), &cuts.source, &out, (10.0, 30.0), false, ClipEncoding::default(), &stop, |_| {})
        });
        halfway.reached();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        halfway.open();
        running.join().unwrap().unwrap_err()
    });
    assert!(crate::is_canceled(&err), "{err:#}");
    assert!(cuts.left_in_clips().is_empty(), "{:?}", cuts.left_in_clips());
}

/// "reporting progress from 0 to 1" (core clips.rs:251).
#[test]
fn a_cut_reports_progress_from_0_to_1() {
    let cuts = Cuts::new("cut-progress");
    let (done, seen) = cuts.cut_with(&cuts.out("clip.mp4"), (10.0, 30.0), false, ClipEncoding::default());
    done.unwrap();
    assert!(seen.len() >= 2, "{seen:?}");
    assert!(seen.iter().all(|p| (0.0..=1.0).contains(p)), "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "only forward: {seen:?}");
    assert!(seen[0] < 1.0, "it starts short of the end: {seen:?}");
    assert_eq!(seen.last(), Some(&1.0));
}

/// "Sound alone (`audio_only`, and cover art is left out) makes an M4A or a
/// WAV" (core clips.rs:251).
#[test]
fn a_cut_of_sound_alone_has_no_picture_and_no_cover_art() {
    let cuts = Cuts::new("cut-sound");
    let m4a = cuts.out("clip.m4a");
    cuts.cut_with(&m4a, (10.0, 20.0), true, ClipEncoding::default()).0.unwrap();
    let made = Media::read(&m4a).unwrap();
    assert!(made.video.is_none(), "no picture");
    assert_eq!((made.audio.as_deref(), made.container.as_str()), (Some("aac"), "mp4"));
    assert!((made.duration - 10.0).abs() < 1e-3);
    let wav = cuts.out("clip.wav");
    let dnxhr = encoding(ClipCodec::Dnxhr, ClipQuality::Proxy);
    cuts.cut_with(&wav, (10.0, 20.0), true, dnxhr).0.unwrap();
    let made = Media::read(&wav).unwrap();
    assert!(made.video.is_none(), "no picture");
    assert_eq!((made.audio.as_deref(), made.container.as_str()), (Some("pcm_s16le"), "wav"));
}

/// "Every picture's sides must be even (4:2:0 and 4:2:2 halve them): `cut`
/// ... make them so" (core clips.rs:136). The real ffmpeg's is
/// `each_codec_is_read_back_by_ffprobe_as_it_was_asked`.
#[test]
fn a_picture_s_odd_sides_are_made_even() {
    let cuts = Cuts::new("cut-even");
    let picture = Video { width: 321, height: 181, ..Media::video(1.0).video.unwrap() };
    Media { video: Some(picture), ..Media::video(60.0) }.write(&cuts.source);
    let out = cuts.out("clip.mp4");
    cuts.cut(&out, (1.0, 5.0)).unwrap();
    let video = Media::read(&out).unwrap().video.unwrap();
    assert_eq!((video.width, video.height), (320, 180));
}

/// "A clip has to end after it starts" (core clips.rs:251 and :34): nothing
/// is made.
#[test]
fn a_cut_that_ends_before_it_starts_is_refused() {
    let cuts = Cuts::new("cut-refused");
    let out = cuts.out("clip.mp4");
    for range in [(20.0, 10.0), (10.0, 10.0), (10.0, f64::NAN), (f64::NAN, 10.0)] {
        let err = cuts.cut(&out, range).unwrap_err();
        assert_eq!(format!("{err:#}"), ENDS_BEFORE_START, "{range:?}");
    }
    assert!(!out.exists() && cuts.left_in_clips().is_empty());
}

/// "Copies `source`'s picture and sound ... into an MP4 at `out` as they are
/// (nothing encoded again ...)" (core clips.rs:295). It is the kept video's
/// (desktop videos.rs): see the pilot plan's unstated item 9.
#[test]
fn a_remux_copies_picture_and_sound_into_an_mp4_as_they_are() {
    let cuts = Cuts::new("remux");
    let picture = Video { codec: "vp9".into(), ..Media::video(1.0).video.unwrap() };
    let section =
        Media { video: Some(picture), audio: Some("opus".into()), container: "matroska".into(), ..Media::video(30.0) };
    section.write(&cuts.source);
    let out = cuts.out("kept.mp4");
    let mut seen = Vec::new();
    remux(cuts.ffmpeg.path(), &cuts.source, &out, 30.0, &AtomicBool::new(false), |p| seen.push(p)).unwrap();
    let kept = Media::read(&out).unwrap();
    assert_eq!(kept.container, "mp4");
    assert_eq!(kept.video, section.video, "the picture as it was");
    assert_eq!(kept.audio, section.audio, "the sound as it was");
    assert!((kept.duration - 30.0).abs() < 1e-3);
    assert_eq!(seen.last(), Some(&1.0));
    assert_eq!(cuts.left_in_clips(), ["kept.mp4"]);
    // And the real ffmpeg copies the streams as they are.
    let Some(real) = Real::find() else { return };
    let mkv = real.source(&cuts.dir, "160x90", &[]);
    let real_out = cuts.dir.join("real-kept.mp4");
    remux(&real.ffmpeg, &mkv, &real_out, 10.0, &AtomicBool::new(false), |_| {}).unwrap();
    let codecs = real.probe(&real_out, &["-show_entries", "stream=codec_name", "-of", "csv=p=0"]);
    assert_eq!(codecs.split_whitespace().collect::<Vec<_>>(), ["vp9", "opus"]);
}

/// "A small picture of `clip`'s first frame, as a PNG at `out`" (core
/// clips.rs:323).
#[test]
fn a_thumbnail_is_a_png_of_the_clip_s_first_frame() {
    let cuts = Cuts::new("thumbnail");
    let clip = cuts.out("clip.mp4");
    cuts.cut(&clip, (10.0, 20.0)).unwrap();
    let png = cuts.dir.join("pictures/thumb.png");
    thumbnail(cuts.ffmpeg.path(), &clip, &png).unwrap();
    assert!(fs::read(&png).unwrap().starts_with(b"\x89PNG"));
    // And the real one's.
    let Some(real) = Real::find() else { return };
    let source = real.source(&cuts.dir, "320x180", &[]);
    let real_clip = cuts.dir.join("real.mp4");
    real.cut(&source, &real_clip, (2.5, 5.0), ClipEncoding::default()).unwrap();
    let real_png = cuts.dir.join("pictures/real.png");
    thumbnail(&real.ffmpeg, &real_clip, &real_png).unwrap();
    assert!(fs::read(&real_png).unwrap().starts_with(b"\x89PNG"));
}

/// The fake against the real ffmpeg (testing.md, "Fakes, not mocks"): a cut
/// is as long as asked, H.264 and AAC, whole, with its progress to 1; sound
/// alone is sound alone; a stop leaves nothing behind; MOV is DNxHR and PCM.
#[test]
fn a_real_ffmpeg_cuts_as_the_fake_does() {
    let Some(real) = Real::find() else { return };
    let dir = crate::TempDir::new("clip-cut");
    let source = real.source(&dir, "320x180", &[]);
    let out = dir.join("clips").join("clip.mp4");
    let mut seen = Vec::new();
    let never = AtomicBool::new(false);
    cut(&real.ffmpeg, &source, &out, (2.5, 5.0), false, ClipEncoding::default(), &never, |p| seen.push(p)).unwrap();
    assert_eq!(seen.last(), Some(&1.0));
    let duration: f64 =
        real.probe(&out, &["-show_entries", "format=duration", "-of", "csv=p=0"]).trim().parse().unwrap();
    assert!((duration - 2.5).abs() < 0.05, "{duration}");
    let codecs = real.probe(&out, &["-show_entries", "stream=codec_name", "-of", "csv=p=0"]);
    assert_eq!(codecs.split_whitespace().collect::<Vec<_>>(), ["h264", "aac"]);
    assert!(!dir.join("clips").join(".clip.mp4.part").exists());

    let audio = dir.join("clips").join("clip.m4a");
    cut(&real.ffmpeg, &source, &audio, (0.0, 1.0), true, ClipEncoding::default(), &never, |_| {}).unwrap();
    let streams = real.probe(&audio, &["-show_entries", "stream=codec_type", "-of", "csv=p=0"]);
    assert_eq!(streams.trim(), "audio");

    let stop = AtomicBool::new(true);
    let stopped = dir.join("stopped.mp4");
    let err =
        cut(&real.ffmpeg, &source, &stopped, (0.0, 9.0), false, ClipEncoding::default(), &stop, |_| {}).unwrap_err();
    assert!(crate::is_canceled(&err));
    assert!(fs::read_dir(&*dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("stopped")));

    let dnxhr = encoding(ClipCodec::Dnxhr, ClipQuality::Proxy);
    let mov = dir.join("clips").join("clip.mov");
    cut(&real.ffmpeg, &source, &mov, (2.5, 5.0), false, dnxhr, &never, |_| {}).unwrap();
    let info = real.probe(&mov, &["-show_entries", "stream=codec_name,profile:format=format_name", "-of", "csv=p=0"]);
    assert!(info.contains("dnxhd,DNXHR LB") && info.contains("pcm_s16le") && info.contains("mov"), "{info}");
    let wav = dir.join("clips").join("clip.wav");
    cut(&real.ffmpeg, &source, &wav, (0.0, 1.0), true, dnxhr, &never, |_| {}).unwrap();
    assert!(fs::read(&wav).unwrap().starts_with(b"RIFF"));
}

/// "its picture read back: the codec, its profile, its tag and its pixels"
/// (tests/e2e/scenarios/clip-format.mjs, header): a two-second clip in each
/// codec, at two steps, as ffprobe reads it. The fake checked against the
/// real ffmpeg. Skipped where ffmpeg lacks an encoder.
#[test]
fn each_codec_is_read_back_by_ffprobe_as_it_was_asked() {
    let Some(real) = Real::find() else { return };
    let dir = crate::TempDir::new("clip-codecs");
    // Odd sides, which every codec here needs made even.
    let source = real.source(&dir, "321x181", &[]);
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
        let encoding = encoding(codec, quality);
        let out = dir.join(format!("{codec:?}-{quality:?}.{}", extension(false, encoding.format())));
        if real.cut(&source, &out, (0.5, 1.5), encoding).is_none() {
            continue;
        }
        let shown = "stream=codec_name,profile,pix_fmt,codec_tag_string,width,height";
        let info = real.probe(&out, &["-select_streams", "v", "-show_entries", shown, "-of", "default=nw=1"]);
        let field = |key: &str| {
            let line = info.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='));
            line.unwrap_or_else(|| panic!("no {key} in {info}"))
        };
        let got = ["codec_name", "profile", "pix_fmt", "codec_tag_string"].map(field).join(",");
        assert_eq!(got, want, "{codec:?} {quality:?}");
        assert_eq!((field("width"), field("height")), ("320", "180"), "{codec:?} {quality:?}: sides made even");
    }
}
