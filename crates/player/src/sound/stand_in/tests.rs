//! The tests' stand-in output keeps a device's pace.

use super::*;

/// The stand-in takes its samples in real time, as a device would, so
/// the time follows the clock (sound_idle.rs: "A stand-in output
/// (`sound::stand_in`) takes the samples in real time, so the time
/// follows it as it would a device's"): a wake-up late by however much
/// is made up by the next chunks, which are due at once.
#[test]
fn the_stand_in_keeps_the_clocks_pace_when_its_wake_ups_run_late() {
    let begun = Instant::now();
    let mut pace = Pace::starting(begun);
    let mut now = begun;
    let mut chunks = 0u32;
    // A second of chunks, each sleep running 20 ms late.
    while now < begun + Duration::from_secs(1) {
        chunks += 1;
        let wait = pace.asked(now);
        if !wait.is_zero() {
            now += wait + Duration::from_millis(20);
        }
    }
    let played = CHUNK_TIME * chunks;
    let behind = (now - begun).saturating_sub(played);
    assert!(behind <= CHUNK_TIME + Duration::from_millis(20), "{played:?} played in {:?}", now - begun);
    // On time, a chunk's time apart.
    let mut pace = Pace::starting(begun);
    assert_eq!(pace.asked(begun), CHUNK_TIME);
    assert_eq!(pace.asked(begun + CHUNK_TIME), CHUNK_TIME);
}
