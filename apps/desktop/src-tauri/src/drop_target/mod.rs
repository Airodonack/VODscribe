//! Files dragged onto the window from other apps, taken in by the app's own
//! drop target. Windows only (`windows`: OLE's IDropTarget), where it also
//! receives virtual files: files with no path to give (a video inside a zip
//! folder in Explorer), read from the app they come from. The mirror of
//! `drag_source`.
//!
//! Nothing here knows the app: it's given the window's raw handle and a
//! `Target`, which hears the drag as it goes (`Event`), says which virtual
//! files it takes, and gives where each one's bytes go (`Incoming`).

use std::path::PathBuf;

mod windows;
pub(crate) use windows::register;

/// A drag over the window, as it goes. Positions are in the window's own
/// pixels (device pixels), from its top left.
#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    /// It came over the window with `paths` (files that are where they are)
    /// or else `names`, virtual files the target takes, to be received
    /// if it's let go.
    Enter {
        paths: Vec<PathBuf>,
        names: Vec<String>,
        at: (f64, f64),
    },
    Over {
        at: (f64, f64),
    },
    Leave,
    /// Let go with `paths`; none for virtual files, which come in through
    /// `Target::incoming` next.
    Drop {
        paths: Vec<PathBuf>,
        at: (f64, f64),
    },
}

/// What the window does with drags onto it: the app's side.
pub(crate) trait Target {
    /// The drag, as it goes; only drags with paths, or with virtual files
    /// it takes, are heard.
    fn heard(&self, event: Event);
    /// Whether a virtual file named `name` is one to receive: a drag with
    /// none it takes (and no paths) is refused as it comes over.
    fn takes(&self, name: &str) -> bool;
    /// A virtual file named `name` (`size` bytes, if the app it comes from
    /// says) was let go on the window: where its bytes go, or None to leave
    /// it.
    fn incoming(&self, name: &str, size: Option<u64>) -> Option<Box<dyn Incoming>>;
}

/// One virtual file being received, read from the app it comes from on a
/// thread of its own (or the window's, if that app can't wait).
pub(crate) trait Incoming: Send {
    /// The next of its bytes. An error stops it: canceled, or not written.
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    /// It's all here (Ok), or it never will be, and why.
    fn done(self: Box<Self>, result: Result<(), String>);
}
