//! The app's own player. The webview's media stack decoded, timed and drew
//! video differently on each platform and version (and wrongly on some);
//! here the app does those itself, and the page keeps only the controls.
//!
//! `decode` reads frames and sound with the ffmpeg the app ships, `timeline`
//! says what plays when (a file, or an edit's pieces end to end), `clock`
//! keeps the time and picks each frame's moment, and `present` draws it on a
//! native surface in the window, which the page tells where its player's
//! box is. `vodscribe-play` (src/bin) plays a file in a bare window with the
//! same pieces.

use serde::{Deserialize, Serialize};

pub mod clock;
pub mod compose;
pub mod decode;
pub mod present;
pub mod sound;
pub mod storyboard;
pub mod timeline;

/// A box on the page, in CSS pixels from the window's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The largest a box's side, or its distance from the corner, may be in CSS
/// pixels: well past any screen, and small enough that it's a texture's size
/// and fits an i32 at any pixel density.
pub const MAX_CSS: f64 = 8192.0;

impl Rect {
    /// Whether it's a box the screens can be placed at: finite, not
    /// negative in size, and within `MAX_CSS`.
    pub fn sane(&self) -> bool {
        let within = |v: f64| v.is_finite() && v.abs() <= MAX_CSS;
        within(self.x)
            && within(self.y)
            && within(self.width)
            && within(self.height)
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

/// An image drawn over the picture (captions, notes): `pixels` is `size.0`
/// by `size.1` pixels of 8-bit RGBA, premultiplied by its alpha, shown in
/// `rect`, a box in the page's CSS pixels from the player box's top-left
/// corner. The page makes it at the screen's pixel density, so it's drawn
/// pixel for pixel.
#[derive(Debug)]
pub struct Overlay {
    pub rect: Rect,
    pub size: (u32, u32),
    pub pixels: Vec<u8>,
    /// Its own number, unlike any other overlay's this run: what's uploaded is told apart by it.
    pub id: u64,
}

impl Overlay {
    /// An overlay of `pixels` (`size`, RGBA premultiplied) shown in `rect`.
    pub fn new(rect: Rect, size: (u32, u32), pixels: Vec<u8>) -> Overlay {
        static IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Overlay { rect, size, pixels, id: IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_boxes_a_screen_could_have_are_sane() {
        let rect = |x, y, width, height| Rect { x, y, width, height };
        assert!(rect(10.0, -20.0, 1280.0, 720.0).sane());
        assert!(!rect(0.0, 0.0, f64::NAN, 720.0).sane());
        assert!(!rect(0.0, 0.0, -1.0, 720.0).sane());
        assert!(!rect(0.0, 0.0, 1e12, 720.0).sane());
        assert!(!rect(f64::INFINITY, 0.0, 10.0, 10.0).sane());
    }
}
