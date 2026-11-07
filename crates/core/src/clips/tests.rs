//! The specs of clips' titles, names, encodings and cuts.

/// A codec or quality changed in Settings makes a file made otherwise
/// out of date, as a change of format does; one made before what with
/// was recorded reads as its format's file was made then.
#[test]
fn a_file_is_made_as_asked_only_in_its_codec_and_quality() {
    use super::made_as;
    let e = |codec, quality| ClipEncoding { codec, quality };
    let h264 = e(ClipCodec::H264, ClipQuality::Standard);
    let mp4 = Path::new("/c/1.mp4");
    assert!(made_as(mp4, Some(h264), h264));
    assert!(!made_as(mp4, Some(h264), e(ClipCodec::H265, ClipQuality::Standard)));
    assert!(!made_as(mp4, Some(h264), e(ClipCodec::H264, ClipQuality::High)));
    assert!(!made_as(mp4, Some(h264), e(ClipCodec::Dnxhr, ClipQuality::Proxy)));
    let mov = Path::new("/c/1.mov");
    assert!(!made_as(
        mov,
        Some(e(ClipCodec::Prores, ClipQuality::Standard)),
        e(ClipCodec::Dnxhr, ClipQuality::Standard)
    ));
    assert!(made_as(mp4, None, h264));
    assert!(!made_as(mp4, None, e(ClipCodec::H265, ClipQuality::Standard)));
    assert!(made_as(mov, None, e(ClipCodec::Dnxhr, ClipQuality::Proxy)));
    assert!(made_as(Path::new("/c/1.m4a"), Some(h264), e(ClipCodec::H265, ClipQuality::High)));
    assert!(!made_as(Path::new("/c/1.wav"), None, h264));
}

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
    let name = |title: &str, start: f64, end: f64| file_path(None, None, title, (start, end), false, ClipFormat::Mp4).1;
    assert_eq!(name("Day 3: the \"big\" raid?", 3723.0, 3725.5), "Day 3 the big raid 01h02m03.000s-01h02m05.500s.mp4");
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
    cut(&ffmpeg, &source, &out, (2.5, 5.0), false, ClipEncoding::default(), &AtomicBool::new(false), |p| seen.push(p))
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
    let keyframes =
        probe(&["-select_streams", "v", "-skip_frame", "nokey", "-show_entries", "frame=pts_time", "-of", "csv=p=0"]);
    assert!(keyframes.lines().count() >= 3, "a keyframe a second: {keyframes}");
    assert!(!dir.join("clips").join(".clip.mp4.part").exists());

    let png = dir.join("thumb.png");
    thumbnail(&ffmpeg, &out, &png).unwrap();
    assert!(fs::read(&png).unwrap().starts_with(b"\x89PNG"));

    // Sound alone, and a stop partway leaves nothing behind.
    let audio = dir.join("clips").join("clip.m4a");
    cut(&ffmpeg, &source, &audio, (0.0, 1.0), true, ClipEncoding::default(), &AtomicBool::new(false), |_| {}).unwrap();
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
