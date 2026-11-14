//! What tests stand in for the world with, so they do no slow I/O and
//! decide the order things happen in. Built for this crate's tests and, with
//! the `testkit` feature, for the app's (apps/desktop's dev-dependency);
//! never in a release.
//!
//! The library and files are real: SQLite with every migration, in a
//! temporary folder (`Library`, `TempDir`). What's outside the app is faked,
//! each behind one seam in the app's code:
//!
//! - programs (`tools`): ffmpeg (`Ffmpeg`), yt-dlp (`YtDlp`) and the
//!   transcription worker (`Worker`), run in this process by
//!   `vodscribe_core::tools::spawn`, `output` and `status`;
//! - the network (`net`): where a name resolves (`net::public_url`), and
//!   the answer to each request, by `net::get` and by the HTTP clients
//!   (`net::agent`); the sites' own APIs answered as they answer (`sites`);
//! - media files (`Media`): what the fake programs write and read, in place
//!   of video.
//!
//! README.md beside this file says how a test uses them.

pub mod ffmpeg;
pub mod library;
pub mod media;
pub mod net;
pub mod sites;
pub mod tools;
pub mod worker;
pub mod yt_dlp;

#[cfg(test)]
mod tests;

pub use ffmpeg::Ffmpeg;
pub use library::{Library, Released, at_version, back_to, newer, said};
pub use media::Media;
pub use net::{Net, Reply, Request};
pub use tools::Gate;
pub use worker::Worker;
pub use yt_dlp::YtDlp;

use std::path::{Path, PathBuf};

/// A fresh folder for a test, removed when it's dropped, even when the test
/// fails. Use it as the `Path` it derefs to.
pub struct TempDir(PathBuf);

impl TempDir {
    /// `vodscribe-<name>-…` in the system's temporary folder, unique to this
    /// process and call, so tests running at once don't share one.
    pub fn new(name: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vodscribe-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Waits up to five seconds for `done`, checking it every few milliseconds;
/// panics with `what` if it never is. For what a test can only see happen
/// (a queue's job ending); never a wait for time to pass.
pub fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !done() {
        assert!(std::time::Instant::now() < until, "never happened: {what}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
