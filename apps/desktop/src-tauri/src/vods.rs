//! Commands on the library: adding, ordering and controlling VODs, and reading
//! their transcripts.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{Db, SearchHit, Status, Vod};
use vodscribe_core::{fetch, local, pipeline, thumbs, tools};

use crate::worker::{self, Stop};
use crate::{AppState, CmdResult, emit_vod, err, thumbnails};

#[derive(Serialize)]
pub(crate) struct Added {
    vod: Vod,
    created: bool,
}

#[tauri::command]
pub(crate) async fn add_vod(app: AppHandle, url: String) -> CmdResult<Added> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("Paste a VOD link first.".into());
    }
    let task_app = app.clone();
    let added = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Added> {
        let state = task_app.state::<AppState>();
        let yt_dlp = tools::find_yt_dlp(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!("yt-dlp isn't installed yet."))?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let probe_path = state.paths.work().join(format!("probe-{nanos}.json"));
        let info = fetch::probe(&yt_dlp, &url, &probe_path)?;

        let db = state.db.lock().unwrap();
        let (mut vod, created) = db.add_vod(&info)?;
        if created {
            // Hand the resolved info to the worker so it doesn't probe again.
            std::fs::rename(&probe_path, pipeline::info_path(&state.paths.work(), vod.id))?;
        } else {
            let _ = std::fs::remove_file(&probe_path);
            if vod.status == Status::Error {
                requeue(&db, vod.id)?;
                vod = db.vod(vod.id)?.unwrap_or(vod);
            }
        }
        Ok(Added { vod, created })
    })
    .await
    .map_err(err)?
    .map_err(err)?;

    let state = app.state::<AppState>();
    state.worker.wake();
    let _ = app.emit("vod", &added.vod);
    if added.created {
        let (app, id) = (app.clone(), added.vod.id);
        std::thread::spawn(move || thumbnails::cache_thumbnail(&app, id));
    }
    Ok(added)
}

#[derive(Serialize)]
pub(crate) struct AddedFiles {
    vods: Vec<Vod>,
    /// One message per file that couldn't be added.
    errors: Vec<String>,
}

/// Adds video or audio files on this computer, dropped on the window or picked
/// with the file dialog. They're transcribed in place and never modified.
#[tauri::command]
pub(crate) async fn add_files(app: AppHandle, paths: Vec<String>) -> CmdResult<AddedFiles> {
    let task_app = app.clone();
    let (added, errors) = tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        let mut added = Vec::new();
        let mut errors = Vec::new();
        for path in paths {
            match add_file(&state, Path::new(&path)) {
                Ok(vod) => added.push(vod),
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
        (added, errors)
    })
    .await
    .map_err(err)?;

    let state = app.state::<AppState>();
    state.worker.wake();
    for (vod, created) in &added {
        allow_playback(&app, vod);
        let _ = app.emit("vod", vod);
        if *created {
            let (app, id) = (app.clone(), vod.id);
            std::thread::spawn(move || thumbnails::cache_thumbnail(&app, id));
        }
    }
    Ok(AddedFiles { vods: added.into_iter().map(|(v, _)| v).collect(), errors })
}

fn add_file(state: &AppState, path: &Path) -> anyhow::Result<(Vod, bool)> {
    if local::is_inside(path, &state.paths.root) || local::is_inside(path, &state.paths.local) {
        anyhow::bail!("{} is part of VODscribe's own data.", path.display());
    }
    let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!("ffmpeg isn't installed."))?;
    let info = local::probe(&ffmpeg, path)?;
    let db = state.db.lock().unwrap();
    let (vod, created) = db.add_vod(&info)?;
    if created {
        db.set_source_path(vod.id, &info.url)?;
    } else if vod.status == Status::Error {
        requeue(&db, vod.id)?;
    }
    Ok((db.vod(vod.id)?.unwrap_or(vod), created))
}

/// Lets the player's <video> read a local VOD's file through the asset
/// protocol. Only files the user added are allowed, one by one.
pub(crate) fn allow_playback(app: &AppHandle, vod: &Vod) {
    if let Some(source) = &vod.source_path {
        let _ = app.asset_protocol_scope().allow_file(source);
    }
}

/// Shows a local VOD's file in the system file manager. Takes the VOD's id, not
/// a path, so the UI can't point it anywhere else.
#[tauri::command]
pub(crate) async fn show_file(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let source = state.db.lock().unwrap().vod(id).map_err(err)?.and_then(|v| v.source_path);
    let source = source.ok_or("That VOD isn't a file on this computer.")?;
    app.opener().reveal_item_in_dir(source).map_err(err)
}

#[tauri::command]
pub(crate) async fn list_vods(state: State<'_, AppState>) -> CmdResult<Vec<Vod>> {
    state.db.lock().unwrap().vods().map_err(err)
}

#[tauri::command]
pub(crate) async fn reorder_vods(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<()> {
    state.db.lock().unwrap().reorder(&ids).map_err(err)
}

#[tauri::command]
pub(crate) async fn delete_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    // A running stage removes the files once its process has stopped.
    let delete_row = || state.db.lock().unwrap().delete_vod(id);
    let running = state
        .worker
        .control(id, Stop::Delete, || {
            pipeline::remove_work_files(&state.paths.work(), id);
            delete_row()
        })
        .map_err(err)?;
    if running {
        delete_row().map_err(err)?;
    }
    thumbs::remove(&state.paths.thumbs(), id);
    let _ = app.emit("vod-removed", id);
    Ok(())
}

/// Requeues a VOD at whichever stage it needs next.
pub(crate) fn requeue(db: &Db, id: i64) -> anyhow::Result<()> {
    let downloaded = db.audio_path(id)?.is_some_and(|p| std::path::Path::new(&p).is_file());
    let progress = db.vod(id)?.map_or(0.0, |v| if v.status == Status::Paused { v.progress } else { 0.0 });
    let status = if downloaded { Status::Ready } else { Status::Queued };
    db.set_status(id, status, progress, None)
}

#[tauri::command]
pub(crate) async fn retry_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    requeue(&db, id).map_err(err)?;
    emit_vod(&app, &db, id)?;
    state.worker.wake();
    Ok(())
}

/// Throws away a finished VOD's transcript and transcribes it again, e.g. with
/// a better model.
#[tauri::command]
pub(crate) async fn retranscribe_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    match db.vod(id).map_err(err)? {
        Some(vod) if matches!(vod.status, Status::Done | Status::Error) => {}
        _ => return Err("Only finished VODs can be re-transcribed.".into()),
    }
    db.atomically(|db| {
        db.clear_segments(id)?;
        db.set_model(id, None)?;
        db.set_note(id, None)?;
        requeue(db, id)
    })
    .map_err(err)?;
    emit_vod(&app, &db, id)?;
    state.worker.wake();
    Ok(())
}

/// Stops a VOD, keeping its partial download and transcript.
#[tauri::command]
pub(crate) async fn pause_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    // A running job is paused by its stage once its process has stopped.
    state
        .worker
        .control(id, Stop::Pause, || {
            let db = state.db.lock().unwrap();
            if let Some(vod) = db.vod(id)?
                && matches!(vod.status, Status::Queued | Status::Ready)
            {
                db.set_status(id, Status::Paused, vod.progress, None)?;
            }
            Ok(())
        })
        .map_err(err)?;
    emit_vod(&app, &state.db.lock().unwrap(), id)
}

#[tauri::command]
pub(crate) async fn resume_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    if db.vod(id).map_err(err)?.is_some_and(|v| v.status == Status::Paused) {
        requeue(&db, id).map_err(err)?;
        emit_vod(&app, &db, id)?;
        state.worker.wake();
    }
    Ok(())
}

/// Stops a VOD and discards its partial work; Retry then starts over.
#[tauri::command]
pub(crate) async fn cancel_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state
        .worker
        .control(id, Stop::Cancel, || worker::discard(&state.db.lock().unwrap(), &state.paths.work(), id))
        .map_err(err)?;
    emit_vod(&app, &state.db.lock().unwrap(), id)
}

#[derive(Serialize)]
pub(crate) struct SearchResponse {
    hits: Vec<SearchHit>,
    truncated: bool,
}

#[tauri::command]
pub(crate) async fn search(state: State<'_, AppState>, query: String, vod_id: Option<i64>) -> CmdResult<SearchResponse> {
    let r = state.db.lock().unwrap().search(&query, vod_id, 500).map_err(err)?;
    Ok(SearchResponse { hits: r.hits, truncated: r.truncated })
}

#[derive(Serialize)]
pub(crate) struct Line {
    start: f64,
    text: String,
}

#[tauri::command]
pub(crate) async fn transcript(state: State<'_, AppState>, vod_id: i64) -> CmdResult<Vec<Line>> {
    let lines = state.db.lock().unwrap().lines(vod_id).map_err(err)?;
    Ok(lines.into_iter().map(|(start, text)| Line { start, text }).collect())
}

/// Playable quality levels for a Kick VOD; Kick has no embeddable VOD player.
#[tauri::command]
pub(crate) async fn kick_stream(url: String) -> CmdResult<Vec<vodscribe_core::kick::Variant>> {
    tauri::async_runtime::spawn_blocking(move || vodscribe_core::kick::variants(&url))
        .await
        .map_err(err)?
        .map_err(err)
}
