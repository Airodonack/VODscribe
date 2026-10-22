//! The window's page, WebKitGTK's view on Linux: found in the window, and
//! set up once as the window is made.

use gtk::prelude::*;

/// The page's view in `window`.
pub(crate) fn page(window: &tauri::WebviewWindow) -> anyhow::Result<gtk::Widget> {
    find_webview(window.gtk_window()?.upcast_ref()).ok_or_else(|| anyhow::anyhow!("no page in the window"))
}

/// Turns off WebKit's Media Session API for `window`'s page, once, as the
/// window is made: it's a setting of the whole page, not of its player.
/// WebKitGTK announces a page's playing media to the desktop as MPRIS players,
/// and Twitch's player, seeking, made and dropped dozens in a second, which
/// hung GNOME Shell (screen and pointer frozen, sound playing on; 2026-10-18);
/// with this off it wasn't seen again. What's known for sure is less: the page
/// has no navigator.mediaSession, and WebKit still announces an MPRIS player
/// for media that plays (there's no switch for that); headless, with or without
/// this, it was one player, never the dozens. The app has its own controls. The
/// switch came in WebKitGTK 2.42, so it's looked up, and skipped where it's
/// not.
pub(crate) fn media_session_off(window: &tauri::WebviewWindow) -> anyhow::Result<()> {
    use std::ffi::{CStr, c_char, c_void};

    use glib::translate::ToGlibPtr;

    let page = page(window)?;

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
            return Ok(());
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
    Ok(())
}

/// The page's WebKitGTK view, inside `widget`.
fn find_webview(widget: &gtk::Widget) -> Option<gtk::Widget> {
    if widget.type_().name() == "WebKitWebView" {
        return Some(widget.clone());
    }
    widget.downcast_ref::<gtk::Container>()?.children().iter().find_map(find_webview)
}
