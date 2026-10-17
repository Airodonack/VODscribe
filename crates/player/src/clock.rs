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
    /// Playing, the time began to move: the sound began playing out
    /// (`Player::running`). From the sound's thread.
    Running,
}

/// The sound's clock: sample pairs played out since the time was set.
struct Sound {
    played: Arc<sound::Played>,
    rate: u32,
    /// Let out to the output; until it is, the count stands at zero.
    live: bool,
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
    /// How many seconds of the timeline a second of play is.
    speed: f64,
    /// The picture has the first frame from where this epoch began, or has
    /// nothing to show there: until it does, the time stands and the sound
    /// waits, so picture and sound start together.
    ready: bool,
}

impl State {
    fn time(&self, duration: f64) -> f64 {
        if !self.playing || !self.ready {
            return self.at;
        }
        let run = match &self.sound {
            Some(s) => s.played.seconds(s.rate),
            None => self.since.elapsed().as_secs_f64(),
        };
        (self.at + run * self.speed).min(duration)
    }

    /// Whether the time moves: playing, and not waiting for the picture or
    /// the sound.
    fn running(&self) -> bool {
        self.playing && self.ready && self.sound.as_ref().is_none_or(|s| s.live)
    }

    /// Starts over from `at` (a seek): the decoders will, and the time runs
    /// from here once the picture has its frame and the sound plays again.
    fn restart(&mut self, at: f64) {
        self.at = at;
        self.epoch += 1;
        self.ready = false;
        self.resume();
    }

    /// Runs on from where it stands (play after a pause): the picture keeps
    /// its decoder and the frame it holds; the sound starts again from here.
    fn resume(&mut self) {
        self.since = Instant::now();
        self.sound = self.output.then(|| Sound { played: Arc::default(), rate: 1, live: false });
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
    /// Where in the timeline the frame showing is (none: no frame shows).
    shown: Mutex<Option<f64>>,
    on_frame: Mutex<Option<OnFrame>>,
    quit: AtomicBool,
    /// Frames shown, and dropped for being late (for measuring).
    shown_count: AtomicU64,
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
                speed: 1.0,
                ready: false,
            }),
            wake: Condvar::new(),
            frame: Mutex::new(None),
            shown: Mutex::new(None),
            on_frame: Mutex::new(None),
            quit: AtomicBool::new(false),
            shown_count: AtomicU64::new(0),
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
        // Unchanged under the playhead, or only played further: no break
        // (the threads take the new timeline up at that stretch's end).
        let same = match (held.at(time), timeline.at(time)) {
            (Some(old), Some(new)) => old.continued_by(new),
            (None, None) => true,
            _ => false,
        };
        *held = timeline;
        let duration = held.duration();
        drop(held);
        // Ended, a timeline that now goes further plays on from the end,
        // rather than from the start again.
        let longer = s.ended && duration > time + 1e-6;
        if !same || longer {
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

    /// Where in the timeline the frame showing is: where the picture really
    /// is, which the time runs ahead of between frames, and stands apart
    /// from while a seek finds its frame. None while no frame shows.
    pub fn shown(&self) -> Option<f64> {
        *self.inner.shown.lock().unwrap()
    }

    pub fn time(&self) -> f64 {
        self.inner.state.lock().unwrap().time(self.inner.duration())
    }

    pub fn playing(&self) -> bool {
        self.inner.state.lock().unwrap().playing
    }

    /// Whether the time moves: playing, and the sound (where there's an
    /// output) playing out. After play or a seek while playing the time
    /// stands until the sound starts (`Event::Running` tells when).
    pub fn running(&self) -> bool {
        self.inner.state.lock().unwrap().running()
    }

    pub fn play(&self) {
        let mut s = self.inner.state.lock().unwrap();
        if s.playing {
            return;
        }
        s.playing = true;
        if s.ended {
            // From the start again: the decoders start over.
            s.ended = false;
            s.restart(0.0);
        } else {
            s.resume();
        }
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
        (self.inner.shown_count.load(Ordering::Relaxed), self.inner.dropped.load(Ordering::Relaxed))
    }

    /// How many seconds of the timeline a second of play is.
    pub fn speed(&self) -> f64 {
        self.inner.state.lock().unwrap().speed
    }

    /// Plays `speed` times as fast (within 0.25 to 4), the sound at the
    /// same pitch. Playing, it starts over from the time now, as a seek
    /// does: the sound's decoder is made for one speed.
    pub fn set_speed(&self, speed: f64) {
        let speed = speed.clamp(0.25, 4.0);
        let mut s = self.inner.state.lock().unwrap();
        if s.speed == speed {
            return;
        }
        if s.playing {
            let at = s.time(self.inner.duration());
            s.speed = speed;
            s.restart(at);
        } else {
            s.speed = speed;
        }
        self.inner.wake.notify_all();
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
        // Under the state's lock, so no thread between reading `quit` and
        // waiting misses the wake.
        let _s = self.inner.state.lock().unwrap();
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
    /// The speed, as it was when this epoch began (a change starts a new one).
    speed: f64,
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
        Some(Walk { epoch: s.epoch, speed: s.speed, timeline, index, from })
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

    /// `next`'s stream, if it was readied for the stretch now being
    /// played, from where it plays: a timeline planned again since may
    /// have changed what comes after.
    fn readied<T>(&self, next: Option<(Stretch, T)>) -> Option<T> {
        next.filter(|(stretch, _)| stretch == self.stretch() && stretch.at == self.from).map(|(_, stream)| stream)
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

/// The picture is ready for epoch `epoch` (its first frame is in hand, or
/// there's none to show): the time may run, and without a sound output it
/// runs from now (told as `Event::Running`).
fn mark_ready(inner: &Inner, epoch: u64) {
    let mut s = inner.state.lock().unwrap();
    if s.epoch != epoch || s.ready {
        return;
    }
    s.ready = true;
    let by_clock = s.sound.is_none();
    if by_clock {
        s.since = Instant::now();
    }
    let running = s.playing && by_clock;
    drop(s);
    inner.wake.notify_all();
    if running && let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
        f(Event::Running);
    }
}

/// The player's thread: decodes from the time, shows each frame at its moment.
fn run(inner: &Inner) {
    'start: loop {
        if inner.quit.load(Ordering::Acquire) {
            return;
        }
        let Some(mut walk) = Walk::start(inner) else {
            // Nothing to play (an edit emptied): no frame shows. Then wait
            // for a timeline, or to quit.
            *inner.shown.lock().unwrap() = None;
            if inner.frame.lock().unwrap().take().is_some()
                && let Some(f) = inner.on_frame.lock().unwrap().as_ref()
            {
                f(Event::Frame);
            }
            let s = inner.state.lock().unwrap();
            let _ = inner.wake.wait_timeout(s, Duration::from_millis(100));
            continue;
        };
        let epoch = walk.epoch;
        let mut first = true;
        let mut next: Option<(Stretch, Stream<Arc<Frame>>)> = None;
        loop {
            let stretch = walk.stretch().clone();
            let video = stretch
                .source
                .as_ref()
                .and_then(|s| inner.info(&s.path)?.picture.map(|p| Video::new(&inner.ffmpeg, &s.path, p)));
            let Some(video) = video else {
                // No footage, or no picture: nothing shows while the time passes it.
                *inner.shown.lock().unwrap() = None;
                mark_ready(inner, epoch);
                next = None;
                if !wait_until(inner, epoch, stretch.end()) {
                    continue 'start;
                }
                if walk.advance(inner).is_none() {
                    break;
                }
                continue;
            };
            let stream = match walk.readied(next.take()) {
                Some(stream) if !first => Some(stream),
                _ => video.from(stretch.file_time(walk.from)).ok(),
            };
            let Some(mut stream) = stream else {
                // ffmpeg wouldn't start: nothing to show till the next seek.
                mark_ready(inner, epoch);
                wait_until(inner, epoch, f64::INFINITY);
                continue 'start;
            };
            while let Some(frame) = stream.recv() {
                if first {
                    mark_ready(inner, epoch);
                }
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
                    next = Video::new(&inner.ffmpeg, &source.path, picture)
                        .from(coming.from)
                        .ok()
                        .map(|stream| (coming.clone(), stream));
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
                inner.shown_count.fetch_add(1, Ordering::Relaxed);
                *inner.frame.lock().unwrap() = Some(frame);
                *inner.shown.lock().unwrap() = Some(time);
                if let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
                    f(Event::Frame);
                }
            }
            drop(stream);
            mark_ready(inner, epoch);
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
        let (epoch, speed) = (walk.epoch, walk.speed);
        // Seconds of the timeline a sample pair plays.
        let pair = speed / rate as f64;
        let mut started = false;
        let mut next: Option<(Stretch, Stream<decode::Samples>)> = None;
        let mut first = true;
        loop {
            let stretch = walk.stretch().clone();
            let audio = stretch
                .source
                .as_ref()
                .filter(|s| inner.info(&s.path).is_some_and(|i| i.audio))
                .map(|s| Audio::new(&inner.ffmpeg, &s.path));
            // Where there's no sound, silence for as long as the stretch.
            let mut stream = match (audio, walk.readied(next.take())) {
                (Some(_), Some(stream)) if !first => Some(stream),
                (Some(audio), _) => audio.from(stretch.file_time(walk.from), rate, speed).ok(),
                (None, _) => None,
            };
            let mut at = walk.from;
            let end = stretch.end();
            while at < end - 1e-6 {
                if !still_playing(inner, &out, epoch, started) {
                    continue 'start;
                }
                // Let out once a little is queued, so it doesn't run dry at
                // once, and the picture is ready to start with it (till then
                // the queue fills, and waits full).
                if !started && out.queued() >= START {
                    match let_out(inner, &out, epoch, rate) {
                        None => continue 'start,
                        Some(live) => started = live,
                    }
                }
                if out.queued() >= sound::AHEAD {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                let chunk = match stream.as_mut().and_then(|s| s.recv()) {
                    Some(samples) => {
                        // Cut at the stretch's end: the rest is the next one's.
                        let time = stretch.time(samples.pts);
                        let pairs = ((end - time) / pair).round().max(0.0) as usize;
                        let pairs = pairs.min(samples.pcm.len() / 2);
                        at = time + pairs as f64 * pair;
                        samples.pcm[..pairs * 2].to_vec()
                    }
                    None => {
                        // The file ran out, or has no sound: silence to the end.
                        let pairs = (((end - at) / pair).round().max(0.0) as usize).min(1024);
                        at += pairs as f64 * pair;
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
                    next = Audio::new(&inner.ffmpeg, &source.path)
                        .from(coming.from, rate, speed)
                        .ok()
                        .map(|stream| (coming.clone(), stream));
                }
            }
            first = false;
            if walk.advance(inner).is_none() {
                break;
            }
        }
        // The timeline's sound is all queued (less of it, near the end, than
        // is let out at): it's let out once the picture is ready.
        while !started {
            if !still_playing(inner, &out, epoch, started) {
                continue 'start;
            }
            match let_out(inner, &out, epoch, rate) {
                None => continue 'start,
                Some(live) => started = live,
            }
            if !started {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        // It plays out, and the picture ends the play.
        let mut s = inner.state.lock().unwrap();
        while s.epoch == epoch && s.playing && ours(&s, &out) && !inner.quit.load(Ordering::Acquire) {
            s = inner.wake.wait(s).unwrap();
        }
    }
}

/// Whether the time follows `out`'s count: a pause and play since it was
/// let out set a new count, which nothing counts.
fn ours(s: &State, out: &Output) -> bool {
    s.sound.as_ref().is_some_and(|sound| Arc::ptr_eq(&sound.played, &out.played()))
}

/// Whether the sound thread plays on in epoch `epoch`: not paused, sought,
/// quit, or (once `started`) paused and played again in between.
fn still_playing(inner: &Inner, out: &Output, epoch: u64, started: bool) -> bool {
    let s = inner.state.lock().unwrap();
    s.epoch == epoch && s.playing && !inner.quit.load(Ordering::Acquire) && (!started || ours(&s, out))
}

/// Lets the sound queued out, if the picture's ready, and the time follows
/// it from now (told as `Event::Running`); whether it did, or None if it's
/// no longer epoch `epoch`.
fn let_out(inner: &Inner, out: &Output, epoch: u64, rate: u32) -> Option<bool> {
    let mut s = inner.state.lock().unwrap();
    if s.epoch != epoch {
        return None;
    }
    if !s.ready {
        return Some(false);
    }
    out.recount();
    s.sound = Some(Sound { played: out.played(), rate, live: true });
    out.set_running(true);
    drop(s);
    if let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
        f(Event::Running);
    }
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paused_at(at: f64) -> State {
        State {
            playing: false,
            at,
            since: Instant::now(),
            epoch: 0,
            ended: false,
            sound: None,
            output: false,
            speed: 1.0,
            ready: true,
        }
    }

    #[test]
    fn the_time_stands_while_paused_and_follows_the_sound_while_playing() {
        let mut s = paused_at(3.0);
        assert_eq!(s.time(10.0), 3.0);
        s.playing = true;
        let played = Arc::new(sound::Played::default());
        s.sound = Some(Sound { played: played.clone(), rate: 100, live: true });
        assert!(s.running());
        assert_eq!(s.time(10.0), 3.0, "nothing played yet");
        played.set(150);
        assert_eq!(s.time(10.0), 4.5);
        played.set(100_000);
        assert_eq!(s.time(10.0), 10.0, "never past the end");
    }

    #[test]
    fn a_restart_waits_for_the_picture_and_then_for_the_sound_where_there_is_an_output() {
        let mut s = paused_at(0.0);
        s.playing = true;
        s.restart(2.0);
        assert_eq!(s.epoch, 1);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(s.time(10.0), 2.0, "the time stands until the picture has its frame");
        assert!(!s.running());
        // No output: the computer's clock runs once the picture is ready.
        s.ready = true;
        s.since = Instant::now();
        assert!(s.sound.is_none());
        assert!(s.running());
        std::thread::sleep(Duration::from_millis(5));
        assert!(s.time(10.0) > 2.0);
        // With an output, the picture ready isn't enough: the sound must play.
        s.output = true;
        s.restart(5.0);
        s.ready = true;
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(s.time(10.0), 5.0, "with an output, the time stands until the sound plays");
        assert!(!s.running(), "and isn't said to move");
    }

    #[test]
    fn play_after_a_pause_keeps_the_picture_s_place() {
        let mut s = paused_at(3.0);
        s.playing = true;
        s.resume();
        assert_eq!((s.epoch, s.ready), (0, true), "no new epoch: the decoders keep where they are");
    }

    #[test]
    fn the_time_runs_at_the_speed() {
        let mut s = paused_at(2.0);
        s.playing = true;
        s.speed = 1.5;
        let played = Arc::new(sound::Played::default());
        s.sound = Some(Sound { played: played.clone(), rate: 100, live: true });
        played.set(200);
        assert_eq!(s.time(10.0), 5.0, "two seconds played out are three of the timeline");
    }
}
