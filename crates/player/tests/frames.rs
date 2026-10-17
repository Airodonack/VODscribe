//! The player shows exactly the frame asked for: a video whose every frame's
//! colour is its number (lossless PNG frames, among the codecs the player
//! lets ffmpeg decode) is sought and stepped through, and what the
//! compositor draws of the frame showing is read back and its number
//! checked. Skipped where there's no ffmpeg on PATH or no software GPU
//! adapter (the compositor is asked for one only), except where
//! VODSCRIBE_TESTS_NEED_ALL is set (CI).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use vodscribe_core::footage::Source;
use vodscribe_player::clock::{Event, Player};
use vodscribe_player::compose::{Compositor, Gpu};
use vodscribe_player::timeline::{Stretch, Timeline};

const FPS: f64 = 30.0;

/// A 4 s video at 30 fps whose frame N is red N mod 256, green N / 256 * 16.
fn numbered(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("numbered.mkv");
    let made = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=black:s=64x36:r=30:d=4,format=gbrp,geq=r='mod(N,256)':g='floor(N/256)*16':b='128'")
        .args(["-c:v", "png"])
        .arg(&path)
        .status()
        .ok()?;
    made.success().then_some(path)
}

#[test]
fn seeks_and_steps_show_exactly_the_frame_asked_for() {
    let dir = std::env::temp_dir().join(format!("vodscribe-frames-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Skipped without them, unless VODSCRIBE_TESTS_NEED_ALL is set (CI).
    let need = std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_some();
    let Some(file) = numbered(&dir) else {
        assert!(!need, "no ffmpeg on PATH");
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let mut compositor = match Compositor::new(Gpu::Software) {
        Ok(c) => c,
        Err(e) if !need => {
            eprintln!("skipped: {e:#}");
            return;
        }
        Err(e) => panic!("{e:#}"),
    };
    let player = Player::open_file(Path::new("ffmpeg"), &file).unwrap();
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    // The number of the frame showing, as drawn and read back.
    let mut showing = || {
        while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
        while rx.try_recv().is_ok() {}
        let frame = player.frame().expect("a frame showing");
        let image = compositor.render((frame.width, frame.height), Some(&frame), None).unwrap();
        let i = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
        image.rgba[i] as u32 + image.rgba[i + 1] as u32 / 16 * 256
    };

    assert_eq!(showing(), 0, "the first frame on opening");
    for (time, want) in [(2.0, 60), (1.0 / FPS * 7.5, 7), (3.999, 119), (0.5, 15), (1.0, 30)] {
        player.seek(time);
        assert_eq!(showing(), want, "after a seek to {time:.3} s");
    }
    for (frames, want) in [(1, 31), (1, 32), (-1, 31), (5, 36), (-6, 30)] {
        player.step(frames);
        assert_eq!(showing(), want, "after a step of {frames}");
    }
    drop(player);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A re-plan that keeps the stretch under the playhead but changes the one
/// after: playing on into it shows the new one's footage, not what was
/// readied for the old.
#[test]
fn a_replanned_next_stretch_plays_its_own_footage() {
    let dir = std::env::temp_dir().join(format!("vodscribe-replan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let need = std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_some();
    let Some(file) = numbered(&dir) else {
        assert!(!need, "no ffmpeg on PATH");
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let mut compositor = match Compositor::new(Gpu::Software) {
        Ok(c) => c,
        Err(e) if !need => {
            eprintln!("skipped: {e:#}");
            return;
        }
        Err(e) => panic!("{e:#}"),
    };
    let source = Source { path: file.clone(), offset: 0.0, section: None };
    let piece = |at: f64, from: f64| Stretch { at, length: 1.0, piece_id: None, source: Some(source.clone()), from };
    let player =
        Player::open(Path::new("ffmpeg"), Timeline::from_stretches(vec![piece(0.0, 0.0), piece(1.0, 2.0)])).unwrap();
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    let mut number = |frame: &vodscribe_player::decode::Frame| {
        let image = compositor.render((frame.width, frame.height), Some(frame), None).unwrap();
        let i = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
        image.rgba[i] as u32 + image.rgba[i + 1] as u32 / 16 * 256
    };
    // Paused within a second of the cut: the next stretch is readied.
    player.seek(0.5);
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
    assert_eq!(number(&player.frame().unwrap()), 15);
    // The second piece now plays from 1 s, not 2 s.
    player.set_timeline(Timeline::from_stretches(vec![piece(0.0, 0.0), piece(1.0, 1.0)]));
    player.play();
    loop {
        let event = rx.recv_timeout(Duration::from_secs(10)).expect("frames");
        assert_ne!(event, Event::Ended, "ended before the second piece showed");
        let (Some(frame), Some(shown)) = (player.frame(), player.shown()) else { continue };
        if shown >= 1.0 - 1e-6 {
            let n = number(&frame);
            assert!((30..60).contains(&n), "frame {n} at {shown:.3} s, from the old plan");
            break;
        }
    }
    drop(player);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Play from the last frame, with sound: it plays out and ends, rather than
/// standing "playing" with the time stopped. (Needs a sound output to show
/// anything; passes without one.)
#[test]
fn play_from_the_last_frame_ends() {
    let dir = std::env::temp_dir().join(format!("vodscribe-end-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let need = std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_some();
    let path = dir.join("sound.mkv");
    let made = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=black:s=64x36:r=30:d=2"])
        .args(["-f", "lavfi", "-i", "sine=f=440:d=2", "-c:v", "png", "-c:a", "flac", "-shortest"])
        .arg(&path)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        assert!(!need, "no ffmpeg on PATH");
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    }
    let player = Player::open_file(Path::new("ffmpeg"), &path).unwrap();
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    // Give the sound thread its output, then stand on the last frame.
    std::thread::sleep(Duration::from_millis(500));
    player.seek(player.duration() - 0.01);
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
    player.play();
    let ended = std::iter::from_fn(|| rx.recv_timeout(Duration::from_secs(5)).ok()).any(|e| e == Event::Ended);
    assert!(
        ended,
        "still playing at {:.3} s of {:.3}, running: {}",
        player.time(),
        player.duration(),
        player.running()
    );
    drop(player);
    let _ = std::fs::remove_dir_all(&dir);
}
