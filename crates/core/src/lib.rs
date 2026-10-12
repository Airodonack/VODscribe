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
        model::model_dir(&self.models(), &model::CPU_MODEL)
    }

    pub fn gpu_model_dir(&self) -> PathBuf {
        model::model_dir(&self.models(), &model::GPU_MODEL)
    }

    /// Everything that can be downloaded again: models, tools, the GPU pack and
    /// in-flight downloads. The library and settings are not cache.
    pub fn cache_dirs(&self) -> [PathBuf; 5] {
        let gpu = self.gpu_pack();
        [self.models(), self.bin(), gpu::staging_dir(&gpu), gpu, self.work()]
    }

    /// Bytes used by `cache_dirs`.
    pub fn cache_size(&self) -> u64 {
        fn size(path: &std::path::Path) -> u64 {
            let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
            if !meta.is_dir() {
                return meta.len();
            }
            std::fs::read_dir(path).map_or(0, |entries| entries.flatten().map(|e| size(&e.path())).sum())
        }
        self.cache_dirs().iter().map(|d| size(d)).sum()
    }

    /// Deletes `cache_dirs`. The ones the app always needs are recreated empty.
    pub fn clear_cache(&self) -> std::io::Result<()> {
        for dir in self.cache_dirs() {
            match std::fs::remove_dir_all(&dir) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        self.ensure()
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.root.clone(), self.bin(), self.models(), self.work()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_the_cache_keeps_the_library_and_settings() {
        let paths = Paths::new(std::env::temp_dir().join(format!("vodscribe-cache-{}", std::process::id())));
        paths.ensure().unwrap();
        for dir in paths.cache_dirs() {
            std::fs::create_dir_all(dir.join("nested")).unwrap();
            std::fs::write(dir.join("nested/file"), b"12345").unwrap();
        }
        std::fs::write(paths.db(), b"library").unwrap();
        std::fs::write(paths.settings(), b"{}").unwrap();
        assert_eq!(paths.cache_size(), 5 * paths.cache_dirs().len() as u64);

        paths.clear_cache().unwrap();
        assert_eq!(paths.cache_size(), 0);
        let empty = |d: &PathBuf| std::fs::read_dir(d).map_or(true, |mut e| e.next().is_none());
        assert!(paths.cache_dirs().iter().all(empty), "cache dirs are gone or empty");
        assert_eq!(std::fs::read(paths.db()).unwrap(), b"library");
        assert_eq!(std::fs::read(paths.settings()).unwrap(), b"{}");
        std::fs::remove_dir_all(&paths.root).unwrap();
    }
}
