//! Everything VODscribe does that isn't UI: fetching VOD audio with yt-dlp,
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
pub mod thumbs;
pub mod tools;
pub mod transcript;

use std::path::PathBuf;

/// Layout of the app's data. The library and settings live in `root`; what can be
/// downloaded again (tools, models, the GPU pack, audio) lives in `local`, which is
/// the same directory except on Windows, where `root` roams and `local` doesn't.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub local: PathBuf,
}

impl Paths {
    /// Everything in one directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self { local: root.clone(), root }
    }

    pub fn split(root: impl Into<PathBuf>, local: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), local: local.into() }
    }

    pub fn db(&self) -> PathBuf {
        self.root.join("library.sqlite")
    }

    pub fn bin(&self) -> PathBuf {
        self.local.join("bin")
    }

    pub fn models(&self) -> PathBuf {
        self.local.join("models")
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn gpu_pack(&self) -> PathBuf {
        self.local.join("gpu")
    }

    /// Local copies of VOD thumbnails. They could be fetched again, but they belong
    /// with the library, so Clear downloads (`cache_dirs`) leaves them.
    pub fn thumbs(&self) -> PathBuf {
        self.local.join("thumbs")
    }

    /// Scratch space for in-flight downloads.
    pub fn work(&self) -> PathBuf {
        self.local.join("work")
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
        for dir in [self.root.clone(), self.local.clone(), self.bin(), self.models(), self.work(), self.thumbs()] {
            std::fs::create_dir_all(dir)?;
        }
        // The library is private to this user: other accounts can't list or read it.
        #[cfg(unix)]
        for dir in [&self.root, &self.local] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
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
