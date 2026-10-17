//! The screen the player draws on, a native surface in the window
//! (crates/player `present`), over the page's player box. The page says
//! where its box is (Screen.svelte).

use std::sync::Mutex;

#[cfg(target_os = "linux")]
use gtk::prelude::*;
use tauri::{AppHandle, Manager};
use vodscribe_player::Rect;

/// The native screen, once the window has one.
#[derive(Default)]
pub(crate) struct Screen(Mutex<Option<vodscribe_player::present::Screen>>);

/// Puts the screen in `window`.
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
    #[cfg(target_os = "macos")]
    {
        // Over the page's WKWebView, from the main thread, where it's handed out.
        let app = app.clone();
        window.with_webview(move |webview| match vodscribe_player::present::Screen::attach(webview.inner()) {
            Ok(screen) => *app.state::<Screen>().0.lock().unwrap() = Some(screen),
            Err(e) => eprintln!("no screen: {e:#}"),
        })?;
    }
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
    if let Some(screen) = app.state::<Screen>().0.lock().unwrap().as_ref() {
        screen.show(frame);
    }
}

/// Where the page's player box is, in CSS pixels from the page's top-left
/// corner (none when no player is open).
#[tauri::command]
pub(crate) fn set_screen(app: AppHandle, rect: Option<Rect>) {
    if let Some(screen) = app.state::<Screen>().0.lock().unwrap().as_ref() {
        screen.set(rect);
    }
}

/// What the page draws over the picture (captions): the body is RGBA pixels
/// as a canvas gives them (not premultiplied), the `overlay` header its box
/// and size as JSON (`{"rect": Rect, "size": [w, h]}`). An empty body takes
/// it away.
#[tauri::command]
pub(crate) fn set_screen_overlay(app: AppHandle, request: tauri::ipc::Request<'_>) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct Placed {
        rect: Rect,
        size: (u32, u32),
    }
    let tauri::ipc::InvokeBody::Raw(rgba) = request.body() else {
        return Err("Send the overlay's pixels as bytes.".into());
    };
    let overlay = if rgba.is_empty() {
        None
    } else {
        let header = request.headers().get("overlay").ok_or("No overlay header.")?;
        let placed: Placed = serde_json::from_slice(header.as_bytes()).map_err(|e| e.to_string())?;
        if rgba.len() != placed.size.0 as usize * placed.size.1 as usize * 4 {
            return Err("The overlay's pixels don't fill its size.".into());
        }
        let mut pixels = rgba.clone();
        for p in pixels.as_chunks_mut::<4>().0 {
            let a = p[3] as u16;
            for c in &mut p[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
        Some(std::sync::Arc::new(vodscribe_player::Overlay::new(placed.rect, placed.size, pixels)))
    };
    if let Some(screen) = app.state::<Screen>().0.lock().unwrap().as_ref() {
        screen.overlay(overlay);
    }
    Ok(())
}
