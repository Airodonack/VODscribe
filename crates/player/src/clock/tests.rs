//! The clock's own rules, with the sound's count set by the test
//! (`sound::Played::set`): what the time is while paused, while the picture
//! and the sound get going, and at a speed. A player can't be made to hold
//! its sound's count still, so these are said of the clock's state itself;
//! the player's own tests (crates/player/tests) say the rest at its API.

use super::*;

/// The clock paused at `at`, its picture ready.
fn paused_at(at: f64) -> State {
    State {
        playing: false,
        at,
        since: Instant::now(),
        epoch: 0,
        ended: false,
        sound: None,
        output: false,
        speed: 1.0,
        ready: true,
    }
}

/// Paused, the time stands; playing, it follows the sound played out, and
/// never passes the end (clock.rs: "Playing, the time follows the sound as
/// it's played out (`sound`) ... paused, it stands").
#[test]
fn the_time_stands_while_paused_and_follows_the_sound_while_playing() {
    let mut s = paused_at(3.0);
    assert_eq!(s.time(10.0), 3.0);
    s.playing = true;
    let played = Arc::new(sound::Played::default());
    s.sound = Some(Sound { played: played.clone(), rate: 100, live: true });
    assert!(s.running());
    assert_eq!(s.time(10.0), 3.0, "nothing played yet");
    played.set(150);
    assert_eq!(s.time(10.0), 4.5);
    played.set(100_000);
    assert_eq!(s.time(10.0), 10.0, "never past the end");
}

/// After a seek the time stands until the picture has its frame, and where
/// there's a sound output, until the sound plays too; without one it runs
/// by the computer's clock (clock.rs `State::ready`: "until it does, the
/// time stands and the sound waits, so picture and sound start together";
/// `Player::running`: "After play or a seek while playing the time stands
/// until the sound starts").
#[test]
fn a_restart_waits_for_the_picture_and_then_for_the_sound_where_there_is_an_output() {
    let mut s = paused_at(0.0);
    s.playing = true;
    s.restart(2.0);
    assert_eq!(s.epoch, 1);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(s.time(10.0), 2.0, "the time stands until the picture has its frame");
    assert!(!s.running());
    // No output: the computer's clock runs once the picture is ready.
    s.ready = true;
    s.since = Instant::now();
    assert!(s.sound.is_none());
    assert!(s.running());
    std::thread::sleep(Duration::from_millis(5));
    assert!(s.time(10.0) > 2.0);
    // With an output, the picture ready isn't enough: the sound must play.
    s.output = true;
    s.restart(5.0);
    s.ready = true;
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(s.time(10.0), 5.0, "with an output, the time stands until the sound plays");
    assert!(!s.running(), "and isn't said to move");
}

/// At a speed, each second of sound played out is that many seconds of the
/// timeline (clock.rs `State::speed`: "How many seconds of the timeline a
/// second of play is"; `Player::set_speed`: "Plays `speed` times as fast").
#[test]
fn the_time_runs_at_the_speed() {
    let mut s = paused_at(2.0);
    s.playing = true;
    s.speed = 1.5;
    let played = Arc::new(sound::Played::default());
    s.sound = Some(Sound { played: played.clone(), rate: 100, live: true });
    played.set(200);
    assert_eq!(s.time(10.0), 5.0, "two seconds played out are three of the timeline");
}
