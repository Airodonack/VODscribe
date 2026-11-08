//! Dragging files out of the window into other apps, before they're made:
//! each system's drag begun here (`start`), from the press that's still
//! down, with its picture. What the drag carries, the receivers' asks and
//! how it ended are `dragndrop_source`'s; clips.rs answers for the files
//! (`Item`) and tells the page what it hears (`Event`). In unit tests no
//! system's drag begins: the testkit's does (`testkit::Drags`), and the
//! test says how it ends.

pub(crate) use dragndrop_source::{Ending, Event, Failure, Item, ReleasedEarly, State};

// (In unit tests the testkit's drag begins instead, so the system's goes unused.)
#[cfg(target_os = "linux")]
#[cfg_attr(test, allow(dead_code))]
mod linux;
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
mod macos;
#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
mod windows;

#[cfg(all(target_os = "linux", not(test)))]
pub(crate) use linux::start;
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) use macos::start;
#[cfg(all(windows, not(test)))]
pub(crate) use windows::start;
// The unit tests' drag, begun as each system's is (`testkit::Drags`).
#[cfg(test)]
pub(crate) use crate::testkit::start_drag as start;

/// A line about a drag's progress, in the tests' builds only (stderr, where
/// the end-to-end tests read them); the drag's own lines too, once one has
/// begun (`traced`).
#[cfg(feature = "e2e")]
pub(crate) fn trace(what: &str) {
    if cfg!(target_os = "macos") {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        eprintln!("drag: [{:.3}] {what}", now.as_secs_f64() % 1000.0);
    } else {
        eprintln!("drag: {what}");
    }
}
#[cfg(not(feature = "e2e"))]
pub(crate) fn trace(_: &str) {}

/// Has the drag's own lines traced too, in the tests' builds.
#[cfg_attr(any(test, not(any(target_os = "linux", target_os = "macos", windows))), allow(dead_code))]
fn traced() {
    #[cfg(feature = "e2e")]
    dragndrop_source::trace_with(trace);
}
