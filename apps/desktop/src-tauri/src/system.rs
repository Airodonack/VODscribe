//! What the app asks of the system's desktop: to show a file in its file
//! manager, to open one in the app it opens with. The commands ask here
//! rather than the plugins themselves, so in the tests the testkit's desktop
//! hears them, instead of the person's own (`testkit::Desktop`).

use std::path::Path;

use crate::{AppHandle, CmdResult};

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
