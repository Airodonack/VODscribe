//! What the player says of itself for a stall's diagnosis
//! (`Player::diagnosis`): its thread, its decoder, the frame showing and
//! the clock's epoch. The part with a file is skipped where there's no
//! ffmpeg on PATH, except where VODSCRIBE_TESTS_NEED_ALL is set (CI).

use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use vodscribe_player::clock::{Event, Player};
use vodscribe_player::timeline::Timeline;

#[test]
fn an_empty_player_is_alive_with_nothing_decoding_or_shown() {
    let player = Player::open(Path::new("ffmpeg"), Timeline::default()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let seen = player.diagnosis();
    assert!(seen.thread_alive);
    assert!(!seen.decoding);
    assert_eq!((seen.shown, seen.frame_pts, seen.last_frame_ms_ago), (None, None, None));
    assert!(seen.frame_known);
    let state = seen.state.expect("the clock's state, not held by a stuck thread");
    assert!(!state.playing);
    assert_eq!(seen.duration, Some(0.0));
}

#[test]
fn a_seek_counts_an_epoch_and_the_frame_shown_is_the_one_there() {
    let dir = std::env::temp_dir().join(format!("vodscribe-diagnosis-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("grey.mkv");
    let made = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=gray:s=64x36:r=30:d=3", "-c:v", "png"])
        .arg(&file)
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        assert!(std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_none(), "no ffmpeg on PATH");
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    }
    let player = Player::open_file(Path::new("ffmpeg"), &file).unwrap();
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    let frame = || assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(Event::Frame));
    frame();
    let first = player.diagnosis();
    assert_eq!(first.frame_pts, Some(0.0));
    assert!(first.decoding, "paused on its first frame, the decoder is kept");
    assert!(first.shown_count >= 1 && first.last_frame_ms_ago.is_some());
    while rx.try_recv().is_ok() {}
    player.seek(2.0);
    frame();
    let after = player.diagnosis();
    assert_eq!(after.state.as_ref().map(|s| s.epoch), first.state.map(|s| s.epoch + 1));
    assert!(after.frame_pts.is_some_and(|pts| (pts - 2.0).abs() < 0.02), "{:?}", after.frame_pts);
    drop(player);
    std::fs::remove_dir_all(&dir).unwrap();
}
