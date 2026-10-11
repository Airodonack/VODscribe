//! Everything vodscribe does that isn't UI: fetching VOD audio with yt-dlp,
//! decoding it with ffmpeg, transcribing it with Parakeet (via sherpa-onnx)
//! and indexing the transcript in SQLite for phrase search.

#[cfg(feature = "asr")]
pub mod asr;
pub mod audio;
pub mod db;
pub mod fetch;
pub mod gpu;
pub mod kick;
pub mod model;
pub mod net;
pub mod pipeline;
pub mod settings;
pub mod tools;
pub mod transcript;

use std::path::PathBuf;

/// Layout of the app's data directory.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn db(&self) -> PathBuf {
        self.root.join("library.sqlite")
    }

    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn gpu_pack(&self) -> PathBuf {
        self.root.join("gpu")
    }

    /// Scratch space for in-flight downloads.
    pub fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    pub fn model_dir(&self) -> PathBuf {
        model::model_dir(&self.models(), &model::PARAKEET_V3)
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.root.clone(), self.bin(), self.models(), self.work()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
