//! Dragging files out of the app into other apps before they're made: the
//! system's drag begins at once, and each file is handed over once it's
//! there. Windows only (`windows`: OLE's drag, a virtual file beside the
//! file itself); Linux's lives in clips.rs, macOS's in `file_drag`, whose
//! `Ending` this shares.
//!
//! Nothing here knows the app: it's given items (`Item`: a file's name, where
//! its file is, as polled; a cancel hook), the window's raw handle and a PNG,
//! and it says how the drag ended (`Outcome`). The app (clips.rs) answers for
//! its clips through `Item` and decides what the page hears from `Outcome`.

use std::path::PathBuf;

pub(crate) use crate::file_drag::Ending;

mod windows;
pub(crate) use windows::run;

/// Where an item's file is: made, at its path; still being made; or not
/// going to be, and why.
pub(crate) enum State {
    Making,
    Ready(PathBuf),
    Failed(String),
}

/// One file in a drag out of the window.
pub(crate) trait Item: Send + Sync {
    /// The name its file lands under, with its extension, as it will be once
    /// it's made (a receiver that copies it, Explorer, names its copy so).
    fn name(&self) -> String;
    /// Where its file is now: polled, from any thread, while the drag waits.
    fn state(&self) -> State;
    /// The app it was let go on gave up on it (Explorer's Cancel) before it
    /// was handed over.
    fn cancel(&self) {}
}

/// How a drag out of the window ended, once it has: for Explorer, once its
/// copies are written.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) ending: Ending,
    /// The files went to the app it was let go on, as themselves or copied.
    pub(crate) delivered: bool,
    /// That app has the files themselves (not copies): they're its now.
    pub(crate) taken: bool,
    /// Why they never came, if one couldn't be made (or was canceled).
    pub(crate) failure: Option<String>,
    /// That app asked for the files themselves once let go on, was refused
    /// because one was still being made, and took nothing else: it takes
    /// finished files only.
    pub(crate) refused_unmade: bool,
    /// That app's name, as its program describes itself, where known.
    pub(crate) receiver: Option<String>,
}
