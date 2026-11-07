//! The kit's own specs: each fake answers as its program does (checked
//! against the real one where it's installed), and the helpers leave the
//! library as the app would.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use super::*;
use crate::settings::ClipEncoding;
use crate::{clips, fetch, local, net, tools};

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
