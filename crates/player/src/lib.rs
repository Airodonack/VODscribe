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
pub mod decode;
pub mod present;
pub mod sound;
pub mod timeline;

/// A box on the page, in CSS pixels from the window's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
