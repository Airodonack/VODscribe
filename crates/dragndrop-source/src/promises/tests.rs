//! The specs of a drag's promise counts: a drag ends told once every
//! promise asked for is written or given up (macos.rs `settle`: "when the
//! session is over and every promise asked for is settled").

use super::*;

/// A promise asked for is waited for from the moment it's counted: no
/// moment has it asked for and not being written (the 0.7.4 quality pass,
/// item 6: counted one by one, a settle between the two said the drag
/// wasn't delivered while Finder got the file).
#[test]
fn a_promise_asked_for_is_waited_for_until_it_is_written() {
    let promises = Promises::default();
    assert_eq!(promises.settled(), Some(Counts::default()), "none asked for");
    promises.asked();
    assert_eq!(promises.settled(), None, "settled while it's being written");
    promises.done(true);
    assert_eq!(promises.settled(), Some(Counts { asked: 1, writing: 0, written: 1 }));
}

/// A promise that wasn't written (failed, or called off by the receiver)
/// settles as asked for and not written.
#[test]
fn a_promise_not_written_settles_as_asked_for_and_not_written() {
    let promises = Promises::default();
    promises.asked();
    promises.done(false);
    assert_eq!(promises.settled(), Some(Counts { asked: 1, writing: 0, written: 0 }));
}

/// Several asked for at once (AppKit asks in any order, on the promises'
/// queue): settled only once the last is done.
#[test]
fn promises_asked_for_together_settle_once_the_last_is_done() {
    let promises = Promises::default();
    promises.asked();
    promises.asked();
    promises.done(true);
    assert_eq!(promises.settled(), None, "settled with one still being written");
    promises.asked();
    promises.done(false);
    promises.done(true);
    assert_eq!(promises.settled(), Some(Counts { asked: 3, writing: 0, written: 2 }));
}
