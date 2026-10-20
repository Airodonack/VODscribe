// A picture and its sound from two files (as a site's player hands them
// over: fragmented MP4 video, WebM Opus audio), played as one stretch.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use vodscribe_core::footage::Source;
use vodscribe_player::clock::{Event, Player};
use vodscribe_player::timeline::{Sound, Stretch, Timeline};

#[test]
fn picture_and_sound_from_two_files_play_together() {
    let dir = std::env::temp_dir().join(format!("vodscribe-apart-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (video, audio) = (dir.join("v.mp4"), dir.join("a.webm"));
    let make = |args: &[&str], out: &Path| {
        std::process::Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).arg(out).status()
    };
    let made = make(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=30:duration=4",
            "-c:v",
            "libx264",
            "-movflags",
            "frag_keyframe+empty_moov",
        ],
        &video,
    )
    .and_then(|_| make(&["-f", "lavfi", "-i", "sine=duration=4", "-c:a", "libopus"], &audio));
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    }
    let source = Source { path: video, offset: 0.0, section: None };
    let timeline = Timeline::from_stretches(vec![Stretch {
        at: 0.0,
        length: 4.0,
        piece_id: None,
        source: Some(source),
        from: 0.0,
        sound: Some(Sound { path: audio, offset: 0.0 }),
    }]);
    let player = Arc::new(Player::open(Path::new("ffmpeg"), timeline).unwrap());
    let frames = Arc::new(AtomicU64::new(0));
    let counted = frames.clone();
    player.on_frame(Box::new(move |event| {
        if event == Event::Frame {
            counted.fetch_add(1, Ordering::Relaxed);
        }
    }));
    player.play();
    let began = Instant::now();
    while player.time() < 2.0 {
        assert!(began.elapsed() < Duration::from_secs(20), "it didn't play: {}", player.time());
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(frames.load(Ordering::Relaxed) > 20, "{} frames in 2 s", frames.load(Ordering::Relaxed));
    let _ = std::fs::remove_dir_all(dir);
}
