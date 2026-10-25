//! Importing annotations for a VOD (`vodscribe_core::annotations`): from the
//! page's parsed file, or from a file picked; and reading a VOD's cast and
//! activity back for the page.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use vodscribe_core::annotations::{self, VodAnnotations};
use vodscribe_core::chat::ChatMessage;
use vodscribe_core::db::Vod;

use crate::history::Doc;
use crate::{AppState, CmdResult, emit_vod, err};

#[derive(Serialize)]
pub(crate) struct Imported {
    vod: Vod,
    /// Added to the library by the import.
    created: bool,
}

/// Makes `annotations` (an annotations file as the page parsed it) the
/// VOD's it names, with `chat` (its chat file) as its chat if given: adding
/// the VOD if the library doesn't have it. What an earlier import brought is
/// replaced; the user's own marks stay.
#[tauri::command]
pub(crate) async fn import_annotations(
    app: AppHandle,
    annotations: serde_json::Value,
    chat: Option<Vec<ChatMessage>>,
) -> CmdResult<Imported> {
    let annotations = annotations::from_value(annotations).map_err(err)?;
    import(app, annotations, chat).await
}

/// As `import_annotations`, from file `path` (picked in the file dialog),
/// with the chat file beside it if there's one.
#[tauri::command]
pub(crate) async fn import_annotations_file(app: AppHandle, path: String) -> CmdResult<Imported> {
    let read = tauri::async_runtime::spawn_blocking(move || annotations::read(Path::new(&path), None));
    let (annotations, chat) = read.await.map_err(err)?.map_err(err)?;
    import(app, annotations, chat).await
}

async fn import(
    app: AppHandle,
    annotations: annotations::Annotations,
    chat: Option<Vec<ChatMessage>>,
) -> CmdResult<Imported> {
    let task_app = app.clone();
    let brings_chat = chat.is_some();
    let (vod, created) = tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        let db = state.db.lock().unwrap();
        db.import_annotations(&annotations, chat.as_deref())
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let state = app.state::<AppState>();
    // Undo there would put marks back from before the import, beside a
    // transcript it doesn't bring back.
    state.history.forget(Doc::Vod(vod.id));
    if brings_chat {
        // A fetch of the site's chat under way would add to the one imported.
        state.chat.stop(vod.id);
    } else if created && vod.chat == vodscribe_core::db::ChatStatus::Pending {
        // No chat came with it: fetched from the site, as for a VOD added by its link.
        state.chat.push(vod.id);
    }
    emit_vod(&app, &state.db.lock().unwrap(), vod.id)?;
    Ok(Imported { vod, created })
}

/// VOD `vod_id`'s cast and activity, if annotations were imported for it.
#[tauri::command]
pub(crate) async fn vod_annotations(app: AppHandle, vod_id: i64) -> CmdResult<Option<VodAnnotations>> {
    crate::with_reads(app, move |db| db.vod_annotations(vod_id)).await
}
