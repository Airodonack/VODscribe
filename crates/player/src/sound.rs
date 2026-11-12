//! Sound out, through the computer's default output (cpal). Samples queued
//! here are played in order; how many have been played is the sound's own
//! clock, which the player's time follows while it plays (`clock`), so the
//! picture keeps to the sound rather than the other way round. With no
//! output to be had (a computer without one, a headless test), there's no
//! sound and the player keeps time by the computer's clock.
//!
//! The output stream runs only while there's sound to play: paused, it's
//! stopped, so an idle player costs the computer no wakeups and can't
//! underrun. Underruns while playing are counted and told once at the end
//! of the stretch of play, rather than a line each.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

/// What the output stream shares with the player: the queue, the count of
/// stereo sample pairs played, whether it's running, and the loudness.
type Shared = (Arc<Mutex<VecDeque<i16>>>, Arc<Played>, Arc<AtomicBool>, Arc<AtomicU32>);

/// The stereo sample pairs played, counted as the output takes them, a
/// chunk at a time (512 pairs, about 11 ms, on a desktop's PipeWire): the
/// sound's clock. Read as time it runs on smoothly between chunks, so
/// frames timed by it come evenly rather than on the chunks' beat.
#[derive(Default)]
pub struct Played(Mutex<Tick>);

#[derive(Default)]
struct Tick {
    count: u64,
    /// When the count last moved, and by how many seconds' worth.
    at: Option<Instant>,
    step: f64,
}

impl Played {
    /// `pairs` more played, of `rate` a second.
    fn add(&self, pairs: u64, rate: u32) {
        let mut t = self.0.lock().unwrap();
        t.count += pairs;
        t.at = Some(Instant::now());
        t.step = pairs as f64 / rate as f64;
    }

    /// Starts the count over.
    pub fn reset(&self) {
        *self.0.lock().unwrap() = Tick::default();
    }

    /// Sets the count, as if that many had just been played (tests).
    pub fn set(&self, count: u64) {
        *self.0.lock().unwrap() = Tick { count, at: None, step: 0.0 };
    }

    /// Seconds played at `rate` a second: the count, and the time since it
    /// last moved, up to as much as it moved then (it won't run ahead of
    /// the next chunk where the sound stops).
    pub fn seconds(&self, rate: u32) -> f64 {
        let t = self.0.lock().unwrap();
        let since = t.at.map_or(0.0, |at| at.elapsed().as_secs_f64().min(t.step));
        t.count as f64 / rate as f64 + since
    }
}

/// The output's configuration: its default if that's stereo, else a stereo
/// one it supports at a rate near the default's, else the default as it is.
fn pick(device: &cpal::Device) -> Option<cpal::SupportedStreamConfig> {
    let default = device.default_output_config().ok()?;
    if default.channels() == 2 {
        return Some(default);
    }
    let Some(stereo) = device.supported_output_configs().ok()?.find(|c| c.channels() == 2) else {
        return Some(default);
    };
    let rate = default.sample_rate().clamp(stereo.min_sample_rate(), stereo.max_sample_rate());
    Some(stereo.with_sample_rate(rate))
}

/// The output stream, writing samples of its kind: the two channels queued
/// on the device's first two, the rest silent (one channel: their mean).
fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    rate: u32,
    shared: Shared,
    underruns: Arc<AtomicU64>,
) -> Option<cpal::Stream> {
    device
        .build_output_stream(
            *config,
            move |out: &mut [T], _| fill(out, channels, rate, &shared),
            move |e| match e.kind() {
                cpal::ErrorKind::Xrun => {
                    underruns.fetch_add(1, Ordering::Relaxed);
                }
                _ => eprintln!("sound: {e}"),
            },
            None,
        )
        .ok()
}

/// Fills `out` (frames of `channels`) from the queue while playing, else
/// with silence, and counts the pairs taken.
fn fill<T: SizedSample + FromSample<f32>>(
    out: &mut [T],
    channels: usize,
    rate: u32,
    (taken, counted, on, gain): &Shared,
) {
    // Silence unless playing, or when the queue runs dry (decoding behind):
    // then the time stands too. The queue's other users hold it only to
    // copy a chunk in or read its length, so waiting for it is
    // microseconds; taking silence instead (try_lock) dropped a whole
    // chunk, a click.
    let mut queue =
        if on.load(Ordering::Acquire) { Some(taken.lock().unwrap_or_else(|e| e.into_inner())) } else { None };
    let gain = f32::from_bits(gain.load(Ordering::Acquire));
    let mut got = 0;
    for frame in out.chunks_mut(channels) {
        let pair = queue.as_mut().and_then(|q| Some((q.pop_front()?, q.pop_front()?)));
        let (l, r) = match pair {
            Some((l, r)) => {
                got += 1;
                (l as f32 / i16::MAX as f32 * gain, r as f32 / i16::MAX as f32 * gain)
            }
            None => (0.0, 0.0),
        };
        for (i, sample) in frame.iter_mut().enumerate() {
            *sample = T::from_sample(match (channels, i) {
                (1, _) => (l + r) / 2.0,
                (_, 0) => l,
                (_, 1) => r,
                _ => 0.0,
            });
        }
    }
    if got > 0 {
        counted.add(got, rate);
    }
}

/// The output stream: the device's, or (tests) a stand-in.
enum Stream {
    Device(cpal::Stream),
    #[cfg(feature = "counts")]
    StandIn(stand_in::Stream),
}

impl Stream {
    fn play(&self) {
        match self {
            Stream::Device(stream) => {
                if let Err(e) = stream.play() {
                    eprintln!("sound: {e}");
                }
            }
            #[cfg(feature = "counts")]
            Stream::StandIn(stream) => stream.set_going(true),
        }
    }

    fn pause(&self) {
        match self {
            // A device that can't pause runs on, writing silence, as it did.
            Stream::Device(stream) => {
                let _ = stream.pause();
            }
            #[cfg(feature = "counts")]
            Stream::StandIn(stream) => stream.set_going(false),
        }
    }
}

/// The underruns counted while playing, from the count kept as the stream
/// reports them: those since sound was let out are told when it stops
/// being, and those while it wasn't (silence, heard by nobody) dropped.
fn told(underruns: &AtomicU64, was: bool, now: bool) -> Option<u64> {
    let counted = if was != now { underruns.swap(0, Ordering::Relaxed) } else { 0 };
    (was && !now && counted > 0).then_some(counted)
}

/// Seconds of sound kept queued ahead of playing.
pub const AHEAD: f64 = 0.5;

/// The output, open; dropped, it stops.
pub struct Output {
    stream: Stream,
    rate: u32,
    queue: Arc<Mutex<VecDeque<i16>>>,
    played: Arc<Played>,
    running: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    underruns: Arc<AtomicU64>,
}

impl Output {
    /// Opens the default output, if there is one. Stereo if it takes
    /// stereo; else its channels, the two spread over them. Its sample
    /// format and rate are whatever it offers.
    pub fn open() -> Option<Output> {
        let queue: Arc<Mutex<VecDeque<i16>>> = Arc::default();
        let played = Arc::new(Played::default());
        let running = Arc::new(AtomicBool::new(false));
        let volume = Arc::new(AtomicU32::new(1f32.to_bits()));
        let underruns = Arc::new(AtomicU64::new(0));
        let shared = (queue.clone(), played.clone(), running.clone(), volume.clone());
        #[cfg(feature = "counts")]
        if stand_in::chosen() {
            let stream = Stream::StandIn(stand_in::Stream::start(shared));
            return Some(Output { stream, rate: stand_in::RATE, queue, played, running, volume, underruns });
        }
        let device = cpal::default_host().default_output_device()?;
        let config = pick(&device)?;
        let (rate, channels) = (config.sample_rate(), config.channels() as usize);
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let counted = underruns.clone();
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, channels, rate, shared, counted),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, channels, rate, shared, counted),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, channels, rate, shared, counted),
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, channels, rate, shared, counted),
            cpal::SampleFormat::F64 => build::<f64>(&device, &config, channels, rate, shared, counted),
            _ => return None,
        }?;
        stream.play().ok()?;
        Some(Output { stream: Stream::Device(stream), rate, queue, played, running, volume, underruns })
    }

    /// Starts the stream again after `pause`: it asks for samples again.
    pub fn resume(&self) {
        self.stream.play();
    }

    /// Stops the stream while nothing plays: no more asking for samples
    /// (wakeups) until `resume`. A device that can't pause runs on, silent.
    pub fn pause(&self) {
        self.stream.pause();
    }

    /// Samples a second, as the output takes them.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Stereo sample pairs played since the queue was last cleared.
    pub fn played(&self) -> Arc<Played> {
        self.played.clone()
    }

    /// The loudness it plays at, 0 to 1 (its count goes on at 0).
    pub fn volume(&self) -> Arc<AtomicU32> {
        self.volume.clone()
    }

    /// Whether sound is let out (and counted). Once it stops being, the
    /// underruns while it was are told, in one line.
    pub fn set_running(&self, running: bool) {
        let was = self.running.swap(running, Ordering::AcqRel);
        if let Some(n) = told(&self.underruns, was, running) {
            eprintln!("sound: {n} buffer underrun{} while playing", if n == 1 { "" } else { "s" });
        }
    }

    /// Queues interleaved stereo samples.
    pub fn push(&self, pcm: &[i16]) {
        self.queue.lock().unwrap().extend(pcm);
    }

    /// Seconds queued and not yet played.
    pub fn queued(&self) -> f64 {
        self.queue.lock().unwrap().len() as f64 / 2.0 / self.rate as f64
    }

    /// Drops what's queued and starts the count over (a seek).
    pub fn clear(&self) {
        let mut queue = self.queue.lock().unwrap();
        queue.clear();
        self.played.reset();
    }

    /// Starts the count over, keeping what's queued.
    pub fn recount(&self) {
        self.played.reset();
    }
}

/// A stand-in output for tests (`counts`): no device, it takes samples in
/// real time on a thread of its own while it's going, and keeps count of
/// how often it asked for them, as a device's stream would wake to.
#[cfg(feature = "counts")]
pub mod stand_in {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::Shared;

    /// Its samples a second, and how many pairs it asks for at a time.
    pub const RATE: u32 = 48_000;
    const CHUNK: usize = 480;
    /// How long a chunk plays.
    const CHUNK_TIME: Duration = Duration::from_micros(CHUNK as u64 * 1_000_000 / RATE as u64);

    /// When the stand-in asks for its next chunk: a chunk's time after the
    /// last one was due, however late its thread woke, as a device's clock
    /// runs on its own. Sleeping a chunk's time after each instead lost
    /// every late wake-up: where sleeps run late (macOS coalesces timers,
    /// more so for work in the background), the time ran at a third of
    /// the clock's pace, and a test waiting two seconds of playing gave up.
    struct Pace {
        due: Instant,
    }

    impl Pace {
        /// Due at once, from `now` (going, or going again after a pause).
        fn starting(now: Instant) -> Pace {
            Pace { due: now }
        }

        /// A chunk asked for at `now`: how long till the next is due (none,
        /// when the thread is behind: it catches up).
        fn asked(&mut self, now: Instant) -> Duration {
            self.due += CHUNK_TIME;
            self.due.saturating_duration_since(now)
        }
    }

    static CHOSEN: AtomicBool = AtomicBool::new(false);
    static GOING: AtomicBool = AtomicBool::new(false);
    static ASKED: AtomicU64 = AtomicU64::new(0);

    /// Outputs opened from now on are stand-ins.
    pub fn choose() {
        CHOSEN.store(true, Ordering::Release);
    }

    pub(super) fn chosen() -> bool {
        CHOSEN.load(Ordering::Acquire)
    }

    /// Whether the stand-in's stream is going (not paused).
    pub fn going() -> bool {
        GOING.load(Ordering::Acquire)
    }

    /// How many times it has asked for samples.
    pub fn asked() -> u64 {
        ASKED.load(Ordering::Acquire)
    }

    pub(super) struct Stream {
        state: Arc<(Mutex<(bool, bool)>, Condvar)>,
    }

    impl Stream {
        /// Started, going.
        pub(super) fn start(shared: Shared) -> Stream {
            let state = Arc::new((Mutex::new((true, false)), Condvar::new()));
            GOING.store(true, Ordering::Release);
            let run = state.clone();
            std::thread::spawn(move || {
                let mut out = vec![0f32; CHUNK * 2];
                let mut pace = Pace::starting(Instant::now());
                loop {
                    {
                        let (lock, wake) = &*run;
                        let mut s = lock.lock().unwrap();
                        if !s.0 && !s.1 {
                            while !s.0 && !s.1 {
                                s = wake.wait(s).unwrap();
                            }
                            // Paused meanwhile: what's due runs from now.
                            pace = Pace::starting(Instant::now());
                        }
                        if s.1 {
                            return;
                        }
                    }
                    ASKED.fetch_add(1, Ordering::AcqRel);
                    super::fill(&mut out, 2, RATE, &shared);
                    let wait = pace.asked(Instant::now());
                    if !wait.is_zero() {
                        std::thread::sleep(wait);
                    }
                }
            });
            Stream { state }
        }

        /// Sets it going or paused.
        pub(super) fn set_going(&self, going: bool) {
            let (lock, wake) = &*self.state;
            lock.lock().unwrap().0 = going;
            GOING.store(going, Ordering::Release);
            wake.notify_all();
        }
    }

    impl Drop for Stream {
        fn drop(&mut self) {
            let (lock, wake) = &*self.state;
            lock.lock().unwrap().1 = true;
            GOING.store(false, Ordering::Release);
            wake.notify_all();
        }
    }

    /// The stand-in takes its samples in real time, as a device would, so
    /// the time follows the clock (sound_idle.rs: "A stand-in output
    /// (`sound::stand_in`) takes the samples in real time, so the time
    /// follows it as it would a device's"): a wake-up late by however much
    /// is made up by the next chunks, which are due at once.
    #[test]
    fn the_stand_in_keeps_the_clocks_pace_when_its_wake_ups_run_late() {
        let begun = Instant::now();
        let mut pace = Pace::starting(begun);
        let mut now = begun;
        let mut chunks = 0u32;
        // A second of chunks, each sleep running 20 ms late.
        while now < begun + Duration::from_secs(1) {
            chunks += 1;
            let wait = pace.asked(now);
            if !wait.is_zero() {
                now += wait + Duration::from_millis(20);
            }
        }
        let played = CHUNK_TIME * chunks;
        let behind = (now - begun).saturating_sub(played);
        assert!(behind <= CHUNK_TIME + Duration::from_millis(20), "{played:?} played in {:?}", now - begun);
        // On time, a chunk's time apart.
        let mut pace = Pace::starting(begun);
        assert_eq!(pace.asked(begun), CHUNK_TIME);
        assert_eq!(pace.asked(begun + CHUNK_TIME), CHUNK_TIME);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underruns_are_told_once_per_stretch_of_play_and_only_while_playing() {
        let underruns = AtomicU64::new(0);
        // Paused (silence): counted, never told, and dropped once playing.
        underruns.fetch_add(4, Ordering::Relaxed);
        assert_eq!(told(&underruns, false, false), None);
        assert_eq!(told(&underruns, false, true), None);
        // Playing: told once, at the end of the stretch, then counted anew.
        underruns.fetch_add(2, Ordering::Relaxed);
        assert_eq!(told(&underruns, true, true), None);
        underruns.fetch_add(1, Ordering::Relaxed);
        assert_eq!(told(&underruns, true, false), Some(3));
        assert_eq!(told(&underruns, false, false), None);
        // A stretch without any: nothing told.
        assert_eq!(told(&underruns, false, true), None);
        assert_eq!(told(&underruns, true, false), None);
    }
}
