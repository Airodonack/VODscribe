//! The page's small choices (crates/core `choices`), read once as the app
//! starts and written as each changes. Apart from the library's state, so
//! they're there on the recovery screen too.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::State;
use vodscribe_core::choices::{self, Choices};

use crate::{CmdResult, err};

pub(crate) struct Kept {
    path: PathBuf,
    choices: Mutex<Choices>,
}

impl Kept {
    pub(crate) fn load(path: PathBuf) -> Self {
        let choices = Mutex::new(choices::load(&path));
        Self { path, choices }
    }
}

/// Every choice kept, for the page to read before it shows (stored.ts).
#[tauri::command]
pub(crate) fn kept_choices(kept: State<'_, Kept>) -> Choices {
    kept.choices.lock().unwrap().clone()
}

/// Keeps `value` under `key`, written at once. The page sends one at a time,
/// in order, so the last sent is the one kept.
#[tauri::command]
pub(crate) async fn keep_choice(kept: State<'_, Kept>, key: String, value: String) -> CmdResult<()> {
    let mut choices = kept.choices.lock().unwrap();
    choices::keep(&kept.path, &mut choices, &key, &value).map_err(err)
}
