//! The size a frame too large for a texture is decoded at. The sound at
//! each speed is tests/decoding.rs's.

use super::*;

/// A frame larger than `MAX_SIDE` is scaled down keeping its shape, both
/// sides even; one within it is as it is (decode.rs `within`: "scaled down,
/// keeping its shape, so neither side is over `max` (and both are even, for
/// ffmpeg's scaler); as it is if neither is"). tests/frames.rs
/// `an_oversized_video_plays_scaled_down` plays one.
#[test]
fn frames_too_large_are_scaled_down_keeping_their_shape() {
    assert_eq!(within(1920, 1080, MAX_SIDE), (1920, 1080));
    assert_eq!(within(4096, 2160, MAX_SIDE), (4096, 2160));
    assert_eq!(within(11520, 5760, MAX_SIDE), (4096, 2048));
    assert_eq!(within(1080, 9000, MAX_SIDE), (492, 4096));
    assert_eq!(within(100_000, 1, MAX_SIDE), (4096, 2));
}
