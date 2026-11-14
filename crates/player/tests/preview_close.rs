//! Closing the player while the timeline's pointer rests on a frame not
//! decoded yet: the close doesn't wait for that decode. The app closes the
//! player on its main thread, so a wait there freezes the window (on a
//! 1080p file with a keyframe every ten seconds, about a quarter of a
//! second). The preview's ffmpeg, the test kit's fake, is held at a gate
//! (`Ffmpeg::holds_decodes`) until the close is done.

use std::sync::mpsc;
use std::time::Duration;

use vodscribe_core::testkit::{Ffmpeg, Gate, Media, TempDir};
use vodscribe_player::clock::{Event, Player};

/// A preview asked for where nothing is decoded starts ffmpeg there
/// (clock.rs, `Previewer::preview`), and closing the player stops what
/// it's doing at once (clock.rs, `Player`'s `Drop`; player.rs `close` runs
/// it on the app's main thread).
#[test]
fn closing_the_player_does_not_wait_for_a_preview_being_decoded() {
    let dir = TempDir::new("preview-close");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let mut source = Media::video(4.0);
    let video = source.video.as_mut().unwrap();
    (video.width, video.height) = (64, 36);
    let file = dir.join("a.mkv");
    source.write(&file);
    let player = Player::open_file(ffmpeg.path(), &file).unwrap();
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}

    // A preview far from what's decoded: its ffmpeg is held at the gate.
    let gate = Gate::new();
    ffmpeg.holds_decodes(&gate);
    let previewer = player.previewer();
    let preview = std::thread::spawn(move || previewer.preview(3.0).is_some());
    gate.reached();

    // Closed with the gate still shut: done without the preview's frame.
    let (closed_tx, closed) = mpsc::channel();
    std::thread::spawn(move || {
        drop(player);
        let _ = closed_tx.send(());
    });
    let done = closed.recv_timeout(Duration::from_secs(5));
    gate.open();
    assert!(done.is_ok(), "the close waited for the preview's decode");
    assert!(preview.join().is_ok());
}
