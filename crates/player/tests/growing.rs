//! An edit's footage still downloading plays as far as it's in, and on
//! as more comes in, without a break: ffmpeg writes a Matroska file in real
//! time (as a section's download does) while the player plays it, its
//! timeline planned again as the file grows (as the edit view does).
//!
//! Ignored by default: it takes about ten seconds, needs ffmpeg on PATH, and
//! opens the default sound output (run it where that's a null sink, as in
//! the Debian box's PulseAudio setup):
//!
//!   cargo test -p vodscribe-player --test growing -- --ignored --nocapture

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vodscribe_core::edits::EditPiece;
use vodscribe_core::footage::Source;
use vodscribe_core::mkv;
use vodscribe_player::clock::{Event, Player};
use vodscribe_player::timeline::Timeline;

/// The edit: one piece, 0 to 10 s of VOD 1, from `path` as far as it's whole.
fn plan(path: &Path) -> Timeline {
    let piece = EditPiece {
        id: 1,
        edit_id: 1,
        vod_id: 1,
        start: 0.0,
        end: 10.0,
        title: String::new(),
        struck: Vec::new(),
        vod_title: String::new(),
    };
    let until = mkv::written(path).map_or(0.0, |w| (w - 1.0).max(0.0));
    let source = Source { path: path.to_path_buf(), offset: 0.0, section: None };
    Timeline::of(&[piece], |_, _, _| Some((source.clone(), until)))
}

#[test]
#[ignore]
fn footage_still_coming_plays_on_without_a_break() {
    let dir = std::env::temp_dir().join(format!("vodscribe-growing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("section.mkv");
    // Written in real time, a cluster each half second, each packet flushed
    // (else ffmpeg writes 256 KB at a time: here six seconds at once).
    let mut writer = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-re", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30"])
        .args(["-f", "lavfi", "-i", "sine=frequency=440", "-t", "14"])
        .args(["-c:v", "libx264", "-g", "30", "-c:a", "aac", "-cluster_time_limit", "500", "-flush_packets", "1"])
        .args(["-f", "matroska"])
        .arg(&file)
        .stdin(Stdio::null())
        .spawn()
        .expect("ffmpeg on PATH");
    let began = Instant::now();
    while mkv::written(&file).is_none_or(|w| w < 3.0) {
        assert!(began.elapsed() < Duration::from_secs(10), "the file never got 3 s in");
        std::thread::sleep(Duration::from_millis(100));
    }

    let first = plan(&file);
    let first_end = first.stretches()[0].end();
    let player = Arc::new(Player::open(Path::new("ffmpeg"), first).unwrap());
    // When each frame showed, and its time in the file.
    let shown = Arc::new(Mutex::new(Vec::<(Instant, f64)>::new()));
    let (p, s) = (Arc::downgrade(&player), shown.clone());
    player.on_frame(Box::new(move |event| {
        if event == Event::Frame
            && let Some(frame) = p.upgrade().and_then(|p| p.frame())
        {
            s.lock().unwrap().push((Instant::now(), frame.pts));
        }
    }));
    player.play();
    // Measured once the time runs: the first play after opening can stand a
    // while (TODO.md), which isn't what's tested here.
    let replan = |player: &Player| player.set_timeline(plan(&file));
    while !(player.running() && player.time() > 0.2) {
        std::thread::sleep(Duration::from_millis(50));
        replan(&player);
    }
    let (began, from) = (Instant::now(), player.time());
    // Planned again as more comes in: here every 300 ms (the edit view does
    // it every ten seconds in; more often is the harder case).
    while began.elapsed() < Duration::from_secs(6) {
        std::thread::sleep(Duration::from_millis(300));
        replan(&player);
    }
    let (ran, played) = (player.time() - from, began.elapsed().as_secs_f64());
    player.pause();
    let _ = writer.kill();
    let _ = writer.wait();

    let shown: Vec<(Instant, f64)> = shown.lock().unwrap().iter().copied().filter(|s| s.0 >= began).collect();
    let longest = shown.windows(2).map(|w| (w[1].0 - w[0].0).as_secs_f64()).fold(0.0, f64::max);
    let last_pts = shown.last().map_or(0.0, |s| s.1);
    let (_, dropped) = player.counts();
    println!(
        "first plan to {first_end:.2} s; in {played:.2} s from {from:.2} s the time ran {ran:.2} s to a frame at \
         {last_pts:.2}, {} shown, {dropped} dropped, longest gap {:.0} ms",
        shown.len(),
        longest * 1e3
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(last_pts > first_end + 2.0, "played on past the first plan's end");
    assert!(ran > played - 0.3, "the time ran {ran:.2} s in {played:.2} s");
    assert!(longest < 0.2, "a break of {:.0} ms", longest * 1e3);
}
