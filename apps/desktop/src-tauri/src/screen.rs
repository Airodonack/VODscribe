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
        media_session_off(&page);
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

/// Turns off WebKit's Media Session API for the page. WebKitGTK announces a
/// page's playing media to the desktop as MPRIS players, and Twitch's player,
/// seeking, made and dropped dozens in a second, which hung GNOME Shell
/// (screen and pointer frozen, sound playing on; 2026-10-18); with this off
/// it wasn't seen again. What's known for sure is less: the page has no
/// navigator.mediaSession, and WebKit still announces an MPRIS player for
/// media that plays (there's no switch for that); headless, with or without
/// this, it was one player, never the dozens. The app has its own controls.
/// The switch came in WebKitGTK 2.42, so it's looked up, and skipped where
/// it's not.
#[cfg(target_os = "linux")]
fn media_session_off(page: &gtk::Widget) {
    use std::ffi::{CStr, c_char, c_void};

    use glib::translate::ToGlibPtr;

    /// `name` from the libraries loaded, as a function of type `T`.
    unsafe fn symbol<T: Copy>(name: &CStr) -> Option<T> {
        // SAFETY: dlsym only looks the name up; the caller names the type.
        let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
        (!found.is_null()).then(|| unsafe { std::mem::transmute_copy(&found) })
    }
    type Settings = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
    type Features = unsafe extern "C" fn() -> *mut c_void;
    type Length = unsafe extern "C" fn(*mut c_void) -> usize;
    type Get = unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
    type Identifier = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type SetEnabled = unsafe extern "C" fn(*mut c_void, *mut c_void, i32);
    type Unref = unsafe extern "C" fn(*mut c_void);
    // SAFETY: WebKitGTK's own functions, by their documented signatures,
    // on the page's WebKitWebView, on the main thread.
    unsafe {
        let (Some(settings), Some(features), Some(length), Some(get), Some(identifier), Some(set), Some(unref)) = (
            symbol::<Settings>(c"webkit_web_view_get_settings"),
            symbol::<Features>(c"webkit_settings_get_all_features"),
            symbol::<Length>(c"webkit_feature_list_get_length"),
            symbol::<Get>(c"webkit_feature_list_get"),
            symbol::<Identifier>(c"webkit_feature_get_identifier"),
            symbol::<SetEnabled>(c"webkit_settings_set_feature_enabled"),
            symbol::<Unref>(c"webkit_feature_list_unref"),
        ) else {
            return;
        };
        let view: *mut gtk::ffi::GtkWidget = page.to_glib_none().0;
        let settings = settings(view.cast());
        let list = features();
        for i in 0..length(list) {
            let feature = get(list, i);
            if CStr::from_ptr(identifier(feature)) == c"MediaSession" {
                set(settings, feature, 0);
            }
        }
        unref(list);
    }
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
/// corner (none when no player is open), and whether the picture goes under
/// the page there (a site's player in the page over it: `screen_under`
/// says where it can) or over it (as for a file).
#[tauri::command]
pub(crate) fn set_screen(app: AppHandle, rect: Option<Rect>, under: Option<bool>) {
    let rect = rect.filter(Rect::sane);
    if let Some(screen) = app.state::<Screen>().0.lock().unwrap().as_ref() {
        screen.set_under(under.unwrap_or(false));
        screen.set(rect);
    }
}

/// Whether the picture can go under the page here: everywhere but X11 (a
/// child window is always over its parent's drawing, where the page is),
/// where the page draws the picture itself instead (player_frame).
#[tauri::command]
pub(crate) fn screen_under() -> bool {
    vodscribe_player::present::can_go_under()
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
        let max = vodscribe_player::MAX_CSS as u32;
        if !placed.rect.sane() || placed.size.0 > max || placed.size.1 > max {
            return Err("The overlay is larger than a screen.".into());
        }
        if rgba.len() as u64 != placed.size.0 as u64 * placed.size.1 as u64 * 4 {
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
