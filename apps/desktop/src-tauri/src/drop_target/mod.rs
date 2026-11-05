//! Files dragged onto the window from other apps, taken in by the app's own
//! drop target, which also receives virtual files: files with no path to
//! give, read from the app they come from by `dragndrop_target`. On Windows
//! (`windows`: OLE's IDropTarget) a video inside a zip folder in Explorer;
//! on macOS (`macos`: wry's web view's dragging methods, hooked) a file
//! promise. The mirror of `drag_source` and `file_drag`.
//!
//! This part owns the window's side (registering, the drag's events, its
//! answer); what a drag offers and its files' bytes are the crate's. Nothing
//! here knows the app: it's given the window's raw handle and a `Target`,
//! which hears the drag as it goes (`Event`), says which virtual files it
//! takes, and gives where each one's bytes go.

use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::register;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::register;

/// A drag over the window, as it goes. Positions are from the window's top
/// left, as Tauri's own events give them: in device pixels on Windows, in
/// points on macOS.
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
    /// A virtual file `item` was let go on the window: where its bytes go, or
    /// None to leave it.
    fn incoming(&self, item: &dragndrop_target::Item) -> Option<dragndrop_target::Destination>;
}
