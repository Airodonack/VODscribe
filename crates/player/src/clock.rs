//! The player's clock: what time it is in the file, and which frame shows.
//! Playing, the time runs with the computer's clock from where play began
//! (sound will take over as the master when it's in); paused, it stands.
//! A thread decodes from the time asked (`decode`), waits for each frame's
//! moment and shows it; a seek, or play after a pause, starts it over from
//! the new time, so a paused seek shows the frame there. Time is kept in
//! seconds but every seek lands on a frame of the file's rate, so a step is
//! exactly one frame.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::decode::{self, Frame, Info, Video};

/// What the player does when a frame is due, if anything's told to be.
pub type OnFrame = Box<dyn Fn() + Send + 'static>;

struct State {
    playing: bool,
    /// The time, when paused; when playing, the time at `since`.
    at: f64,
    since: Instant,
    /// Counted up at every seek and play, so the decoder starts over.
    epoch: u64,
    /// Played to the end.
    ended: bool,
}

impl State {
    fn time(&self, duration: f64) -> f64 {
        if self.playing { (self.at + self.since.elapsed().as_secs_f64()).min(duration) } else { self.at }
    }
}

struct Inner {
    info: Info,
    fps: f64,
    duration: f64,
    state: Mutex<State>,
    wake: Condvar,
    frame: Mutex<Option<Arc<Frame>>>,
    on_frame: Mutex<Option<OnFrame>>,
    quit: Mutex<bool>,
}

/// A file, playing or paused at a time.
pub struct Player {
    inner: Arc<Inner>,
}

impl Player {
    /// Opens `path`, paused at its start.
    pub fn open(ffmpeg: &Path, path: &Path) -> Result<Player> {
        let info = decode::probe(ffmpeg, path)?;
        let picture = info.picture.context("no picture in the file")?;
        let inner = Arc::new(Inner {
            fps: picture.fps,
            duration: info.duration.unwrap_or(f64::MAX),
            info,
            state: Mutex::new(State { playing: false, at: 0.0, since: Instant::now(), epoch: 0, ended: false }),
            wake: Condvar::new(),
            frame: Mutex::new(None),
            on_frame: Mutex::new(None),
            quit: Mutex::new(false),
        });
        let video = Video::new(ffmpeg, path, picture);
        let shared = inner.clone();
        std::thread::Builder::new().name("player".into()).spawn(move || run(&shared, &video))?;
        Ok(Player { inner })
    }

    pub fn info(&self) -> &Info {
        &self.inner.info
    }

    pub fn fps(&self) -> f64 {
        self.inner.fps
    }

    pub fn duration(&self) -> Option<f64> {
        self.inner.info.duration
    }

    /// Told what to do when a frame is due (shown by `frame`), from the player's thread.
    pub fn on_frame(&self, f: OnFrame) {
        *self.inner.on_frame.lock().unwrap() = Some(f);
    }

    /// The frame showing now.
    pub fn frame(&self) -> Option<Arc<Frame>> {
        self.inner.frame.lock().unwrap().clone()
    }

    pub fn time(&self) -> f64 {
        self.inner.state.lock().unwrap().time(self.inner.duration)
    }

    pub fn playing(&self) -> bool {
        self.inner.state.lock().unwrap().playing
    }

    pub fn play(&self) {
        let mut s = self.inner.state.lock().unwrap();
        if s.playing {
            return;
        }
        if s.ended {
            s.at = 0.0;
            s.ended = false;
        }
        s.playing = true;
        s.since = Instant::now();
        s.epoch += 1;
        self.inner.wake.notify_all();
    }

    pub fn pause(&self) {
        let mut s = self.inner.state.lock().unwrap();
        if !s.playing {
            return;
        }
        s.at = s.time(self.inner.duration);
        s.playing = false;
        self.inner.wake.notify_all();
    }

    pub fn toggle(&self) {
        if self.playing() { self.pause() } else { self.play() }
    }

    /// Goes to the frame showing at `time`.
    pub fn seek(&self, time: f64) {
        let fps = self.inner.fps;
        let time = ((time.max(0.0) * fps + 1e-6).floor() / fps).min(self.inner.duration);
        let mut s = self.inner.state.lock().unwrap();
        s.at = time;
        s.since = Instant::now();
        s.ended = false;
        s.epoch += 1;
        self.inner.wake.notify_all();
    }

    /// Moves `frames` frames (back, if negative), pausing.
    pub fn step(&self, frames: i64) {
        self.pause();
        let at = self.time();
        self.seek(at + frames as f64 / self.inner.fps);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        *self.inner.quit.lock().unwrap() = true;
        self.inner.wake.notify_all();
    }
}

/// How late a frame may be and still show, rather than be dropped for the next.
const LATE: f64 = 0.5;

/// The player's thread: decodes from the time, shows each frame at its moment.
fn run(inner: &Inner, video: &Video) {
    'start: loop {
        if *inner.quit.lock().unwrap() {
            return;
        }
        let (epoch, from) = {
            let s = inner.state.lock().unwrap();
            (s.epoch, s.time(inner.duration))
        };
        let Ok(mut stream) = video.from(from) else { return };
        let mut first = true;
        loop {
            let Some(frame) = stream.next() else {
                // The file ended: the time stays at its end, paused.
                let mut s = inner.state.lock().unwrap();
                if s.epoch == epoch {
                    s.at = s.time(inner.duration);
                    s.playing = false;
                    s.ended = true;
                    while s.epoch == epoch && !*inner.quit.lock().unwrap() {
                        s = inner.wake.wait(s).unwrap();
                    }
                }
                continue 'start;
            };
            // Its moment: when the time reaches its pts; at once if paused
            // and it's the first (the frame at the time); dropped if late.
            let mut s = inner.state.lock().unwrap();
            loop {
                if s.epoch != epoch || *inner.quit.lock().unwrap() {
                    continue 'start;
                }
                if !s.playing {
                    if first {
                        break;
                    }
                    s = inner.wake.wait(s).unwrap();
                    continue;
                }
                let now = s.time(inner.duration);
                let wait = frame.pts - now;
                if wait <= 0.0 {
                    break;
                }
                s = inner.wake.wait_timeout(s, Duration::from_secs_f64(wait)).unwrap().0;
            }
            let late = s.playing && s.time(inner.duration) - frame.pts > LATE;
            drop(s);
            first = false;
            if late {
                continue;
            }
            *inner.frame.lock().unwrap() = Some(frame);
            if let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
                f();
            }
        }
    }
}
