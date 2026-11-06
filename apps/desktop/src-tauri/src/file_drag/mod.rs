//! The system's drag of files out of a window, begun before the files are
//! made: each is handed over once it's there, and the app hears how the drag
//! ended. Kept apart from the app (no Tauri, no library types), so it can
//! stand alone: the app gives it a window's handle and one `Item` per file,
//! and hears `Event`s back.
//!
//! macOS only, so far (`macos`): Linux's drag is still in `clips`, and
//! Windows' is `drag_source`.

// (Elsewhere only `Ending` and `ReleasedEarly` are used, by Linux's and
// Windows' drags.)
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod placeholder;
#[cfg(target_os = "macos")]
pub(crate) use macos::start;

/// Where a file being dragged out is.
pub(crate) enum State {
    /// Being made, this far along (0 to 1) if that's known.
    Making(Option<f64>),
    /// Made, here.
    Ready(PathBuf),
    /// Not going to be, and why.
    Failed(String),
}

/// One file in a drag, as the app has it.
pub(crate) trait Item: Send + Sync {
    /// The name it goes by where it's dropped, known before it's made.
    fn file_name(&self) -> String;
    /// Its type, as the system names types (on macOS a UTI: "public.mpeg-4").
    fn file_type(&self) -> String;
    /// Where it is now. Asked on the main thread too: it mustn't wait long.
    fn state(&self) -> State;
    /// Puts the made file `from` at `to`, where the receiver asked for it,
    /// as a new file: never onto or through one there, which is refused
    /// (AlreadyExists). A copy, by default: on APFS a clone, at once.
    #[cfg(unix)]
    fn write(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        placeholder::copy_new(from, to)
    }
    /// The receiver called its copy off (Finder's cancel on the file).
    fn cancelled(&self) {}
    /// The receiver asked for the file while it's still being made, and
    /// waits for it (a promise to write).
    fn waited_for(&self) {}
}

/// Where a drag out of the window ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Ending {
    /// Called off (Escape). (macOS can't tell it from a drop on nothing.)
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    CalledOff,
    /// Dropped back on the app's own window.
    Back,
    /// Dropped where nothing took it: on nothing, or on an app that refused
    /// it (one whose ask for the files was refused, its making failed).
    Nowhere,
    /// Let go on another app, which took it or didn't: also one that refused
    /// the drop while a file was still being made (it may take the file
    /// once it's made).
    Elsewhere,
}

/// Why a drag wasn't begun: the button was already up when it was asked
/// for (the release may come in the moment between the page asking and the
/// drag's start). One begun then would never end, so it's refused, and the
/// app tells the page by this, not by the sentence, that no drag began.
#[derive(Debug)]
pub(crate) struct ReleasedEarly;

impl std::fmt::Display for ReleasedEarly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The button was released before the drag could begin.")
    }
}

impl std::error::Error for ReleasedEarly {}

/// Why a file never got where it was dropped.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Failure {
    /// The receiver called it off.
    Cancelled,
    /// It couldn't be made or written, and why.
    Failed(String),
}

/// What became of a drag, once it's over and every file asked for is
/// written or failed.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Ended {
    pub(crate) ending: Ending,
    /// The receiver has every file it asked for.
    pub(crate) given: bool,
    /// ...written where it asked (its own copies), not given the app's own
    /// files' addresses.
    pub(crate) written: bool,
    /// Why one didn't get there.
    pub(crate) failure: Option<Failure>,
    /// Every file was made as the drag ended, or the receiver never asked
    /// for one that wasn't (it took nothing for its own reasons).
    pub(crate) made: bool,
    /// The app it was let go on, by its name, where the system says.
    pub(crate) receiver: Option<String>,
}

/// What the app hears of a drag.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Event {
    /// Let go on another app: its files are being handed over.
    LetGo,
    /// Over (`Ended`), once, last.
    Ended(Ended),
}
