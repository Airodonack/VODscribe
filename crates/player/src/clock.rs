//! The player's clock: what time it is in the timeline, and which frame
//! shows. Playing, the time follows the sound as it's played out (`sound`),
//! or the computer's clock from where play began when there's no sound;
//! paused, it stands. A thread decodes frames from the time asked
//! (`decode`), stretch by stretch of the timeline (`timeline`), waits for
//! each one's moment and shows it; another decodes the sound and keeps the
//! output fed. The next stretch's decoder is started a little before the
//! current one ends, so a join costs no wait. A seek, or play after a
//! pause, starts both over from the new time, so a paused seek shows the
//! frame there. Time is kept in seconds but every seek lands on a frame of
//! the file's rate there, so a step is exactly one frame.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use vodscribe_core::local::Picture;

use crate::decode::{self, Audio, Frame, Info, Stream, Video};
use crate::sound::{self, Output};
use crate::timeline::{Stretch, Timeline};

/// What the player does when a frame is due, or the file ends, if anything's told to be.
pub type OnFrame = Box<dyn Fn(Event) + Send + 'static>;

/// What the player's thread tells of (`Player::on_frame`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A frame is due, and is the one `Player::frame` gives now.
    Frame,
    /// The timeline played to its end: paused there.
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
    ffmpeg: PathBuf,
    /// What plays.
    timeline: Mutex<Timeline>,
    /// What each file holds, as probed when first needed (None: couldn't be read).
    sources: Mutex<HashMap<PathBuf, Option<Info>>>,
    /// The loudness asked for, 0 to 1, as bits of an f32 (the output's own, once there is one).
    volume: Mutex<Arc<AtomicU32>>,
    state: Mutex<State>,
    wake: Condvar,
    frame: Mutex<Option<Arc<Frame>>>,
    on_frame: Mutex<Option<OnFrame>>,
    quit: AtomicBool,
    /// Frames shown, and dropped for being late (for measuring).
    shown: AtomicU64,
    dropped: AtomicU64,
}

impl Inner {
    fn duration(&self) -> f64 {
        self.timeline.lock().unwrap().duration()
    }

    /// What `path` holds, probing it the first time.
    fn info(&self, path: &Path) -> Option<Info> {
        if let Some(known) = self.sources.lock().unwrap().get(path) {
            return known.clone();
        }
        let info = decode::probe(&self.ffmpeg, path).ok();
        self.sources.lock().unwrap().insert(path.to_path_buf(), info.clone());
        info
    }

    /// The picture of the file playing at `time`, if there's one.
    fn picture_at(&self, time: f64) -> Option<Picture> {
        let source = self.timeline.lock().unwrap().at(time)?.source.clone()?;
        self.info(&source.path)?.picture
    }
}

/// A timeline, playing or paused at a time.
pub struct Player {
    inner: Arc<Inner>,
}

/// Where no file's rate is known: a seek lands on a step of this.
const DEFAULT_FPS: f64 = 30.0;

impl Player {
    /// Opens `path` alone, paused at its start.
    pub fn open_file(ffmpeg: &Path, path: &Path) -> Result<Player> {
        let info = decode::probe(ffmpeg, path)?;
        info.picture.context("no picture in the file")?;
        let duration = info.duration.context("the file's length isn't known")?;
        let player = Player::open(ffmpeg, Timeline::file(path, duration))?;
        player.inner.sources.lock().unwrap().insert(path.to_path_buf(), Some(info));
        Ok(player)
    }

    /// Opens `timeline`, paused at its start.
    pub fn open(ffmpeg: &Path, timeline: Timeline) -> Result<Player> {
        let inner = Arc::new(Inner {
            ffmpeg: ffmpeg.to_path_buf(),
            timeline: Mutex::new(timeline),
            sources: Mutex::default(),
            volume: Mutex::new(Arc::new(AtomicU32::new(1f32.to_bits()))),
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
            shown: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        });
        let shared = inner.clone();
        std::thread::Builder::new().name("player".into()).spawn(move || run(&shared))?;
        let shared = inner.clone();
        std::thread::Builder::new().name("sound".into()).spawn(move || run_sound(&shared))?;
        Ok(Player { inner })
    }

    /// Plays `timeline` instead, from the same time: without a break where
    /// what's under the playhead is the same (the threads take the new one
    /// up at the next stretch).
    pub fn set_timeline(&self, timeline: Timeline) {
        let mut s = self.inner.state.lock().unwrap();
        let time = s.time(self.inner.duration());
        let mut held = self.inner.timeline.lock().unwrap();
        let same = held.at(time) == timeline.at(time);
        *held = timeline;
        let duration = held.duration();
        drop(held);
        if !same {
            s.ended = false;
            s.restart(time.min(duration));
        }
        self.inner.wake.notify_all();
    }

    pub fn timeline(&self) -> Timeline {
        self.inner.timeline.lock().unwrap().clone()
    }

    pub fn duration(&self) -> f64 {
        self.inner.duration()
    }

    /// The frame rate of what plays at `time` (the file's there), if known.
    pub fn fps_at(&self, time: f64) -> Option<f64> {
        self.inner.picture_at(time).map(|p| p.fps)
    }

    /// The frame rate where the playhead is, if known.
    pub fn fps(&self) -> Option<f64> {
        self.fps_at(self.time())
    }

    /// Told what to do when a frame is due (shown by `frame`) or the
    /// timeline ends, from the player's thread.
    pub fn on_frame(&self, f: OnFrame) {
        *self.inner.on_frame.lock().unwrap() = Some(f);
    }

    /// The frame showing now.
    pub fn frame(&self) -> Option<Arc<Frame>> {
        self.inner.frame.lock().unwrap().clone()
    }

    pub fn time(&self) -> f64 {
        self.inner.state.lock().unwrap().time(self.inner.duration())
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
        s.at = s.time(self.inner.duration());
        s.playing = false;
        s.sound = None;
        self.inner.wake.notify_all();
    }

    pub fn toggle(&self) {
        if self.playing() { self.pause() } else { self.play() }
    }

    /// Goes to the frame showing at `time`.
    pub fn seek(&self, time: f64) {
        let fps = self.fps_at(time).unwrap_or(DEFAULT_FPS);
        let time = ((time.max(0.0) * fps + 1e-6).floor() / fps).min(self.inner.duration());
        let mut s = self.inner.state.lock().unwrap();
        s.ended = false;
        s.restart(time);
        self.inner.wake.notify_all();
    }

    /// Moves `frames` frames (back, if negative), pausing.
    pub fn step(&self, frames: i64) {
        self.pause();
        let at = self.time();
        let fps = self.fps_at(at).unwrap_or(DEFAULT_FPS);
        self.seek(at + frames as f64 / fps);
    }

    /// Whether there's a sound output (so the time follows the sound).
    pub fn has_sound(&self) -> bool {
        self.inner.state.lock().unwrap().output
    }

    /// Frames shown, and dropped for being late, since the player was opened.
    pub fn counts(&self) -> (u64, u64) {
        (self.inner.shown.load(Ordering::Relaxed), self.inner.dropped.load(Ordering::Relaxed))
    }

    /// The loudness, 0 to 1.
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.inner.volume.lock().unwrap().load(Ordering::Acquire))
    }

    pub fn set_volume(&self, volume: f32) {
        self.inner.volume.lock().unwrap().store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Release);
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
/// How long before a stretch ends the next one's decoder is started.
const PREFETCH: f64 = 1.0;

/// What a thread walks the timeline with: where it is, and what's next.
struct Walk {
    epoch: u64,
    timeline: Timeline,
    /// The stretch being played, by index.
    index: usize,
    /// Where in it to start (the time asked, the first time; then its start).
    from: f64,
}

impl Walk {
    /// Where the player is now: the stretch under its time, from there.
    fn start(inner: &Inner) -> Option<Walk> {
        let s = inner.state.lock().unwrap();
        let timeline = inner.timeline.lock().unwrap().clone();
        let from = s.time(timeline.duration());
        let index = timeline.index_at(from)?;
        Some(Walk { epoch: s.epoch, timeline, index, from })
    }

    fn stretch(&self) -> &Stretch {
        &self.timeline.stretches()[self.index]
    }

    /// On to the next stretch, with the timeline as it is now; None at the end.
    fn advance(&mut self, inner: &Inner) -> Option<()> {
        let end = self.stretch().end();
        self.timeline = inner.timeline.lock().unwrap().clone();
        self.index = self.timeline.index_at(end)?;
        if self.stretch().end() <= end + 1e-6 {
            return None;
        }
        self.from = self.stretch().at.max(end);
        Some(())
    }

    /// The stretch after this one, if there is one and it has footage.
    fn next(&self) -> Option<&Stretch> {
        self.timeline.stretches().get(self.index + 1).filter(|s| s.source.is_some())
    }
}

/// Waits until the time passes `until` or the player is paused, seeks or
/// quits; whether it's still this epoch.
fn wait_until(inner: &Inner, epoch: u64, until: f64) -> bool {
    let mut s = inner.state.lock().unwrap();
    loop {
        if s.epoch != epoch || inner.quit.load(Ordering::Acquire) {
            return false;
        }
        if !s.playing {
            s = inner.wake.wait(s).unwrap();
            continue;
        }
        let wait = until - s.time(inner.duration());
        if wait <= 0.0 {
            return true;
        }
        s = inner.wake.wait_timeout(s, Duration::from_secs_f64(wait.min(0.01))).unwrap().0;
    }
}

/// The player's thread: decodes from the time, shows each frame at its moment.
fn run(inner: &Inner) {
    'start: loop {
        if inner.quit.load(Ordering::Acquire) {
            return;
        }
        let Some(mut walk) = Walk::start(inner) else {
            // Nothing to play: wait for a timeline, or to quit.
            let s = inner.state.lock().unwrap();
            let _ = inner.wake.wait_timeout(s, Duration::from_millis(100));
            continue;
        };
        let epoch = walk.epoch;
        let mut first = true;
        let mut next: Option<Stream<Arc<Frame>>> = None;
        loop {
            let stretch = walk.stretch().clone();
            let video = stretch
                .source
                .as_ref()
                .and_then(|s| inner.info(&s.path)?.picture.map(|p| Video::new(&inner.ffmpeg, &s.path, p)));
            let Some(video) = video else {
                // No footage, or no picture: nothing shows while the time passes it.
                next = None;
                if !wait_until(inner, epoch, stretch.end()) {
                    continue 'start;
                }
                if walk.advance(inner).is_none() {
                    break;
                }
                continue;
            };
            let stream = match next.take() {
                Some(stream) if !first => Some(stream),
                _ => video.from(stretch.file_time(walk.from)).ok(),
            };
            let Some(mut stream) = stream else { return };
            while let Some(frame) = stream.recv() {
                let time = stretch.time(frame.pts);
                if time >= stretch.end() - 1e-6 {
                    break;
                }
                if next.is_none()
                    && stretch.end() - time <= PREFETCH
                    && let Some(coming) = walk.next()
                    && let Some(source) = coming.source.as_ref()
                    && let Some(picture) = inner.info(&source.path).and_then(|i| i.picture)
                {
                    next = Video::new(&inner.ffmpeg, &source.path, picture).from(coming.from).ok();
                }
                // Its moment: when the time reaches it; at once if paused and
                // it's the first (the frame at the time); dropped if late.
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
                    let wait = time - s.time(inner.duration());
                    if wait <= 0.0 {
                        break;
                    }
                    // The sound's clock isn't signalled, so looked at often.
                    s = inner.wake.wait_timeout(s, Duration::from_secs_f64(wait.min(0.01))).unwrap().0;
                }
                let late = s.playing && s.time(inner.duration()) - time > LATE;
                drop(s);
                first = false;
                if late {
                    inner.dropped.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                inner.shown.fetch_add(1, Ordering::Relaxed);
                *inner.frame.lock().unwrap() = Some(frame);
                if let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
                    f(Event::Frame);
                }
            }
            drop(stream);
            // The file ran out before the stretch's end: the time passes the rest.
            if !wait_until(inner, epoch, stretch.end()) {
                continue 'start;
            }
            if walk.advance(inner).is_none() {
                break;
            }
        }
        // The timeline ended: the time stays at its end, paused.
        let mut s = inner.state.lock().unwrap();
        if s.epoch == epoch {
            s.at = inner.duration();
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
    }
}

/// Sound queued before it's let out, so it doesn't run dry at once.
const START: f64 = 0.1;

/// The sound's thread: decodes from the time and keeps the output fed while
/// playing, stretch by stretch (silence where one has no sound); once it's
/// playing out, the time follows it.
fn run_sound(inner: &Inner) {
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
            let at = s.time(inner.duration());
            s.restart(at);
        }
    }
    let rate = out.rate();
    'start: loop {
        out.set_running(false);
        out.clear();
        {
            let mut s = inner.state.lock().unwrap();
            while !s.playing && !inner.quit.load(Ordering::Acquire) {
                s = inner.wake.wait(s).unwrap();
            }
        }
        if inner.quit.load(Ordering::Acquire) {
            return;
        }
        let Some(mut walk) = Walk::start(inner) else { continue };
        let epoch = walk.epoch;
        let mut started = false;
        let mut next: Option<Stream<decode::Samples>> = None;
        let mut first = true;
        loop {
            let stretch = walk.stretch().clone();
            let audio = stretch
                .source
                .as_ref()
                .filter(|s| inner.info(&s.path).is_some_and(|i| i.audio))
                .map(|s| Audio::new(&inner.ffmpeg, &s.path));
            // Where there's no sound, silence for as long as the stretch.
            let mut stream = match (audio, next.take()) {
                (Some(_), Some(stream)) if !first => Some(stream),
                (Some(audio), _) => audio.from(stretch.file_time(walk.from), rate).ok(),
                (None, _) => None,
            };
            let mut at = walk.from;
            let end = stretch.end();
            while at < end - 1e-6 {
                {
                    let s = inner.state.lock().unwrap();
                    if s.epoch != epoch || !s.playing || inner.quit.load(Ordering::Acquire) {
                        continue 'start;
                    }
                }
                // Let out once a little is queued, so it doesn't run dry at once.
                if !started && out.queued() >= START {
                    let mut s = inner.state.lock().unwrap();
                    if s.epoch != epoch {
                        continue 'start;
                    }
                    out.recount();
                    s.sound = Some(Sound { played: out.played(), rate });
                    out.set_running(true);
                    started = true;
                }
                if out.queued() >= sound::AHEAD {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                let chunk = match stream.as_mut().and_then(|s| s.recv()) {
                    Some(samples) => {
                        // Cut at the stretch's end: the rest is the next one's.
                        let time = stretch.time(samples.pts);
                        let pairs = (((end - time) * rate as f64).round().max(0.0) as usize).min(samples.pcm.len() / 2);
                        at = time + pairs as f64 / rate as f64;
                        samples.pcm[..pairs * 2].to_vec()
                    }
                    None => {
                        // The file ran out, or has no sound: silence to the end.
                        let pairs = (((end - at) * rate as f64).round().max(0.0) as usize).min(1024);
                        at += pairs as f64 / rate as f64;
                        if pairs == 0 {
                            break;
                        }
                        vec![0i16; pairs * 2]
                    }
                };
                out.push(&chunk);
                if next.is_none()
                    && end - at <= PREFETCH
                    && let Some(coming) = walk.next()
                    && let Some(source) = coming.source.as_ref()
                    && inner.info(&source.path).is_some_and(|i| i.audio)
                {
                    next = Audio::new(&inner.ffmpeg, &source.path).from(coming.from, rate).ok();
                }
            }
            first = false;
            if walk.advance(inner).is_none() {
                break;
            }
        }
        // The timeline's sound is all queued: it plays out, and the picture
        // ends the play.
        let mut s = inner.state.lock().unwrap();
        while s.epoch == epoch && s.playing && !inner.quit.load(Ordering::Acquire) {
            s = inner.wake.wait(s).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paused_at(at: f64) -> State {
        State { playing: false, at, since: Instant::now(), epoch: 0, ended: false, sound: None, output: false }
    }

    #[test]
    fn the_time_stands_while_paused_and_follows_the_sound_while_playing() {
        let mut s = paused_at(3.0);
        assert_eq!(s.time(10.0), 3.0);
        s.playing = true;
        let played = Arc::new(AtomicU64::new(0));
        s.sound = Some(Sound { played: played.clone(), rate: 100 });
        assert_eq!(s.time(10.0), 3.0, "nothing played yet");
        played.store(150, Ordering::Release);
        assert_eq!(s.time(10.0), 4.5);
        played.store(100_000, Ordering::Release);
        assert_eq!(s.time(10.0), 10.0, "never past the end");
    }

    #[test]
    fn a_restart_waits_for_the_sound_only_where_there_is_an_output() {
        let mut s = paused_at(0.0);
        s.playing = true;
        s.restart(2.0);
        assert_eq!(s.epoch, 1);
        assert!(s.sound.is_none(), "no output: the computer's clock runs at once");
        assert!(s.time(10.0) >= 2.0);
        s.output = true;
        s.restart(5.0);
        assert_eq!(s.epoch, 2);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(s.time(10.0), 5.0, "with an output, the time stands until the sound plays");
    }
}
