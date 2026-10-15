//! Commands on the library: adding, ordering and controlling VODs, and reading
//! their transcripts.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{Db, SearchFilter, SearchHit, Status, Vod};
use vodscribe_core::{fetch, local, pipeline, thumbs, tools};

use crate::worker::{self, Stop};
use crate::{AppState, CmdResult, emit_vod, err, thumbnails};

#[derive(Serialize)]
pub(crate) struct Added {
    vod: Vod,
    created: bool,
}

/// Unregisters an add's ticket when the add ends, however it ends.
struct Unregister<'a>(&'a AppState, Option<u64>);

impl Drop for Unregister<'_> {
    fn drop(&mut self) {
        if let Some(ticket) = self.1 {
            self.0.adding.lock().unwrap().remove(&ticket);
        }
    }
}

/// Stops looking up the link being added under `ticket` (add_vod), which then
/// fails as canceled. A cancel that gets here before its add has started is
/// left for the add to find; one after the add is done leaves a flag nothing
/// reads (tickets aren't reused).
#[tauri::command]
pub(crate) async fn cancel_add(state: State<'_, AppState>, ticket: u64) -> CmdResult<()> {
    let mut adding = state.adding.lock().unwrap();
    adding.entry(ticket).or_default().store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub(crate) async fn add_vod(app: AppHandle, url: String, ticket: Option<u64>) -> CmdResult<Added> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("Paste a VOD link first.".into());
    }
    let task_app = app.clone();
    let added = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Added> {
        let state = task_app.state::<AppState>();
        // Registered under the interface's ticket while it's checked and looked
        // up, for cancel_add, which may have come first.
        let cancel = match ticket {
            Some(ticket) => state.adding.lock().unwrap().entry(ticket).or_default().clone(),
            None => Default::default(),
        };
        let _unregister = Unregister(&state, ticket);
        // Checked, and passed on as the name it resolves as. Resolving a name
        // can take a while, and the add may have been canceled meanwhile.
        let url = vodscribe_core::net::public_url(&url)?;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(vodscribe_core::Canceled.into());
        }
        let yt_dlp = tools::find_yt_dlp(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!(fetch::NO_YT_DLP))?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let probe_path = state.paths.work().join(format!("probe-{nanos}.json"));
        let info = fetch::probe(&yt_dlp, &url, &probe_path, &cancel);
        // Canceled once looked up, but before it's in the library: not added either.
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = std::fs::remove_file(&probe_path);
            return Err(vodscribe_core::Canceled.into());
        }
        let info = info?;

        let db = state.db.lock().unwrap();
        let (mut vod, created) = db.add_vod(&info)?;
        if created {
            // Hand the resolved info to the worker so it doesn't probe again. Only a
            // shortcut: if the file can't be moved (a virus scanner holding it, say),
            // the worker probes again, and the VOD, already saved, must still be
            // shown and queued.
            if std::fs::rename(&probe_path, pipeline::info_path(&state.paths.work(), vod.id)).is_err() {
                let _ = std::fs::remove_file(&probe_path);
            }
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
        state.chat.push(added.vod.id);
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
        let _ = app.emit("vod", vod);
        if *created {
            // A file has no chat; the queue records that.
            state.chat.push(vod.id);
            let (app, id) = (app.clone(), vod.id);
            std::thread::spawn(move || thumbnails::cache_thumbnail(&app, id));
        }
    }
    Ok(AddedFiles { vods: added.into_iter().map(|(v, _)| v).collect(), errors })
}

fn add_file(state: &AppState, path: &Path) -> anyhow::Result<(Vod, bool)> {
    let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).ok_or_else(|| anyhow::anyhow!(crate::worker::NO_FFMPEG))?;
    // probe refuses network paths and non-media files before touching them.
    let info = local::probe(&ffmpeg, path)?;
    let canonical = Path::new(&info.url);
    if local::is_inside(canonical, &state.paths.root) || local::is_inside(canonical, &state.paths.local) {
        anyhow::bail!("{} is part of VODscribe's own data.", path.display());
    }
    let db = state.db.lock().unwrap();
    // One transaction, so a file never shows up as an online VOD with a path for a link.
    let (vod, created) = db.atomically(|db| {
        let (vod, created) = db.add_vod(&info)?;
        if created {
            db.set_source_path(vod.id, &info.url)?;
        }
        Ok((vod, created))
    })?;
    if !created && vod.status == Status::Error {
        requeue(&db, vod.id)?;
    }
    Ok((db.vod(vod.id)?.unwrap_or(vod), created))
}

/// Opens a local VOD's file in the system's video player, for formats the
/// built-in player can't play. Takes the VOD's id, not a path.
#[tauri::command]
pub(crate) async fn open_file(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let source = state.db.lock().unwrap().vod(id).map_err(err)?.and_then(|v| v.source_path);
    let source = source.ok_or("That VOD isn't a file on this computer.")?;
    // Opening a program would run it, so only ever open media files.
    if !local::is_media(Path::new(&source)) {
        return Err("Only video and audio files can be opened.".into());
    }
    app.opener().open_path(source, None::<String>).map_err(err)
}

/// The file extensions VODscribe accepts, for the file picker.
#[tauri::command]
pub(crate) async fn media_extensions() -> CmdResult<Vec<&'static str>> {
    Ok(local::MEDIA_TYPES.iter().map(|(ext, _)| *ext).collect())
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
pub(crate) async fn list_vods(app: AppHandle) -> CmdResult<Vec<Vod>> {
    crate::with_reads(app, |db| db.vods()).await
}

#[tauri::command]
pub(crate) async fn reorder_vods(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<()> {
    state.db.lock().unwrap().reorder(&ids).map_err(err)
}

/// Rows erased at a time: a few ms each (chat; segments, a third as many), so
/// the library is free in between for everything else.
const ERASE_PART: usize = 1000;

/// Runs `part` on a blocking thread until it erases nothing more, taking the
/// library's lock for each part only: a VOD with a busy chat and a long
/// transcript is a hundred thousand rows and more.
async fn erase_in_parts(
    app: &AppHandle,
    part: impl Fn(&Db) -> anyhow::Result<usize> + Send + 'static,
) -> CmdResult<()> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<()> {
        let state = handle.state::<AppState>();
        // Letting go between parts, so what's waiting gets its turn.
        while part(&state.db.lock().unwrap())? > 0 {
            std::thread::yield_now();
        }
        Ok(())
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[tauri::command]
pub(crate) async fn delete_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    // Its chat and transcript first, a part at a time on a blocking thread, so
    // the rest of the app isn't kept waiting on the library while they're
    // erased. (No more chat is fetched for it.)
    state.chat.stop(id);
    let _ = crate::videos::stop_video(app.clone(), state.clone(), id).await;
    state.clips.stop_vod(&state.db.lock().unwrap(), id);
    erase_in_parts(&app, move |db| db.delete_some_of(id, ERASE_PART)).await?;
    // A running stage removes the files once its process has stopped.
    // With the reads held too, so none is under way when the WAL is emptied.
    let delete_row = || {
        let db = state.db.lock().unwrap();
        let _reads = state.reads.lock().unwrap();
        db.delete_vod(id)
    };
    let running = state
        .worker
        .control(id, Stop::Delete, || {
            pipeline::remove_work_files(&state.paths.work(), id);
            pipeline::remove_media(&state.paths.work(), id);
            delete_row()
        })
        .map_err(err)?;
    if running {
        delete_row().map_err(err)?;
    }
    thumbs::remove(&state.paths.thumbs(), id);
    vodscribe_core::peaks::remove(&state.paths.peaks(), id);
    let _ = app.emit("vod-removed", id);
    Ok(())
}

/// Deletes the video kept for VOD `id`, keeping its transcript; it then
/// plays on its site instead.
#[tauri::command]
pub(crate) async fn delete_video(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    let vod = db.vod(id).map_err(err)?.ok_or("That VOD is gone.")?;
    if busy(vod.status) {
        return Err("Its video is still in use: wait until it's transcribed.".into());
    }
    forget_video(&db, &state.paths.work(), id).map_err(err)?;
    emit_vod(&app, &db, id)
}

/// Whether a VOD is being transcribed, or waiting to be: its kept video may be in use.
pub(crate) fn busy(status: Status) -> bool {
    matches!(status, Status::Downloading | Status::Transcribing | Status::Ready | Status::Queued)
}

/// Deletes VOD `id`'s kept video and forgets it (and its audio, if that was the video).
pub(crate) fn forget_video(db: &Db, work: &std::path::Path, id: i64) -> anyhow::Result<()> {
    pipeline::remove_media(work, id);
    db.set_media_path(id, None)?;
    if db.audio_path(id)?.is_some_and(|p| !std::path::Path::new(&p).is_file()) {
        db.set_audio_path(id, None)?;
    }
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
    {
        let db = state.db.lock().unwrap();
        match db.vod(id).map_err(err)? {
            Some(vod) if matches!(vod.status, Status::Done | Status::Error) => {}
            _ => return Err("Only finished VODs can be transcribed again.".into()),
        }
        // First, so a quit while the old transcript is erased leaves it marked
        // outdated, and transcribing it next clears the rest.
        db.set_model(id, None).map_err(err)?;
    }
    // The old transcript, a part at a time on a blocking thread: a long one
    // is thousands of rows, and the app shouldn't wait on the library meanwhile.
    erase_in_parts(&app, move |db| db.clear_some_segments(id, ERASE_PART / 3)).await?;
    let db = state.db.lock().unwrap();
    db.atomically(|db| {
        // None left to delete; tidies the search index of the old transcript's words.
        db.clear_segments(id)?;
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
pub(crate) async fn search(app: AppHandle, query: String, filter: SearchFilter) -> CmdResult<SearchResponse> {
    let r = crate::with_reads(app, move |db| db.search(&query, &filter, 500)).await?;
    Ok(SearchResponse { hits: r.hits, truncated: r.truncated })
}

#[derive(Serialize)]
pub(crate) struct Line {
    start: f64,
    end: f64,
    text: String,
}

#[tauri::command]
pub(crate) async fn transcript(app: AppHandle, vod_id: i64, after: Option<f64>) -> CmdResult<Vec<Line>> {
    let lines = crate::with_reads(app, move |db| db.lines(vod_id, after)).await?;
    Ok(lines.into_iter().map(|(start, end, text)| Line { start, end, text }).collect())
}

/// VOD `vod_id`'s captions, for the player of a file played here.
#[tauri::command]
pub(crate) async fn captions(app: AppHandle, vod_id: i64) -> CmdResult<Vec<vodscribe_core::transcript::Cue>> {
    crate::with_reads(app, move |db| Ok(vodscribe_core::transcript::cues(&db.segments(vod_id)?))).await
}

/// A URL that plays Twitch clip `slug`'s video (see `vodscribe_core::twitch`).
#[tauri::command]
pub(crate) async fn twitch_clip_video(slug: String) -> CmdResult<String> {
    if !vodscribe_core::twitch::is_clip(&slug) {
        return Err("not a Twitch clip".into());
    }
    tauri::async_runtime::spawn_blocking(move || vodscribe_core::twitch::clip_video(&slug))
        .await
        .map_err(err)?
        .map_err(err)
}

/// Playable quality levels for a Kick VOD; Kick has no embeddable VOD player.
#[tauri::command]
pub(crate) async fn kick_stream(url: String) -> CmdResult<Vec<vodscribe_core::kick::Variant>> {
    tauri::async_runtime::spawn_blocking(move || vodscribe_core::kick::variants(&url)).await.map_err(err)?.map_err(err)
}
