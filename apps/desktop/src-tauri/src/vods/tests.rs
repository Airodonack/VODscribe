//! The specs of `vods.rs`: the files in `tests/` that hold them, and what
//! they share.

use super::*;

mod adding;
mod list;
mod reading;
mod removing;
mod transcribing;

/// Runs `command` on a thread of its own, held where it takes the queue's
/// lock (`Worker::holds_next_control`): whether it held the library's lock
/// there already, and its answer once let go. The library's lock comes
/// first everywhere (worker.rs `control`), as an erasure takes them
/// (`erase_unused`): one taking the queue's first could wait for the
/// library while an erasure holding it waits for the queue, for good.
fn took_the_library_first<T: Send + 'static>(
    app: &crate::testkit::TestApp,
    command: impl FnOnce(AppHandle) -> T + Send + 'static,
) -> (bool, T) {
    let gate = vodscribe_core::testkit::Gate::new();
    app.state().worker.holds_next_control(&gate);
    let handle = app.handle().clone();
    let running = std::thread::spawn(move || command(handle));
    gate.reached();
    // Held all the while it waits, it's the command's: another's hold of a
    // moment (a job's progress, say) lets go.
    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    let mut first = true;
    while first && std::time::Instant::now() < until {
        first = app.state().db.try_lock().is_err();
    }
    gate.open();
    (first, running.join().unwrap())
}
