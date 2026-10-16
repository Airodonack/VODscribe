//! Sound out, through the computer's default output (cpal). Samples queued
//! here are played in order; how many have been played is the sound's own
//! clock, which the player's time follows while it plays (`clock`), so the
//! picture keeps to the sound rather than the other way round. With no
//! output to be had (a computer without one, a headless test), there's no
//! sound and the player keeps time by the computer's clock.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Seconds of sound kept queued ahead of playing.
pub const AHEAD: f64 = 0.5;

/// The output, open; dropped, it stops.
pub struct Output {
    _stream: cpal::Stream,
    rate: u32,
    queue: Arc<Mutex<VecDeque<i16>>>,
    played: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
}

impl Output {
    /// Opens the default output for stereo sound, if there is one that takes it.
    pub fn open() -> Option<Output> {
        let device = cpal::default_host().default_output_device()?;
        let config = device.default_output_config().ok()?;
        if config.channels() != 2 || config.sample_format() != cpal::SampleFormat::F32 {
            return None;
        }
        let rate = config.sample_rate();
        let config: cpal::StreamConfig = config.into();
        let queue: Arc<Mutex<VecDeque<i16>>> = Arc::default();
        let played = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(false));
        let (taken, counted, on) = (queue.clone(), played.clone(), running.clone());
        let stream = device
            .build_output_stream(
                config,
                move |out: &mut [f32], _| {
                    // Silence unless playing, or when the queue runs dry
                    // (decoding behind): then the time stands too.
                    let mut queue = if on.load(Ordering::Acquire) { taken.try_lock().ok() } else { None };
                    let mut got = 0;
                    for sample in out.iter_mut() {
                        *sample = match queue.as_mut().and_then(|q| q.pop_front()) {
                            Some(s) => {
                                got += 1;
                                s as f32 / i16::MAX as f32
                            }
                            None => 0.0,
                        };
                    }
                    counted.fetch_add(got / 2, Ordering::AcqRel);
                },
                |e| eprintln!("sound: {e}"),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(Output { _stream: stream, rate, queue, played, running })
    }

    /// Samples a second, as the output takes them.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Stereo sample pairs played since the queue was last cleared.
    pub fn played(&self) -> Arc<AtomicU64> {
        self.played.clone()
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
