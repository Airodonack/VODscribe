//! DaVinci Resolve on Linux can't read MP4 clips (its free edition has no
//! H.264, and no edition reads AAC): they come in blank and silent. An MP4
//! clip dragged out onto Resolve is refused there, and once the drag is over
//! the page asks whether to make clips as MOV (resolveDrop.svelte.ts), while
//! its "Don't show again" isn't ticked (the setting `suggest_mov_for_resolve`).
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
mod tests {
    use super::*;

    /// Resolve is told by its windows' class, or by its program where the
    /// class doesn't say.
    #[test]
    fn resolve_is_told_by_its_window_or_its_process() {
        assert!(is_resolve(Some("resolve"), None));
        assert!(is_resolve(Some("Resolve"), Some(Path::new("/usr/bin/python3"))), "the test's stand-in");
        assert!(is_resolve(None, Some(Path::new(PROGRAM))));
        assert!(is_resolve(Some("unknown"), Some(Path::new("/opt/resolve/bin/resolve"))));
        assert!(!is_resolve(Some("org.gnome.Nautilus"), Some(Path::new("/usr/bin/nautilus"))));
        assert!(!is_resolve(Some("kdenlive"), None));
        assert!(!is_resolve(None, None));
        let watch = Watch::new(true);
        assert!(!watch.refuses(Some("dolphin"), None));
        assert!(watch.refuses(Some("resolve"), None));
        assert!(!Watch::new(false).refuses(Some("resolve"), None), "not watched: never refused");
    }

    /// Installed is its program where its installer puts it.
    #[test]
    fn resolve_is_installed_where_its_program_is() {
        let dir = std::env::temp_dir().join(format!("vodscribe-resolve-{}", std::process::id()));
        let program = dir.join("bin/resolve");
        assert!(!installed_at(&program));
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, b"").unwrap();
        assert!(installed_at(&program));
        assert!(!installed_at(program.parent().unwrap()), "a folder of that name isn't it");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Watched while asked to and an MP4 (or M4A) is dragged, on Linux only.
    #[test]
    fn a_drag_of_mp4_is_watched_while_asked_to() {
        let on = Settings::default();
        let off = Settings { suggest_mov_for_resolve: false, ..Settings::default() };
        let linux = cfg!(target_os = "linux");
        assert_eq!(applies(&on, &["a.mp4"]), linux);
        assert_eq!(applies(&on, &["a.mov", "b.M4A"]), linux);
        assert!(!applies(&on, &["a.mov", "b.wav"]));
        assert!(!applies(&off, &["a.mp4"]));
        assert!(!applies(&on, &[]));
    }

    /// Refused by Resolve: said, wherever it ended but called off or back in
    /// the window. Let go on an app not named: said where Resolve is
    /// installed. Else nothing.
    #[test]
    fn a_drop_on_resolve_is_said() {
        use Ending::*;
        let yes = || true;
        let no = || false;
        for ending in [Elsewhere, Nowhere] {
            assert_eq!(heard(true, true, false, ending, false, no), Some(ResolveDrop::Refused), "{ending:?}");
        }
        assert_eq!(heard(true, true, false, Elsewhere, true, no), None, "went over Resolve, given to another");
        assert_eq!(heard(true, true, false, CalledOff, false, yes), None);
        assert_eq!(heard(true, true, false, Back, false, yes), None);
        assert_eq!(heard(false, true, false, Elsewhere, false, yes), None, "not watched");
        assert_eq!(heard(true, false, true, Elsewhere, true, yes), Some(ResolveDrop::Maybe));
        assert_eq!(heard(true, false, true, Elsewhere, false, yes), Some(ResolveDrop::Maybe));
        assert_eq!(heard(true, false, true, Elsewhere, true, no), None, "not installed");
        assert_eq!(heard(true, false, true, Nowhere, false, yes), None, "dropped on nothing");
        assert_eq!(heard(true, false, false, Elsewhere, true, yes), None, "named, not Resolve");
    }
}
