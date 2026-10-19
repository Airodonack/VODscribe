//! Local copies of VOD thumbnails: fetched when a VOD is added (or at startup,
//! for VODs that don't have one yet) and handed to the UI over IPC.

use std::path::Path;

use tauri::{AppHandle, Manager, State};
use vodscribe_core::{local, thumbs, tools};

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

/// How long a site's image that can't be fetched (offline, a server error) is
/// tried again, at each launch, before it's given up on.
const RETRY_FOR_SECONDS: i64 = 7 * 24 * 3600;

/// Saves a local copy of VOD `id`'s thumbnail: a frame of a local file, or the
/// platform's image. A VOD with no usable image gets an empty `thumb` so it
/// isn't tried again; network failures are retried next launch, for a week
/// after it was added (`RETRY_FOR_SECONDS`).
pub(crate) fn cache_thumbnail(app: &AppHandle, id: i64) {
    let state = app.state::<AppState>();
    let Ok(Some(vod)) = state.db.lock().unwrap().vod(id) else { return };
    let thumb = if let Some(source) = &vod.source_path {
        let Some(ffmpeg) = tools::find_ffmpeg(&state.paths.bin()) else { return };
        let frame = local::thumbnail(&ffmpeg, Path::new(source), &state.paths.thumbs(), id, vod.info.duration);
        frame.ok().flatten().unwrap_or_default()
    } else {
        match vod.info.thumbnail.filter(|u| !u.is_empty()) {
            None => String::new(),
            Some(url) => match thumbs::fetch(&url, &state.paths.thumbs(), id) {
                Ok(thumb) => thumb.unwrap_or_default(),
                Err(_) if now() - vod.added_at > RETRY_FOR_SECONDS => String::new(),
                Err(_) => return,
            },
        }
    };
    let db = state.db.lock().unwrap();
    if db.set_thumb(id, &thumb).is_ok() {
        let _ = emit_vod(app, &db, id);
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
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
