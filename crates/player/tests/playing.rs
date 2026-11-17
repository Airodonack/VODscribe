//! Playing, pausing, seeking and the speed, at the player's API: where a
//! seek lands, that it leaves playing or paused as it was, what Play does
//! at the end, a speed asked for, and a timeline changed under it. On a
//! video whose every frame's grey says its number, made by ffmpeg
//! (skipped without it, except where VODSCRIBE_TESTS_NEED_ALL is set, as
//! on CI); where the order of the player's threads must be forced, on the
//! test kit's fake ffmpeg, its decodes held at a gate.

mod common;

use std::path::Path;

use common::{FPS, Heard, ffmpeg, numbered, or_skip};
use vodscribe_core::footage::Source;
use vodscribe_core::testkit::{Ffmpeg, Gate, Media, TempDir, eventually};
use vodscribe_player::clock::Player;
use vodscribe_player::timeline::{Stretch, Timeline};

/// A stretch of `file` at `at` for `length` seconds, from `from` in it.
fn stretch(file: &Path, at: f64, length: f64, from: f64) -> Stretch {
    let source = Source { path: file.to_path_buf(), offset: 0.0, section: None };
    Stretch { at, length, piece_id: None, source: Some(source), from, sound: None }
}

/// `length` seconds from `at` with no footage.
fn gap(at: f64, length: f64) -> Stretch {
    Stretch { at, length, piece_id: None, source: None, from: 0.0, sound: None }
}

/// A seek lands on the start of the frame showing at the time asked,
/// within the timeline, and not back out of footage that starts off the
/// frame grid into the gap before it (clock.rs `Player::snap`: "Where a
/// seek to `time` lands: the start of the frame showing there. Not back out
/// of the footage `time` is in, into a gap before it: a site's footage can
/// start off the frame grid").
#[test]
fn a_seek_lands_on_the_start_of_the_frame_showing_there() {
    let dir = TempDir::new("snap");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let player = Player::open_file(ffmpeg(), &file).unwrap();
    assert_eq!(player.snap(1.51), 1.5);
    // A frame's start, a hair below in floating point, is that frame.
    assert_eq!(player.snap(52.0 / FPS - 1e-9), 52.0 / FPS);
    assert_eq!(player.snap(9.0), 4.0, "within the timeline");
    assert_eq!(player.snap(-1.0), 0.0);
    // Footage from 0.067 s, as Twitch's first frame shows, after a gap.
    let timeline = Timeline::from_stretches(vec![gap(0.0, 0.067), stretch(&file, 0.067, 3.0, 0.067)]);
    let player = Player::open(ffmpeg(), timeline).unwrap();
    assert_eq!(player.snap(0.07), 0.067, "not back into the gap");
    assert_eq!(player.snap(0.05), 1.0 / FPS, "before it, as ever");
}

/// A seek leaves the player playing or paused as it was: paused, it shows
/// the frame there and stays paused; playing, it plays on from there.
/// Decided for 0.7.4: seek never changes play state (clock.rs
/// `Player::seek`: "Goes to the frame showing at `time`";
/// tests/e2e/scenarios/player-seek.mjs: "a seek doesn't pause").
#[test]
fn a_seek_leaves_playing_or_paused_as_it_was() {
    let dir = TempDir::new("seek-state");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let heard = Heard::new(Player::open_file(ffmpeg(), &file).unwrap());
    let player = &heard.player;
    assert_eq!(heard.number_at(0.0), 0);
    player.seek(2.0);
    assert!(!player.playing(), "paused, a seek leaves it paused");
    assert_eq!(heard.number_at(2.0), 60, "showing the frame there");
    assert!(!player.playing());

    player.seek(0.5);
    player.play();
    heard.until("playing past 0:00.6", |p| p.shown().is_some_and(|t| t > 0.6));
    player.seek(1.5);
    assert!(player.playing(), "playing, a seek leaves it playing");
    heard.until("playing on from 0:01.5", |p| p.shown().is_some_and(|t| t > 1.6));
    assert!(player.playing());
}

/// Played to its end, the player stands paused at the end; Play then plays
/// it from the start again (clock.rs `end`: "the time stays at its end,
/// paused"; `Player::play`: "From the start again: the decoders start
/// over").
#[test]
fn play_after_the_end_plays_from_the_start() {
    let dir = TempDir::new("replay");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let heard = Heard::new(Player::open_file(ffmpeg(), &file).unwrap());
    let player = &heard.player;
    player.seek(3.8);
    heard.number_at(3.8);
    player.play();
    heard.ended();
    assert!(!player.playing(), "paused at the end");
    assert_eq!(player.time(), player.duration(), "at the end");
    player.play();
    assert!(player.playing());
    heard.until("a frame from the start", |p| p.shown().is_some_and(|t| t < 1.0));
}

/// The speed asked for is kept within a quarter and four times as fast
/// (clock.rs `Player::set_speed`: "Plays `speed` times as fast (within
/// 0.25 to 4)").
#[test]
fn the_speed_is_kept_within_a_quarter_and_four_times_as_fast() {
    let player = Player::open(ffmpeg(), Timeline::default()).unwrap();
    assert_eq!(player.speed(), 1.0, "as it opens");
    for (asked, kept) in [(1.5, 1.5), (10.0, 4.0), (0.1, 0.25), (4.0, 4.0), (0.25, 0.25)] {
        player.set_speed(asked);
        assert_eq!(player.speed(), kept, "asked for {asked}");
    }
}

/// A speed asked for leaves the time where it is and the player playing or
/// paused as it was (clock.rs `Player::set_speed`: "Playing, it starts over
/// from the time now, as a seek does").
#[test]
fn a_speed_change_leaves_the_time_and_playing_or_paused_as_they_were() {
    let dir = TempDir::new("speed");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let heard = Heard::new(Player::open_file(ffmpeg(), &file).unwrap());
    let player = &heard.player;
    player.seek(1.0);
    heard.number_at(1.0);
    player.set_speed(1.5);
    assert_eq!((player.time(), player.playing()), (1.0, false), "paused, it stands where it was");
    assert_eq!(heard.number_at(1.0), 30, "on the same frame");

    player.play();
    heard.until("playing past 0:01.2", |p| p.shown().is_some_and(|t| t > 1.2));
    let before = player.time();
    player.set_speed(0.5);
    let after = player.time();
    assert!(player.playing(), "playing, it plays on");
    assert!(after >= before && after - before < 0.5, "from the time it was: {before} then {after}");
    heard.until("playing on from there", |p| p.shown().is_some_and(|t| t > after + 0.1));
}

/// A timeline emptied while it plays (its last piece taken out) leaves the
/// player paused at its start, showing nothing (clock.rs
/// `Player::set_timeline`: "Nothing left to play (its last piece taken
/// out): paused"; `run`: "Nothing to play (an edit emptied): no frame
/// shows").
#[test]
fn a_timeline_emptied_while_it_plays_pauses_at_its_start() {
    let dir = TempDir::new("emptied");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let timeline = Timeline::from_stretches(vec![stretch(&file, 0.0, 4.0, 0.0)]);
    let heard = Heard::new(Player::open(ffmpeg(), timeline).unwrap());
    let player = &heard.player;
    player.play();
    heard.until("playing past 0:00.2", |p| p.shown().is_some_and(|t| t > 0.2));
    player.set_timeline(Timeline::default());
    assert!(!player.playing(), "paused");
    assert_eq!(player.time(), 0.0, "at its start");
    heard.until("no frame showing", |p| p.frame().is_none() && p.shown().is_none());
}

/// A timeline played to its end and then made longer (more of its footage
/// in) plays on from where it ended, not from the start (clock.rs
/// `Player::set_timeline`: "Ended, a timeline that now goes further plays
/// on from the end, rather than from the start again"; CHANGELOG.md, 0.6.0:
/// "it plays in VODscribe as it comes in").
#[test]
fn a_timeline_made_longer_after_its_end_plays_on_from_there() {
    let dir = TempDir::new("longer");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let one = Timeline::from_stretches(vec![stretch(&file, 0.0, 1.0, 0.0)]);
    let heard = Heard::new(Player::open(ffmpeg(), one).unwrap());
    let player = &heard.player;
    player.seek(0.8);
    heard.number_at(0.8);
    player.play();
    heard.ended();
    player.set_timeline(Timeline::from_stretches(vec![stretch(&file, 0.0, 2.0, 0.0)]));
    assert_eq!(player.time(), 1.0, "where it ended");
    assert_eq!(heard.number_at(1.0), 30, "showing the frame there");
    player.play();
    let mut least = f64::INFINITY;
    heard.until("playing on past 0:01.2", |p| {
        let shown = p.shown().unwrap_or(f64::INFINITY);
        least = least.min(shown);
        shown > 1.2 && shown < f64::INFINITY
    });
    assert!(least >= 1.0, "a frame from before where it ended: {least}");
}

/// A timeline played to its end and planned again no longer (an edit's
/// plan asked for again, unchanged) stays ended: Play plays it from the
/// start (clock.rs `Player::set_timeline`: "Ended, a timeline that now goes
/// further plays on from the end"; `Player::play`: "From the start again").
#[test]
fn a_timeline_planned_again_no_longer_after_its_end_plays_from_the_start() {
    let dir = TempDir::new("same-again");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let one = || Timeline::from_stretches(vec![stretch(&file, 0.0, 1.0, 0.0)]);
    let heard = Heard::new(Player::open(ffmpeg(), one()).unwrap());
    let player = &heard.player;
    player.seek(0.8);
    heard.number_at(0.8);
    player.play();
    heard.ended();
    player.set_timeline(one());
    assert_eq!((player.time(), player.playing()), (1.0, false), "at the end, paused");
    player.play();
    heard.until("a frame from the start", |p| p.shown().is_some_and(|t| t < 0.5));
}

/// A preview where nothing plays (no footage there), or once the player
/// has closed, gives no frame (clock.rs `Previewer::preview`: "None where
/// nothing plays at `time`, ffmpeg gives nothing, or the player is
/// closed").
#[test]
fn a_preview_where_nothing_plays_or_of_a_closed_player_gives_no_frame() {
    let dir = TempDir::new("preview-none");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let timeline = Timeline::from_stretches(vec![gap(0.0, 1.0), stretch(&file, 1.0, 2.0, 1.0)]);
    let player = Player::open(ffmpeg(), timeline).unwrap();
    assert!(player.preview(0.5).is_none(), "no footage there");
    let previewer = player.previewer();
    assert!(previewer.preview(1.5).is_some_and(|p| common::number(&p.frame) == 45), "footage there");
    drop(player);
    assert!(previewer.preview(1.5).is_none(), "closed");
}

/// A seek while playing holds the time at the frame it lands on, not
/// moving, until that frame is decoded; then it runs on from there
/// (clock.rs `Player::running`: "After play or a seek while playing the
/// time stands until the sound starts"; `State::ready`: "until it does, the
/// time stands and the sound waits, so picture and sound start
/// together"). On the fake ffmpeg, its decode held at a gate: a long run of
/// frames to decode before the one asked for.
#[test]
fn a_seek_while_playing_holds_the_time_there_until_its_frame_is_decoded() {
    let dir = TempDir::new("seek-holds");
    let fake = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let mut media = Media::video(10.0);
    let video = media.video.as_mut().unwrap();
    (video.width, video.height) = (64, 36);
    let file = dir.join("a.mp4");
    media.write(&file);
    let heard = Heard::new(Player::open_file(fake.path(), &file).unwrap());
    let player = &heard.player;
    player.play();
    heard.until("playing past 0:00.2", |p| p.shown().is_some_and(|t| t > 0.2));

    let gate = Gate::new();
    fake.holds_decodes(&gate);
    player.seek(5.0);
    gate.reached();
    assert_eq!(player.time(), 5.0, "the time is the seek's at once");
    assert!(player.playing() && !player.running(), "playing, the time standing");
    assert_eq!(player.time(), 5.0, "and stays there");
    gate.open();
    eventually("the time running on from 0:05", || player.time() > 5.0);
    heard.until("the frame at 0:05 and on", |p| p.shown().is_some_and(|t| t >= 5.0));
}
