//! The player's clock: what time it is in the file, and which frame shows.
//! Playing, the time follows the sound as it's played out (`sound`), or the
//! computer's clock from where play began when there's no sound; paused,
//! it stands. A thread decodes frames from the time asked (`decode`),
//! waits for each one's moment and shows it; another decodes the sound and
//! keeps the output fed. A seek, or play after a pause, starts both over
//! from the new time, so a paused seek shows the frame there. Time is kept
//! in seconds but every seek lands on a frame of the file's rate, so a step
//! is exactly one frame.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::decode::{self, Audio, Frame, Info, Video};
use crate::sound::{self, Output};

/// What the player does when a frame is due, or the file ends, if anything's told to be.
pub type OnFrame = Box<dyn Fn(Event) + Send + 'static>;

/// What the player's thread tells of (`Player::on_frame`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A frame is due, and is the one `Player::frame` gives now.
    Frame,
    /// The file played to its end: paused there.
    Ended,
}

/// The sound's clock: sample pairs played out since the time was set.
struct Sound {
    played: Arc<AtomicU64>,
    rate: u32,
}

struct State {
    playing: bool,
    /// The time, when paused; when playing, the time at `since` (or when
    /// the sound's count began).
    at: f64,
    since: Instant,
    /// Counted up at every seek and play, so the decoders start over.
    epoch: u64,
    /// Played to the end.
    ended: bool,
    /// The sound playing out, while it is: the time follows it. Until it
    /// does, where there's an output, a count standing at zero: the time
    /// waits for the sound rather than running ahead of it.
    sound: Option<Sound>,
    /// There's a sound output (the sound thread says so once it has one).
    output: bool,
}

impl State {
    fn time(&self, duration: f64) -> f64 {
        if !self.playing {
            return self.at;
        }
        let run = match &self.sound {
            Some(s) => s.played.load(Ordering::Acquire) as f64 / s.rate as f64,
            None => self.since.elapsed().as_secs_f64(),
        };
        (self.at + run).min(duration)
    }

    /// Starts over from `at` (a seek, or play): the decoders will, and the
    /// time runs from here once the sound does again.
    fn restart(&mut self, at: f64) {
        self.at = at;
        self.since = Instant::now();
        self.epoch += 1;
        self.sound = self.output.then(|| Sound { played: Arc::new(AtomicU64::new(0)), rate: 1 });
    }
}

struct Inner {
    info: Info,
    /// The loudness asked for, 0 to 1, as bits of an f32 (the output's own, once there is one).
    volume: Mutex<Arc<AtomicU32>>,
    fps: f64,
    duration: f64,
    state: Mutex<State>,
    wake: Condvar,
    frame: Mutex<Option<Arc<Frame>>>,
    on_frame: Mutex<Option<OnFrame>>,
    quit: AtomicBool,
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
            state: Mutex::new(State {
                playing: false,
                at: 0.0,
                since: Instant::now(),
                epoch: 0,
                ended: false,
                sound: None,
                output: false,
            }),
            wake: Condvar::new(),
            frame: Mutex::new(None),
            on_frame: Mutex::new(None),
            quit: AtomicBool::new(false),
            volume: Mutex::new(Arc::new(AtomicU32::new(1f32.to_bits()))),
            info,
        });
        let video = Video::new(ffmpeg, path, picture);
        let shared = inner.clone();
        std::thread::Builder::new().name("player".into()).spawn(move || run(&shared, &video))?;
        if inner.info.audio {
            let audio = Audio::new(ffmpeg, path);
            let shared = inner.clone();
            std::thread::Builder::new().name("sound".into()).spawn(move || run_sound(&shared, &audio))?;
        }
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

    /// The loudness, 0 to 1.
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.inner.volume.lock().unwrap().load(Ordering::Acquire))
    }

    pub fn set_volume(&self, volume: f32) {
        self.inner.volume.lock().unwrap().store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Release);
    }

    /// Told what to do when a frame is due (shown by `frame`) or the file
    /// ends, from the player's thread.
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
        let at = s.at;
        s.restart(at);
        self.inner.wake.notify_all();
    }

    pub fn pause(&self) {
        let mut s = self.inner.state.lock().unwrap();
        if !s.playing {
            return;
        }
        s.at = s.time(self.inner.duration);
        s.playing = false;
        s.sound = None;
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
        s.ended = false;
        s.restart(time);
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
        self.inner.quit.store(true, Ordering::Release);
        self.inner.wake.notify_all();
    }
}

/// How late a frame may be and still show, rather than be dropped for the next.
const LATE: f64 = 0.5;

/// The player's thread: decodes from the time, shows each frame at its moment.
fn run(inner: &Inner, video: &Video) {
    'start: loop {
        if inner.quit.load(Ordering::Acquire) {
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
                    s.sound = None;
                    s.ended = true;
                    drop(s);
                    if let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
                        f(Event::Ended);
                    }
                    s = inner.state.lock().unwrap();
                    while s.epoch == epoch && !inner.quit.load(Ordering::Acquire) {
                        s = inner.wake.wait(s).unwrap();
                    }
                }
                continue 'start;
            };
            // Its moment: when the time reaches its pts; at once if paused
            // and it's the first (the frame at the time); dropped if late.
            let mut s = inner.state.lock().unwrap();
            loop {
                if s.epoch != epoch || inner.quit.load(Ordering::Acquire) {
                    continue 'start;
                }
                if !s.playing {
                    if first {
                        break;
                    }
                    s = inner.wake.wait(s).unwrap();
                    continue;
                }
                let wait = frame.pts - s.time(inner.duration);
                if wait <= 0.0 {
                    break;
                }
                // The sound's clock isn't signalled, so looked at often.
                let wait = Duration::from_secs_f64(wait.min(0.01));
                s = inner.wake.wait_timeout(s, wait).unwrap().0;
            }
            let late = s.playing && s.time(inner.duration) - frame.pts > LATE;
            drop(s);
            first = false;
            if late {
                continue;
            }
            *inner.frame.lock().unwrap() = Some(frame);
            if let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
                f(Event::Frame);
            }
        }
    }
}

/// Sound queued before it's let out, so it doesn't run dry at once.
const START: f64 = 0.1;

/// The sound's thread: decodes from the time and keeps the output fed while
/// playing; once it's playing out, the time follows it.
fn run_sound(inner: &Inner, audio: &Audio) {
    let Some(out) = Output::open() else { return };
    {
        // The output's own gain takes over, at the loudness asked so far.
        let mut volume = inner.volume.lock().unwrap();
        out.volume().store(volume.load(Ordering::Acquire), Ordering::Release);
        *volume = out.volume();
    }
    {
        let mut s = inner.state.lock().unwrap();
        s.output = true;
        if s.playing {
            let at = s.time(inner.duration);
            s.restart(at);
        }
    }
    'start: loop {
        out.set_running(false);
        out.clear();
        let (epoch, from) = {
            let mut s = inner.state.lock().unwrap();
            while !s.playing && !inner.quit.load(Ordering::Acquire) {
                s = inner.wake.wait(s).unwrap();
            }
            if inner.quit.load(Ordering::Acquire) {
                return;
            }
            (s.epoch, s.time(inner.duration))
        };
        let Ok(mut stream) = audio.from(from, out.rate()) else {
            // No sound to be had after all: the time runs by the computer's clock.
            let mut s = inner.state.lock().unwrap();
            s.output = false;
            if s.sound.is_some() {
                s.since = Instant::now();
                s.sound = None;
            }
            return;
        };
        let mut started = false;
        loop {
            {
                let s = inner.state.lock().unwrap();
                if s.epoch != epoch || !s.playing || inner.quit.load(Ordering::Acquire) {
                    continue 'start;
                }
            }
            if out.queued() < sound::AHEAD {
                match stream.next() {
                    Some(samples) => out.push(&samples.pcm),
                    None => {
                        // The sound ended before the picture: the time goes
                        // on by the computer's clock from here.
                        let mut s = inner.state.lock().unwrap();
                        if s.epoch == epoch && s.sound.is_some() {
                            s.at = s.time(inner.duration);
                            s.since = Instant::now();
                            s.sound = None;
                        }
                        while s.epoch == epoch && s.playing && !inner.quit.load(Ordering::Acquire) {
                            s = inner.wake.wait(s).unwrap();
                        }
                        continue 'start;
                    }
                }
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
            if !started && out.queued() >= START {
                let mut s = inner.state.lock().unwrap();
                if s.epoch != epoch {
                    continue 'start;
                }
                out.recount();
                s.sound = Some(Sound { played: out.played(), rate: out.rate() });
                out.set_running(true);
                started = true;
            }
        }
    }
}
