//! Files dragged onto the window from other apps, taken in by the app's own
//! drop target, which also receives virtual files: files with no path to
//! give, read from the app they come from by `dragndrop_target`. On Windows
//! (`windows`: OLE's IDropTarget) it takes every drop, a video inside a zip
//! folder in Explorer and a link from a browser too; on macOS (`macos`:
//! wry's web view's dragging methods, hooked) a file promise, or a link
//! from another app; on Linux (`linux`: GTK's drag signals
//! on the page) the drops the web view can't, a file dragged out of an
//! archive manager or a sandboxed app. The mirror of `drag_source`.
//!
//! This part owns the window's side (registering, the drag's events, its
//! answer); what a drag offers and its files' bytes are the crate's. Nothing
//! here knows the app: it's given the window (its handle, view or page) and a `Target`,
//! which hears the drag as it goes (`Event`), says which virtual files it
//! takes, and gives where each one's bytes go.

use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
pub(crate) use linux::register;
#[cfg(target_os = "macos")]
pub(crate) use macos::register;
#[cfg(windows)]
pub(crate) use windows::register;

/// A drag over the window, as it goes. Positions are from the window's top
/// left, as Tauri's own events give them: in device pixels on Windows, in
/// points on macOS, in the page's pixels (GTK's) on Linux.
#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    /// It came over the window with `paths` (files that are where they are),
    /// `names` (virtual files the target takes, to be received if it's let
    /// go; with paths too in a drag mixing the two, on macOS) or else
    /// `links` (web links, http or https: a link from a browser).
    Enter {
        paths: Vec<PathBuf>,
        names: Vec<String>,
        links: Vec<String>,
        at: (f64, f64),
    },
    Over {
        at: (f64, f64),
    },
    Leave,
    /// Let go with `paths` or `links`; virtual files come in through
    /// `Target::incoming` next.
    Drop {
        paths: Vec<PathBuf>,
        links: Vec<String>,
        at: (f64, f64),
    },
}

/// A drag taken by the app's own drop target on macOS (`taken_on_mac`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, PartialEq)]
pub(crate) enum Offer<T> {
    /// The promises the target takes, and the paths of the items beside
    /// them that promise nothing.
    Promises { files: Vec<T>, paths: Vec<PathBuf> },
    /// Web links, and no files.
    Links(Vec<String>),
}

/// How a drag from another app is taken on macOS, or None when wry takes
/// it: as its promises the target takes, when they're better than its file
/// URLs (`promised`: those files, and the paths beside them); else, when it
/// has no file URLs (`file_urls` false), as its web links (`links`), if it
/// has some. A drag of promises the target doesn't take (an image, say)
/// beside a link is taken as the link, as on Windows.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn taken_on_mac<T>(
    promised: Option<(Vec<T>, Vec<PathBuf>)>,
    file_urls: bool,
    links: impl FnOnce() -> Vec<String>,
) -> Option<Offer<T>> {
    match promised {
        Some((files, paths)) if !files.is_empty() => return Some(Offer::Promises { files, paths }),
        Some((_, paths)) if !paths.is_empty() => return None,
        _ => {}
    }
    if file_urls {
        return None;
    }
    let links = links();
    if links.is_empty() { None } else { Some(Offer::Links(links)) }
}

/// What the window does with drags onto it: the app's side.
pub(crate) trait Target {
    /// The drag, as it goes; only drags with paths, with virtual files it
    /// takes, or with links (from another app), are heard.
    fn heard(&self, event: Event);
    /// Whether a virtual file named `name` is one to receive: a drag with
    /// none it takes (and no paths) is refused as it comes over.
    fn takes(&self, name: &str) -> bool;
    /// Virtual files `items` were let go on the window, together: those it
    /// receives, each with where its bytes go (the rest are left).
    fn incoming(
        &self,
        items: Vec<dragndrop_target::Item>,
    ) -> Vec<(dragndrop_target::Item, dragndrop_target::Destination)>;
    /// Where an app that saves its files itself (Linux: an archive
    /// manager) saves them first, or None to refuse such drops.
    #[cfg(target_os = "linux")]
    fn stage(&self) -> Option<PathBuf>;
    /// A drop's source gave nothing, and why (Linux: an archive manager
    /// that couldn't save, a portal transfer that didn't come).
    #[cfg(target_os = "linux")]
    fn nothing_given(&self, why: &dragndrop_target::Error);
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "https://www.twitch.tv/videos/1";

    fn links() -> Vec<String> {
        vec![LINK.into()]
    }

    #[test]
    fn promises_taken_are_taken_with_the_paths_beside_them() {
        let promised = Some((vec!["a.mp4"], vec![PathBuf::from("/b.mp4")]));
        assert_eq!(
            taken_on_mac(promised, true, links),
            Some(Offer::Promises { files: vec!["a.mp4"], paths: vec![PathBuf::from("/b.mp4")] })
        );
    }

    /// A browser's drag promising a file the target doesn't take (an image)
    /// beside its link, with no file URL: the link.
    #[test]
    fn promises_not_taken_fall_back_to_links() {
        let promised: Option<(Vec<&str>, _)> = Some((vec![], vec![]));
        assert_eq!(taken_on_mac(promised, false, links), Some(Offer::Links(links())));
    }

    #[test]
    fn a_drag_with_file_urls_is_wrys() {
        assert_eq!(taken_on_mac::<&str>(None, true, links), None);
        // Promises not taken, beside a plain file, or with stand-ins.
        assert_eq!(taken_on_mac::<&str>(Some((vec![], vec![PathBuf::from("/b.mp4")])), true, links), None);
        assert_eq!(taken_on_mac::<&str>(Some((vec![], vec![])), true, links), None);
    }

    #[test]
    fn links_alone_are_taken_and_nothing_is_wrys() {
        assert_eq!(taken_on_mac::<&str>(None, false, links), Some(Offer::Links(links())));
        assert_eq!(taken_on_mac::<&str>(None, false, Vec::new), None);
    }
}
