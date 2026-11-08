//! The sound output stream stops while nothing plays, and starts again on
//! Play: an idle player costs no wakeups, and can't underrun when the
//! computer is busy. A stand-in output (`sound::stand_in`) takes the
//! samples in real time, so the time follows it as it would a device's.
//! Alone in its file: the stand-in is chosen for the whole process.

use std::path::Path;
use std::time::{Duration, Instant};

use vodscribe_player::clock::Player;
use vodscribe_player::sound::stand_in;

/// Waits up to five seconds for `done`.
fn until(what: &str, done: impl Fn() -> bool) {
    let began = Instant::now();
    while !done() {
        assert!(began.elapsed() < Duration::from_secs(5), "{what}: not after 5 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Asserts the stream asks for nothing over a while.
fn stays_paused(what: &str) {
    let asked = stand_in::asked();
    std::thread::sleep(Duration::from_millis(400));
    assert!(!stand_in::going(), "{what}: the stream is going");
    assert_eq!(stand_in::asked(), asked, "{what}: the stream asked for samples");
}

#[test]
fn the_stream_is_paused_while_paused_and_play_resumes_it() {
    let dir = std::env::temp_dir().join(format!("vodscribe-sound-idle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tone.mp4");
    let made = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=gray:s=160x90:r=30:d=10"])
        .args(["-f", "lavfi", "-i", "sine=f=440:d=10", "-c:v", "libx264", "-c:a", "aac", "-shortest"])
        .arg(&file)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        assert!(std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_none(), "no ffmpeg on PATH");
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    }
    stand_in::choose();
    let player = Player::open_file(Path::new("ffmpeg"), &file).unwrap();

    // Opened, paused: the stream starts with the output, and stops once
    // it's settled.
    until("the output opened", || stand_in::asked() > 0);
    until("paused after opening", || !stand_in::going());
    stays_paused("opened");
    player.seek(2.0);
    stays_paused("sought while paused");

    // Play: the stream goes again, and the time follows the sound it takes.
    player.play();
    until("the time running from the seek", || player.time() > 2.3);
    assert!(stand_in::going(), "playing, the stream is paused");

    player.pause();
    until("paused after playing", || !stand_in::going());
    stays_paused("paused");
    let at = player.time();

    // Play again after the pause: the stream resumes, the time runs on.
    player.play();
    until("the time running again", || player.time() > at + 0.3);
    assert!(stand_in::going(), "playing again, the stream is paused");
    drop(player);
    let _ = std::fs::remove_dir_all(&dir);
}
