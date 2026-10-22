//! Commands on the library: adding, ordering and controlling VODs, and reading
//! their transcripts.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{Db, SearchFilter, SearchHit, Status, Vod};
use vodscribe_core::edits::LibraryItem;
use vodscribe_core::settings::ClipFormat;
use vodscribe_core::{clips, fetch, local, pipeline, thumbs, tools};

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
        anyhow::bail!("{} is part of VODscribe's own data.", vodscribe_core::tools::shown(path));
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
    crate::with_reads(app, |db| db.listed_vods()).await
}

/// The library's list, VODs and edits, in its order.
#[tauri::command]
pub(crate) async fn library_order(app: AppHandle) -> CmdResult<Vec<LibraryItem>> {
    crate::with_reads(app, |db| db.library_order()).await
}

/// Puts the library's list, VODs and edits, in the order of `items`.
#[tauri::command]
pub(crate) async fn reorder_library(state: State<'_, AppState>, items: Vec<LibraryItem>) -> CmdResult<()> {
    state.db.lock().unwrap().reorder_library(&items).map_err(err)
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

/// Takes VOD `id` out of the library's list: its own clip goes, with its
/// marks and its clip bar (the clips' files too, unless dragged out of the
/// app), and its download or transcription stops where it is. What other
/// clips use of it stays, a source only, until nothing does; then it's
/// erased (`sweep`) as the app quits, or after it next starts, so what's
/// undone meanwhile still finds it.
///
/// With `delete_video`, it's erased now: first each stretch other clips use
/// is realized (`realize_all`); if one can't be (offline, a full disk), the
/// error says why after `REALIZE_FAILED` and nothing is removed, for the
/// page to ask what then. With `anyway` too, it's erased without that: the
/// pieces other clips have of it are holes.
#[tauri::command]
pub(crate) async fn delete_vod(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    delete_video: Option<bool>,
    anyway: Option<bool>,
) -> CmdResult<()> {
    let (delete_video, anyway) = (delete_video.unwrap_or(false), anyway.unwrap_or(false));
    if delete_video && !anyway {
        let handle = app.clone();
        tauri::async_runtime::spawn_blocking(move || realize_all(&handle, id))
            .await
            .map_err(err)?
            .map_err(|e| format!("{REALIZE_FAILED}{e:#}"))?;
    }
    state.chat.stop(id);
    state.history.forget(crate::history::Doc::Vod(id));
    let _ = crate::videos::stop_video(app.clone(), state.clone(), id).await;
    state.footage.stop_vod(id);
    state.clips.stop_vod(&state.db.lock().unwrap(), id);
    let removed = (|| {
        let db = state.db.lock().unwrap();
        for clip in db.clips(id)? {
            crate::clips::remove(&app, &db, clip.id)?;
        }
        db.remove_entry(id)?;
        state.worker.control(id, Stop::Pause, || Ok(()))?;
        anyhow::Ok(())
    })();
    state.clips.vod_removed(&state.db.lock().unwrap(), id);
    removed.map_err(err)?;
    if delete_video {
        // Its chat and transcript a part at a time, then the rest; one whose
        // work is still stopping goes when the app next quits or starts.
        erase_in_parts(&app, move |db| db.delete_some_of(id, ERASE_PART)).await?;
        let db = state.db.lock().unwrap();
        state.worker.control(id, Stop::Pause, || erase_source(&app, &db, id)).map_err(err)?;
    }
    let _ = app.emit("vod-removed", id);
    Ok(())
}

/// What the error of a `delete_vod` whose realizing failed starts with.
const REALIZE_FAILED: &str = "realize: ";

/// Erases VOD `id`'s files, footage and pictures (its transcript and chat
/// erased already); then the VOD itself if nothing uses it, or else it's
/// gone (`Db::set_gone`), its pieces holes.
fn erase_source(app: &AppHandle, db: &Db, id: i64) -> anyhow::Result<()> {
    let paths = &app.state::<AppState>().paths;
    pipeline::remove_work_files(&paths.work(), id);
    pipeline::remove_media(&paths.work(), id);
    thumbs::remove(&paths.thumbs(), id);
    vodscribe_core::peaks::remove(&paths.peaks(), id);
    crate::footage::remove(app, id);
    if db.uses(id)? > 0 {
        db.set_gone(id)?;
        db.clear_segments(id)?;
        return Ok(());
    }
    db.delete_vod(id)
}

/// Realizes every stretch of VOD `id` that clips other than its own (and its
/// clip bar's) use: each made into a source of its own (`realize`).
fn realize_all(app: &AppHandle, id: i64) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).context(worker::NO_FFMPEG)?;
    let stop = AtomicBool::new(false);
    loop {
        // Each realized piece leaves the list.
        let Some(piece) = state.db.lock().unwrap().pieces_using(id)?.first().map(|p| p.id) else { return Ok(()) };
        realize(app, &ffmpeg, id, piece, &stop)?;
    }
}

/// Makes the stretch of VOD `vod_id` that piece `piece` is into a source of
/// its own, exactly that stretch, cut from its footage (downloaded first if
/// need be) into the kept videos' folder, and moves the piece to it
/// (`Db::realize_piece`). Made from the clip at one revision and moved
/// against it: if the clip changed meanwhile, made again.
fn realize(app: &AppHandle, ffmpeg: &Path, vod_id: i64, piece: i64, stop: &AtomicBool) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    for _ in 0..5 {
        let (row, base, vod) = {
            let db = state.db.lock().unwrap();
            let Some(row) = db.pieces_using(vod_id)?.into_iter().find(|p| p.id == piece) else { return Ok(()) };
            let base = db.revision(row.clip_id)?.context("That clip is gone.")?;
            (row, base, db.vod(vod_id)?.context("That VOD is gone.")?)
        };
        let (start, end) = (row.start.seconds(), row.end.context("A piece without an end.")?.seconds());
        crate::footage::ensure(app, vod_id, start, end, stop, &|_| {})?;
        let source = {
            let db = state.db.lock().unwrap();
            crate::footage::find(app, &db, vod_id, start, end).context("Its footage didn't download.")?
        };
        let (new, out) = {
            let db = state.db.lock().unwrap();
            let new = db.add_realized(vod_id, start, end)?;
            let name = clips::extension(vod.audio_only, ClipFormat::Mp4);
            (
                new,
                pipeline::media_stem(&pipeline::media_dir(&state.paths.work()), new, &vod.info.title)
                    .with_extension(name),
            )
        };
        let within = (start - source.offset, end - source.offset);
        let made = clips::cut(ffmpeg, &source.path, &out, within, vod.audio_only, ClipFormat::Mp4, stop, |_| {});
        let db = state.db.lock().unwrap();
        let moved = made.and_then(|()| {
            db.set_media_path(new, Some(&out.to_string_lossy()))?;
            match db.realize_piece(piece, new, base) {
                Err(e) if e.downcast_ref::<vodscribe_core::edits::Stale>().is_some() => Ok(false),
                moved => moved,
            }
        });
        if !matches!(moved, Ok(true)) {
            // Not used: made again, or not at all.
            let _ = std::fs::remove_file(&out);
            db.delete_vod(new)?;
        }
        if moved? {
            return Ok(());
        }
    }
    anyhow::bail!("A clip using it kept changing while it was being kept; try again.")
}

/// How long quitting spends erasing sources nothing uses; the rest are
/// erased after the next start.
pub(crate) const SWEEP_AT_QUIT: Duration = Duration::from_secs(3);

/// Erases the sources nothing uses (`Db::unused_sources`): their transcript,
/// chat, files, footage and pictures. As the app quits (`db` held, within
/// `SWEEP_AT_QUIT`), or after it starts (`db` None: the library's lock taken
/// a part at a time). One whose work is still
/// stopping waits for the next time.
pub(crate) fn sweep(app: &AppHandle, db: Option<&Db>) {
    let state = app.state::<AppState>();
    let began = Instant::now();
    let unused = match db {
        Some(db) => db.unused_sources(),
        None => state.db.lock().unwrap().unused_sources(),
    };
    for id in unused.unwrap_or_default() {
        if db.is_some() && began.elapsed() > SWEEP_AT_QUIT {
            break;
        }
        if db.is_none() {
            // Its chat and transcript a part at a time, so the app isn't kept waiting.
            loop {
                let db = state.db.lock().unwrap();
                // Listed again meanwhile (added anew): kept.
                if db.uses(id).unwrap_or(1) > 0 || !db.delete_some_of(id, ERASE_PART).is_ok_and(|n| n > 0) {
                    break;
                }
                drop(db);
                std::thread::yield_now();
            }
        }
        let erase = |db: &Db| -> anyhow::Result<()> {
            // Used again meanwhile (listed anew): kept.
            if db.uses(id)? > 0 {
                return Ok(());
            }
            erase_source(app, db, id)
        };
        let erased = match db {
            Some(db) => state.worker.control(id, Stop::Pause, || erase(db)),
            None => {
                let db = state.db.lock().unwrap();
                state.worker.control(id, Stop::Pause, || erase(&db))
            }
        };
        if let Err(e) = erased {
            eprintln!("erasing unused VOD {id}: {e:#}");
        }
    }
}

/// Deletes the video kept for VOD `id`, keeping its transcript; it then
/// plays on its site instead.
#[tauri::command]
pub(crate) async fn delete_video(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let had = {
        let db = state.db.lock().unwrap();
        let vod = db.vod(id).map_err(err)?.ok_or("That VOD is gone.")?;
        if busy(vod.status) {
            return Err("Its video is still in use: wait until it's transcribed.".into());
        }
        forget_video(&db, &state.paths.work(), id).map_err(err)?;
        emit_vod(&app, &db, id)?;
        vod.media
    };
    // Forgotten first, so nothing starts on it; deleted with the library let
    // go (a big video can take a moment). Unless a "Keep video" queued it
    // again meanwhile: those files are that download's.
    if had && !state.videos.wanted(id) {
        pipeline::remove_media(&state.paths.work(), id);
    }
    Ok(())
}

/// Whether a VOD is being transcribed, or waiting to be: its kept video may be in use.
pub(crate) fn busy(status: Status) -> bool {
    matches!(status, Status::Downloading | Status::Transcribing | Status::Ready | Status::Queued)
}

/// Forgets VOD `id`'s kept video (and its audio, if that was kept with it);
/// the caller deletes the files (`pipeline::remove_media`), once it has let
/// the library go.
pub(crate) fn forget_video(db: &Db, work: &std::path::Path, id: i64) -> anyhow::Result<()> {
    db.set_media_path(id, None)?;
    let media = pipeline::media_dir(work);
    if db.audio_path(id)?.is_some_and(|p| std::path::Path::new(&p).parent() == Some(media.as_path())) {
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
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) text: String,
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
