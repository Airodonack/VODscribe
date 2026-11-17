//! The sound output's account of its underruns.

use super::*;

/// Underruns while playing are counted and told once, at the end of the
/// stretch of play, and those while paused (silence) never (sound.rs:
/// "Underruns while playing are counted and told once at the end of the
/// stretch of play, rather than a line each"; `told`: "those while it
/// wasn't (silence, heard by nobody) dropped").
#[test]
fn underruns_are_told_once_per_stretch_of_play_and_only_while_playing() {
    let underruns = AtomicU64::new(0);
    // Paused (silence): counted, never told, and dropped once playing.
    underruns.fetch_add(4, Ordering::Relaxed);
    assert_eq!(told(&underruns, false, false), None);
    assert_eq!(told(&underruns, false, true), None);
    // Playing: told once, at the end of the stretch, then counted anew.
    underruns.fetch_add(2, Ordering::Relaxed);
    assert_eq!(told(&underruns, true, true), None);
    underruns.fetch_add(1, Ordering::Relaxed);
    assert_eq!(told(&underruns, true, false), Some(3));
    assert_eq!(told(&underruns, false, false), None);
    // A stretch without any: nothing told.
    assert_eq!(told(&underruns, false, true), None);
    assert_eq!(told(&underruns, true, false), None);
}
