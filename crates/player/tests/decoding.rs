//! What ffmpeg gives the player, from files made here by ffmpeg: video at
//! the NTSC rates (23.976, 29.97 and 59.94 frames a second) frame by frame,
//! and the sound at the slowest speeds. Skipped where there's no ffmpeg on PATH, except where
//! VODSCRIBE_TESTS_NEED_ALL is set (CI).

mod common;

use std::path::{Path, PathBuf};

use common::{Heard, ffmpeg, or_skip, run};
use vodscribe_core::testkit::TempDir;
use vodscribe_player::clock::Player;
use vodscribe_player::decode::{self, Audio, Frame, Video};

/// Bars across a picture: up to 65535 frames.
const BARS: u32 = 16;

/// ffmpeg's filter saying each frame's number in sixteen bars across its
/// luma, the highest bit at the left: white (235) for 1, black (16) for 0.
const NUMBER_BARS: &str = "geq=lum='if(mod(floor(N/pow(2\\,15-floor(X*16/W)))\\,2)\\,235\\,16)':cb=128:cr=128";

/// `seconds` of video at `rate` (ffmpeg's way of saying it, 24000/1001),
/// 64 by 8, losslessly encoded, each frame's number in bars.
fn barred(dir: &Path, rate: &str, seconds: u32) -> Option<PathBuf> {
    let path = dir.join(format!("{}.mp4", rate.replace('/', "-")));
    let graph = format!("color=black:s=64x8:r={rate}:d={seconds},format=yuv420p,{NUMBER_BARS}");
    let x264 = ["-c:v", "libx264", "-preset", "ultrafast", "-qp", "0", "-pix_fmt", "yuv420p"];
    let args = [&["-f", "lavfi", "-i", graph.as_str()][..], &x264[..], &[path.to_str().unwrap()][..]].concat();
    run(&args).then_some(path)
}

/// The number the bars of `frame` say, read along its middle row.
fn barred_number(frame: &Frame) -> u32 {
    let (y, _, _) = frame.planes();
    let (w, row) = (frame.width as usize, frame.height as usize / 2 * frame.width as usize);
    (0..BARS as usize).fold(0, |n, i| n << 1 | (y[row + (2 * i + 1) * w / (2 * BARS as usize)] > 128) as u32)
}

/// Video at 23.976, 29.97 or 59.94 frames a second plays at that exact
/// rate: ten minutes of 23.976 decode as each of the file's frames once, in
/// order, stamped at the rate, and a seek and a step far in show the frame
/// there (CHANGELOG.md, 0.4.1 Fixes: "Videos at 23.976, 29.97 or 59.94
/// frames a second showed one frame twice every few minutes, and stepping
/// frame by frame drifted.").
#[test]
fn video_at_the_ntsc_rates_shows_each_frame_once_and_steps_without_drifting() {
    let dir = TempDir::new("ntsc");
    for (rate, exact) in [("30000/1001", 30000.0 / 1001.0), ("60000/1001", 60000.0 / 1001.0)] {
        let Some(file) = or_skip(barred(&dir, rate, 2), rate) else { return };
        let fps = decode::probe(ffmpeg(), &file).unwrap().picture.unwrap().fps;
        assert_eq!(fps, exact, "{rate} read as its exact rate");
    }

    let Some(file) = or_skip(barred(&dir, "24000/1001", 600), "ten minutes at 24000/1001") else { return };
    let picture = decode::probe(ffmpeg(), &file).unwrap().picture.unwrap();
    let fps = 24000.0 / 1001.0;
    assert_eq!(picture.fps, fps, "23.976 read as its exact rate");
    let mut frames = Video::new(ffmpeg(), &file, picture).from(0.0).unwrap();
    let mut n = 0u32;
    while let Some(frame) = frames.recv() {
        assert_eq!(barred_number(&frame), n, "the frame decoded at {:.3} s", frame.pts);
        assert!((frame.pts - n as f64 / fps).abs() < 1e-6, "frame {n} stamped {}", frame.pts);
        n += 1;
    }
    assert_eq!(n, 14386, "every frame of ten minutes, once");

    let heard = Heard::new(Player::open_file(ffmpeg(), &file).unwrap());
    let player = &heard.player;
    assert_eq!(player.fps(), Some(fps));
    let far = 14000.0 / fps;
    player.seek(far);
    heard.until("the frame at 9:44", |p| p.shown().is_some_and(|t| (t - far).abs() < 1e-6));
    assert_eq!(barred_number(&player.frame().unwrap()), 14000, "a seek far in");
    player.step(1);
    let next = 14001.0 / fps;
    heard.until("the next frame", |p| p.shown().is_some_and(|t| (t - next).abs() < 1e-6));
    assert_eq!(barred_number(&player.frame().unwrap()), 14001, "a step from there");
}

/// The sound at a speed is the file's from the time asked at that speed,
/// the slowest ones under half included: each second of it is `speed`
/// seconds of the file, each chunk stamped with its time in the file
/// (decode.rs `Samples`: "from `pts` (seconds into the file) on";
/// `Audio::from`: "played `speed` times as fast at the same
/// pitch (ffmpeg's atempo): each second of it is `speed` seconds of the
/// file"; clock.rs `Player::set_speed`: "within 0.25 to 4").
#[test]
fn the_sound_at_a_speed_is_the_files_at_that_speed_however_slow() {
    let dir = TempDir::new("tempo");
    let file = dir.join("tone.m4a");
    let made = run(&["-f", "lavfi", "-i", "sine=f=440:d=2", "-c:a", "aac", file.to_str().unwrap()]).then_some(());
    if or_skip(made, "two seconds of sound").is_none() {
        return;
    }
    let (rate, from) = (48_000, 0.5);
    for speed in [0.25, 0.3, 0.5, 1.0, 2.0, 4.0] {
        let mut sound = Audio::new(ffmpeg(), &file).from(from, rate, speed).unwrap();
        let mut pairs = 0usize;
        while let Some(samples) = sound.recv() {
            let file_time = from + pairs as f64 / rate as f64 * speed;
            assert!((samples.pts - file_time).abs() < 1e-9, "at {speed}: samples at {}, not {file_time}", samples.pts);
            pairs += samples.pcm.len() / decode::CHANNELS;
        }
        let seconds = pairs as f64 / rate as f64;
        let want = (2.0 - from) / speed;
        assert!((seconds - want).abs() < want * 0.03, "at {speed}: {seconds:.3} s of sound, not {want:.3}");
    }
}
