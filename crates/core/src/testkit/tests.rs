//! The kit's own specs: each fake answers as its program does (checked
//! against the real one where it's installed), and the helpers leave the
//! library as the app would.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use super::*;
use crate::settings::ClipEncoding;
use crate::{audio, clips, fetch, local, net, tools};

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// A minute's video at `dir`/vod.mp4, as fake media.
fn a_minute(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("vod.mp4");
    Media::video(60.0).write(&path);
    path
}

#[test]
fn a_fake_ffmpegs_cut_is_the_stretch_asked_for_of_its_source() {
    let dir = TempDir::new("kit-cut");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let source = a_minute(&dir);
    let out = dir.join("clips/clip.mp4");
    let mut seen = Vec::new();
    clips::cut(ffmpeg.path(), &source, &out, (10.0, 20.0), false, ClipEncoding::default(), &never(), |p| seen.push(p))
        .unwrap();
    let made = Media::read(&out).unwrap();
    assert!((made.duration - 10.0).abs() < 1e-3, "{}", made.duration);
    let (from, start) = made.cut_from.clone().unwrap();
    assert_eq!(from, source.to_string_lossy());
    assert!((start - 10.0).abs() < 1e-3, "{start}");
    assert_eq!(made.video.unwrap().codec, "h264");
    assert_eq!(made.audio.as_deref(), Some("aac"));
    assert_eq!(seen.last(), Some(&1.0), "progress to the end");
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "progress only goes forward: {seen:?}");
    assert!(!dir.join("clips/.clip.mp4.part").exists());
}

#[test]
fn a_cut_of_a_cut_says_where_it_is_in_the_first() {
    let dir = TempDir::new("kit-cut-of-cut");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let source = a_minute(&dir);
    let (first, second) = (dir.join("a.mp4"), dir.join("b.m4a"));
    let cut = |from: &Path, to: &Path, range, sound| {
        clips::cut(ffmpeg.path(), from, to, range, sound, ClipEncoding::default(), &never(), |_| {}).unwrap()
    };
    cut(&source, &first, (10.0, 40.0), false);
    cut(&first, &second, (5.0, 6.0), true);
    let made = Media::read(&second).unwrap();
    let (from, start) = made.cut_from.unwrap();
    assert_eq!(from, source.to_string_lossy());
    assert!((start - 15.0).abs() < 1e-2, "{start}");
    assert!(made.video.is_none(), "sound alone");
}

#[test]
fn a_fake_ffmpeg_fails_partway_as_ffmpeg_does_and_the_cut_leaves_nothing() {
    let dir = TempDir::new("kit-fail");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    ffmpeg.fails_at(0.5, "Error while decoding stream #0:0: Invalid data found when processing input");
    let out = dir.join("clip.mp4");
    let cut =
        || clips::cut(ffmpeg.path(), &a_minute(&dir), &out, (0.0, 30.0), false, Default::default(), &never(), |_| {});
    let err = cut().unwrap_err();
    assert!(format!("{err:#}").contains("Invalid data found"), "{err:#}");
    assert!(!out.exists() && !dir.join(".clip.mp4.part").exists());
    ffmpeg.works();
    cut().unwrap();
}

#[test]
fn a_fake_ffmpeg_held_at_a_gate_is_stopped_by_a_cancel() {
    let dir = TempDir::new("kit-hold");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let gate = Gate::new();
    ffmpeg.holds_at(0.5, &gate);
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let (path, source, out) = (ffmpeg.path().to_path_buf(), a_minute(&dir), dir.join("clip.mp4"));
    let (done, result) = mpsc::channel();
    let (stop, outcome) = (cancel.clone(), out.clone());
    std::thread::spawn(move || {
        let made = clips::cut(&path, &source, &outcome, (0.0, 30.0), false, Default::default(), &stop, |_| {});
        done.send(made).unwrap();
    });
    gate.reached();
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    // The cut sees the stop at ffmpeg's next line, written once the gate opens.
    gate.open();
    let err = result.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap_err();
    assert!(crate::is_canceled(&err), "{err:#}");
    assert!(!out.exists() && !dir.join(".clip.mp4.part").exists());
}

/// A file the fake ffmpeg makes is there from its start, as ffmpeg's is,
/// but isn't media till it's whole: a cut held partway has its part file.
#[test]
fn a_fake_ffmpegs_file_is_there_as_it_is_written_and_media_once_whole() {
    let dir = TempDir::new("kit-writes");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let gate = Gate::new();
    ffmpeg.holds_at(0.5, &gate);
    let (path, source, out) = (ffmpeg.path().to_path_buf(), a_minute(&dir), dir.join("clip.mp4"));
    let (done, result) = mpsc::channel();
    let made = out.clone();
    std::thread::spawn(move || {
        done.send(clips::cut(&path, &source, &made, (0.0, 30.0), false, Default::default(), &never(), |_| {})).unwrap();
    });
    gate.reached();
    let part = dir.join(".clip.mp4.part");
    assert!(part.is_file(), "no part file while it's written");
    assert_eq!(Media::read(&part), None, "media before it's whole");
    gate.open();
    result.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();
    assert!(Media::read(&out).is_some_and(|m| (m.duration - 30.0).abs() < 1e-3));
    assert!(!part.exists());
}

/// A still the fake ffmpeg is held at has its input read already, as
/// ffmpeg has the file open while it decodes: the file deleted then, the
/// picture is still written once the gate opens.
#[test]
fn a_fake_ffmpegs_still_held_at_a_gate_has_read_its_input() {
    let dir = TempDir::new("kit-still");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let gate = Gate::new();
    ffmpeg.holds_stills(&gate);
    let (path, source, out) = (ffmpeg.path().to_path_buf(), a_minute(&dir), dir.join("clip.png"));
    let (done, result) = mpsc::channel();
    let (input, made) = (source.clone(), out.clone());
    std::thread::spawn(move || done.send(clips::thumbnail(&path, &input, &made)).unwrap());
    gate.reached();
    std::fs::remove_file(&source).unwrap();
    gate.open();
    result.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();
    assert!(out.is_file(), "no picture written");
}

/// What a file is made with, as ffmpeg was asked (its quality step,
/// profile, pixel format and tag), is in the fake's file; a picture copied
/// keeps its source's.
#[test]
fn a_fake_ffmpegs_file_keeps_what_it_was_made_with() {
    use crate::settings::{ClipCodec, ClipQuality};
    let dir = TempDir::new("kit-quality");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let source = a_minute(&dir);
    let made = |codec, quality, name: &str| {
        let out = dir.join(name);
        let encoding = ClipEncoding { codec, quality };
        clips::cut(ffmpeg.path(), &source, &out, (0.0, 5.0), false, encoding, &never(), |_| {}).unwrap();
        let v = Media::read(&out).unwrap().video.unwrap();
        (v.crf, v.profile, v.pix_fmt, v.tag)
    };
    let said = |s: &str| Some(s.to_string());
    assert_eq!(made(ClipCodec::H265, ClipQuality::High, "a.mp4"), (said("18"), None, said("yuv420p"), said("hvc1")));
    assert_eq!(made(ClipCodec::Dnxhr, ClipQuality::Proxy, "b.mov"), (None, said("dnxhr_lb"), said("yuv422p"), None));
    let copied = dir.join("copied.mp4");
    clips::remux(ffmpeg.path(), &dir.join("a.mp4"), &copied, 5.0, &never(), |_| {}).unwrap();
    assert_eq!(Media::read(&copied).unwrap().video.unwrap().crf, said("18"), "copied as it was");
}

/// The fake and the real ffmpeg, asked for H.265 and for DNxHR, make files
/// of the same pixel format and tag: the real one's read back by ffprobe,
/// the fake's from what it kept. Skipped where ffmpeg, ffprobe or either
/// encoder isn't installed.
#[test]
fn the_fake_ffmpeg_keeps_the_pixel_format_and_tag_the_real_one_makes() {
    use crate::settings::{ClipCodec, ClipQuality};
    let (Ok(real), Ok(ffprobe)) = (which::which("ffmpeg"), which::which("ffprobe")) else {
        eprintln!("skipped: no ffmpeg or ffprobe on PATH");
        return;
    };
    let has = tools::encoders(&real).unwrap_or_default();
    if !has.contains("libx265") || !has.contains("dnxhd") {
        eprintln!("skipped: this ffmpeg lacks libx265 or dnxhd");
        return;
    }
    let dir = TempDir::new("kit-real-quality");
    let fake = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let sample = std::process::Command::new(&real)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=2"])
        .args(["-f", "lavfi", "-i", "sine=duration=2", "-c:v", "libx264", "-c:a", "aac", "-shortest"])
        .arg(dir.join("real.mp4"))
        .status();
    assert!(sample.is_ok_and(|s| s.success()), "the sample");
    Media::video(2.0).write(&dir.join("fake.mp4"));
    // Each of what ffprobe says of the picture (`pix_fmt=yuv420p`), by name.
    let probed = |file: &Path, key: &str| {
        let out = std::process::Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=pix_fmt,codec_tag_string"])
            .args(["-of", "default=noprint_wrappers=1"])
            .arg(file)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.lines().find_map(|l| l.strip_prefix(&format!("{key}="))).map(str::to_string)
    };
    for (codec, ext) in [(ClipCodec::H265, "mp4"), (ClipCodec::Dnxhr, "mov")] {
        let encoding = ClipEncoding { codec, quality: ClipQuality::Standard };
        let cut = |ffmpeg: &Path, name: &str| {
            let out = dir.join(format!("{name}-{ext}.{ext}"));
            clips::cut(ffmpeg, &dir.join(format!("{name}.mp4")), &out, (0.0, 1.0), false, encoding, &never(), |_| {})
                .unwrap();
            out
        };
        let real_made = cut(&real, "real");
        let fake_made = Media::read(&cut(fake.path(), "fake")).unwrap().video.unwrap();
        assert_eq!(fake_made.pix_fmt, probed(&real_made, "pix_fmt"), "{codec:?}");
        // Untagged, a stream has what its container gives it.
        if let Some(asked) = fake_made.tag {
            assert_eq!(Some(asked), probed(&real_made, "codec_tag_string"), "{codec:?}");
        }
    }
}

/// A download held at the fake yt-dlp's gate has said nothing of how far
/// it is: what the app reads of its progress comes once it's let go, a
/// quarter at a time.
#[test]
fn a_fake_yt_dlp_held_says_nothing_of_its_progress_till_let_go() {
    let dir = TempDir::new("kit-yt-dlp-hold");
    let fake = YtDlp::at(dir.join("bin/yt-dlp"));
    let url = "https://www.youtube.com/watch?v=noa29qsTc4g";
    fake.serves(yt_dlp::details("youtube", "noa29qsTc4g", url, "The test video", 600.0));
    let yt_dlp = tools::YtDlp { path: fake.path().into(), js_runtime: None };
    let info_path = dir.join("info.json");
    fetch::probe(&yt_dlp, url, &info_path, &never()).unwrap();
    let gate = Gate::new();
    fake.holds(&gate);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (done, result) = mpsc::channel();
    let (heard, out) = (seen.clone(), dir.join("audio"));
    std::thread::spawn(move || {
        let report = |p| heard.lock().unwrap().push(p);
        done.send(fetch::download(&yt_dlp, &info_path, &out, fetch::Wanted::Audio, &never(), report).is_ok()).unwrap();
    });
    gate.reached();
    assert!(seen.lock().unwrap().is_empty(), "progress before it was let go");
    gate.open();
    assert_eq!(result.recv_timeout(std::time::Duration::from_secs(5)), Ok(true));
    assert_eq!(*seen.lock().unwrap(), [0.25, 0.5, 0.75, 1.0]);
}

#[test]
fn a_fake_ffmpeg_refuses_what_ffmpeg_refuses() {
    let dir = TempDir::new("kit-refuse");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let cut = |source: &Path, encoding| {
        let out = dir.join("x.mp4");
        clips::cut(ffmpeg.path(), source, &out, (0.0, 1.0), false, encoding, &never(), |_| {})
            .map_err(|e| format!("{e:#}"))
    };
    assert!(cut(&dir.join("gone.mp4"), ClipEncoding::default()).unwrap_err().contains("No such file"));
    std::fs::write(dir.join("text.mp4"), "not a video").unwrap();
    assert!(cut(&dir.join("text.mp4"), ClipEncoding::default()).unwrap_err().contains("Invalid data"));
    let sound = dir.join("sound.m4a");
    Media::sound(10.0).write(&sound);
    assert!(cut(&sound, ClipEncoding::default()).unwrap_err().contains("matches no streams"), "a picture of sound");
    ffmpeg.lacks("libx265");
    let h265 = ClipEncoding { codec: crate::settings::ClipCodec::H265, ..Default::default() };
    assert!(cut(&a_minute(&dir), h265).unwrap_err().contains("Unknown encoder 'libx265'"));
}

/// The fake's list of encoders leaves out those it lacks, and reads as the
/// real one's does (checked where ffmpeg is installed): AAC and PCM, which
/// every build has, among them.
#[test]
fn a_fake_ffmpeg_lists_the_encoders_it_has() {
    use crate::settings::ClipCodec;
    let dir = TempDir::new("kit-encoders");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let codecs = |path: &Path| ClipCodec::ALL.map(|c| tools::encoders(path).unwrap().contains(clips::encoder(c)));
    assert_eq!(codecs(ffmpeg.path()), [true; 4]);
    ffmpeg.lacks("libx265");
    ffmpeg.updated();
    assert_eq!(codecs(ffmpeg.path()), [true, false, true, true]);
    let Ok(real) = which::which("ffmpeg") else {
        eprintln!("skipped the real one: no ffmpeg on PATH");
        return;
    };
    for path in [real.as_path(), ffmpeg.path()] {
        let listed = tools::encoders(path).unwrap();
        assert!(listed.contains("aac") && listed.contains("pcm_s16le"), "{}: {listed:?}", path.display());
    }
}

#[test]
fn a_fake_ffmpeg_tells_a_files_details_and_makes_its_pictures() {
    let dir = TempDir::new("kit-details");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let path = dir.join("Day 3.mp4");
    Media::video(3723.5).write(&path);
    let info = local::probe(ffmpeg.path(), &path).unwrap();
    assert_eq!((info.title.as_str(), info.duration), ("Day 3", Some(3723.5)));
    let streams = local::codecs(ffmpeg.path(), &path).unwrap();
    assert_eq!((streams.video.as_deref(), streams.audio.as_deref()), (Some("h264"), Some("aac")));
    assert_eq!(local::frame_rate(ffmpeg.path(), &path).unwrap(), Some(30.0));
    let media = local::media(ffmpeg.path(), &path).unwrap();
    assert_eq!(media.picture.map(|p| (p.width, p.height)), Some((1920, 1080)));
    assert!(local::frame(ffmpeg.path(), &path, 5.0, 320).unwrap().starts_with(&[0xff, 0xd8]));
    let png = dir.join("thumb.png");
    clips::thumbnail(ffmpeg.path(), &path, &png).unwrap();
    assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
}

/// The fake and the real ffmpeg, given the same cut and the same file
/// (described, for the fake), come to the same: its details, the cut's
/// length, codecs and progress, and a missing source refused. Skipped
/// where ffmpeg isn't installed.
#[test]
fn the_fake_ffmpeg_answers_as_the_real_one() {
    let Ok(real) = which::which("ffmpeg") else {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let dir = TempDir::new("kit-real");
    let fake = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let made = std::process::Command::new(&real)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=4"])
        .args(["-f", "lavfi", "-i", "sine=duration=4", "-c:v", "libx264", "-c:a", "aac", "-shortest"])
        .arg(dir.join("real.mp4"))
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("skipped: this ffmpeg can't make the sample");
        return;
    }
    let mut described = Media::video(4.0);
    let picture = described.video.as_mut().unwrap();
    (picture.width, picture.height) = (320, 180);
    described.write(&dir.join("fake.mp4"));
    let mut outcomes = Vec::new();
    for (ffmpeg, name) in [(real.as_path(), "real"), (fake.path(), "fake")] {
        let source = dir.join(format!("{name}.mp4"));
        let info = local::probe(ffmpeg, &source).unwrap();
        let streams = local::codecs(ffmpeg, &source).unwrap();
        let media = local::media(ffmpeg, &source).unwrap();
        let out = dir.join(format!("{name}-clip.mp4"));
        let mut seen = Vec::new();
        clips::cut(ffmpeg, &source, &out, (1.0, 3.0), false, ClipEncoding::default(), &never(), |p| seen.push(p))
            .unwrap();
        let cut = local::probe(ffmpeg, &out).unwrap().duration.unwrap();
        let cut_streams = local::codecs(ffmpeg, &out).unwrap();
        let gone = dir.join("gone.mp4");
        let refused =
            clips::cut(ffmpeg, &gone, &dir.join("x.mp4"), (0.0, 1.0), false, Default::default(), &never(), |_| {});
        outcomes.push((
            (info.duration.unwrap() * 10.0).round(),
            (streams.video, streams.audio),
            media.picture.map(|p| (p.width, p.height, p.fps)),
            (cut * 10.0).round(),
            (cut_streams.video, cut_streams.audio),
            seen.last().copied(),
            refused.is_err(),
        ));
    }
    assert_eq!(outcomes[0], outcomes[1], "real, then fake");
}

#[test]
fn a_fake_yt_dlp_serves_details_and_sections_of_a_vod() {
    let dir = TempDir::new("kit-yt-dlp");
    let fake = YtDlp::at(dir.join("bin/yt-dlp"));
    let url = "https://www.youtube.com/watch?v=noa29qsTc4g";
    fake.serves(yt_dlp::details("youtube", "noa29qsTc4g", url, "The test video", 600.0));
    let yt_dlp = tools::YtDlp { path: fake.path().into(), js_runtime: None };
    let info_path = dir.join("info.json");
    let info = fetch::probe(&yt_dlp, url, &info_path, &never()).unwrap();
    assert_eq!((info.platform.as_str(), info.platform_id.as_str()), ("youtube", "noa29qsTc4g"));
    assert_eq!((info.title.as_str(), info.duration), ("The test video", Some(600.0)));
    let ffmpeg = dir.join("bin/ffmpeg");
    let section = |start, end| fetch::Wanted::Section {
        selector: "b",
        ffmpeg: &ffmpeg,
        start,
        end,
        matroska: true,
        sound_only: false,
    };
    let mut seen = Vec::new();
    let file =
        fetch::download(&yt_dlp, &info_path, &dir.join("section"), section(120.0, 180.0), &never(), |p| seen.push(p))
            .unwrap();
    assert_eq!(file, dir.join("section.mkv"));
    let media = Media::read(&file).unwrap();
    assert_eq!((media.duration, media.container.as_str()), (60.0, "matroska"));
    assert_eq!(media.cut_from, Some((url.to_string(), 120.0)));
    assert_eq!(seen.last(), Some(&1.0));
    // Refused by the site, and a link it doesn't have.
    fake.refuses(url);
    let err =
        fetch::download(&yt_dlp, &info_path, &dir.join("again"), section(0.0, 1.0), &never(), |_| {}).unwrap_err();
    assert!(format!("{err:#}").contains("403") || fetch::is_refused(&err), "{err:#}");
    assert!(fetch::probe(&yt_dlp, "https://www.youtube.com/watch?v=other", &info_path, &never()).is_err());
}

#[test]
fn a_fake_worker_says_the_words_heard_in_its_stretch() {
    let dir = TempDir::new("kit-worker");
    let fake = Worker::at(dir.join("vodscribe-worker"));
    fake.hears(said(10.0, "The Eagle has landed").into_iter().chain(said(100.0, "Later on")).collect());
    let audio = dir.join("audio.m4a");
    Media::sound(120.0).write(&audio);
    let mut cmd = tools::command(fake.path());
    cmd.args(["--start", "5", "--end", "90"]).arg(&audio);
    let out = tools::output(&mut cmd).unwrap();
    assert!(out.status.success());
    let lines: Vec<serde_json::Value> =
        String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let ends: Vec<f64> = lines.iter().map(|l| l["end"].as_f64().unwrap()).collect();
    assert_eq!(ends, [65.0, 90.0], "a batch a minute, to the end asked for");
    let words: Vec<&str> =
        lines.iter().flat_map(|l| l["words"].as_array().unwrap()).map(|w| w["text"].as_str().unwrap()).collect();
    assert_eq!(words, ["The", "Eagle", "has", "landed"]);
}

#[test]
fn a_programs_fake_ends_with_its_test() {
    let dir = TempDir::new("kit-fake-ends");
    let path = dir.join("bin/ffmpeg");
    {
        let _fake = Ffmpeg::at(&path);
        assert!(path.is_file(), "found where the app looks");
    }
    // Run again, it's the empty file there: no program at all.
    assert!(tools::output(&mut tools::command(&path)).is_err());
}

#[test]
fn recorded_answers_stand_for_the_network() {
    let net = Net::new();
    net.serves("https://kit-test.example/", "a body").answers("https://kit-test.example/gone", 404);
    assert_eq!(net::get_bytes("https://kit-test.example/x", 100, &never()).unwrap(), b"a body");
    let err = net::get_bytes("https://kit-test.example/gone/x", 100, &never()).unwrap_err();
    assert!(net::is_client_error(&err), "{err:#}");
    assert!(net::get_bytes("https://kit-test.example/x", 3, &never()).is_err(), "longer than allowed");
    drop(net);
    assert!(super::net::answer("https://kit-test.example/x").is_none(), "gone with its test");
}

#[test]
fn a_librarys_file_is_a_vod_read_where_it_is_with_what_is_said() {
    let mut library = Library::new();
    let vod = library.add_file("Day 3.mp4", &Media::video(60.0));
    assert_eq!((vod.info.title.as_str(), vod.info.duration), ("Day 3", Some(60.0)));
    let file = vod.source_path.clone().unwrap();
    assert!(Path::new(&file).starts_with(dunce::canonicalize(&library.files).unwrap()));
    assert_eq!(Media::read(Path::new(&file)), Some(Media::video(60.0)));
    library.says(vod.id, said(10.0, "The Eagle has landed"));
    let words = library.db.words_between(vod.id, 10.0, 11.0).unwrap();
    assert_eq!(words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["The", "Eagle"]);
    let site = library.add_site_vod("twitch", "2901646294", "A stream", 7200.0);
    assert_eq!(site.info.url, "https://www.twitch.tv/videos/2901646294");
}

/// A test's library is written without waiting on the disk (README.md:
/// the kit is there "so a unit or integration test does no slow I/O"):
/// nothing is flushed at a commit, so a test's time isn't the disk's (CI's
/// Windows runners stalled seconds on it, and a test's five seconds ran
/// out). What the app sees is the same: a commit is there at once for
/// another connection.
#[test]
fn a_librarys_commits_wait_on_no_flush_to_the_disk() {
    let library = Library::new();
    let synchronous: i64 = library.db.conn.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
    assert_eq!(synchronous, 0, "OFF");
    let vod = library.add_file("Day 3.mp4", &Media::video(60.0));
    let reader = crate::db::Db::open_reader(&library.paths).unwrap();
    assert_eq!(reader.vod(vod.id).unwrap().map(|v| v.id), Some(vod.id), "seen at once by another connection");
}

/// The schema of the library at `path`: each table's, index's, view's and
/// trigger's SQL, by name.
fn schema(path: &Path) -> Vec<(String, Option<String>)> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn.prepare("SELECT name, sql FROM sqlite_master ORDER BY name").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
}

/// A library at each format is that format, and opened it upgrades to the
/// library a new one is.
#[test]
fn a_library_at_a_format_upgrades_to_a_new_ones() {
    let dir = TempDir::new("kit-formats");
    let new = dir.join("new.sqlite");
    drop(crate::db::Db::open(&new).unwrap());
    let latest = crate::db::MIGRATIONS.len();
    for version in 0..=latest {
        let path = dir.join(format!("v{version}.sqlite"));
        let conn = at_version(&path, version);
        assert_eq!(format_of(&conn), version);
        drop(conn);
        let db = crate::db::Db::open(&path).unwrap();
        assert_eq!(format_of(&db.conn), latest);
        drop(db);
        assert_eq!(schema(&path), schema(&new), "format {version} upgraded");
    }
}

/// A library a newer version changed is at a format past this version's,
/// with that version's change, and opens as it is: neither upgraded nor
/// refused.
#[test]
fn a_library_a_newer_version_changed_opens_as_it_left_it() {
    let dir = TempDir::new("kit-newer");
    let path = dir.join("library.sqlite");
    drop(crate::db::Db::open(&path).unwrap());
    newer(&path, "ALTER TABLE renders DROP COLUMN encoding;");
    let db = crate::db::Db::open(&path).unwrap();
    assert_eq!(format_of(&db.conn), crate::db::MIGRATIONS.len() + 1);
    let columns: Vec<String> = db
        .conn
        .prepare("SELECT name FROM pragma_table_info('renders')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|c| c.unwrap())
        .collect();
    assert!(!columns.contains(&"encoding".to_string()), "{columns:?}");
}

/// A library at the format a release left is the release's: the same
/// tables, columns, indexes and triggers as its library in the fixtures.
#[test]
fn a_library_at_a_releases_format_is_laid_out_as_its_library() {
    let dir = TempDir::new("kit-released-format");
    for version in Released::versions() {
        let released = Released::new(&version);
        let format = format_of(&rusqlite::Connection::open(&released.library).unwrap());
        let path = dir.join(format!("{version}.sqlite"));
        drop(at_version(&path, format));
        assert_eq!(schema(&path), schema(&released.library), "{version}, format {format}");
    }
}

/// A library taken back to a format has that format's tables and columns,
/// with the rows it had, counted and found by search as that format does;
/// opened again, it is upgraded with them all.
#[test]
fn a_library_taken_back_keeps_its_rows_in_that_formats_columns() {
    let dir = TempDir::new("kit-back");
    let path = dir.join("library.sqlite");
    let shown = |db: &crate::db::Db| serde_json::to_value(db.vods().unwrap()).unwrap();
    let (vod, vods, clips) = {
        let mut db = crate::db::Db::open(&path).unwrap();
        let vod = super::library::add_site_vod(&db, "twitch", "1", "A stream", 600.0).id;
        super::library::says(&mut db, vod, said(10.0, "The Eagle has landed"));
        db.add_clip(vod, 10.0, 20.0, "Landing", true).unwrap();
        db.add_mark(vod, "chapter", 5.0, Some("Descent")).unwrap();
        (vod, shown(&db), db.clips(vod).unwrap())
    };
    let made = dir.join("made.sqlite");
    rusqlite::Connection::open(&path).unwrap().execute("VACUUM INTO ?1", [made.to_string_lossy()]).unwrap();
    let conn = back_to(&path, 36);
    assert_eq!(format_of(&conn), 36);
    // Each table's rows, in the columns both formats have, and no others.
    conn.execute("ATTACH ?1 AS made", [made.to_string_lossy()]).unwrap();
    let tables: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT name FROM pragma_table_list WHERE schema = 'main' AND type = 'table'").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().map(|t| t.unwrap()).collect()
    };
    for table in tables.iter().filter(|t| !t.starts_with("sqlite_")) {
        let columns = |schema: &str| -> Vec<String> {
            let mut stmt = conn.prepare("SELECT name FROM pragma_table_info(?1, ?2)").unwrap();
            stmt.query_map([table.as_str(), schema], |r| r.get(0)).unwrap().map(|c| c.unwrap()).collect()
        };
        let made_columns = columns("made");
        let both: Vec<String> = columns("main").into_iter().filter(|c| made_columns.contains(c)).collect();
        if both.is_empty() {
            continue;
        }
        let rows = |schema: &str| -> Vec<Vec<rusqlite::types::Value>> {
            let sql = format!("SELECT {} FROM {schema}.{table} ORDER BY rowid", both.join(", "));
            let mut stmt = conn.prepare(&sql).unwrap();
            stmt.query_map([], |r| (0..both.len()).map(|i| r.get(i)).collect()).unwrap().map(|r| r.unwrap()).collect()
        };
        assert_eq!(rows("main"), rows("made"), "{table}");
    }
    conn.execute("DETACH made", []).unwrap();
    let at_36 = dir.join("at-36.sqlite");
    drop(at_version(&at_36, 36));
    assert_eq!(schema(&path), schema(&at_36), "laid out as format 36");
    let one = |sql: &str| -> i64 { conn.query_row(sql, [vod], |r| r.get(0)).unwrap() };
    let found = |word: &str| -> i64 {
        conn.query_row("SELECT count(*) FROM segments_fts WHERE segments_fts MATCH ?1", [word], |r| r.get(0)).unwrap()
    };
    assert_eq!(one("SELECT pieces FROM vods WHERE id = ?1"), 2, "its own clip's piece and the clip's, counted once");
    assert_eq!(found("eagle"), 1, "found by search");
    assert_eq!(one("SELECT count(*) FROM marks WHERE vod_id = ?1 AND text = 'Descent'"), 1);
    // The format's triggers are there again.
    conn.execute(
        "INSERT INTO segments (vod_id, start, end, text, words) VALUES (?1, 30, 31, 'Tranquility', '[]')",
        [vod],
    )
    .unwrap();
    assert_eq!(found("tranquility"), 1);
    conn.execute("DELETE FROM segments WHERE vod_id = ?1 AND start = 30", [vod]).unwrap();
    drop(conn);
    let db = crate::db::Db::open(&path).unwrap();
    assert_eq!(format_of(&db.conn), crate::db::MIGRATIONS.len());
    assert_eq!(shown(&db), vods, "upgraded, the same VODs");
    assert_eq!(db.clips(vod).unwrap(), clips, "and clips");
}

/// Each release's library and settings file in the fixtures are there to
/// copy, oldest first, and a copy is the test's own: what it does leaves the
/// fixture as it is.
#[test]
fn a_releases_library_and_settings_are_copies_of_the_fixtures() {
    let versions = Released::versions();
    assert_eq!(versions, ["0.3.0", "0.4.0", "0.5.0", "0.5.1", "0.5.2-dev"]);
    for version in &versions {
        let fixture = Released::fixtures().join(format!("library-{version}.sqlite"));
        let before = std::fs::read(&fixture).unwrap();
        let released = Released::new(version);
        assert_eq!(released.library, released.dir().join("library.sqlite"));
        assert_eq!(released.settings, released.dir().join("settings.json"));
        assert_eq!(std::fs::read(&released.library).unwrap(), before, "{version}");
        let settings: serde_json::Value = serde_json::from_slice(&std::fs::read(&released.settings).unwrap()).unwrap();
        assert!(settings.get("theme").is_some(), "{version}'s settings: {settings}");
        drop(crate::db::Db::open(&released.library).unwrap());
        assert_eq!(std::fs::read(&fixture).unwrap(), before, "{version}'s fixture as it was");
    }
}

/// A fake writing more than a pipe holds to an app that has stopped
/// reading it still ends when the app stops it, as a killed program does.
#[test]
fn a_fake_stuck_writing_ends_when_stopped() {
    let dir = TempDir::new("kit-stuck");
    let fake = super::tools::fake(dir.join("bin/chatty"), |run| {
        run.out_bytes(&vec![b'x'; 1 << 20]);
        0
    });
    let mut child = tools::spawn(&mut tools::command(fake.path())).unwrap();
    // A byte read, so it's writing; the rest goes unread, the pipe full.
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0u8];
    std::io::Read::read_exact(&mut stdout, &mut first).unwrap();
    let (done, stopped) = mpsc::channel();
    std::thread::spawn(move || {
        child.kill_tree();
        done.send(child.wait().unwrap().success()).unwrap();
    });
    assert_eq!(stopped.recv_timeout(std::time::Duration::from_secs(5)), Ok(false), "stopped, not hung");
    drop(stdout);
}

/// A fake dropped takes back only its own answer: a newer fake at the same
/// path still answers.
#[test]
fn a_fake_dropped_leaves_a_newer_one_at_its_path() {
    let dir = TempDir::new("kit-refake");
    let path = dir.join("bin/tool");
    let older = super::tools::fake(&path, |_| 3);
    let newer = super::tools::fake(&path, |_| 0);
    drop(older);
    assert!(tools::status(&mut tools::command(newer.path())).unwrap().success());
}

/// A fake video's decoded frame says its number, read from its YUV and from
/// the RGB the player turns it into (video's range spread to the whole), and
/// a fake storyboard picture says its keyframe's time.
#[test]
fn a_fake_frame_says_its_number_and_a_storyboard_picture_its_time() {
    for n in [0, 1, 218, 219, 300, 1799, 47_960] {
        let yuv = media::numbered_frame(n, 64, 36);
        assert_eq!(yuv.len(), 64 * 36 * 3 / 2);
        assert_eq!(media::frame_number(&yuv, 64, 36, 0), Some(n));
        let rgb: Vec<u8> =
            yuv[..64 * 36].iter().flat_map(|&y| [((y as f64 - 16.0) * 255.0 / 219.0).round() as u8; 3]).collect();
        assert_eq!(media::frame_number(&rgb, 64, 36, 3), Some(n), "frame {n} as RGB");
    }
    assert_eq!(media::frame_number(&[255, 0, 0].repeat(4), 2, 2, 3), None, "not grey");
    assert_eq!(media::storyboard_time(&media::storyboard_jpeg(12.0, 160, 90)), Some(12.0));
    assert_eq!(media::storyboard_time(&[0xff, 0xd8, 0xff, 0xd9]), None);
}

/// Sound decoded as the waveform reads it (`peaks::read`, `audio::decode`:
/// `-vn -ac 1 -ar 16000 -f s16le pipe:1`) comes down the fake ffmpeg's
/// stdout, as ffmpeg's does: a sample for each 16,000th of a second asked
/// for, and no file named `pipe:1` where the test runs (an invalid name on
/// Windows). A file with no sound has none to give, and fails as ffmpeg
/// does, writing nothing.
#[test]
fn a_fake_ffmpeg_decodes_sound_down_its_output_never_to_a_file() {
    let dir = TempDir::new("kit-decode-sound");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let decoded = |path: &Path, start: f64, end: Option<f64>| {
        let mut samples = 0;
        audio::decode(ffmpeg.path(), path, start, end, &never(), |s| {
            samples += s.len();
            Ok(())
        })
        .map(|()| samples)
    };
    let mut silent = Media::video(3.0);
    silent.audio = None;
    let picture = dir.join("silent.mp4");
    silent.write(&picture);
    let refused = decoded(&picture, 0.0, None);
    let stray = || Path::new("pipe:1").exists();
    assert!(!stray(), "a file named pipe:1 made in {}", std::env::current_dir().unwrap().display());
    assert!(format!("{:#}", refused.unwrap_err()).contains("does not contain any stream"));
    let sound = dir.join("sound.m4a");
    Media::sound(3.0).write(&sound);
    assert_eq!(decoded(&sound, 0.0, None).unwrap(), 3 * audio::SAMPLE_RATE, "the whole file, mono");
    assert_eq!(decoded(&sound, 1.0, Some(2.5)).unwrap(), 3 * audio::SAMPLE_RATE / 2, "from 1 s for 1.5 s");
    assert!(!stray());
}

/// The fake yt-dlp writes a download as it comes, to a part file beside
/// it, as yt-dlp does: held partway, the part file is there and no file
/// yet (`fetch::part_files` finds it); stopped there, it's left; let go,
/// it's renamed into place, whole.
#[test]
fn a_fake_yt_dlps_download_is_a_part_file_till_its_whole() {
    let dir = TempDir::new("kit-yt-dlp-part");
    let fake = YtDlp::at(dir.join("bin/yt-dlp"));
    let url = "https://www.youtube.com/watch?v=noa29qsTc4g";
    fake.serves(yt_dlp::details("youtube", "noa29qsTc4g", url, "The test video", 600.0));
    let yt_dlp = std::sync::Arc::new(tools::YtDlp { path: fake.path().into(), js_runtime: None });
    let info_path = dir.join("info.json");
    fetch::probe(&yt_dlp, url, &info_path, &never()).unwrap();
    let download = |out: std::path::PathBuf, stop: std::sync::Arc<AtomicBool>| {
        let (yt_dlp, info_path) = (yt_dlp.clone(), info_path.clone());
        std::thread::spawn(move || fetch::download(&yt_dlp, &info_path, &out, fetch::Wanted::Audio, &stop, |_| {}))
    };
    let gate = Gate::new();
    fake.holds(&gate);
    // Stopped while held: its part is left.
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stopped = download(dir.join("stopped"), stop.clone());
    gate.reached();
    let parts = fetch::part_files(&dir.join("stopped"));
    assert_eq!(parts.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(), [dir.join("stopped.m4a.part")]);
    assert!(parts[0].1 > 0, "nothing written yet");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(stopped.join().unwrap().is_err());
    assert!(dir.join("stopped.m4a.part").is_file(), "a stopped download's part file");
    assert!(!dir.join("stopped.m4a").exists());
    // Let go: renamed into place, whole.
    let whole = download(dir.join("whole"), std::sync::Arc::new(never()));
    eventually("held again", || fetch::part_files(&dir.join("whole")).len() == 1);
    gate.open();
    assert_eq!(whole.join().unwrap().unwrap(), dir.join("whole.m4a"));
    assert!(fetch::part_files(&dir.join("whole")).is_empty(), "its part file left");
    assert_eq!(Media::read(&dir.join("whole.m4a")).map(|m| m.duration), Some(600.0));
}

/// The fake ffmpeg held at a file's details has read the file and answers
/// once let go, as a probe of a slow disk does (`local::probe`, the
/// player's open).
#[test]
fn a_fake_ffmpeg_held_at_a_files_details_answers_once_let_go() {
    let dir = TempDir::new("kit-details-hold");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let path = a_minute(&dir);
    let gate = Gate::new();
    ffmpeg.holds_details(&gate);
    let probing = std::thread::spawn({
        let (ffmpeg, path) = (ffmpeg.path().to_path_buf(), path.clone());
        move || local::probe(&ffmpeg, &path).map(|info| info.duration)
    });
    gate.reached();
    assert!(!probing.is_finished(), "answered while held");
    gate.open();
    assert_eq!(probing.join().unwrap().unwrap(), Some(60.0));
}

/// The fake worker says words at the times of what its file is cut from:
/// a file cut from second 60 says the word at 70 ten seconds in. Told it
/// works again, it no longer fails, and hears what it heard.
#[test]
fn a_fake_worker_says_words_where_its_files_source_says_them() {
    let dir = TempDir::new("kit-worker-cut");
    let fake = Worker::at(dir.join("vodscribe-worker"));
    fake.hears(said(70.0, "In the section").into_iter().chain(said(10.0, "Before it")).collect());
    let mut section = Media::sound(60.0);
    section.cut_from = Some(("https://www.youtube.com/watch?v=noa29qsTc4g".into(), 60.0));
    let audio = dir.join("section.m4a");
    section.write(&audio);
    let words = |fake: &Worker| -> Result<Vec<(String, f64)>, String> {
        let mut cmd = tools::command(fake.path());
        cmd.arg(&audio);
        let out = tools::output(&mut cmd).unwrap();
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        let lines = String::from_utf8(out.stdout).unwrap();
        let lines = lines.lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap());
        Ok(lines
            .flat_map(|l| l["words"].as_array().unwrap().clone())
            .map(|w| (w["text"].as_str().unwrap().to_string(), w["start"].as_f64().unwrap()))
            .collect())
    };
    assert_eq!(words(&fake).unwrap(), [("In".to_string(), 10.0), ("the".into(), 10.5), ("section".into(), 11.0)]);
    fake.fails_at(5.0, "Error: the model couldn't be read");
    assert!(words(&fake).unwrap_err().contains("couldn't be read"));
    fake.works();
    assert_eq!(words(&fake).unwrap().len(), 3);
}

/// A look-up of a link's details held at the fake yt-dlp's gate answers
/// once let go, and a cancel meanwhile stops it, as an add's does.
#[test]
fn a_fake_yt_dlps_details_held_answer_once_let_go() {
    let dir = TempDir::new("kit-yt-dlp-details");
    let fake = YtDlp::at(dir.join("bin/yt-dlp"));
    let url = "https://www.youtube.com/watch?v=noa29qsTc4g";
    fake.serves(yt_dlp::details("youtube", "noa29qsTc4g", url, "The test video", 600.0));
    let gate = Gate::new();
    fake.holds_details(&gate);
    let yt_dlp = tools::YtDlp { path: fake.path().into(), js_runtime: None };
    let info_path = dir.join("info.json");
    let probing = std::thread::spawn(move || fetch::probe(&yt_dlp, url, &info_path, &never()).map(|i| i.title));
    gate.reached();
    assert!(!probing.is_finished(), "answered while held");
    gate.open();
    assert_eq!(probing.join().unwrap().unwrap(), "The test video");
}

/// A gate no run reached fails its test, and can still be opened after,
/// as a test's end opens what it held: the test fails, not the process.
#[test]
fn a_gate_never_reached_fails_its_test_and_still_opens() {
    let gate = Gate::new();
    let missed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gate.reached()));
    assert!(missed.is_err(), "reached with no run at it");
    gate.open();
    gate.wait();
}

/// A test's quit stops the runs the app tracks that work in its folder, and
/// those tracked there after, as quitting stops processes
/// (`tools::stop_tracked`); a run another test's folder has, or one not
/// tracked, goes on. Once the test lets go of its folder, a run tracked
/// there runs.
#[test]
fn a_tests_quit_stops_only_the_runs_it_tracks() {
    let (ours, theirs, elsewhere) =
        (TempDir::new("kit-quit-ours"), TempDir::new("kit-quit-theirs"), TempDir::new("kit-quit-elsewhere"));
    // Each run works till it's stopped or the test lets it end.
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let fake = |at: &Path| {
        let done = done.clone();
        super::tools::fake(at.join("bin/tool"), move |run| {
            loop {
                if run.stopped() || done.load(std::sync::atomic::Ordering::SeqCst) {
                    return 0;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        })
    };
    let (in_ours, in_theirs, in_elsewhere) = (fake(&ours), fake(&theirs), fake(&elsewhere));
    let start = |program: &Path, arg: &Path, track: bool| {
        let child = tools::spawn(tools::command(program).arg(arg)).unwrap();
        let tracked = if track { child.track() } else { None };
        (child, tracked)
    };
    // Whether it ended as a stopped program does, once it has.
    let stopped = |child: &mut tools::Child| {
        let mut status = None;
        eventually("the run ended", || {
            status = child.try_wait().unwrap();
            status.is_some()
        });
        !status.unwrap().success()
    };
    let mut tracked = start(in_ours.path(), &ours.join("a.mp4"), true);
    // Another folder's program working on a path in ours is ours too, as
    // the app's worker works on its library's model.
    let mut working_here = start(in_elsewhere.path(), &ours.join("model"), true);
    let mut untracked = start(in_ours.path(), &ours.join("b.mp4"), false);
    let mut other = start(in_theirs.path(), &theirs.join("a.mp4"), true);
    super::tools::stop_tracked_in(&ours);
    let mut later = start(in_ours.path(), &ours.join("c.mp4"), true);
    super::tools::let_go(&ours);
    let mut next = start(in_ours.path(), &ours.join("d.mp4"), true);
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(stopped(&mut tracked.0), "a tracked run in the folder was stopped");
    assert!(stopped(&mut working_here.0), "a run working in the folder was stopped");
    assert!(stopped(&mut later.0), "a run tracked there after the quit was stopped at once");
    assert!(!stopped(&mut untracked.0), "a run not tracked went on");
    assert!(!stopped(&mut other.0), "another folder's run went on");
    assert!(!stopped(&mut next.0), "once the folder was let go of, a run tracked there ran");
}

/// A host a test names resolves where it says, so a link to it is checked
/// without a real lookup (`net::public_url`): one said to be public is
/// followed, as the name it resolves as; one said to be local is refused,
/// as a router's name is.
#[test]
fn a_host_a_test_names_resolves_where_it_says() {
    let net = Net::new();
    net.public("kit-public.example").local("kit-router.example");
    assert_eq!(net::public_url("https://KIT-Public.example./w/1").unwrap(), "https://kit-public.example/w/1");
    let refused = net::public_url("https://kit-router.example/admin").unwrap_err();
    assert!(net::is_not_public(&refused), "{refused:#}");
    assert_eq!(super::net::resolve("kit-router.example").unwrap().unwrap(), [super::net::LOCAL]);
    drop(net);
    assert!(super::net::resolve("kit-router.example").is_none(), "gone with its test");
}

/// A request made with the HTTP client (`net::agent`), as Kick's API and
/// Twitch's GQL are asked, reaches the fake as the site gets it (its method,
/// headers and body) and comes back as the site answers, the client doing
/// with it what it does with a site's: a status that isn't success is an
/// error unless the request takes it as an answer. A request to a host the
/// test names that nothing answers gets 404.
#[test]
fn a_request_through_the_http_client_is_answered_by_the_fake_as_a_site_answers() {
    let net = Net::new();
    net.public("kit-api.example").replies("https://kit-api.example/v1/", |asked| {
        let named = asked.json()?["name"].as_str()?.to_string();
        let said = format!("{} {} {named} by {}", asked.method, asked.url, asked.header("client-id")?);
        Some(Reply::json(&serde_json::json!({ "said": said })).with_header("ETag", "\"7\""))
    });
    let mut answer = net::agent()
        .post("https://kit-api.example/v1/hello?x=1")
        .header("Client-ID", "kit")
        .send(r#"{"name":"ada"}"#)
        .unwrap();
    assert_eq!(answer.headers().get("etag").unwrap(), "\"7\"");
    let said: serde_json::Value = serde_json::from_str(&answer.body_mut().read_to_string().unwrap()).unwrap();
    assert_eq!(said["said"], "POST https://kit-api.example/v1/hello?x=1 ada by kit");
    let missing = net::agent().get("https://kit-api.example/v2/nothing").call();
    assert!(matches!(missing, Err(ureq::Error::StatusCode(404))), "{missing:?}");
    let taken = net::agent().get("https://kit-api.example/v2/nothing").config().http_status_as_error(false).build();
    assert_eq!(taken.call().unwrap().status(), 404);
    let head = net::agent().head("https://kit-api.example/v2/nothing").call();
    assert!(matches!(head, Err(ureq::Error::StatusCode(404))), "{head:?}");
}

/// A request held at the fake's gate is answered once let go, as a slow
/// site answers.
#[test]
fn a_request_held_at_the_fakes_gate_is_answered_once_let_go() {
    let net = Net::new();
    let gate = Gate::new();
    net.serves("https://kit-slow.example/", "late").holds("https://kit-slow.example/", &gate);
    let asking = std::thread::spawn(|| net::get_bytes("https://kit-slow.example/x", 100, &never()).unwrap());
    gate.reached();
    assert!(!asking.is_finished(), "answered while held");
    gate.open();
    assert_eq!(asking.join().unwrap(), b"late");
}

fn a_video(platform: &str, id: &str, url: &str, duration: f64) -> fetch::VodInfo {
    fetch::VodInfo {
        platform: platform.into(),
        platform_id: id.into(),
        url: url.into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(duration),
        thumbnail: None,
        published: None,
        started_at: None,
    }
}

/// The fake yt-dlp downloads a YouTube video's chat replay as yt-dlp does,
/// a line at a time, read by the chat's fetch as it comes; a video it
/// serves none of has none (`chat::fetch` answers false).
#[test]
fn a_fake_yt_dlp_downloads_a_chat_replay_as_yt_dlp_does() {
    let dir = TempDir::new("kit-yt-dlp-chat");
    let fake = YtDlp::at(dir.join("bin/yt-dlp"));
    let url = "https://www.youtube.com/watch?v=noa29qsTc4g";
    let lines = vec![yt_dlp::chat_line(10.0, "@ada", "hello"), yt_dlp::chat_line(20.5, "@bo", "o7")];
    fake.serves_chat(url, lines);
    let yt_dlp = tools::YtDlp { path: fake.path().into(), js_runtime: None };
    let mut store = crate::chat::MemoryChat::default();
    let video = a_video("youtube", "noa29qsTc4g", url, 60.0);
    assert!(crate::chat::fetch(&video, Some(&yt_dlp), &dir.join("chat-1"), &mut store, &never(), |_| {}).unwrap());
    let said: Vec<(f64, String)> = store.messages.iter().map(|m| (m.time, m.text())).collect();
    assert_eq!(said, [(10.0, "hello".to_string()), (20.5, "o7".to_string())]);
    let other = a_video("youtube", "aaaaaaaaaaa", "https://www.youtube.com/watch?v=aaaaaaaaaaa", 60.0);
    let mut none = crate::chat::MemoryChat::default();
    assert!(!crate::chat::fetch(&other, Some(&yt_dlp), &dir.join("chat-2"), &mut none, &never(), |_| {}).unwrap());
    assert_eq!(std::fs::read_dir(&*dir).unwrap().count(), 1, "nothing left but the fake's folder");
}

/// Twitch's API as the fake answers it has a VOD's chat (`Net::twitch_chat`),
/// as the chat's fetch reads it, each test's VOD its own.
#[test]
fn the_fake_twitch_api_has_a_vods_chat() {
    let net = Net::new();
    net.twitch_chat("1000000101", &[(10.0, "ada", "hello"), (30.0, "bo", "o7")]);
    net.twitch_chat("1000000102", &[(5.0, "cy", "someone else's")]);
    let mut store = crate::chat::MemoryChat::default();
    let video = a_video("twitch", "v1000000101", "https://www.twitch.tv/videos/1000000101", 60.0);
    let dir = TempDir::new("kit-twitch-chat");
    assert!(crate::chat::fetch(&video, None, &dir.join("chat-1"), &mut store, &never(), |_| {}).unwrap());
    let said: Vec<(f64, &str, String)> = store.messages.iter().map(|m| (m.time, m.author.as_str(), m.text())).collect();
    assert_eq!(said, [(10.0, "ada", "hello".to_string()), (30.0, "bo", "o7".to_string())]);
}

/// The fake yt-dlp's download held halfway has written half of it to its
/// part file and said so of a quarter only; let go, it says the rest.
/// Stopped there, the next download carries on from the part file, as
/// yt-dlp does, and once whole it isn't downloaded again.
#[test]
fn a_fake_yt_dlps_download_is_written_as_it_goes_and_carried_on() {
    let dir = TempDir::new("kit-yt-dlp-goes");
    let fake = YtDlp::at(dir.join("bin/yt-dlp"));
    let url = "https://www.youtube.com/watch?v=noa29qsTc4g";
    fake.serves(yt_dlp::details("youtube", "noa29qsTc4g", url, "The test video", 600.0));
    let yt_dlp = std::sync::Arc::new(tools::YtDlp { path: fake.path().into(), js_runtime: None });
    let info_path = dir.join("info.json");
    fetch::probe(&yt_dlp, url, &info_path, &never()).unwrap();
    let out = dir.join("audio");
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let download = |stop: std::sync::Arc<AtomicBool>| {
        let (yt_dlp, info_path, out, heard) = (yt_dlp.clone(), info_path.clone(), out.clone(), seen.clone());
        std::thread::spawn(move || {
            fetch::download(&yt_dlp, &info_path, &out, fetch::Wanted::Audio, &stop, |p| heard.lock().unwrap().push(p))
        })
    };
    let half = Gate::new();
    fake.holds_at(0.5, &half);
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stopped = download(stop.clone());
    half.reached();
    assert_eq!(fetch::part_files(&out), [(dir.join("audio.m4a.part"), 2000)], "half of its 4000 bytes");
    eventually("a quarter said", || *seen.lock().unwrap() == [0.25]);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(stopped.join().unwrap().is_err());
    // Carried on: from the half it had.
    fake.holds_at(1.0, &Gate::opened());
    seen.lock().unwrap().clear();
    assert_eq!(download(std::sync::Arc::new(never())).join().unwrap().unwrap(), dir.join("audio.m4a"));
    assert_eq!(*seen.lock().unwrap(), [0.75, 1.0]);
    assert_eq!(Media::read(&dir.join("audio.m4a")).map(|m| m.duration), Some(600.0));
    // Whole: not again.
    seen.lock().unwrap().clear();
    assert_eq!(download(std::sync::Arc::new(never())).join().unwrap().unwrap(), dir.join("audio.m4a"));
    assert_eq!(*seen.lock().unwrap(), Vec::<f64>::new());
}

/// A section, which yt-dlp hands to ffmpeg, has its progress gauged from
/// its part file's growth against the bitrate the fake prints before it
/// (`tbr`, as the site's format has it): held halfway, having said only a
/// quarter, the download reads as half done.
#[test]
fn a_fake_yt_dlps_section_grows_as_its_bitrate_says() {
    let dir = TempDir::new("kit-yt-dlp-rate");
    let fake = YtDlp::at(dir.join("bin/yt-dlp"));
    let url = "https://www.youtube.com/watch?v=noa29qsTc4g";
    let mut details = yt_dlp::details("youtube", "noa29qsTc4g", url, "The test video", 600.0);
    details["tbr"] = 8.0.into();
    fake.serves(details);
    let yt_dlp = tools::YtDlp { path: fake.path().into(), js_runtime: None };
    let info_path = dir.join("info.json");
    fetch::probe(&yt_dlp, url, &info_path, &never()).unwrap();
    let half = Gate::new();
    fake.holds_at(0.5, &half);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (heard, ffmpeg, out) = (seen.clone(), dir.join("bin/ffmpeg"), dir.join("section"));
    let downloading = std::thread::spawn(move || {
        let wanted = fetch::Wanted::Section {
            selector: "b",
            ffmpeg: &ffmpeg,
            start: 120.0,
            end: 180.0,
            matroska: true,
            sound_only: false,
        };
        fetch::download(&yt_dlp, &info_path, &out, wanted, &never(), |p| heard.lock().unwrap().push(p))
    });
    half.reached();
    // 8 kbit/s for a minute: 60,000 bytes, half of them in.
    assert_eq!(fetch::part_files(&dir.join("section")), [(dir.join("section.mkv.part"), 30_000)]);
    eventually("half gauged", || seen.lock().unwrap().last() == Some(&0.5));
    half.open();
    assert_eq!(downloading.join().unwrap().unwrap(), dir.join("section.mkv"));
}

/// The fake yt-dlp says its version, as `--version` prints it, and a new
/// one once told.
#[test]
fn a_fake_yt_dlp_says_its_version() {
    let dir = TempDir::new("kit-yt-dlp-version");
    let fake = YtDlp::at(dir.join("bin/yt-dlp"));
    let version = || {
        let out = tools::output(tools::command(fake.path()).arg("--version")).unwrap();
        (out.status.success(), String::from_utf8(out.stdout).unwrap())
    };
    assert_eq!(version(), (true, format!("{}\n", yt_dlp::VERSION)));
    fake.is_version("2025.11.12");
    assert_eq!(version(), (true, "2025.11.12\n".to_string()));
}

/// A PATH a test gives the app's programs' folder is the only one its
/// lookups see (`tools::on_path`): an empty one finds no system program,
/// though this computer has it; one holding a fake `nvidia-smi` finds that,
/// and the GPU check reads the card it says, or none.
#[test]
fn a_tests_path_is_all_the_app_finds_on_it() {
    let dir = TempDir::new("kit-path");
    let bin = dir.join("bin");
    super::tools::path_for(&bin, &[]);
    assert_eq!(crate::tools::on_path(&bin, "sh"), None, "the computer's PATH looked in");
    assert!(crate::tools::find_yt_dlp(&bin).is_none());
    assert_eq!(crate::gpu::detect(&bin).map(|g| g.name), None);
    let smi = NvidiaSmi::in_dir(&dir.join("driver"));
    super::tools::path_for(&bin, &[dir.join("driver")]);
    assert_eq!(crate::tools::on_path(&bin, "nvidia-smi").as_deref(), Some(smi.path()));
    assert!(crate::gpu::detect(&bin).is_none(), "no card");
    smi.shows("NVIDIA GeForce RTX 4070", 12282, 11000);
    let card = crate::gpu::detect(&bin).unwrap();
    assert_eq!((card.name.as_str(), card.total_mb, card.free_mb), ("NVIDIA GeForce RTX 4070", 12282, 11000));
    // Let go of with its folder: the computer's PATH again.
    super::tools::let_go(&dir);
    assert_eq!(super::tools::path_of(&bin), None);
}
