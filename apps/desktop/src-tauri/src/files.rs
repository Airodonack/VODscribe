//! Deleting files the app made (clips' and renders'), where Windows won't
//! while something has them open: those go later.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Files that couldn't be deleted when they were to go: on Windows a file
/// can't be while something has it open (a render or the player reading a
/// clip's file). Tried again as renders and cuts end, and as the app quits.
static LEFTOVERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Deletes `path` now, or once nothing has it open (`delete_leftovers`).
pub(crate) fn delete_file(path: impl AsRef<Path>) {
    let path = path.as_ref();
    if std::fs::remove_file(path).is_err() && path.exists() {
        LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner()).push(path.to_path_buf());
    }
}

/// `path` is about to be written: if an earlier file there was left to delete
/// later (and has gone since), the new one isn't.
pub(crate) fn writing(path: &Path) {
    LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| p != path);
}

/// Deletes what couldn't be deleted before, if it can be now.
pub(crate) fn delete_leftovers() {
    LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| std::fs::remove_file(p).is_err() && p.exists());
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// On Windows a file another program has open can't be deleted: it goes
    /// once that program lets go.
    #[test]
    fn a_file_in_use_goes_once_let_go() {
        use std::os::windows::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!("vodscribe-in-use-{}.mp4", std::process::id()));
        std::fs::write(&path, b"video").unwrap();
        // Shared for reading only, as ffmpeg opens what it reads.
        let reader = std::fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        delete_file(&path);
        assert!(path.exists(), "still open");
        delete_leftovers();
        assert!(path.exists(), "still open");
        drop(reader);
        delete_leftovers();
        assert!(!path.exists(), "gone once let go");
    }
}
