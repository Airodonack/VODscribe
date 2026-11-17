//! What the player's tests share: ffmpeg found or the test skipped, a
//! video whose every frame says its number, and a player whose events are
//! heard.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use vodscribe_player::clock::{Event, Player};
use vodscribe_player::decode::Frame;

/// The ffmpeg on PATH, as the tests run it.
pub fn ffmpeg() -> &'static Path {
    Path::new("ffmpeg")
}

/// Runs ffmpeg with `args` (quiet, overwriting); whether it made its output.
pub fn run(args: &[&str]) -> bool {
    Command::new(ffmpeg())
        .args(["-hide_banner", "-v", "error", "-y"])
        .args(args)
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `made`, or None for a test to return: skipped without ffmpeg, failed
/// where it's needed (VODSCRIBE_TESTS_NEED_ALL, as on CI).
pub fn or_skip<T>(made: Option<T>, what: &str) -> Option<T> {
    if made.is_none() {
        assert!(std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_none(), "ffmpeg couldn't make {what}");
        eprintln!("skipped: no ffmpeg (or no encoder) for {what}");
    }
    made
}

/// The frames' rate in `numbered`.
pub const FPS: f64 = 30.0;

/// A 4 s video at 30 fps, 64 by 36, whose frame N is grey 2N (lossless
/// PNG frames), in `dir`; None if ffmpeg couldn't make it.
pub fn numbered(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("numbered.mkv");
    let graph = "color=black:s=64x36:r=30:d=4,format=gbrp,geq=r='2*N':g='2*N':b='2*N'";
    run(&["-f", "lavfi", "-i", graph, "-c:v", "png", path.to_str().unwrap()]).then_some(path)
}

/// The number of the `numbered` frame `frame` is, by its luma at the
/// middle: a grey's luma is the grey.
pub fn number(frame: &Frame) -> u32 {
    let (y, _, _) = frame.planes();
    (y[(frame.height / 2 * frame.width + frame.width / 2) as usize] as u32).div_ceil(2)
}

/// A player, and what it tells of (`Player::on_frame`).
pub struct Heard {
    pub player: Player,
    events: Receiver<Event>,
}

impl Heard {
    pub fn new(player: Player) -> Heard {
        let (tx, events) = mpsc::channel();
        player.on_frame(Box::new(move |event| {
            let _ = tx.send(event);
        }));
        Heard { player, events }
    }

    /// Waits for what it tells of until `done` holds (looked at after each
    /// event, and first); panics if it hasn't within ten seconds.
    pub fn until(&self, what: &str, mut done: impl FnMut(&Player) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(&self.player) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(self.events.recv_timeout(left).is_ok(), "never happened: {what}; {:?}", self.player.diagnosis());
        }
    }

    /// The number of the frame showing once the picture is at `time` in
    /// the timeline (where a seek there lands).
    pub fn number_at(&self, time: f64) -> u32 {
        self.until(&format!("a frame at {time:.4} s"), |p| p.shown().is_some_and(|t| (t - time).abs() < 1e-6));
        number(&self.player.frame().expect("a frame showing"))
    }

    /// Waits for the timeline to end (`Event::Ended`).
    pub fn ended(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Ended) => return,
                Ok(_) => {}
                Err(_) => panic!("it didn't end; {:?}", self.player.diagnosis()),
            }
        }
    }
}
