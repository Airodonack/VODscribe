//! Edits (`vodscribe_core::edits`): the commands the Edit view makes and
//! changes them with, and the queue that renders them, one at a time, into
//! the Videos folder beside the clips (VODscribe/Edits). Rendering again
//! replaces the same file, so a video editor linked to it gets the new one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{ClipStatus, Db};
use vodscribe_core::edits::{self, Edit, EditPiece};
use vodscribe_core::{clips, render, tools};

use crate::{AppState, CmdResult, err};

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The edit being rendered, and its stop.
type Running = Arc<Mutex<Option<(i64, Arc<AtomicBool>)>>>;

pub(crate) struct Renderer {
    queue: Sender<i64>,
    /// Taken by `run`.
    jobs: Mutex<Option<Receiver<i64>>>,
    current: Running,
}

impl Renderer {
    pub(crate) fn new() -> Self {
        let (queue, jobs) = mpsc::channel();
        Self { queue, jobs: Mutex::new(Some(jobs)), current: Arc::default() }
    }

    /// Starts rendering what's asked, once the app's state is in place.
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        let jobs = self.jobs.lock().unwrap().take().context("the render queue is already running")?;
        app.state::<AppState>().db.lock().unwrap().reset_renders()?;
        let running = self.current.clone();
        std::thread::Builder::new().name("renders".into()).spawn(move || {
            for id in jobs {
                let stop = Arc::new(AtomicBool::new(false));
                *running.lock().unwrap() = Some((id, stop.clone()));
                if let Err(e) = render_edit(&app, id, &stop) {
                    eprintln!("edit {id}: {e:#}");
                }
                *running.lock().unwrap() = None;
            }
        })?;
        Ok(())
    }

    fn stop(&self, id: i64) {
        if let Some((current, stop)) = &*self.current.lock().unwrap()
            && *current == id
        {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Serialize, Clone)]
struct Progress {
    id: i64,
    progress: f64,
}

fn emit(app: &AppHandle, db: &Db, id: i64) {
    if let Ok(Some(edit)) = db.edit(id) {
        let _ = app.emit("edit", edit);
    }
}

/// Where edit videos go: Edits beside the clips' folders.
fn edits_dir(app: &AppHandle) -> PathBuf {
    crate::clips::clips_dir(app).join("Edits")
}

/// Renders edit `id` into its file.
fn render_edit(app: &AppHandle, id: i64, stop: &AtomicBool) -> Result<()> {
    let state = app.state::<AppState>();
    let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).context(crate::worker::NO_FFMPEG);
    let prepared = (|| {
        let db = state.db.lock().unwrap();
        let edit = db.edit(id)?.context("That edit is gone.")?;
        let version = db.edit_version(id)?;
        let pieces = edits::render_pieces(&db.edit_pieces(id)?)?;
        let ffmpeg = ffmpeg?;
        // The same file as last time, if it's still named for the edit.
        let name = format!("{}.mp4", clips::safe_name(&edit.title));
        let out = match &edit.file_path {
            Some(path) if Path::new(path).file_name().is_some_and(|n| *n == *name) => PathBuf::from(path),
            _ => clips::unique_path(&edits_dir(app), &name),
        };
        db.set_edit_render(id, ClipStatus::Cutting, None, None, None)?;
        emit(app, &db, id);
        anyhow::Ok((pieces, ffmpeg, out, version))
    })();
    let (pieces, ffmpeg, out, version) = match prepared {
        Ok(ready) => ready,
        Err(e) => {
            let db = state.db.lock().unwrap();
            db.set_edit_render(id, ClipStatus::Error, None, Some(&format!("{e:#}")), None)?;
            emit(app, &db, id);
            return Ok(());
        }
    };
    let mut last = Instant::now();
    let report = |progress: f64| {
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            let _ = app.emit("edit-progress", Progress { id, progress });
        }
    };
    // Sound alone makes an M4A; the name says MP4 either way, which players read.
    let result = render::render(&ffmpeg, &pieces, None, &out, stop, report);
    let db = state.db.lock().unwrap();
    let path = out.to_string_lossy();
    match &result {
        Ok(()) => db.set_edit_render(id, ClipStatus::Ready, Some(&path), None, version)?,
        // Stopped, or deleted: as it was before, if it's still there.
        Err(e) if vodscribe_core::is_canceled(e) => db.set_edit_render(id, ClipStatus::Waiting, None, None, None)?,
        Err(e) => db.set_edit_render(id, ClipStatus::Error, None, Some(&format!("{e:#}")), None)?,
    }
    if result.is_ok() && db.edit(id)?.is_none() {
        let _ = std::fs::remove_file(&out);
    }
    emit(app, &db, id);
    Ok(())
}

/// Edit `id` and its pieces, for the Edit view.
#[derive(Serialize)]
pub(crate) struct EditView {
    edit: Edit,
    pieces: Vec<EditPiece>,
}

fn view(app: &AppHandle, db: &Db, id: i64) -> CmdResult<EditView> {
    let edit = db.edit(id).map_err(err)?.ok_or("That edit is gone.")?;
    let pieces = db.edit_pieces(id).map_err(err)?;
    let _ = app.emit("edit", edit.clone());
    Ok(EditView { edit, pieces })
}

/// Runs `change` on the library, then gives edit `edit_id` as it is now.
fn changing(app: &AppHandle, edit_id: i64, change: impl FnOnce(&Db) -> Result<()>) -> CmdResult<EditView> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    change(&db).map_err(err)?;
    view(app, &db, edit_id)
}

/// The edit piece `id` is in.
fn edit_of(app: &AppHandle, id: i64) -> CmdResult<i64> {
    let state = app.state::<AppState>();
    let edit = state.db.lock().unwrap().piece_edit(id).map_err(err)?;
    edit.ok_or_else(|| "That piece is gone.".into())
}

/// A title as edits keep it: trimmed, at most 200 characters.
fn title(text: &str) -> CmdResult<String> {
    let title: String = text.trim().chars().take(200).collect();
    if title.is_empty() { Err("An edit needs a name.".into()) } else { Ok(title) }
}

#[tauri::command]
pub(crate) async fn list_edits(app: AppHandle) -> CmdResult<Vec<Edit>> {
    crate::with_reads(app, |db| db.edits()).await
}

/// A new, empty edit, named "Edit 1", "Edit 2" and so on.
#[tauri::command]
pub(crate) async fn create_edit(state: State<'_, AppState>) -> CmdResult<Edit> {
    let db = state.db.lock().unwrap();
    let taken: Vec<String> = db.edits().map_err(err)?.into_iter().map(|e| e.title).collect();
    let name = (1..).map(|n| format!("Edit {n}")).find(|n| !taken.contains(n)).expect("a free name");
    db.add_edit(&name).map_err(err)
}

#[tauri::command]
pub(crate) async fn get_edit(app: AppHandle, id: i64) -> CmdResult<EditView> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    view(&app, &db, id)
}

#[tauri::command]
pub(crate) async fn rename_edit(app: AppHandle, id: i64, title: String) -> CmdResult<EditView> {
    let title = self::title(&title)?;
    changing(&app, id, |db| db.rename_edit(id, &title))
}

/// Deletes edit `id`, and its video if it was rendered.
#[tauri::command]
pub(crate) async fn delete_edit(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.renders.stop(id);
    let file = {
        let db = state.db.lock().unwrap();
        let file = db.edit(id).map_err(err)?.and_then(|e| e.file_path);
        db.delete_edit(id).map_err(err)?;
        file
    };
    if let Some(file) = file {
        let _ = std::fs::remove_file(file);
    }
    let _ = app.emit("edit-removed", id);
    Ok(())
}

/// Puts all of clip `clip_id` in edit `edit_id`, at `index` (or last).
#[tauri::command]
pub(crate) async fn add_to_edit(
    app: AppHandle,
    edit_id: i64,
    clip_id: i64,
    index: Option<usize>,
) -> CmdResult<EditView> {
    changing(&app, edit_id, |db| {
        db.edit(edit_id)?.context("That edit is gone.")?;
        db.add_piece(edit_id, clip_id, index)
    })
}

#[tauri::command]
pub(crate) async fn move_piece(app: AppHandle, id: i64, index: usize) -> CmdResult<EditView> {
    let edit = edit_of(&app, id)?;
    changing(&app, edit, |db| db.move_piece(id, index))
}

/// Trims piece `id` to `start`..`end` of its VOD (within its clip).
#[tauri::command]
pub(crate) async fn trim_piece(app: AppHandle, id: i64, start: f64, end: f64) -> CmdResult<EditView> {
    if !(start.is_finite() && end.is_finite() && end > start) {
        return Err("A piece has to end after it starts.".into());
    }
    let edit = edit_of(&app, id)?;
    changing(&app, edit, |db| db.set_piece_range(id, start, end))
}

/// Sets the stretches of piece `id` (in its VOD's time) to jump over.
#[tauri::command]
pub(crate) async fn strike_piece(app: AppHandle, id: i64, struck: Vec<(f64, f64)>) -> CmdResult<EditView> {
    if struck.iter().any(|(a, b)| !(a.is_finite() && b.is_finite())) || struck.len() > 10_000 {
        return Err("Those aren't times in the video.".into());
    }
    let edit = edit_of(&app, id)?;
    changing(&app, edit, |db| db.set_piece_struck(id, &struck))
}

#[tauri::command]
pub(crate) async fn remove_piece(app: AppHandle, id: i64) -> CmdResult<EditView> {
    let edit = edit_of(&app, id)?;
    changing(&app, edit, |db| db.remove_piece(id))
}

/// Renders edit `id` (after any being rendered).
#[tauri::command]
pub(crate) async fn render_edit_now(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let _ = state.renders.queue.send(id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn stop_render(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.renders.stop(id);
    Ok(())
}

/// Edit `id`'s rendered video, if it's there.
fn rendered(app: &AppHandle, id: i64) -> CmdResult<PathBuf> {
    let state = app.state::<AppState>();
    let edit = state.db.lock().unwrap().edit(id).map_err(err)?.ok_or("That edit is gone.")?;
    edit.file_path.map(PathBuf::from).filter(|p| p.is_file()).ok_or_else(|| "This edit isn't rendered yet.".into())
}

/// Shows edit `id`'s video in the system file manager.
#[tauri::command]
pub(crate) async fn show_edit(app: AppHandle, id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = rendered(&app, id)?;
    app.opener().reveal_item_in_dir(path).map_err(err)
}

/// Starts dragging edit `id`'s video out of the window, as for a clip
/// (`clips::drag_clip`).
#[tauri::command]
pub(crate) async fn drag_edit(app: AppHandle, window: tauri::WebviewWindow, id: i64) -> CmdResult<()> {
    let path = rendered(&app, id)?;
    crate::clips::drag_file(window, path, None).await
}
