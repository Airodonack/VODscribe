//! Sound out, through the computer's default output (cpal). Samples queued
//! here are played in order; how many have been played is the sound's own
//! clock, which the player's time follows while it plays (`clock`), so the
//! picture keeps to the sound rather than the other way round. With no
//! output to be had (a computer without one, a headless test), there's no
//! sound and the player keeps time by the computer's clock.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
    (taken, counted, on, gain): Shared,
) -> Option<cpal::Stream> {
    device
        .build_output_stream(
            *config,
            move |out: &mut [T], _| {
                // Silence unless playing, or when the queue runs dry
                // (decoding behind): then the time stands too. The queue's
                // other users hold it only to copy a chunk in or read its
                // length, so waiting for it is microseconds; taking silence
                // instead (try_lock) dropped a whole chunk, a click.
                let mut queue = if on.load(Ordering::Acquire) {
                    Some(taken.lock().unwrap_or_else(|e| e.into_inner()))
                } else {
                    None
                };
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
            },
            |e| eprintln!("sound: {e}"),
            None,
        )
        .ok()
}

/// Seconds of sound kept queued ahead of playing.
pub const AHEAD: f64 = 0.5;

/// The output, open; dropped, it stops.
pub struct Output {
    _stream: cpal::Stream,
    rate: u32,
    queue: Arc<Mutex<VecDeque<i16>>>,
    played: Arc<Played>,
    running: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
}

impl Output {
    /// Opens the default output, if there is one. Stereo if it takes
    /// stereo; else its channels, the two spread over them. Its sample
    /// format and rate are whatever it offers.
    pub fn open() -> Option<Output> {
        let device = cpal::default_host().default_output_device()?;
        let config = pick(&device)?;
        let (rate, channels) = (config.sample_rate(), config.channels() as usize);
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let queue: Arc<Mutex<VecDeque<i16>>> = Arc::default();
        let played = Arc::new(Played::default());
        let running = Arc::new(AtomicBool::new(false));
        let volume = Arc::new(AtomicU32::new(1f32.to_bits()));
        let shared = (queue.clone(), played.clone(), running.clone(), volume.clone());
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, channels, rate, shared),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, channels, rate, shared),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, channels, rate, shared),
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, channels, rate, shared),
            cpal::SampleFormat::F64 => build::<f64>(&device, &config, channels, rate, shared),
            _ => return None,
        }?;
        stream.play().ok()?;
        Some(Output { _stream: stream, rate, queue, played, running, volume })
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

    /// Whether sound is let out (and counted).
    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Release);
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
