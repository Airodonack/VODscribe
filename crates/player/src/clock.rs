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
//!
//! Besides the decoder playing, a second one previews: the exact frame at a
//! time the timeline's pointer rests on (`Player::preview`), and half a
//! second on from it. A seek to a moment it holds (play where the pointer
//! was) takes that decoder over rather than starting another (`adopt`):
//! the picture is there at once. The one that was playing becomes the
//! preview decoder, with the frames it holds.

use std::collections::{HashMap, VecDeque};
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

/// How far past the frame previewed the preview decoder reads on, in seconds.
const PREVIEW_AHEAD: f64 = 0.5;
/// The most a decoder holds of frames read, in bytes: half a second of
/// 1080p and more, fewer frames of larger video.
const HOLD_BYTES: usize = 96 << 20;
/// How much of what it's shown the playing decoder keeps, in seconds: what
/// it held when it's taken over plays, so it needn't keep more, and the
/// rest would only weigh on playing. The preview decoder keeps all it can.
const PLAYED_KEPT: f64 = 0.5;

/// A decoder: one ffmpeg reading a stretch's file from a frame on, and the
/// frames read from it, held.
struct Decoder {
    stretch: Stretch,
    picture: Picture,
    stream: Stream<Arc<Frame>>,
    /// Frames read, in order: those before `next` given out already (kept
    /// to preview from), the rest still to come.
    frames: VecDeque<Arc<Frame>>,
    next: usize,
    /// Bytes of `frames`.
    bytes: usize,
    /// Read ahead (by `fill`) to this pts in the file.
    until: f64,
    /// How many seconds of the frames given out it keeps, behind the last.
    kept: f64,
}

impl Decoder {
    /// Decoding `stretch`'s file, of `picture`, from `file_time`.
    fn start(ffmpeg: &Path, stretch: &Stretch, picture: Picture, file_time: f64) -> Option<Decoder> {
        let path = &stretch.source.as_ref()?.path;
        let stream = Video::new(ffmpeg, path, picture).from(file_time).ok()?;
        Some(Decoder::new(stretch.clone(), picture, stream))
    }

    fn new(stretch: Stretch, picture: Picture, stream: Stream<Arc<Frame>>) -> Decoder {
        Decoder {
            stretch,
            picture,
            stream,
            frames: VecDeque::new(),
            next: 0,
            bytes: 0,
            until: f64::NEG_INFINITY,
            kept: PLAYED_KEPT,
        }
    }

    /// The next frame: one held, or the next out of ffmpeg.
    fn recv(&mut self) -> Option<Arc<Frame>> {
        if let Some(frame) = self.frames.get(self.next).cloned() {
            self.next += 1;
            self.trim();
            return Some(frame);
        }
        let frame = self.stream.recv()?;
        self.hold(frame.clone());
        self.next = self.frames.len();
        self.trim();
        Some(frame)
    }

    fn hold(&mut self, frame: Arc<Frame>) {
        self.bytes += frame.yuv.len();
        self.frames.push_back(frame);
        self.trim();
    }

    /// Lets go of the frames given out longest ago: those more than `kept`
    /// seconds behind the last given out, and any while over `HOLD_BYTES`.
    fn trim(&mut self) {
        let last = self.next.checked_sub(1).map(|i| self.frames[i].pts);
        while self.next > 0 {
            let old = self.frames[0].pts < last.unwrap_or(f64::NEG_INFINITY) - self.kept - 1e-6;
            if !old && self.bytes <= HOLD_BYTES {
                break;
            }
            let gone = self.frames.pop_front().expect("one given out");
            self.bytes -= gone.yuv.len();
            self.next -= 1;
        }
    }

    /// Reads on what ffmpeg has decoded, up to `until` (within
    /// `HOLD_BYTES`); whether it's to read more when there's more.
    fn fill(&mut self) -> bool {
        loop {
            let reached = self.frames.back().is_some_and(|f| f.pts >= self.until - 1e-6);
            if reached || self.bytes >= HOLD_BYTES {
                return false;
            }
            match self.stream.try_recv() {
                Some(frame) => self.hold(frame),
                None => return true,
            }
        }
    }

    /// Which of the frames held is the one at `file_time`, if this decodes
    /// `stretch` at `picture`'s rate and size.
    fn holds(&self, stretch: &Stretch, picture: Picture, file_time: f64) -> Option<usize> {
        if self.stretch != *stretch || self.picture != picture {
            return None;
        }
        let half = 0.5 / picture.fps;
        self.frames.iter().position(|f| (f.pts - file_time).abs() < half)
    }
}

/// What `Player::preview` gives: the frame at a time, where in the
/// timeline it is, and whether it's exactly the one showing there (else the
/// first ffmpeg gave, the file not reaching the time).
pub struct Preview {
    pub frame: Arc<Frame>,
    pub time: f64,
    pub exact: bool,
}

/// Where a seek to `time` lands: on the frame showing there, at `fps`,
/// within `duration`; but not back out of a stretch with footage that
/// starts at `start`, off the frame grid, into the gap before it.
fn snap(time: f64, fps: f64, duration: f64, start: Option<f64>) -> f64 {
    let time = time.max(0.0);
    let snapped = ((time * fps + 1e-6).floor() / fps).min(duration);
    match start {
        Some(start) if snapped < start && time >= start => start,
        _ => snapped,
    }
}

struct Inner {
    ffmpeg: PathBuf,
    /// The preview decoder (`Player::preview`), and what wakes its thread to read on.
    preview: Mutex<Option<Decoder>>,
    preview_wake: Condvar,
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
    /// Where the footage isn't here, the time stands until it is (a VOD from
    /// a site, fetched as it plays), rather than passing it in black (an edit).
    waits_for_footage: AtomicBool,
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

    /// Where a seek to `time` lands (`Player::snap`).
    fn snap(&self, time: f64) -> f64 {
        let fps = self.picture_at(time).map_or(DEFAULT_FPS, |p| p.fps);
        let start = self.timeline.lock().unwrap().at(time.max(0.0)).filter(|s| s.source.is_some()).map(|s| s.at);
        snap(time, fps, self.duration(), start)
    }

    /// The preview decoder, if it holds the frame at `file_time` of
    /// `stretch` (at `picture`): taken to play from that frame on.
    fn adopt(&self, stretch: &Stretch, picture: Picture, file_time: f64) -> Option<Decoder> {
        let mut slot = self.preview.lock().unwrap();
        let at = slot.as_ref()?.holds(stretch, picture, file_time)?;
        let mut decoder = slot.take()?;
        // Those before it count as given out: kept, to preview from later.
        decoder.next = at;
        decoder.until = f64::NEG_INFINITY;
        decoder.kept = PLAYED_KEPT;
        decoder.trim();
        Some(decoder)
    }

    /// `decoder`, played from till now, previews from here on with what it
    /// holds, unless a preview was asked for meanwhile.
    fn keep_for_preview(&self, mut decoder: Decoder) {
        let mut slot = self.preview.lock().unwrap();
        if slot.is_none() {
            decoder.until = f64::NEG_INFINITY;
            decoder.kept = f64::INFINITY;
            *slot = Some(decoder);
        }
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
            preview: Mutex::new(None),
            preview_wake: Condvar::new(),
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
            waits_for_footage: AtomicBool::new(false),
        });
        let shared = inner.clone();
        std::thread::Builder::new().name("player".into()).spawn(move || run(&shared))?;
        let shared = inner.clone();
        std::thread::Builder::new().name("sound".into()).spawn(move || run_sound(&shared))?;
        let shared = inner.clone();
        std::thread::Builder::new().name("preview".into()).spawn(move || run_preview(&shared))?;
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
        // Nothing left to play (its last piece taken out): paused.
        if duration <= 0.0 && s.playing {
            s.at = 0.0;
            s.playing = false;
            s.sound = None;
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
        // Nothing to play (an edit with no pieces): it stays paused.
        if s.playing || self.inner.duration() <= 0.0 {
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

    /// Where a seek to `time` lands: the start of the frame showing there.
    /// Not back out of the footage `time` is in, into a gap before it: a
    /// site's footage can start off the frame grid (Twitch's, where its
    /// first frame shows, 0.067 s in its millisecond times, just past the
    /// grid's 0.0667), and in the gap the player waits for footage.
    pub fn snap(&self, time: f64) -> f64 {
        self.inner.snap(time)
    }

    /// Goes to the frame showing at `time`.
    pub fn seek(&self, time: f64) {
        let time = self.snap(time);
        let mut s = self.inner.state.lock().unwrap();
        s.ended = false;
        s.restart(time);
        self.inner.wake.notify_all();
    }

    /// The frame a seek to `time` would show, without seeking (`Previewer::preview`).
    pub fn preview(&self, time: f64) -> Option<Preview> {
        self.previewer().preview(time)
    }

    /// What previews this player's frames, apart from it: so the app can
    /// wait for a frame without holding the player meanwhile.
    pub fn previewer(&self) -> Previewer {
        Previewer { inner: self.inner.clone() }
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

    /// Whether the time stands where the footage isn't here, until it is
    /// (`set_timeline` with it there plays on), rather than passing it.
    pub fn set_waits_for_footage(&self, waits: bool) {
        self.inner.waits_for_footage.store(waits, Ordering::Release);
        // Started over where it is, as a seek does: the threads may have
        // looked at what's under the playhead already (they start as it
        // opens), and marked it ready to run past footage that isn't here.
        let mut s = self.inner.state.lock().unwrap();
        let time = s.time(self.inner.duration());
        s.restart(time);
        drop(s);
        self.inner.wake.notify_all();
    }

    /// Whether the time stands where it is for footage not here yet.
    pub fn waiting_for_footage(&self) -> bool {
        if !self.inner.waits_for_footage.load(Ordering::Acquire) {
            return false;
        }
        let s = self.inner.state.lock().unwrap();
        let time = s.time(self.inner.duration());
        let playing = s.playing;
        drop(s);
        playing && self.inner.timeline.lock().unwrap().at(time).is_some_and(|stretch| stretch.source.is_none())
    }

    pub fn set_volume(&self, volume: f32) {
        self.inner.volume.lock().unwrap().store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Release);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Under the state's lock, so no thread between reading `quit` and
        // waiting misses the wake.
        let s = self.inner.state.lock().unwrap();
        self.inner.quit.store(true, Ordering::Release);
        self.inner.wake.notify_all();
        drop(s);
        // And the preview's thread, under its lock likewise.
        let _p = self.inner.preview.lock().unwrap();
        self.inner.preview_wake.notify_all();
    }
}

/// Previews a player's frames (`Player::previewer`).
pub struct Previewer {
    inner: Arc<Inner>,
}

impl Previewer {
    /// The frame a seek to `time` would show, without seeking: from the
    /// preview decoder if it holds it, else from a new one started there
    /// (which then reads on half a second, for the next times asked, and for
    /// play from one of them to take over). None where nothing plays at
    /// `time`, ffmpeg gives nothing, or the player is closed.
    pub fn preview(&self, time: f64) -> Option<Preview> {
        if self.inner.quit.load(Ordering::Acquire) {
            return None;
        }
        let time = self.inner.snap(time);
        let stretch = self.inner.timeline.lock().unwrap().at(time)?.clone();
        let source = stretch.source.as_ref()?;
        let picture = self.inner.info(&source.path)?.picture?;
        let file_time = stretch.file_time(time);
        let mut slot = self.inner.preview.lock().unwrap();
        if let Some(decoder) = slot.as_mut()
            && let Some(at) = decoder.holds(&stretch, picture, file_time)
        {
            let frame = decoder.frames[at].clone();
            decoder.until = decoder.until.max(frame.pts + PREVIEW_AHEAD);
            self.inner.preview_wake.notify_all();
            return Some(Preview { time: stretch.time(frame.pts), frame, exact: true });
        }
        // The one there stops before another starts.
        *slot = None;
        let mut decoder = Decoder::start(&self.inner.ffmpeg, &stretch, picture, file_time)?;
        decoder.kept = f64::INFINITY;
        let frame = decoder.stream.recv()?;
        decoder.hold(frame.clone());
        decoder.until = frame.pts + PREVIEW_AHEAD;
        let exact = (frame.pts - file_time).abs() < 0.5 / picture.fps;
        // Closed meanwhile: its thread no longer clears the slot.
        if !self.inner.quit.load(Ordering::Acquire) {
            *slot = Some(decoder);
        }
        self.inner.preview_wake.notify_all();
        Some(Preview { time: stretch.time(frame.pts), frame, exact })
    }
}

/// The preview decoder's thread: reads on what it's decoded, up to half a
/// second past the frame last previewed, so the next frames are at hand.
fn run_preview(inner: &Inner) {
    let mut slot = inner.preview.lock().unwrap();
    loop {
        if inner.quit.load(Ordering::Acquire) {
            // Its ffmpeg stops now, not when the last of the player goes.
            *slot = None;
            return;
        }
        if slot.as_mut().is_some_and(Decoder::fill) {
            // More to read, not decoded yet.
            drop(slot);
            std::thread::sleep(Duration::from_millis(5));
            slot = inner.preview.lock().unwrap();
        } else {
            slot = inner.preview_wake.wait(slot).unwrap();
        }
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
    // The decoder playing, kept past a seek: if the one previewing is taken
    // over there, this one previews in its place.
    let mut current: Option<Decoder> = None;
    'start: loop {
        if inner.quit.load(Ordering::Acquire) {
            return;
        }
        let Some(mut walk) = Walk::start(inner) else {
            // Nothing to play (an edit emptied): no frame shows. Then wait
            // for a timeline, or to quit.
            show_nothing(inner);
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
            if stretch.source.is_none() && inner.waits_for_footage.load(Ordering::Acquire) {
                // Footage not here yet, and the time waits for it: it stands
                // at the stretch's start (begun again there if it ran into
                // it) till a timeline with the footage, or a seek, starts over.
                *inner.shown.lock().unwrap() = None;
                if !first {
                    let mut s = inner.state.lock().unwrap();
                    if s.epoch == epoch {
                        s.restart(walk.from.max(stretch.at));
                        inner.wake.notify_all();
                    }
                    continue 'start;
                }
                wait_until(inner, epoch, f64::INFINITY);
                continue 'start;
            }
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
            let file_time = stretch.file_time(walk.from);
            let picture = video.picture();
            let (decoder, adopted) = match walk.readied(next.take()) {
                Some(stream) if !first => (Some(Decoder::new(stretch.clone(), picture, stream)), false),
                // The preview decoder, if it holds the frame here; else a new one.
                _ => match inner.adopt(&stretch, picture, file_time) {
                    Some(decoder) => (Some(decoder), true),
                    None => (Decoder::start(&inner.ffmpeg, &stretch, picture, file_time), false),
                },
            };
            let old = std::mem::replace(&mut current, decoder);
            if let Some(old) = old.filter(|_| adopted) {
                inner.keep_for_preview(old);
            }
            let Some(stream) = current.as_mut() else {
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
                match moment(inner, epoch, time, first) {
                    Moment::Gone => continue 'start,
                    Moment::Late => {
                        inner.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    Moment::Now => show(inner, frame, time),
                }
                first = false;
            }
            current = None;
            mark_ready(inner, epoch);
            // The file ran out before the stretch's end: the time passes the rest.
            if !wait_until(inner, epoch, stretch.end()) {
                continue 'start;
            }
            if walk.advance(inner).is_none() {
                break;
            }
        }
        end(inner, epoch);
    }
}

/// No frame shows (nothing to play).
fn show_nothing(inner: &Inner) {
    *inner.shown.lock().unwrap() = None;
    if inner.frame.lock().unwrap().take().is_some()
        && let Some(f) = inner.on_frame.lock().unwrap().as_ref()
    {
        f(Event::Frame);
    }
}

/// When a frame due at `time` shows (`moment`).
enum Moment {
    Now,
    /// Its moment passed while it decoded: dropped.
    Late,
    /// A seek, a new timeline or quitting came first.
    Gone,
}

/// Waits for the moment of the frame due at `time` in epoch `epoch`: when
/// the time reaches it; at once if paused and it's the `first` (the frame at
/// the time).
fn moment(inner: &Inner, epoch: u64, time: f64, first: bool) -> Moment {
    let mut s = inner.state.lock().unwrap();
    loop {
        if s.epoch != epoch || inner.quit.load(Ordering::Acquire) {
            return Moment::Gone;
        }
        if !s.playing {
            if first {
                return Moment::Now;
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
    if s.time(inner.duration()) - time > LATE { Moment::Late } else { Moment::Now }
}

/// Shows `frame`, the one at `time`.
fn show(inner: &Inner, frame: Arc<Frame>, time: f64) {
    inner.shown_count.fetch_add(1, Ordering::Relaxed);
    *inner.frame.lock().unwrap() = Some(frame);
    *inner.shown.lock().unwrap() = Some(time);
    if let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
        f(Event::Frame);
    }
}

/// The timeline of epoch `epoch` ended: the time stays at its end, paused,
/// until a seek, a new timeline or quitting.
fn end(inner: &Inner, epoch: u64) {
    let mut s = inner.state.lock().unwrap();
    if s.epoch != epoch {
        return;
    }
    s.at = inner.duration();
    s.playing = false;
    s.sound = None;
    s.ended = true;
    drop(s);
    // The sound thread waits while playing: it lets its output go.
    inner.wake.notify_all();
    if let Some(f) = inner.on_frame.lock().unwrap().as_ref() {
        f(Event::Ended);
    }
    s = inner.state.lock().unwrap();
    while s.epoch == epoch && !inner.quit.load(Ordering::Acquire) {
        s = inner.wake.wait(s).unwrap();
    }
}

/// Sound queued before it's let out, so it doesn't run dry at once.
const START: f64 = 0.1;

/// Where `stretch`'s sound comes from: its own file, or its source's.
fn sound_of(stretch: &Stretch) -> Option<&Path> {
    stretch.sound.as_ref().map(|s| s.path.as_path()).or(stretch.source.as_ref().map(|s| s.path.as_path()))
}

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
        let Some(mut walk) = Walk::start(inner) else {
            // Nothing to play where it is: wait for a change (a new timeline,
            // a seek, a pause), rather than look again at once.
            let s = inner.state.lock().unwrap();
            let epoch = s.epoch;
            let _ = inner.wake.wait_timeout_while(s, Duration::from_millis(250), |s| {
                s.epoch == epoch && s.playing && !inner.quit.load(Ordering::Acquire)
            });
            continue;
        };
        let (epoch, speed) = (walk.epoch, walk.speed);
        // Seconds of the timeline a sample pair plays.
        let pair = speed / rate as f64;
        let mut started = false;
        let mut next: Option<(Stretch, Stream<decode::Samples>)> = None;
        let mut first = true;
        loop {
            let stretch = walk.stretch().clone();
            let audio = sound_of(&stretch)
                .filter(|path| inner.info(path).is_some_and(|i| i.audio))
                .map(|path| Audio::new(&inner.ffmpeg, path));
            // Where there's no sound, silence for as long as the stretch.
            let mut stream = match (audio, walk.readied(next.take())) {
                (Some(_), Some(stream)) if !first => Some(stream),
                (Some(audio), _) => audio.from(stretch.sound_time(walk.from), rate, speed).ok(),
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
                let Some(chunk) = next_chunk(stream.as_mut(), &stretch, &mut at, pair) else { break };
                out.push(&chunk);
                if next.is_none()
                    && end - at <= PREFETCH
                    && let Some(coming) = walk.next()
                    && let Some(path) = sound_of(coming)
                    && inner.info(path).is_some_and(|i| i.audio)
                {
                    next = Audio::new(&inner.ffmpeg, path)
                        .from(coming.sound_time(coming.at), rate, speed)
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

/// The next sound of `stretch` from `at` (moved on past it), `pair` seconds
/// of the timeline a sample pair: from `stream`, cut at the stretch's end
/// (the rest is the next one's), or silence where the file ran out or has
/// no sound; None once the stretch is all queued.
fn next_chunk(
    stream: Option<&mut Stream<decode::Samples>>,
    stretch: &Stretch,
    at: &mut f64,
    pair: f64,
) -> Option<Vec<i16>> {
    let end = stretch.end();
    match stream.and_then(|s| s.recv()) {
        Some(samples) => {
            let time = stretch.time(samples.pts);
            let pairs = (((end - time) / pair).round().max(0.0) as usize).min(samples.pcm.len() / 2);
            *at = time + pairs as f64 * pair;
            Some(samples.pcm[..pairs * 2].to_vec())
        }
        None => {
            let pairs = (((end - *at) / pair).round().max(0.0) as usize).min(1024);
            *at += pairs as f64 * pair;
            (pairs > 0).then(|| vec![0i16; pairs * 2])
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
    fn a_seek_and_a_preview_land_on_the_frame_showing() {
        assert_eq!(snap(1.51, 30.0, 4.0, None), 1.5);
        // A frame's start, a hair below in floating point, is that frame.
        assert_eq!(snap(52.0 / 30.0 - 1e-9, 30.0, 4.0, None), 52.0 / 30.0);
        assert_eq!(snap(9.0, 30.0, 4.0, None), 4.0, "within the timeline");
        assert_eq!(snap(-1.0, 30.0, 4.0, None), 0.0);
        // Footage starting off the grid: not back into the gap before it.
        assert_eq!(snap(0.07, 30.0, 4.0, Some(0.067)), 0.067);
        assert_eq!(snap(0.05, 30.0, 4.0, Some(0.067)), 1.0 / 30.0, "before it, as ever");
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
