//! The app's own player. The webview's media stack decoded, timed and drew
//! video differently on each platform and version (and wrongly on some);
//! here the app does those itself, and the page keeps only the controls.
//!
//! The picture reaches the window through a native surface under the page
//! (`present`): the page is drawn with a transparent background, leaves the
//! player's box clear, and tells the surface where the box is. The surface
//! paints the page's background colour everywhere else, so the window looks
//! as it did.

use serde::{Deserialize, Serialize};

pub mod present;

/// A box on the page, in CSS pixels from the window's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// What the page asks the surface to show: where the picture goes (nothing,
/// when no player is open) and the page's background colour, for the rest.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Layout {
    pub rect: Option<Rect>,
    /// Red, green and blue, 0 to 1.
    pub background: [f32; 3],
}

impl Layout {
    /// A layout with the background colour written `#rrggbb` (the page's
    /// `--bg`); black if it isn't.
    pub fn with_background(rect: Option<Rect>, hex: &str) -> Layout {
        let hex = hex.trim().trim_start_matches('#');
        let channel =
            |at: usize| u8::from_str_radix(hex.get(at..at + 2).unwrap_or("00"), 16).unwrap_or(0) as f32 / 255.0;
        let background = if hex.len() == 6 { [channel(0), channel(2), channel(4)] } else { [0.0; 3] };
        Layout { rect, background }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_background_is_read_from_hex() {
        let l = Layout::with_background(None, " #141413 ");
        assert_eq!(l.background, [20.0 / 255.0, 20.0 / 255.0, 19.0 / 255.0]);
        assert_eq!(Layout::with_background(None, "rgb(1, 2, 3)").background, [0.0; 3]);
    }
}
