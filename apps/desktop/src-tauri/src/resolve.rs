//! DaVinci Resolve on Linux can't read MP4 clips (its free edition has no
//! H.264, and no edition reads AAC): they come in blank and silent. An MP4
//! clip or edit dragged out onto Resolve is refused there, and once the drag is over
//! the page asks whether to make clips as MOV (resolveDrop.svelte.ts), while
//! its "Don't ask again" isn't ticked (the setting `suggest_mov_for_resolve`).
//! Asked then, not at the drag's start: the drop on Resolve is what makes the
//! question make sense. Resolve is told by the window the drag is let go on,
//! where X11 names it (`dragndrop_source::linux::Asker`); where it doesn't
//! (Wayland), a drop on any other app asks, if Resolve is installed, and
//! nothing is refused. Linux only: elsewhere a drag never asks.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use vodscribe_core::settings::Settings;

use crate::drag_source::Ending;

/// Where Resolve's Linux installer puts its program.
const PROGRAM: &str = "/opt/resolve/bin/resolve";

/// Whether Resolve is installed.
fn installed() -> bool {
    installed_at(Path::new(PROGRAM))
}

/// Whether Resolve's program is at `program`.
fn installed_at(program: &Path) -> bool {
    program.is_file()
}

/// Whether the app asking for a drag's files is Resolve, by its windows'
/// class or the program its process runs.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_resolve(class: Option<&str>, program: Option<&Path>) -> bool {
    class.is_some_and(|c| c.eq_ignore_ascii_case("resolve"))
        || program.and_then(Path::file_name).is_some_and(|name| name == "resolve")
}

/// Whether a drag of the files `names` is watched for Resolve: on Linux,
/// while `settings` say to ask, and one of them is MP4 (or its sound alone,
/// M4A: AAC too).
pub(crate) fn applies(settings: &Settings, names: &[&str]) -> bool {
    cfg!(target_os = "linux")
        && settings.suggest_mov_for_resolve
        && names.iter().any(|name| {
            let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default();
            ext.eq_ignore_ascii_case("mp4") || ext.eq_ignore_ascii_case("m4a")
        })
}

/// What the page hears of a drag that may have gone to Resolve
/// (`DragEnded::resolve`).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ResolveDrop {
    /// Let go on Resolve, which was given none of the files.
    Refused,
    /// Let go on another app, which may be Resolve: the system doesn't say.
    Maybe,
}

/// One drag's watch for Resolve.
#[derive(Default)]
pub(crate) struct Watch {
    applies: bool,
    /// Resolve asked for the files and was refused them.
    refused: AtomicBool,
    /// The system doesn't name the apps it goes to.
    unnamed: AtomicBool,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl Watch {
    /// A watch on a drag, if it `applies` (`applies`); else one that never
    /// refuses or asks.
    pub(crate) fn new(applies: bool) -> Self {
        Watch { applies, ..Default::default() }
    }

    pub(crate) fn applies(&self) -> bool {
        self.applies
    }

    /// Whether an app asking for the files, of windows of `class` and running
    /// `program`, is refused them: Resolve, while this applies.
    pub(crate) fn refuses(&self, class: Option<&str>, program: Option<&Path>) -> bool {
        let refused = self.applies && is_resolve(class, program);
        if refused {
            self.refused.store(true, Ordering::SeqCst);
        }
        refused
    }

    /// The system doesn't name the apps the drag goes to (Wayland).
    pub(crate) fn unnamed(&self) {
        self.unnamed.store(true, Ordering::SeqCst);
    }

    /// What the page hears of it once it's over (`ending`; the files
    /// `delivered` to the app let go on, or not).
    pub(crate) fn heard(&self, ending: Ending, delivered: bool) -> Option<ResolveDrop> {
        heard(
            self.applies,
            self.refused.load(Ordering::SeqCst),
            self.unnamed.load(Ordering::SeqCst),
            ending,
            delivered,
            installed,
        )
    }
}

/// What the page hears of a drag that `applies`, over (`ending`, the files
/// `delivered` or not): Resolve `refused`, or let go on another app the
/// system didn't name (`unnamed`) where Resolve is `installed`.
fn heard(
    applies: bool,
    refused: bool,
    unnamed: bool,
    ending: Ending,
    delivered: bool,
    installed: impl FnOnce() -> bool,
) -> Option<ResolveDrop> {
    match ending {
        _ if !applies => None,
        Ending::CalledOff | Ending::Back => None,
        // (Resolve refuses a drop it gets nothing from: let go nowhere.)
        _ if refused && !delivered => Some(ResolveDrop::Refused),
        Ending::Elsewhere if unnamed && installed() => Some(ResolveDrop::Maybe),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
