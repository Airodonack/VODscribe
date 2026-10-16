//! The screen the player draws on, a native surface in the window
//! (crates/player `present`). The page says where its player box is and what
//! colour its background is (Screen.svelte), and is told whether to draw
//! itself transparent over the screen. Where there's no screen yet (other
//! platforms), the box stays the page's.

use std::sync::Mutex;

#[cfg(target_os = "linux")]
use gtk::prelude::*;
use tauri::{AppHandle, Manager};
use vodscribe_player::{Layout, Rect};

/// The native screen, once the window has one.
#[derive(Default)]
pub(crate) struct Screen(#[allow(dead_code)] Mutex<Option<vodscribe_player::present::Screen>>);

/// Whether the page is drawn transparent, over the screen: so far only
/// when asked (VODSCRIBE_SCREEN=under), to try that placement (present.rs).
pub(crate) fn page_transparent() -> bool {
    cfg!(target_os = "linux") && std::env::var("VODSCRIBE_SCREEN").as_deref() == Ok("under")
}

/// Puts the screen in `window`, on platforms that have one.
pub(crate) fn attach(app: &AppHandle, window: &tauri::WebviewWindow) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use vodscribe_player::present::{Place, Screen as Native};
        let place = if page_transparent() { Place::Under } else { Place::Over };
        let screen = Native::attach(window.gtk_window()?.upcast_ref(), place)?;
        *app.state::<Screen>().0.lock().unwrap() = Some(screen);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (app, window);
    Ok(())
}

/// Where the page's player box is (none when no player is open) and the
/// page's background colour (`#rrggbb`). Whether the page is to draw itself
/// transparent over the screen.
#[tauri::command]
pub(crate) fn set_screen(app: AppHandle, rect: Option<Rect>, background: String) -> bool {
    #[cfg(target_os = "linux")]
    if let Some(screen) = app.state::<Screen>().0.lock().unwrap().as_ref() {
        screen.set(Layout::with_background(rect, &background));
        return page_transparent();
    }
    let _ = (app, rect, background, Layout::default());
    false
}
