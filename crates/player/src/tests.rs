//! The page's player box as the screens take it (`Rect`).

use super::*;

/// The screens are placed only at a box they could have: finite, not
/// negative in size, and within `MAX_CSS` (lib.rs `Rect::sane`: "Whether
/// it's a box the screens can be placed at: finite, not negative in size,
/// and within `MAX_CSS`").
#[test]
fn only_boxes_a_screen_could_have_are_sane() {
    let rect = |x, y, width, height| Rect { x, y, width, height };
    assert!(rect(10.0, -20.0, 1280.0, 720.0).sane());
    assert!(!rect(0.0, 0.0, f64::NAN, 720.0).sane());
    assert!(!rect(0.0, 0.0, -1.0, 720.0).sane());
    assert!(!rect(0.0, 0.0, 1e12, 720.0).sane());
    assert!(!rect(f64::INFINITY, 0.0, 10.0, 10.0).sane());
}
