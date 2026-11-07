//! What the app asks of the system's desktop: to show a file in its file
//! manager, to open one in the app it opens with, to ask where to save one.
//! The commands ask here rather than the plugins themselves, so in the tests
//! the testkit's desktop hears them, and answers the save dialog, instead
//! of the person's own (`testkit::Desktop`).

use std::path::{Path, PathBuf};

use crate::{AppHandle, CmdResult, WebviewWindow};

/// Shows `path` selected in the system's file manager.
pub(crate) fn reveal(app: &AppHandle, path: &Path) -> CmdResult<()> {
    #[cfg(test)]
    return crate::testkit::desktop(app).reveal(path);
    #[cfg(not(test))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().reveal_item_in_dir(path).map_err(crate::err)
    }
}

/// Opens `path` (a file, or a folder in the file manager) as the system
/// opens it.
pub(crate) fn open(app: &AppHandle, path: &Path) -> CmdResult<()> {
    #[cfg(test)]
    return crate::testkit::desktop(app).open(path);
    #[cfg(not(test))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().open_path(path.display().to_string(), None::<String>).map_err(crate::err)
    }
}

/// Asks in the system's save dialog, over `window`, where to save a file
/// named `name`, of the kind `kind` says ("Video") with extension `ext`; None
/// if the person cancels. Waits for the answer: call it off the main thread.
pub(crate) fn pick_save(
    app: &AppHandle,
    window: &WebviewWindow,
    name: &str,
    kind: &str,
    ext: &str,
) -> CmdResult<Option<PathBuf>> {
    #[cfg(test)]
    {
        let _ = (window, kind, ext);
        Ok(crate::testkit::desktop(app).pick_save(name))
    }
    #[cfg(not(test))]
    {
        use tauri_plugin_dialog::DialogExt;
        let picked =
            app.dialog().file().set_parent(window).set_file_name(name).add_filter(kind, &[ext]).blocking_save_file();
        picked.map(|p| p.into_path().map_err(crate::err)).transpose()
    }
}
