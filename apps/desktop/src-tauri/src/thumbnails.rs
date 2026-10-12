//! Local copies of VOD thumbnails: fetched when a VOD is added (or at startup,
//! for VODs that don't have one yet) and handed to the UI over IPC.

use tauri::{AppHandle, Manager, State};
use vodscribe_core::thumbs;

use crate::{AppState, CmdResult, emit_vod, err};

/// A thumbnail's bytes, by the file name in `Vod::thumb`, and nothing else.
#[tauri::command]
pub(crate) async fn thumbnail(state: State<'_, AppState>, name: String) -> CmdResult<tauri::ipc::Response> {
    if thumbs::mime(&name).is_none() {
        return Err("Not a thumbnail.".into());
    }
    let bytes = std::fs::read(state.paths.thumbs().join(&name)).map_err(err)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Saves a local copy of VOD `id`'s thumbnail. A VOD with no usable image gets
/// an empty `thumb` so it isn't tried again; network failures are retried next launch.
pub(crate) fn cache_thumbnail(app: &AppHandle, id: i64) {
    let state = app.state::<AppState>();
    let Ok(Some(vod)) = state.db.lock().unwrap().vod(id) else { return };
    let thumb = match vod.info.thumbnail.filter(|u| !u.is_empty()) {
        None => String::new(),
        Some(url) => match thumbs::fetch(&url, &state.paths.thumbs(), id) {
            Ok(thumb) => thumb.unwrap_or_default(),
            Err(_) => return,
        },
    };
    let db = state.db.lock().unwrap();
    if db.set_thumb(id, &thumb).is_ok() {
        let _ = emit_vod(app, &db, id);
    }
}

/// Fills in local thumbnails for VODs that don't have one yet, such as those
/// added before thumbnails were stored locally.
pub(crate) fn cache_missing_thumbnails(app: AppHandle) {
    let state = app.state::<AppState>();
    let dir = state.paths.thumbs();
    let Ok(vods) = state.db.lock().unwrap().vods() else { return };
    for vod in vods {
        let missing = match vod.thumb.as_deref() {
            None => true,
            Some("") => false,
            Some(name) => !dir.join(name).is_file(),
        };
        if missing {
            cache_thumbnail(&app, vod.id);
        }
    }
}
