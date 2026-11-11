//! Closing the player while the timeline's pointer rests on a frame not
//! decoded yet: the close doesn't wait for that decode. The app closes the
//! player on its main thread, so a wait there freezes the window (on a
//! 1080p file with a keyframe every ten seconds, about a quarter of a
//! second). The preview's ffmpeg is held at a gate (a stand-in script that
//! waits while a file is there) until the close is done. Unix only, for the
//! script; the locking it tests is the same on every system. Skipped where
//! there's no ffmpeg on PATH, except where VODSCRIBE_TESTS_NEED_ALL is set (CI).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use vodscribe_player::clock::{Event, Player};

/// ffmpeg, held before it starts while `hold` is there, `reached` written
/// once it's come to that point.
fn gated_ffmpeg(dir: &Path, hold: &Path, reached: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("ffmpeg");
    let script = format!(
        "#!/bin/sh\nif [ -e '{hold}' ]; then\n  : > '{reached}'\n  while [ -e '{hold}' ]; do sleep 0.01; done\nfi\nexec ffmpeg \"$@\"\n",
        hold = hold.display(),
        reached = reached.display(),
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// The gate's file, taken away (the gate opened) when this goes, a failing
/// test's too, so no held ffmpeg is left waiting.
struct Hold(PathBuf);

impl Drop for Hold {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A preview asked for where nothing is decoded starts ffmpeg there
/// (clock.rs, `Previewer::preview`), and closing the player stops what
/// it's doing at once (clock.rs, `Player`'s `Drop`; player.rs `close` runs
/// it on the app's main thread).
#[test]
fn closing_the_player_does_not_wait_for_a_preview_being_decoded() {
    let dir = std::env::temp_dir().join(format!("vodscribe-preview-close-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.mkv");
    let made = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=s=64x36:r=30:d=4", "-c:v", "png"])
        .arg(&file)
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        assert!(std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_none(), "no ffmpeg on PATH");
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    }
    let (hold, reached) = (dir.join("hold"), dir.join("reached"));
    let ffmpeg = gated_ffmpeg(&dir, &hold, &reached);
    let player = Player::open_file(&ffmpeg, &file).unwrap();
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}

    // A preview far from what's decoded: its ffmpeg is held at the gate.
    std::fs::write(&hold, b"").unwrap();
    let gate = Hold(hold);
    let previewer = player.previewer();
    let preview = std::thread::spawn(move || previewer.preview(3.0).is_some());
    let until = Instant::now() + Duration::from_secs(10);
    while !reached.exists() {
        assert!(Instant::now() < until, "the preview's ffmpeg never started");
        std::thread::sleep(Duration::from_millis(5));
    }

    // Closed with the gate still shut: done without the preview's frame.
    let (closed_tx, closed) = mpsc::channel();
    std::thread::spawn(move || {
        drop(player);
        let _ = closed_tx.send(());
    });
    let done = closed.recv_timeout(Duration::from_secs(5));
    assert!(gate.0.exists(), "the preview was held all along");
    drop(gate);
    assert!(done.is_ok(), "the close waited for the preview's decode");
    assert!(preview.join().is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}
