//! Events for the page, sent from any thread.
//!
//! Tauri sends an event from the main thread at once, and one from another
//! thread through its event loop's queue, which it reads only between its own
//! turns of the main loop. On Linux a drag out of the window holds the main
//! thread in a loop of its own while its clips are made (clips.rs
//! `start_drag`), so what other threads sent would wait for the drag to end:
//! the jobs, the time shown, the drag's own state. So there an event from
//! another thread goes to the main thread as a GLib source of its own, which
//! that loop runs too, and is sent from there. Elsewhere it goes as Tauri
//! sends it.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Sends `event` with `payload` to the page.
pub(crate) fn emit<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    #[cfg(target_os = "linux")]
    if !gtk::is_initialized_main_thread() {
        // The source only sends: it takes no lock, so the loop it runs in
        // can't be waiting on one. Sources of one priority run in the order
        // they came, so the page hears a thread's events in order.
        let Ok(payload) = serde_json::to_value(payload) else { return };
        let (app, event) = (app.clone(), event.to_owned());
        glib::idle_add_full(glib::Priority::DEFAULT, move || {
            let _ = app.emit(&event, &payload);
            glib::ControlFlow::Break
        });
        return;
    }
    let _ = app.emit(event, payload);
}
