use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, anyhow};
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use crate::audio::{Chunk, SAMPLE_RATE};
use crate::transcript::{Word, tokens_to_words};

pub struct Transcriber {
    recognizer: OfflineRecognizer,
    workers: usize,
}

/// How to spread transcription over the hardware. ONNX Runtime stops scaling at
/// around 8 threads per decode, so beyond that it is faster to decode several
/// chunks at once with a few threads each (about 1.5x on a 32-core machine).
#[derive(Clone, Copy, Debug)]
pub struct Parallelism {
    /// Chunks decoded concurrently.
    pub workers: usize,
    /// ONNX threads per decode.
    pub threads: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Cpu,
    Cuda,
}

impl Parallelism {
    /// Uses all cores but two, leaving room for the UI and the download.
    pub fn auto() -> Self {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        let budget = cores.saturating_sub(2).max(1);
        if budget < 8 {
            Self { workers: 1, threads: budget }
        } else {
            Self { workers: (budget / 4).min(6), threads: 4 }
        }
    }
}

impl Transcriber {
    pub fn load(model_dir: &Path, provider: Provider, parallelism: Parallelism) -> Result<Self> {
        let file = |name: &str| Some(model_dir.join(name).to_string_lossy().into_owned());
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: file("encoder.int8.onnx"),
            decoder: file("decoder.int8.onnx"),
            joiner: file("joiner.int8.onnx"),
        };
        config.model_config.tokens = file("tokens.txt");
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.num_threads = parallelism.threads as i32;
        config.model_config.provider = Some(match provider {
            Provider::Cpu => "cpu".into(),
            Provider::Cuda => "cuda".into(),
        });
        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| anyhow!("failed to load model from {}", model_dir.display()))?;
        Ok(Self { recognizer, workers: parallelism.workers })
    }

    /// How many chunks to pass to `transcribe` at once to keep every worker busy.
    pub fn batch_size(&self) -> usize {
        self.workers * 3
    }

    /// Transcribes chunks concurrently, returning words with absolute
    /// timestamps in chunk order.
    pub fn transcribe(&self, chunks: &[Chunk]) -> Vec<Word> {
        let next = AtomicUsize::new(0);
        let mut done: Vec<(usize, Vec<Word>)> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..self.workers.min(chunks.len()))
                .map(|_| {
                    scope.spawn(|| {
                        let mut out = Vec::new();
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(chunk) = chunks.get(i) else { return out };
                            out.push((i, self.transcribe_one(chunk)));
                        }
                    })
                })
                .collect();
            workers.into_iter().flat_map(|w| w.join().unwrap()).collect()
        });
        done.sort_by_key(|(i, _)| *i);
        done.into_iter().flat_map(|(_, words)| words).collect()
    }

    fn transcribe_one(&self, chunk: &Chunk) -> Vec<Word> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, &chunk.samples);
        self.recognizer.decode(&stream);
        let Some(r) = stream.get_result() else { return Vec::new() };
        let Some(starts) = r.timestamps else { return Vec::new() };
        tokens_to_words(&r.tokens, &starts, r.durations.as_deref(), chunk.start)
    }
}
