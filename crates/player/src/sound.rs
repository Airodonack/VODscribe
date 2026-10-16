//! Sound out, through the computer's default output (cpal). Samples queued
//! here are played in order; how many have been played is the sound's own
//! clock, which the player's time follows while it plays (`clock`), so the
//! picture keeps to the sound rather than the other way round. With no
//! output to be had (a computer without one, a headless test), there's no
//! sound and the player keeps time by the computer's clock.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

/// What the output stream shares with the player: the queue, the count of
/// stereo sample pairs played, whether it's running, and the loudness.
type Shared = (Arc<Mutex<VecDeque<i16>>>, Arc<AtomicU64>, Arc<AtomicBool>, Arc<AtomicU32>);

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
    (taken, counted, on, gain): Shared,
) -> Option<cpal::Stream> {
    device
        .build_output_stream(
            config.clone(),
            move |out: &mut [T], _| {
                // Silence unless playing, or when the queue runs dry
                // (decoding behind): then the time stands too.
                let mut queue = if on.load(Ordering::Acquire) { taken.try_lock().ok() } else { None };
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
                counted.fetch_add(got, Ordering::AcqRel);
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
    played: Arc<AtomicU64>,
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
        let played = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(false));
        let volume = Arc::new(AtomicU32::new(1f32.to_bits()));
        let shared = (queue.clone(), played.clone(), running.clone(), volume.clone());
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, channels, shared),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, channels, shared),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, channels, shared),
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, channels, shared),
            cpal::SampleFormat::F64 => build::<f64>(&device, &config, channels, shared),
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
    pub fn played(&self) -> Arc<AtomicU64> {
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
        self.played.store(0, Ordering::Release);
    }

    /// Starts the count over, keeping what's queued.
    pub fn recount(&self) {
        self.played.store(0, Ordering::Release);
    }
}
