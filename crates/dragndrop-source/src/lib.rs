//! Dragging files out of your window before they exist: the drag begins at
//! once, and each file is made on demand and handed over once it's there.
//! File managers wait for it (Explorer and Finder show their own copy
//! progress); an app that takes only files already made is told no, and you
//! hear that it was.
//!
//! Data only. You start your own drag (`DoDragDrop`, `beginDraggingSession`,
//! `gtk_drag_begin`) and handle its events; this crate never owns an event
//! loop or starts a drag session. It builds what the drag carries, answers
//! the receivers' asks for it, and tells you how the drag went. It has no
//! async runtime: each file's making is yours, polled through [`Item`].
//!
//! # On each system
//!
//! - **Windows** ([`windows`]): [`windows::Drag`] gives the `IDataObject`
//!   and `IDropSource` for `DoDragDrop`. Each file is offered as a virtual
//!   file (FileGroupDescriptorW and FileContents, with
//!   IDataObjectAsyncCapability: Explorer copies it on a thread of its own
//!   once it's made, showing its copy dialog meanwhile) and as itself
//!   (CF_HDROP, given only once every file is made).
//! - **macOS** ([`macos`]): [`macos::Drag`] gives the `NSDraggingItem`s for
//!   `beginDraggingSession`: per file a file promise (`NSFilePromiseProvider`,
//!   written where the receiver says once made, with an empty file there
//!   meanwhile and Finder's progress and cancel on it) beside its address,
//!   given at once if made. Your `NSDraggingSource` tells it how the session
//!   ended.
//! - **Linux** ([`linux`], GTK 3): [`linux::Drag`] gives the target list for
//!   `gtk_drag_begin` and fills the selection in your `drag-data-get`:
//!   text/uri-list, answered once the files are made (the hold: a main loop
//!   run inside the ask), and KDE's Ark extraction, which Dolphin takes, the
//!   files copied into its folder as they're made.
//!
//! On every system you give the same things: one [`Item`] per file (its
//! name, its [`State`], told when it's waited for or called off), and a
//! callback that hears [`Event`]s: let go on another app, then how it ended
//! ([`Outcome`]).

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg(target_os = "linux")]
mod ark;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(unix)]
mod placeholder;
#[cfg(windows)]
pub mod windows;

/// Where a file being dragged out is.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Being made, this far along (0 to 1) if that's known (macOS shows it).
    Making(Option<f64>),
    /// Made, here.
    Ready(PathBuf),
    /// Not going to be, and why.
    Failed(String),
}

/// One file in a drag, yours: asked after from any thread while the drag
/// lasts.
pub trait Item: Send + Sync {
    /// The name it goes by where it's dropped, with its extension, known
    /// before it's made (a receiver names its copy so).
    fn name(&self) -> String;
    /// Where it is now. Asked on the UI thread too: it mustn't wait long.
    fn state(&self) -> State;
    /// Its type as macOS names types, a UTI ("public.mpeg-4"); "public.data"
    /// unless you say. Read on macOS only.
    fn file_type(&self) -> String {
        "public.data".into()
    }
    /// Puts the made file `from` at `to`, where a macOS receiver asked for
    /// it, as a new file: never onto or through one there, which is refused
    /// (AlreadyExists). A copy, by default: on APFS a clone, at once.
    #[cfg(unix)]
    fn write(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        placeholder::copy_new(from, to)
    }
    /// The receiver called its copy off before it was handed over
    /// (Explorer's Cancel, Finder's cancel on the file).
    fn cancelled(&self) {}
    /// The receiver asked for the file while it's still being made, and
    /// waits for it. May be called more than once.
    fn waited_for(&self) {}
}

/// Where a drag out of the window ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    /// Called off (Escape). macOS can't tell it from a drop on nothing.
    CalledOff,
    /// Dropped back on your own window.
    Back,
    /// Dropped where nothing took it: on nothing, or on an app that refused
    /// it.
    Nowhere,
    /// Let go on another app, which took it or didn't: also one that refused
    /// the drop while a file was still being made (it may take the file once
    /// it's made).
    Elsewhere,
}

/// Why a file never got where it was dropped.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Failure {
    /// The receiver called it off.
    Cancelled,
    /// It couldn't be made or written, and why (an item's
    /// [`State::Failed`], say).
    Failed(String),
}

/// What became of a drag, once it's over and every file asked for is
/// written or failed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub ending: Ending,
    /// The receiver has every file it asked for.
    pub delivered: bool,
    /// ...as copies written for it (a promise, a virtual file, a folder it
    /// named), not your files' own paths: those are still yours.
    pub copied: bool,
    /// Why one didn't get there.
    pub failure: Option<Failure>,
    /// Every file was made as the drag ended, or the receiver never asked
    /// for one that wasn't. False when it took nothing because one wasn't
    /// made yet: it takes finished files only.
    pub made: bool,
    /// The app it was let go on, by its name, where the system says.
    pub receiver: Option<String>,
}

/// What you hear of a drag.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Event {
    /// Let go on another app: its files are being handed over. Heard on the
    /// UI thread.
    LetGo,
    /// Over, once, last. Never heard on the UI thread, so you may wait for
    /// a lock there.
    Ended(Outcome),
}

/// Why a drag wasn't begun: the button was already up when it was asked
/// for (the release may come in the moment between a press and the drag's
/// start). A drag begun then would never end, so don't begin one; each
/// system's `button_held` says. Tell this by its type, not its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleasedEarly;

impl std::fmt::Display for ReleasedEarly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The button was released before the drag could begin.")
    }
}

impl std::error::Error for ReleasedEarly {}

static TRACE: OnceLock<fn(&str)> = OnceLock::new();

/// Has `to` hear a line about each step of every drag (what was asked for,
/// when, what was answered), to see why a receiver did what it did. The
/// first one set stays.
pub fn trace_with(to: fn(&str)) {
    let _ = TRACE.set(to);
}

/// A line for the trace, if one is set.
#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
pub(crate) fn trace(what: &str) {
    if let Some(to) = TRACE.get() {
        to(what);
    }
}

/// The paths of `states` if all are made; else, if one won't be, why; else
/// (some still being made) None.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub(crate) fn settled(states: impl IntoIterator<Item = State>) -> Option<Result<Vec<PathBuf>, String>> {
    let mut paths = Vec::new();
    let mut making = false;
    for state in states {
        match state {
            State::Ready(path) => paths.push(path),
            State::Making(_) => making = true,
            State::Failed(why) => return Some(Err(why)),
        }
    }
    (!making).then_some(Ok(paths))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drag's answer waits while any of its files is still being made,
    /// and is none once one can't be: all or nothing, as one drop.
    #[test]
    fn a_drag_is_answered_once_all_its_files_are_made() {
        let ready = |p: &str| State::Ready(PathBuf::from(p));
        assert!(settled(vec![ready("/a"), State::Making(None)]).is_none());
        assert_eq!(settled(vec![ready("/a"), ready("/b")]), Some(Ok(vec![PathBuf::from("/a"), PathBuf::from("/b")])));
        assert_eq!(settled(vec![State::Making(Some(0.5)), State::Failed("no".into())]), Some(Err("no".into())));
        assert_eq!(settled(vec![]), Some(Ok(vec![])));
    }

    /// An early release is told by its type.
    #[test]
    fn an_early_release_says_why() {
        let e: Box<dyn std::error::Error> = Box::new(ReleasedEarly);
        assert!(e.is::<ReleasedEarly>());
        assert_eq!(e.to_string(), "The button was released before the drag could begin.");
    }
}
