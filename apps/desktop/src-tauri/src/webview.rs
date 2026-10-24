//! The window's page, as a widget in the window: WebKitGTK's view on Linux,
//! or under Servo (the `servo` feature) the window's box that Servo's
//! surface covers.

use gtk::prelude::*;

use crate::WebviewWindow;

/// The page's view in `window`.
#[cfg(not(feature = "servo"))]
pub(crate) fn page(window: &WebviewWindow) -> anyhow::Result<gtk::Widget> {
    find_webview(window.gtk_window()?.upcast_ref()).ok_or_else(|| anyhow::anyhow!("no page in the window"))
}

/// The page's WebKitGTK view, inside `widget`.
#[cfg(not(feature = "servo"))]
fn find_webview(widget: &gtk::Widget) -> Option<gtk::Widget> {
    if widget.type_().name() == "WebKitWebView" {
        return Some(widget.clone());
    }
    widget.downcast_ref::<gtk::Container>()?.children().iter().find_map(find_webview)
}

/// Where the page is in `window`. Servo draws into a child X window of the
/// GTK window's, which no widget stands for; it fills the window's box (the
/// one webview, from the window's top-left corner), so the box's coordinates
/// are the page's.
#[cfg(feature = "servo")]
pub(crate) fn page(window: &WebviewWindow) -> anyhow::Result<gtk::Widget> {
    Ok(window.default_vbox()?.upcast())
}
