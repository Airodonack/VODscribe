// Footage not here yet: a site VOD's player waits for it (the time stands,
// and it says it's waiting), where an edit's passes it in black.

use std::path::Path;
use std::time::Duration;

use vodscribe_player::clock::Player;
use vodscribe_player::timeline::{Stretch, Timeline};

/// Waits until `done` holds, or panics after `secs` seconds (under load the
/// player's threads may take a while).
fn until(what: &str, secs: u64, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Ten seconds with no footage.
fn nothing_here() -> Timeline {
    Timeline::from_stretches(vec![Stretch {
        at: 0.0,
        length: 10.0,
        piece_id: None,
        source: None,
        from: 0.0,
        sound: None,
    }])
}

#[test]
fn the_time_waits_for_footage_where_asked_to() {
    let player = Player::open(Path::new("ffmpeg"), nothing_here()).unwrap();
    player.set_waits_for_footage(true);
    player.play();
    until("it to say it's waiting", 10, || player.waiting_for_footage());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(player.time(), 0.0, "it stands");
    player.pause();
    assert!(!player.waiting_for_footage(), "paused, it isn't waiting");
}

#[test]
fn otherwise_the_time_passes_where_there_is_no_footage() {
    let player = Player::open(Path::new("ffmpeg"), nothing_here()).unwrap();
    player.play();
    until("the time to run on", 10, || player.time() > 0.3);
    assert!(!player.waiting_for_footage());
}

#[test]
fn running_into_footage_not_here_it_stops_there_and_waits() {
    let dir = std::env::temp_dir().join(format!("vodscribe-waiting-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("one.mkv");
    let made = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=160x90:rate=25:duration=1"])
        .args(["-c:v", "libx264", "-f", "matroska"])
        .arg(&file)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    }
    // A second of footage, then nine waiting for theirs.
    let source = vodscribe_core::footage::Source { path: file, offset: 0.0, section: None };
    let timeline = Timeline::from_stretches(vec![
        Stretch { at: 0.0, length: 1.0, piece_id: None, source: Some(source), from: 0.0, sound: None },
        Stretch { at: 1.0, length: 9.0, piece_id: None, source: None, from: 0.0, sound: None },
    ]);
    let player = Player::open(Path::new("ffmpeg"), timeline).unwrap();
    player.set_waits_for_footage(true);
    player.play();
    until("it to wait where the footage ends", 20, || player.waiting_for_footage());
    std::thread::sleep(Duration::from_millis(300));
    let time = player.time();
    assert!((time - 1.0).abs() < 0.05, "it stops where the footage does: {time}");
    let _ = std::fs::remove_dir_all(dir);
}
