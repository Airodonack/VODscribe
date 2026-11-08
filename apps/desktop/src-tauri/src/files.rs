//! Deleting files the app made (clips' and renders'), where Windows won't
//! while something has them open: those go later, and if the app quits
//! first, at its next start (`keep_leftovers_in`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// A file the app couldn't delete when it was to go, as it was then: one
/// changed or replaced since is another file, not this one, and stays.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Leftover {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

impl Leftover {
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Leftover { path: path.to_path_buf(), size: meta.len(), modified: meta.modified().ok() })
    }
}

/// Files that couldn't be deleted when they were to go: on Windows a file
/// can't be while something has it open (a render or the player reading a
/// clip's file, or a paste's copy of a cut clip's). Tried again as renders
/// and cuts end, and as the app quits; kept in `store` (the library's
/// folder) as the list changes, so what's still left at quit is tried again
/// at the next start. Only what the app itself was deleting is listed, never
/// a file it inferred to be unused.
#[derive(Default)]
pub(crate) struct Leftovers {
    store: Option<PathBuf>,
    list: Vec<Leftover>,
}

impl Leftovers {
    const fn new() -> Self {
        Leftovers { store: None, list: Vec::new() }
    }

    /// Keeps the list in `store` from now: what it held (the files left when
    /// the app last quit) is added and tried again.
    pub(crate) fn keep_in(&mut self, store: PathBuf) {
        let kept: Vec<Leftover> =
            std::fs::read(&store).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        for left in kept {
            if !self.list.contains(&left) {
                self.list.push(left);
            }
        }
        self.store = Some(store);
        self.retry();
    }

    /// `path`, which couldn't be deleted, to be tried again.
    pub(crate) fn add(&mut self, path: &Path) {
        if let Some(left) = Leftover::of(path) {
            self.list.retain(|l| l.path != left.path);
            self.list.push(left);
            self.save();
        }
    }

    /// `path` is being written anew: it isn't left to delete.
    pub(crate) fn writing(&mut self, path: &Path) {
        let before = self.list.len();
        self.list.retain(|l| l.path != path);
        if self.list.len() != before {
            self.save();
        }
    }

    /// Deletes each file listed that is still the one listed; keeps those
    /// that still can't be.
    pub(crate) fn retry(&mut self) {
        if self.list.is_empty() {
            return;
        }
        self.list.retain(|left| {
            // Gone, or not the file it was (written anew since): nothing to do.
            if Leftover::of(&left.path).as_ref() != Some(left) {
                return false;
            }
            std::fs::remove_file(&left.path).is_err() && left.path.exists()
        });
        self.save();
    }

    /// Writes the list to its store (removed once it's empty), under a name
    /// of its own first so a write cut short leaves the last one whole.
    fn save(&self) {
        let Some(store) = &self.store else { return };
        if self.list.is_empty() {
            let _ = std::fs::remove_file(store);
            return;
        }
        let part = store.with_extension("json.part");
        let written = serde_json::to_vec(&self.list)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(&part, bytes))
            .and_then(|()| std::fs::rename(&part, store));
        if let Err(e) = written {
            eprintln!("keeping the files left to delete in {}: {e}", store.display());
        }
    }

    /// Its callers, specs with a read-only folder, are unix only.
    #[cfg(all(test, unix))]
    pub(crate) fn listed(&self) -> Vec<PathBuf> {
        self.list.iter().map(|l| l.path.clone()).collect()
    }
}

/// The files the app has left to delete later (`delete_file`), in every
/// library's list, for specs with a read-only folder.
#[cfg(all(test, unix))]
pub(crate) fn left_to_delete() -> Vec<PathBuf> {
    let mut all = Vec::new();
    with_leftovers(None, |l| all.extend(l.listed()));
    all
}

/// The app's leftovers, one list for the library it has open.
#[cfg(not(test))]
static LEFTOVERS: Mutex<Leftovers> = Mutex::new(Leftovers::new());

/// Runs `f` on the leftovers list, the one of the library holding `path`
/// where one is named (`with_leftovers` in the tests, where it matters).
#[cfg(not(test))]
fn with_leftovers<T>(_path: Option<&Path>, f: impl FnOnce(&mut Leftovers) -> T) -> T {
    f(&mut LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner()))
}

/// In the tests several apps run at once, each on a library of its own
/// (`testkit::TestApp`): a list per library, by its store, so one's
/// leftovers go to its own store, and a list for files in none.
#[cfg(test)]
static LEFTOVERS: Mutex<Vec<Leftovers>> = Mutex::new(Vec::new());

/// Runs `f` on the list of the library holding `path` (its store's
/// folder), or the list for files in none; with no `path`, on each.
#[cfg(test)]
fn with_leftovers<T>(path: Option<&Path>, mut f: impl FnMut(&mut Leftovers) -> T) -> T {
    let mut lists = LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner());
    if lists.is_empty() {
        lists.push(Leftovers::new());
    }
    let Some(path) = path else {
        let mut last = None;
        for list in lists.iter_mut() {
            last = Some(f(list));
        }
        return last.unwrap();
    };
    let held = |l: &Leftovers| l.store.as_ref().and_then(|s| s.parent()).is_some_and(|dir| path.starts_with(dir));
    match lists.iter().position(held) {
        Some(i) => f(&mut lists[i]),
        None => f(&mut lists[0]),
    }
}

/// Deletes `path` now, or once nothing has it open (`delete_leftovers`).
pub(crate) fn delete_file(path: impl AsRef<Path>) {
    let path = path.as_ref();
    if std::fs::remove_file(path).is_err() && path.exists() {
        with_leftovers(Some(path), |l| l.add(path));
    }
}

/// `path` is about to be written: if an earlier file there was left to delete
/// later (and has gone since), the new one isn't.
pub(crate) fn writing(path: &Path) {
    with_leftovers(Some(path), |l| l.writing(path));
}

/// Deletes what couldn't be deleted before, if it can be now.
pub(crate) fn delete_leftovers() {
    with_leftovers(None, |l| l.retry());
}

/// At start: the leftovers are kept in `store` from now, and those the app
/// last quit with are tried again (`Leftovers::keep_in`).
#[cfg(not(test))]
pub(crate) fn keep_leftovers_in(store: PathBuf) {
    with_leftovers(None, move |l| l.keep_in(store));
}

/// At a test app's start (`keep_leftovers_in`): its library's list begins
/// anew from its store, as the app's does at a start after a quit.
#[cfg(test)]
pub(crate) fn keep_leftovers_in(store: PathBuf) {
    let mut lists = LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner());
    if lists.is_empty() {
        lists.push(Leftovers::new());
    }
    lists.retain(|l| l.store.as_ref() != Some(&store));
    let mut list = Leftovers::new();
    list.keep_in(store);
    lists.push(list);
}

/// A test app's library is gone: its list goes with it.
#[cfg(test)]
pub(crate) fn forget_leftovers(store: &Path) {
    LEFTOVERS.lock().unwrap_or_else(|e| e.into_inner()).retain(|l| l.store.as_deref() != Some(store));
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
