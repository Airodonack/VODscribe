//! When the library won't open (a damaged file, say), the app still starts,
//! on a screen that says so (LibraryProblem.svelte) instead of the rest: none
//! of the app's state or background work is set up, so these commands need
//! none of it. Trying again restarts the app (from the page); starting a new
//! library sets the old one aside, its videos and clips with it, never
//! deleting it.

use std::path::PathBuf;

use tauri::State;

use crate::AppHandle;

/// Why the library didn't open, if it didn't, and where it is: set as the
/// app starts, and not changed after.
pub(crate) struct Problem {
    error: Option<String>,
    db: PathBuf,
    /// The library's layout, when it didn't open: what's set aside.
    paths: Option<vodscribe_core::Paths>,
}

impl Problem {
    pub(crate) fn none(db: PathBuf) -> Problem {
        Problem { error: None, db, paths: None }
    }

    pub(crate) fn new(error: String, paths: vodscribe_core::Paths) -> Problem {
        Problem { error: Some(error), db: paths.db(), paths: Some(paths) }
    }
}

/// Why the library didn't open when the app started; none if it did.
#[tauri::command]
pub(crate) fn library_problem(problem: State<'_, Problem>) -> Option<String> {
    problem.error.clone()
}

/// Moves the library that won't open aside, with its videos and clips
/// (`db::set_aside`), so a new one is made when the app starts again;
/// returns the name of the folder it went to. When another app has one of
/// its files open (on Windows), nothing moves, and the error the page shows
/// names that file.
#[tauri::command]
pub(crate) fn set_library_aside(problem: State<'_, Problem>) -> crate::CmdResult<String> {
    let (Some(_), Some(paths)) = (&problem.error, &problem.paths) else {
        return Err("The library opened; there's nothing to set aside.".into());
    };
    let aside = vodscribe_core::db::set_aside(paths).map_err(crate::err)?;
    Ok(aside.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Shows the library's file in the file manager.
#[tauri::command]
pub(crate) fn show_library(app: AppHandle, problem: State<'_, Problem>) -> crate::CmdResult<()> {
    let shown =
        if problem.db.is_file() { problem.db.clone() } else { problem.db.parent().unwrap_or(&problem.db).into() };
    crate::system::reveal(&app, &shown)
}

#[cfg(test)]
mod tests;
