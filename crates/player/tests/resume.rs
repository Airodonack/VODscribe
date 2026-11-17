//! Play after a pause plays on from the frame showing, with the decoder
//! that showed it. Alone in its file: it counts the decoders started
//! (`decode::video_starts`), which other tests would add to. Skipped where
//! there's no ffmpeg on PATH, except where VODSCRIBE_TESTS_NEED_ALL is set (CI).

mod common;

use common::{Heard, ffmpeg, number, numbered, or_skip};
use vodscribe_core::testkit::TempDir;
use vodscribe_player::clock::Player;
use vodscribe_player::decode::video_starts;

/// Play after a pause shows the next frame after the one paused on, from
/// the decoder already reading there: no ffmpeg started again, so no wait
/// (clock.rs `State::resume`: "Runs on from where it stands (play after a
/// pause): the picture keeps its decoder and the frame it holds").
#[test]
fn play_after_a_pause_plays_on_from_the_frame_showing_with_its_decoder() {
    let dir = TempDir::new("resume");
    let Some(file) = or_skip(numbered(&dir), "a numbered video") else { return };
    let heard = Heard::new(Player::open_file(ffmpeg(), &file).unwrap());
    let player = &heard.player;
    player.seek(1.0);
    heard.number_at(1.0);
    player.play();
    heard.until("playing past 0:01.2", |p| p.shown().is_some_and(|t| t > 1.2));
    player.pause();
    let paused_on = number(&player.frame().unwrap());
    let started = video_starts();

    player.play();
    heard.until("the next frame", |p| p.frame().is_some_and(|f| number(&f) != paused_on));
    assert_eq!(number(&player.frame().unwrap()), paused_on + 1, "the frame after the one paused on");
    heard.until("a few frames on", |p| p.frame().is_some_and(|f| number(&f) >= paused_on + 5));
    assert_eq!(video_starts(), started, "no decoder started again");
}
