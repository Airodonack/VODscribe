//! The window's page, WebKitGTK's view on Linux, found in the window.

use gtk::prelude::*;

/// The page's view in `window`.
pub(crate) fn page(window: &tauri::WebviewWindow) -> anyhow::Result<gtk::Widget> {
    find_webview(window.gtk_window()?.upcast_ref()).ok_or_else(|| anyhow::anyhow!("no page in the window"))
}

/// The page's WebKitGTK view, inside `widget`.
fn find_webview(widget: &gtk::Widget) -> Option<gtk::Widget> {
    if widget.type_().name() == "WebKitWebView" {
        return Some(widget.clone());
    }
    widget.downcast_ref::<gtk::Container>()?.children().iter().find_map(find_webview)
}
