//! The test kit's fake ffmpeg decodes as ffmpeg does, for the app's tests
//! of the player (vodscribe_core::testkit::Ffmpeg): the same checks run on
//! the fake's media and on a real file made to match it (64 by 36 at 30
//! fps, ten seconds, a keyframe each second, frame N's grey saying N as the
//! fake's does), decoded by the real ffmpeg. The real file is skipped where
//! there's no ffmpeg on PATH, except where VODSCRIBE_TESTS_NEED_ALL is set (CI).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use vodscribe_core::testkit::{Ffmpeg, Media, TempDir, media};
use vodscribe_player::decode::{self, Audio, Video};
use vodscribe_player::storyboard::{self, Layout};

/// The fake's media: what `real` makes, as a fake file.
fn fake(dir: &Path) -> (Ffmpeg, PathBuf) {
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let mut source = Media::video(10.0);
    let video = source.video.as_mut().unwrap();
    (video.width, video.height) = (64, 36);
    let path = dir.join("Day 3.mp4");
    source.write(&path);
    (ffmpeg, path)
}

/// Ten seconds at 30 fps, 64 by 36, lossless, a keyframe each second, frame
/// N grey as `media::numbered_frame` makes it, with sound; None without
/// ffmpeg (or its libx264) on PATH.
fn real(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("real.mp4");
    let lum = "if(lt(Y,H/2),16+mod(floor(N/219),219),16+mod(N,219))";
    let made = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("color=black:s=64x36:r=30:d=10,format=yuv420p,geq=lum='{lum}':cb=128:cr=128"))
        .args(["-f", "lavfi", "-i", "sine=d=10:sample_rate=48000"])
        .args(["-c:v", "libx264", "-qp", "0", "-g", "30", "-keyint_min", "30", "-sc_threshold", "0"])
        .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&path)
        .status()
        .ok()?;
    made.success().then_some(path)
}

/// The real ffmpeg and its file, or None (skipped) where there's none.
fn real_or_skip(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    match real(dir) {
        Some(file) => Some((PathBuf::from("ffmpeg"), file)),
        None if std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_some() => panic!("no ffmpeg on PATH"),
        None => {
            eprintln!("skipped: no ffmpeg on PATH");
            None
        }
    }
}

/// From a time, the frames are the file's own from the one showing there
/// to its end, each of the probe's size and stamped at its rate.
fn frames_are_the_files_from_the_time_asked(ffmpeg: &Path, file: &Path) {
    let info = decode::probe(ffmpeg, file).unwrap();
    let picture = info.picture.unwrap();
    assert_eq!((picture.width, picture.height, picture.fps), (64, 36, 30.0));
    assert!(info.audio);
    assert!((info.duration.unwrap() - 10.0).abs() < 0.1, "{:?}", info.duration);
    let video = Video::new(ffmpeg, file, picture);
    let mut stream = video.from(2.0).unwrap();
    for n in 60..120 {
        let frame = stream.recv().expect("a frame");
        assert_eq!((frame.width, frame.height, frame.yuv.len()), (64, 36, 64 * 36 * 3 / 2));
        assert_eq!(media::frame_number(&frame.yuv, 64, 36, 0), Some(n), "frame {n}");
        assert!((frame.pts - n as f64 / 30.0).abs() < 1e-9, "{} for frame {n}", frame.pts);
    }
    drop(stream);
    // Near the end: the last three frames, then nothing.
    let mut end = video.from(9.9).unwrap();
    let numbers: Vec<u64> =
        std::iter::from_fn(|| end.recv()).map(|f| media::frame_number(&f.yuv, 64, 36, 0).unwrap()).collect();
    assert_eq!(numbers, [297, 298, 299]);
}

/// The sound from a time is the rest of the file at the rate asked, and
/// half as many samples played twice as fast.
fn sound_is_the_rest_of_the_file_at_the_rate_and_speed_asked(ffmpeg: &Path, file: &Path) {
    let pairs = |speed: f64| -> usize {
        let mut stream = Audio::new(ffmpeg, file).from(8.0, 48_000, speed).unwrap();
        let first = stream.recv().expect("sound");
        assert!((first.pts - 8.0).abs() < 1e-9);
        first.pcm.len() / 2 + std::iter::from_fn(|| stream.recv()).map(|s| s.pcm.len() / 2).sum::<usize>()
    };
    // Within a twentieth of a second: AAC's frames pad the end a little.
    let near = |got: usize, want: usize| got.abs_diff(want) < 2_400;
    let (normal, fast) = (pairs(1.0), pairs(2.0));
    assert!(near(normal, 96_000), "{normal} pairs for two seconds");
    assert!(near(fast, 48_000), "{fast} pairs for two seconds twice as fast");
}

/// A storyboard is made whole, each slot the keyframe at or before its time.
fn storyboards_take_each_slots_keyframe(ffmpeg: &Path, file: &Path, dir: &Path) {
    let board = dir.join(format!("{}.sb", file.file_stem().unwrap().to_string_lossy()));
    let made = storyboard::make(ffmpeg, file, &board, &AtomicBool::new(false), |_| true).unwrap().unwrap();
    assert!(made.complete());
    let picture = decode::probe(ffmpeg, file).unwrap().picture.unwrap();
    let layout = Layout::of(picture, made.layout.duration);
    assert_eq!(layout.slots, 5, "{layout:?}");
    for slot in 0..layout.slots {
        let pts = made.slot(slot).expect("a keyframe");
        assert!((pts - layout.time(slot).floor()).abs() < 1e-3, "slot {slot}: {pts}");
        assert!(!made.frame(pts).unwrap().is_empty());
    }
}

#[test]
fn the_fake_decodes_frames_from_the_time_asked_as_ffmpeg_does() {
    let dir = TempDir::new("fake-decode-frames");
    let (fake, file) = fake(&dir);
    frames_are_the_files_from_the_time_asked(fake.path(), &file);
    if let Some((ffmpeg, file)) = real_or_skip(&dir) {
        frames_are_the_files_from_the_time_asked(&ffmpeg, &file);
    }
}

#[test]
fn the_fake_decodes_sound_at_the_rate_and_speed_asked_as_ffmpeg_does() {
    let dir = TempDir::new("fake-decode-sound");
    let (fake, file) = fake(&dir);
    sound_is_the_rest_of_the_file_at_the_rate_and_speed_asked(fake.path(), &file);
    if let Some((ffmpeg, file)) = real_or_skip(&dir) {
        sound_is_the_rest_of_the_file_at_the_rate_and_speed_asked(&ffmpeg, &file);
    }
}

#[test]
fn the_fake_makes_storyboards_of_keyframes_as_ffmpeg_does() {
    let dir = TempDir::new("fake-decode-storyboard");
    let (fake, file) = fake(&dir);
    storyboards_take_each_slots_keyframe(fake.path(), &file, &dir);
    // The fake's pictures name the keyframe they are.
    let board = storyboard::Storyboard::open(&dir.join("Day 3.sb")).unwrap();
    assert_eq!(media::storyboard_time(&board.frame(4.0).unwrap()), Some(4.0));
    if let Some((ffmpeg, file)) = real_or_skip(&dir) {
        storyboards_take_each_slots_keyframe(&ffmpeg, &file, &dir);
    }
}

/// A stream let go of while the fake still has frames to give stops it, as
/// a decoder's ffmpeg is killed: the app doesn't wait for the rest (an
/// hour's frames, here).
#[test]
fn a_stream_let_go_of_stops_the_fake() {
    let dir = TempDir::new("fake-decode-stop");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let file = dir.join("Long.mp4");
    Media::video(3600.0).write(&file);
    let picture = decode::probe(ffmpeg.path(), &file).unwrap().picture.unwrap();
    let mut stream = Video::new(ffmpeg.path(), &file, picture).from(0.0).unwrap();
    stream.recv().unwrap();
    let (let_go, gone) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(stream);
        let _ = let_go.send(());
    });
    assert!(gone.recv_timeout(std::time::Duration::from_secs(5)).is_ok(), "still decoding after it was let go of");
}
