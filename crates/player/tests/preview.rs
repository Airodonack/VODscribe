//! The preview decoder: the frame at a time without seeking, then played
//! from without starting another ffmpeg. A video whose every frame's grey
//! says its number (`common::numbered`) is previewed and played,
//! and each frame's number read from its luma. Alone in its file: it counts
//! the decoders started, which other tests would add to. Skipped where
//! there's no ffmpeg on PATH, except where VODSCRIBE_TESTS_NEED_ALL is set (CI).

mod common;

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use common::{number, numbered};
use vodscribe_player::clock::{Event, Player};
use vodscribe_player::decode::video_starts;

/// A preview is the exact frame a seek there would show, the player not
/// moved, and Play there takes its decoder over, so it starts at once; the
/// decoder that played previews in its place (CHANGELOG.md, 0.6.0: "rest
/// the pointer and the picture becomes exactly the frame there, the one
/// Play would start on, and Play there starts at once, without the wait
/// while it finds the frame"; clock.rs: "The one that was playing becomes
/// the preview decoder, with the frames it holds").
#[test]
fn a_preview_is_the_exact_frame_and_play_there_takes_its_decoder_over() {
    let dir = std::env::temp_dir().join(format!("vodscribe-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let need = std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_some();
    let Some(file) = numbered(&dir) else {
        assert!(!need, "no ffmpeg on PATH");
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let player = Player::open_file(Path::new("ffmpeg"), &file).unwrap();
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
    assert_eq!(number(&player.frame().unwrap()), 0, "the first frame on opening");

    // The frame showing at 1.51 s: frame 45, on the grid, the player not moved.
    let preview = player.preview(1.51).expect("a frame");
    assert!(preview.exact);
    assert_eq!((number(&preview.frame), preview.time), (45, 1.5));
    assert_eq!(player.snap(1.51), 1.5, "the same place a seek lands");
    assert_eq!(number(&player.frame().unwrap()), 0, "the picture stays where it was");

    // A little further: from what it read on, no new decoder.
    let started = video_starts();
    std::thread::sleep(Duration::from_millis(300));
    let later = player.preview(1.75).expect("a frame");
    assert_eq!((number(&later.frame), later.exact), (52, true));
    assert_eq!(video_starts(), started, "read on already, not started again");

    // Played from there: that decoder plays on, its frame at once, no new one started.
    player.seek(1.75);
    player.play();
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
    assert_eq!(number(&player.frame().unwrap()), 52, "the first frame is the one previewed");
    assert_eq!(player.shown(), Some(later.time), "on the grid, where the preview was");
    let mut last = 52;
    for _ in 0..10 {
        while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
        let n = number(&player.frame().unwrap());
        assert!(n > last, "frame {n} after {last}");
        last = n;
    }
    assert_eq!(video_starts(), started, "playing took the preview's decoder over");
    player.pause();

    // The decoder that played before previews now, with what it held:
    // a frame it read is there without a new decoder.
    let back = player.preview(0.0).expect("a frame");
    assert_eq!((number(&back.frame), back.exact), (0, true));
    assert_eq!(video_starts(), started, "the frame it had played, held");

    // Somewhere else: a new decoder starts, at the frame asked for.
    let far = player.preview(3.4).expect("a frame");
    assert_eq!((number(&far.frame), far.time), (102, 3.4));
    assert_eq!(video_starts(), started + 1);
    drop(player);
    let _ = std::fs::remove_dir_all(&dir);
}
