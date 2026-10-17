//! The screen the player draws on, a native surface in the window
//! (crates/player `present`), over the page's player box. The page says
//! where its box is (Screen.svelte). Where there's no screen yet (macOS),
//! the box stays the page's.

use std::sync::Mutex;

#[cfg(target_os = "linux")]
use gtk::prelude::*;
use tauri::{AppHandle, Manager};
use vodscribe_player::Rect;

/// The native screen, once the window has one.
#[derive(Default)]
pub(crate) struct Screen(#[allow(dead_code)] Mutex<Option<vodscribe_player::present::Screen>>);

/// Puts the screen in `window`, on platforms that have one.
pub(crate) fn attach(app: &AppHandle, window: &tauri::WebviewWindow) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // Over the page's view: the box the page reports is in its coordinates.
        let page =
            find_webview(window.gtk_window()?.upcast_ref()).ok_or_else(|| anyhow::anyhow!("no page in the window"))?;
        let screen = vodscribe_player::present::Screen::attach(&page);
        *app.state::<Screen>().0.lock().unwrap() = Some(screen);
    }
    #[cfg(windows)]
    {
        let screen = vodscribe_player::present::Screen::attach(window.hwnd()?.0 as isize)?;
        *app.state::<Screen>().0.lock().unwrap() = Some(screen);
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    let _ = (app, window);
    Ok(())
}

/// The page's WebKitGTK view, inside `widget`.
#[cfg(target_os = "linux")]
fn find_webview(widget: &gtk::Widget) -> Option<gtk::Widget> {
    if widget.type_().name() == "WebKitWebView" {
        return Some(widget.clone());
    }
    widget.downcast_ref::<gtk::Container>()?.children().iter().find_map(find_webview)
}

/// Whether the window has a screen.
pub(crate) fn attached(app: &AppHandle) -> bool {
    app.state::<Screen>().0.lock().unwrap().is_some()
}

/// Shows `frame` on the screen (none: black), if there is one.
pub(crate) fn show(app: &AppHandle, frame: Option<std::sync::Arc<vodscribe_player::decode::Frame>>) {
    #[cfg(any(target_os = "linux", windows))]
    if let Some(screen) = app.state::<Screen>().0.lock().unwrap().as_ref() {
        screen.show(frame);
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    let _ = (app, frame);
}

/// Where the page's player box is, in CSS pixels from the page's top-left
/// corner (none when no player is open).
#[tauri::command]
pub(crate) fn set_screen(app: AppHandle, rect: Option<Rect>) {
    #[cfg(any(target_os = "linux", windows))]
    if let Some(screen) = app.state::<Screen>().0.lock().unwrap().as_ref() {
        screen.set(rect);
        return;
    }
    let _ = (app, rect);
}
