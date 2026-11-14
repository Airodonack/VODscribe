//! What the app asks of the system: its desktop, to show a file in its file
//! manager or open one in the app it opens with; the variables it was
//! started with; the time. The commands ask here rather than the plugins or
//! the process themselves, so in the tests the testkit answers, instead of
//! the person's own desktop (`testkit::Desktop`), the process every test
//! shares (`testkit::Env`) or the real clock (`testkit::Clock`).

use std::path::Path;
use std::time::Instant;

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

/// The variable `name` the app was started with, if it was. In a test, the
/// test's own (`TestApp::started_with`): the tests run at once, in one process.
pub(crate) fn var(app: &AppHandle, name: &str) -> Option<String> {
    #[cfg(test)]
    return crate::testkit::env(app).var(name);
    #[cfg(not(test))]
    {
        let _ = app;
        std::env::var(name).ok()
    }
}

/// The time now, for what goes by it (a cut's progress, sent at most so
/// often; how long a VOD's work is waited for). In a test, the test's clock, which stands still till the test
/// moves it (`TestApp::clock`).
pub(crate) fn now(app: &AppHandle) -> Instant {
    #[cfg(test)]
    return crate::testkit::clock(app).now();
    #[cfg(not(test))]
    {
        let _ = app;
        Instant::now()
    }
}
