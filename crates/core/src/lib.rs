//! Everything VODscribe does that isn't UI: fetching VOD audio with yt-dlp,
//! decoding it with ffmpeg, transcribing it with Parakeet (via sherpa-onnx)
//! and indexing the transcript in SQLite for phrase search.

#[cfg(feature = "asr")]
pub mod asr;
pub mod audio;
pub mod chat;
pub mod db;
pub mod deps;
pub mod emotes;
pub mod fetch;
pub mod gpu;
pub mod kick;
pub mod layers;
pub mod local;
pub mod model;
pub mod net;
pub mod pipeline;
pub mod playable;
pub mod settings;
pub mod thumbs;
pub mod tools;
pub mod transcript;
pub mod twitch;
pub mod word_emotes;

use std::path::PathBuf;

/// Where a download is, as the app shows it.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Progress<'a> {
    /// "Downloading <name>", then "<name> matches its SHA-256".
    pub step: &'a str,
    /// Bytes so far across the whole install, for a bar that only moves
    /// forward, and the install's size if known.
    pub done: u64,
    pub total: Option<u64>,
    /// Bytes of the file under way, and its size if known.
    pub file_done: u64,
    pub file_size: Option<u64>,
    /// Which of the install's files this is, from 1, out of how many. Shown as
    /// "(2 of 5)", so a run of small downloads doesn't look like one repeating.
    pub part: usize,
    pub parts: usize,
}

impl<'a> Progress<'a> {
    /// A step of a single-file install with no bytes to count yet, such as
    /// checking a signature.
    pub fn step(step: &'a str) -> Self {
        Progress { step, done: 0, total: None, file_done: 0, file_size: None, part: 1, parts: 1 }
    }

    /// A file of `size` bytes (if known) with `n` of them done, as the whole of
    /// a single-file install. Place it in a bigger one with `within`.
    pub fn file(step: &'a str, n: u64, size: Option<u64>) -> Self {
        Progress { step, done: n, total: size, file_done: n, file_size: size, part: 1, parts: 1 }
    }

    /// This file as part `part` of `parts` in an install of `total` bytes,
    /// after `before` bytes of the files ahead of it.
    pub fn within(self, before: u64, total: Option<u64>, part: usize, parts: usize) -> Self {
        Progress { done: before + self.file_done, total, part, parts, ..self }
    }
}

/// The error a job ends with when it was told to stop (pause, cancel, delete).
/// Check for it with `is_canceled` rather than by its message.
#[derive(Debug)]
pub struct Canceled;

impl std::fmt::Display for Canceled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("canceled")
    }
}

impl std::error::Error for Canceled {}

/// Whether `e` is, or was caused by, `Canceled`, including inside an I/O error.
pub fn is_canceled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.is::<Canceled>()
            || c.downcast_ref::<std::io::Error>()
                .and_then(|io| io.get_ref())
                .is_some_and(|inner| inner.is::<Canceled>())
    })
}

/// Layout of the app's data. The library and settings live in `root`; what can be
/// downloaded again (tools, models, the GPU pack, audio) lives in `local`, which is
/// the same directory except on Windows, where `root` roams and `local` doesn't.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub local: PathBuf,
}

/// Bytes in `path`: a file's, or everything under a folder (not following links).
fn size_on_disk(path: &std::path::Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).map_or(0, |entries| entries.flatten().map(|e| size_on_disk(&e.path())).sum())
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

    /// What this computer's webview plays (`playable::Playable`).
    pub fn playable(&self) -> PathBuf {
        self.root.join("playable.json")
    }

    /// Timeline layers (see `layers`).
    pub fn layers(&self) -> PathBuf {
        self.root.join("layers.json")
    }

    pub fn gpu_pack(&self) -> PathBuf {
        self.local.join("gpu")
    }

    /// Local copies of VOD thumbnails. They could be fetched again, but they belong
    /// with the library, so Clear downloads (`cache_dirs`) leaves them.
    pub fn thumbs(&self) -> PathBuf {
        self.local.join("thumbs")
    }

    /// Chat emote images (see `emotes`).
    pub fn emotes(&self) -> PathBuf {
        self.local.join("emotes")
    }

    /// Scratch space for in-flight downloads.
    pub fn work(&self) -> PathBuf {
        self.local.join("work")
    }

    /// The videos kept from sites other than YouTube, Twitch and Kick, to play
    /// here (`pipeline::keeps_video`), as `<vod id>.<ext>`. Not a cache: a VOD's
    /// stays until it's deleted, or removed from the VOD.
    pub fn media(&self) -> PathBuf {
        pipeline::media_dir(&self.work())
    }

    pub fn model_dir(&self) -> PathBuf {
        model::model_dir(&self.models(), &model::CPU_MODEL)
    }

    pub fn gpu_model_dir(&self) -> PathBuf {
        model::model_dir(&self.models(), &model::GPU_MODEL)
    }

    /// Everything that can be downloaded again: models, tools, the GPU pack,
    /// emote images and in-flight downloads. The library and settings are not cache.
    pub fn cache_dirs(&self) -> [PathBuf; 6] {
        let gpu = self.gpu_pack();
        [self.models(), self.bin(), gpu::staging_dir(&gpu), gpu, self.emotes(), self.work()]
    }

    /// Bytes used by `cache_dirs`.
    pub fn cache_size(&self) -> u64 {
        self.cache_dirs().iter().map(|d| size_on_disk(d)).sum()
    }

    /// Bytes used by the kept videos (`media`).
    pub fn media_size(&self) -> u64 {
        size_on_disk(&self.media())
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

/// A fresh folder for a test, removed when it's dropped, even when the test
/// fails. Use it as the `Path` it derefs to.
#[cfg(test)]
pub(crate) struct TempDir(PathBuf);

#[cfg(test)]
impl TempDir {
    /// `vodscribe-<name>-…` in the system's temporary folder, unique to this
    /// process and call, so tests running at once don't share one.
    pub(crate) fn new(name: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vodscribe-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

#[cfg(test)]
impl std::ops::Deref for TempDir {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(test)]
impl AsRef<std::path::Path> for TempDir {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_the_cache_keeps_the_library_and_settings() {
        let dir = TempDir::new("cache");
        let paths = Paths::new(dir.join("data"));
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
    }

    #[test]
    fn a_file_placed_within_an_install() {
        let p = Progress::file("Downloading b", 30, Some(100)).within(250, Some(400), 2, 3);
        assert_eq!((p.done, p.total, p.file_done, p.file_size, p.part, p.parts), (280, Some(400), 30, Some(100), 2, 3));
        let alone = Progress::file("Downloading yt-dlp", 7, None);
        assert_eq!((alone.done, alone.total, alone.part, alone.parts), (7, None, 1, 1));
    }
}
