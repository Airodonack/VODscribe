//! Edits (`vodscribe_core::edits`): the commands the Edit view makes and
//! changes them with, and their rendering, a job of the clips' queue
//! (`clips::Cutter`, one encode at a time), into the Videos folder beside
//! the clips (VODscribe/Edits). Rendering again
//! replaces the same file, so a video editor linked to it gets the new one;
//! renamed, it's a new file, and the one before goes unless it was dragged
//! out to another app.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{ClipStatus, Db};
use vodscribe_core::edits::{self, Edit, EditPiece};
use vodscribe_core::settings::Settings;
use vodscribe_core::{clips, render, tools};

use crate::history::{self, Doc};
use crate::{AppState, CmdResult, err};

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Serialize, Clone)]
struct Progress {
    id: i64,
    progress: f64,
}

/// News of edit `id`; of a snippet open in the editor, for its clip bar too.
pub(crate) fn emit(app: &AppHandle, db: &Db, id: i64) {
    if let Ok(Some(edit)) = db.edit(id) {
        let snippet = edit.snippet_of.is_some();
        let _ = app.emit("edit", edit);
        if snippet && let Ok(Some(clip)) = db.clip(id) {
            let _ = app.emit("clip", clip);
        }
    }
}

/// Snippet `id` changed in the editor (or by undo there): its file is made
/// again, as a clip moved in its clip bar is. Nothing for a listed clip,
/// which renders when asked.
pub(crate) fn remake_snippet(app: &AppHandle, db: &Db, id: i64) {
    if db.is_snippet(id).ok().flatten() != Some(true) {
        return;
    }
    // Its file as it is if what it's made of hasn't changed (a highlight or
    // a cut added, say), unless that file's gone.
    if let Ok(Some(edit)) = db.edit(id)
        && edit.current
        && edit.status == ClipStatus::Ready
        && edit.file_path.as_deref().is_some_and(|p| Path::new(p).is_file())
    {
        return;
    }
    let state = app.state::<AppState>();
    state.clips.stop(id);
    if db.set_edit_render(id, ClipStatus::Waiting, None, None, None).is_ok() {
        emit(app, db, id);
        // Once: changes made one after another remake it once, not once each.
        state.clips.queue_once(id);
    }
}

/// Where edit videos go: Edits beside the clips' folders.
fn edits_dir(app: &AppHandle) -> PathBuf {
    crate::clips::clips_dir(app).join("Edits")
}

/// Edit `id`'s pieces, each a hole if its footage is gone for good: its VOD
/// is gone (deleted without keeping what clips use, or a file on this
/// computer that isn't where it was: `footage::lost`), and what's kept here
/// doesn't hold all of the piece (what it holds still plays and renders).
pub(crate) fn pieces(app: &AppHandle, db: &Db, id: i64) -> Result<Vec<EditPiece>> {
    let mut pieces = db.edit_pieces(id)?;
    for p in &mut pieces {
        p.hole = crate::footage::lost(db, p.vod_id) && !all_here(app, db, p);
    }
    Ok(pieces)
}

/// Whether all of piece `p`'s footage is here (from however many files).
fn all_here(app: &AppHandle, db: &Db, p: &EditPiece) -> bool {
    p.spans().iter().all(|&(a, b)| edits::covers(&crate::footage::cover(app, db, p.vod_id, a, b), a, b))
}

/// The stretches (VOD, start, end) of `pieces` whose footage isn't here: of
/// each piece some of whose spans have none, all of it. Not holes': theirs
/// can't be had.
pub(crate) fn missing(app: &AppHandle, db: &Db, pieces: &[EditPiece]) -> Vec<(i64, f64, f64)> {
    pieces.iter().filter(|p| !p.hole && !all_here(app, db, p)).map(|p| (p.vod_id, p.start, p.end)).collect()
}

/// Renders edit `id` into its file, fetching any footage it needs first
/// (a job of the clips' queue: `clips::Cutter`).
pub(crate) fn render_edit(app: &AppHandle, id: i64, stop: &AtomicBool) -> Result<()> {
    let state = app.state::<AppState>();
    // A snippet whose file is of it as it is already (made for a change
    // undone since, say) isn't made again.
    {
        let db = state.db.lock().unwrap();
        if db.is_snippet(id)? == Some(true)
            && let Some(edit) = db.edit(id)?
            && edit.current
            && edit.file_path.as_deref().is_some_and(|p| Path::new(p).is_file())
        {
            db.set_edit_render(id, ClipStatus::Ready, None, None, None)?;
            emit(app, &db, id);
            return Ok(());
        }
    }
    let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).context(crate::worker::NO_FFMPEG);
    let prepared = (|| {
        let missing = {
            let db = state.db.lock().unwrap();
            db.set_edit_render(id, ClipStatus::Cutting, None, None, None)?;
            emit(app, &db, id);
            missing(app, &db, &pieces(app, &db, id)?)
        };
        for (vod_id, start, end) in missing {
            crate::footage::ensure(app, vod_id, start, end, stop, &|_| {})?;
        }
        let db = state.db.lock().unwrap();
        let edit = db.edit(id)?.context("That edit is gone.")?;
        let version = db.revision(id)?;
        let edit_pieces = pieces(app, &db, id)?;
        // What it's made of, to know later whether a change touched it.
        let made_of = db.made_of(id, &edit.title, &edit_pieces)?;
        let chapters = db.file_chapters(id, &edit_pieces)?;
        let pieces = edits::render_pieces(&edit_pieces, |vod, a, b| crate::footage::cover(app, &db, vod, a, b))?;
        let ffmpeg = ffmpeg?;
        // Sound alone if every piece's VOD is, named for the format clips are made in.
        let format = Settings::load(&state.paths.settings()).clip_format;
        let audio_only = edit_pieces.iter().all(|p| db.vod(p.vod_id).ok().flatten().is_some_and(|v| v.audio_only));
        // The same file as last time, if it's still named for the edit (its
        // extension may be the one for sound alone, found only by rendering).
        let name = format!("{}.{}", clips::safe_name(&edit.title), clips::extension(audio_only, format));
        let out = match &edit.file_path {
            // (render() gives it the extension for what it holds: the same, written over.)
            Some(path) if Path::new(path).file_stem() == Path::new(&name).file_stem() => PathBuf::from(path),
            // A snippet's goes with the clips; a listed clip's in Edits.
            _ if edit.snippet_of.is_some() => clips::unique_path(&crate::clips::clips_dir(app), &name),
            _ => clips::unique_path(&edits_dir(app), &name),
        };
        crate::files::writing(&out);
        anyhow::Ok((pieces, ffmpeg, out, version, format, edit.file_path, made_of, chapters))
    })();
    let (pieces, ffmpeg, out, version, format, before, made_of, chapters) = match prepared {
        Ok(ready) => ready,
        Err(e) => {
            let db = state.db.lock().unwrap();
            if vodscribe_core::is_canceled(&e) {
                db.set_edit_render(id, ClipStatus::Waiting, None, None, None)?;
            } else {
                db.set_edit_render(id, ClipStatus::Error, None, Some(&format!("{e:#}")), None)?;
            }
            emit(app, &db, id);
            return Ok(());
        }
    };
    let mut last = Instant::now();
    let report = |progress: f64| {
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            let _ = app.emit("edit-progress", Progress { id, progress });
            crate::job_list::progressed(app, id, progress);
        }
    };
    let result = render::render(&ffmpeg, &pieces, None, &chapters, format, &out, stop, report);
    // As made: sound alone is named so (an .m4a or .wav), found only then.
    let out = result.as_ref().map_or(out, |made| made.clone());
    let db = state.db.lock().unwrap();
    let path = out.to_string_lossy();
    match &result {
        Ok(_) => {
            db.set_edit_render(id, ClipStatus::Ready, Some(&path), None, version)?;
            db.set_made_of(id, &made_of)?;
        }
        // Stopped, or deleted: as it was before, if it's still there.
        Err(e) if vodscribe_core::is_canceled(e) => db.set_edit_render(id, ClipStatus::Waiting, None, None, None)?,
        Err(e) => db.set_edit_render(id, ClipStatus::Error, None, Some(&format!("{e:#}")), None)?,
    }
    if result.is_ok() && db.edit(id)?.is_none() {
        crate::files::delete_file(&out);
    }
    // The video made before under another name (renamed, or the other format)
    // goes, unless another app took it.
    if let Some(old) = before.filter(|p| result.is_ok() && Path::new(p) != out)
        && !db.edit_handed_out(id, true)?
    {
        crate::files::delete_file(old);
    }
    emit(app, &db, id);
    // The render may have been what held files deleted meanwhile.
    crate::files::delete_leftovers();
    Ok(())
}

/// Edit `id` and its pieces, for the Edit view.
#[derive(Serialize)]
pub(crate) struct EditView {
    edit: Edit,
    pieces: Vec<EditPiece>,
}

pub(crate) fn view(app: &AppHandle, db: &Db, id: i64) -> CmdResult<EditView> {
    let edit = db.edit(id).map_err(err)?.ok_or("That edit is gone.")?;
    let pieces = pieces(app, db, id).map_err(err)?;
    let _ = app.emit("edit", edit.clone());
    Ok(EditView { edit, pieces })
}

/// Runs `change` on the library, recording it in edit `edit_id`'s history as
/// `label` (`history::change`: all of it, or none), then gives the edit as it
/// is now.
fn changing(
    app: &AppHandle,
    edit_id: i64,
    label: &'static str,
    change: impl FnOnce(&Db) -> Result<()>,
) -> CmdResult<EditView> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    history::change(app, &db, Doc::Edit(edit_id), label, change).map_err(err)?;
    view(app, &db, edit_id)
}

/// The edit piece `id` is in.
fn edit_of(app: &AppHandle, id: i64) -> CmdResult<i64> {
    let state = app.state::<AppState>();
    let edit = state.db.lock().unwrap().piece_edit(id).map_err(err)?;
    edit.ok_or_else(|| "That piece is gone.".into())
}

/// A title as edits keep it (`clips::written_title`).
fn title(text: &str) -> CmdResult<String> {
    clips::written_title(text).ok_or_else(|| "An edit needs a name.".into())
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

/// Edit (any clip) `id`'s captions, on its timeline: its pieces' VODs'
/// captions where they play (`vodscribe_core::edits::clip_cues`).
#[tauri::command]
pub(crate) async fn edit_captions(app: AppHandle, id: i64) -> CmdResult<Vec<vodscribe_core::transcript::Cue>> {
    crate::with_reads(app, move |db| {
        let pieces = db.edit_pieces(id)?;
        vodscribe_core::edits::clip_cues(&pieces, |vod| db.segments(vod))
    })
    .await
}

/// Edit (any clip) `id`'s transcript, on its timeline: its pieces' VODs'
/// lines where they play, each with only the words said there.
#[tauri::command]
pub(crate) async fn edit_lines(app: AppHandle, id: i64) -> CmdResult<Vec<crate::vods::Line>> {
    crate::with_reads(app, move |db| {
        let pieces = db.edit_pieces(id)?;
        let lines = vodscribe_core::edits::clip_segments(&pieces, |vod| db.segments(vod))?;
        Ok(lines
            .into_iter()
            .map(|s| crate::vods::Line { start: s.start, end: s.end, text: s.text, speaker: None })
            .collect())
    })
    .await
}

/// Takes `from`..`to` (seconds of its timeline) out of edit `id`: the
/// pieces it reaches split at its ends and what's between goes, its cuts
/// there made a real split, as one change.
#[tauri::command]
pub(crate) async fn remove_stretch(app: AppHandle, id: i64, from: f64, to: f64) -> CmdResult<EditView> {
    changing(&app, id, "Stretch removed", |db| db.remove_edit_stretch(id, from, to))
}

#[tauri::command]
pub(crate) async fn rename_edit(app: AppHandle, id: i64, title: String) -> CmdResult<EditView> {
    let title = self::title(&title)?;
    changing(&app, id, "Edit renamed", |db| db.rename_edit(id, &title))
}

/// Deletes edit `id`, and its video if it was rendered; the clips on its
/// clip bar go with it, as a VOD's do.
#[tauri::command]
pub(crate) async fn delete_edit(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let gone = {
        let db = state.db.lock().unwrap();
        let gone = delete_edit_in(&db, id, |clip| state.clips.stop(clip)).map_err(err)?;
        state.history.forget(Doc::Edit(id));
        gone
    };
    // Its render stops (one that ends now finds it gone, and deletes its file).
    state.clips.cancel(id);
    if let Some(file) = gone.file {
        crate::files::delete_file(file);
    }
    // Its clips' files once they're out of the library, as `clips::remove`
    // does for one.
    for (clip, handed_out) in gone.clips {
        crate::clips::after_delete(&app, clip.id, clip.file_path, handed_out);
    }
    let _ = app.emit("edit-removed", id);
    Ok(())
}

/// What `delete_edit` takes away with an edit: its video's file, unless
/// another app took it (dragged out), and the clips that were on its clip
/// bar, each with whether its file was dragged out (another app's then).
pub(crate) struct EditGone {
    pub file: Option<String>,
    pub clips: Vec<(vodscribe_core::db::Clip, bool)>,
}

/// What `delete_edit` does in the library: edit `id` deleted with the clips
/// on its clip bar, in one transaction (all of it goes, or none of it does),
/// then `stop` given each clip's id (its file being made is stopped). An
/// error if it isn't an edit.
pub(crate) fn delete_edit_in(db: &Db, id: i64, stop: impl Fn(i64)) -> Result<EditGone> {
    let gone = db.atomically(|db| {
        let file = db.edit(id)?.and_then(|e| e.file_path);
        let handed_out = db.edit_handed_out(id, false)?;
        let mut clips = Vec::new();
        for clip in db.bar_clips(id)? {
            let out = db.clip_handed_out(clip.id)?;
            db.delete_clip(clip.id)?;
            clips.push((clip, out));
        }
        anyhow::ensure!(db.delete_edit(id)?, "That isn't an edit.");
        Ok(EditGone { file: file.filter(|_| !handed_out), clips })
    })?;
    for (clip, _) in &gone.clips {
        stop(clip.id);
    }
    Ok(gone)
}

/// Puts a copy of clip `clip_id` in edit `edit_id` as a group, at `index`
/// (or last): shown as one, its pieces as the clip has them. It's the
/// edit's own from then on.
#[tauri::command]
pub(crate) async fn add_to_edit(
    app: AppHandle,
    edit_id: i64,
    clip_id: i64,
    index: Option<usize>,
) -> CmdResult<EditView> {
    changing(&app, edit_id, "Piece added", |db| {
        db.edit(edit_id)?.context("That edit is gone.")?;
        db.add_group(edit_id, clip_id, index).map(|_| ())
    })
}

/// Puts `start`..`end` of VOD `vod_id` in edit `edit_id`, at `index` (or
/// last), named `title` (a clip's it was), or for what's said in it as a clip
/// would be.
#[tauri::command]
pub(crate) async fn add_range_to_edit(
    app: AppHandle,
    edit_id: i64,
    vod_id: i64,
    start: f64,
    end: f64,
    title: Option<String>,
    index: Option<usize>,
) -> CmdResult<EditView> {
    let title = title.as_deref().and_then(clips::written_title);
    changing(&app, edit_id, "Piece added", |db| {
        db.edit(edit_id)?.context("That edit is gone.")?;
        db.vod(vod_id)?.context("That VOD is gone.")?;
        let title = match title {
            Some(title) => title,
            None => clips::title(&db.words_between(vod_id, start, end)?, start),
        };
        db.add_piece(edit_id, vod_id, (start, end), &title, index)
    })
}

/// Puts `items` (several things picked) in edit `edit_id`, in their order,
/// as one change undo takes back whole: on its clip bar if `bar`, else on its
/// timeline at `index` of its top level (or last). The clips made on the bar
/// are queued to be made.
#[tauri::command]
pub(crate) async fn put_in_edit(
    app: AppHandle,
    edit_id: i64,
    items: Vec<edits::Put>,
    bar: bool,
    index: Option<usize>,
) -> CmdResult<EditView> {
    let onto = if bar { edits::Onto::Bar } else { edits::Onto::Timeline(index) };
    let label = match (bar, items.len()) {
        (true, 1) => "Clip added",
        (true, _) => "Clips added",
        (false, 1) => "Piece added",
        (false, _) => "Pieces added",
    };
    let mut made = Vec::new();
    let view = changing(&app, edit_id, label, |db| {
        made = db.put(edit_id, &items, onto)?;
        Ok(())
    })?;
    let state = app.state::<AppState>();
    for id in made {
        state.clips.queue(id);
    }
    Ok(view)
}

#[tauri::command]
pub(crate) async fn move_piece(app: AppHandle, id: i64, index: usize) -> CmdResult<EditView> {
    let edit = edit_of(&app, id)?;
    changing(&app, edit, "Piece moved", |db| db.move_piece(id, index))
}

/// Trims piece `id` to `start`..`end` of its VOD.
#[tauri::command]
pub(crate) async fn trim_piece(app: AppHandle, id: i64, start: f64, end: f64) -> CmdResult<EditView> {
    let edit = edit_of(&app, id)?;
    changing(&app, edit, "Piece resized", |db| db.set_piece_range(id, start, end))
}

/// Splits piece `id` in two at `at` seconds of its VOD. No control does
/// this now (a delete between cuts splits pieces); the e2e tests make
/// pieces with it.
#[tauri::command]
pub(crate) async fn split_piece(app: AppHandle, id: i64, at: f64) -> CmdResult<EditView> {
    let edit = edit_of(&app, id)?;
    changing(&app, edit, "Piece split", |db| {
        if !db.split_piece(id, at)? {
            anyhow::bail!("That's not inside the piece.");
        }
        Ok(())
    })
}

#[tauri::command]
pub(crate) async fn remove_piece(app: AppHandle, id: i64) -> CmdResult<EditView> {
    let edit = edit_of(&app, id)?;
    changing(&app, edit, "Piece removed", |db| db.remove_piece(id))
}

/// Takes several things out of edit `id` at once (selected on its timeline
/// and its clip bar), as one change: the stretches `stretches` (seconds of
/// its timeline, as `remove_stretch` takes one) and the pieces `pieces` (as
/// `remove_piece`), `Db::remove_from_edit`, then the clips `clips` of its
/// clip bar (as `delete_clip`).
#[tauri::command]
pub(crate) async fn remove_from_edit(
    app: AppHandle,
    id: i64,
    pieces: Vec<i64>,
    stretches: Vec<(f64, f64)>,
    clips: Vec<i64>,
) -> CmdResult<EditView> {
    let label = if pieces.is_empty() && stretches.is_empty() { "Clips deleted" } else { "Pieces removed" };
    changing(&app, id, label, |db| {
        db.remove_from_edit(id, &stretches, &pieces)?;
        for clip in clips {
            // Only a clip of this edit's own clip bar.
            if db.clip(clip)?.is_some_and(|c| c.bar == Some(vodscribe_core::db::ClipBar::Edit { id })) {
                crate::clips::remove(&app, db, clip)?;
            }
        }
        Ok(())
    })
}

/// Renders edit `id` (after any being rendered); shown as rendering from
/// now, so it isn't asked for twice. Asked again while it waits, once is enough.
#[tauri::command]
pub(crate) async fn render_edit_now(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    // Waiting already: nothing to do (it renders the edit as it is when its
    // turn comes). Asked first, as `queue_once` answers only after the status
    // below is written and sent. One running renders the edit as it was, so
    // it's queued again.
    if state.clips.is_waiting(id) {
        return Ok(());
    }
    // Shown as rendering before it's queued: its render could otherwise end
    // first, and be shown as rendering after.
    {
        let db = state.db.lock().unwrap();
        db.set_edit_render(id, ClipStatus::Cutting, None, None, None).map_err(err)?;
        emit(&app, &db, id);
    }
    state.clips.queue_once(id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn stop_render(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    // Not begun: as it was before it was asked for.
    if state.clips.cancel(id) {
        let db = state.db.lock().unwrap();
        db.set_edit_render(id, ClipStatus::Waiting, None, None, None).map_err(err)?;
        emit(&app, &db, id);
    }
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
    let taken = move || {
        let _ = app.state::<AppState>().db.lock().unwrap().hand_out_edit(id);
    };
    let files = move || vec![crate::clips::Made::Ready(path.clone())];
    crate::clips::drag_file(window, files, None, taken).await
}

/// Edit `id`'s picture: its first piece's first frame (a JPEG), or failing
/// that (its footage isn't here yet) its VOD's thumbnail; empty if neither.
#[tauri::command]
pub(crate) async fn edit_thumb(app: AppHandle, id: i64) -> CmdResult<tauri::ipc::Response> {
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (source, thumb) = {
            let db = state.db.lock().unwrap();
            let pieces = pieces(&app, &db, id).ok().unwrap_or_default();
            let Some((p, (a, b))) = pieces.iter().find_map(|p| Some((p, *p.spans().first()?))) else {
                return Vec::new();
            };
            // Only its start needs to be here (a section may hold no more).
            let source = crate::footage::find(&app, &db, p.vod_id, a, b.min(a + 1.0)).map(|s| (s.path, a - s.offset));
            (source, db.vod(p.vod_id).ok().flatten().and_then(|v| v.thumb).filter(|t| !t.is_empty()))
        };
        let frame = source
            .zip(tools::find_ffmpeg(&state.paths.bin()))
            .and_then(|((path, at), ffmpeg)| vodscribe_core::local::frame(&ffmpeg, &path, at, 320).ok());
        frame.or_else(|| std::fs::read(state.paths.thumbs().join(thumb?)).ok()).unwrap_or_default()
    })
    .await
    .map_err(err)?;
    Ok(tauri::ipc::Response::new(bytes))
}
