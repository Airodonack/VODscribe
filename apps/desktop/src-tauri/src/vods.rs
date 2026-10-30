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
use vodscribe_core::query::QueryInfo;
use vodscribe_core::search::AuthorSuggestion;
use vodscribe_core::settings::ClipFormat;
use vodscribe_core::stretches::{self, Stretch};
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
        let (vod, created) = add_to_library(&db, &info, None)?;
        if created {
            // Hand the resolved info to the worker so a transcription asked
            // for soon doesn't probe again. Only a shortcut: if the file can't
            // be moved (a virus scanner holding it, say), the worker probes
            // again, and the VOD, already saved, must still be shown.
            if std::fs::rename(&probe_path, pipeline::info_path(&state.paths.work(), vod.id)).is_err() {
                let _ = std::fs::remove_file(&probe_path);
            }
        } else {
            let _ = std::fs::remove_file(&probe_path);
        }
        Ok(Added { vod, created })
    })
    .await
    .map_err(err)?
    .map_err(err)?;

    // Not transcribed: that's asked for (Transcribe), all of it or a stretch.
    let state = app.state::<AppState>();
    let _ = app.emit("vod", &added.vod);
    if added.created {
        state.chat.push(added.vod.id);
        crate::annotations::look_up_added(&app, &added.vod);
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
/// with the file dialog. They're played and transcribed in place and never modified.
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
    add_to_library(&db, &info, Some(&info.url))
}

/// Adds a VOD (a file at `source`, if given), in one transaction; one
/// already there is left as it is. Nothing is transcribed till asked for.
fn add_to_library(db: &Db, info: &fetch::VodInfo, source: Option<&str>) -> anyhow::Result<(Vod, bool)> {
    db.atomically(|db| {
        let (vod, created) = db.add_vod(info)?;
        if created && let Some(source) = source {
            db.set_source_path(vod.id, source)?;
        }
        Ok((db.vod(vod.id)?.unwrap_or(vod), created))
    })
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

/// Shows the file VOD `id` plays from here in the system file manager: a
/// local VOD's own file, or the video downloaded for a site's
/// (`Db::playable_path`). Takes the VOD's id, not a path, so the UI can't
/// point it anywhere else.
#[tauri::command]
pub(crate) async fn show_file(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let file = state.db.lock().unwrap().playable_path(id).map_err(err)?;
    let file = file.ok_or("That VOD has no file on this computer.")?;
    app.opener().reveal_item_in_dir(file).map_err(err)
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
/// clips use of it stays, a source only, until nothing does; if nothing
/// does now, it's erased at once, a job beside the encodes (`Sweeper`).
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
        state.worker.control_vod(id, Stop::Pause, |_| Ok(()))?;
        anyhow::Ok(())
    })();
    state.clips.vod_removed(&state.db.lock().unwrap(), id);
    removed.map_err(err)?;
    let mut erased = false;
    if delete_video {
        // Its chat and transcript a part at a time, then the rest; one whose
        // work is still stopping is left to the sweeper's job, below.
        erase_in_parts(&app, move |db| db.delete_some_of(id, ERASE_PART)).await?;
        let db = state.db.lock().unwrap();
        state
            .worker
            .control_vod(id, Stop::Pause, |running| {
                erased = running.is_empty();
                if erased { erase_source(&app, &db, id, false) } else { Ok(()) }
            })
            .map_err(err)?;
    }
    if !erased && state.db.lock().unwrap().uses(id).map_err(err)? <= 0 {
        state.sweeper.removed(id);
    }
    let _ = app.emit("vod-removed", id);
    Ok(())
}

/// What the error of a `delete_vod` whose realizing failed starts with.
const REALIZE_FAILED: &str = "realize: ";

/// Erases VOD `id`'s files, footage and pictures (its transcript and chat
/// erased already); then the VOD itself if nothing uses it and it isn't to
/// be `kept`, or else it's gone (`Db::set_gone`), its pieces holes.
fn erase_source(app: &AppHandle, db: &Db, id: i64, kept: bool) -> anyhow::Result<()> {
    let paths = &app.state::<AppState>().paths;
    pipeline::remove_work_files(&paths.work(), id);
    pipeline::remove_media(&paths.work(), id);
    thumbs::remove(&paths.thumbs(), id);
    vodscribe_core::peaks::remove(&paths.peaks(), id);
    crate::storyboards::remove(app, id);
    crate::footage::remove(app, id);
    if kept || db.uses(id)? > 0 {
        db.set_gone(id)?;
        db.clear_segments(id)?;
        db.clear_stretches(id)?;
        // What the search indexes had of its words goes now too, not with the row.
        db.tidy_indexes()?;
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

/// A VOD nothing uses any more, to erase (`Sweeper`): one just removed
/// keeps its row as a gone source till the next start (`kept`), so an undo
/// in an edit that puts back a piece of it finds it, a hole; one left from
/// before the app started goes whole.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Sweep {
    pub(crate) vod: i64,
    kept: bool,
}

/// How long an erasure waits for the VOD's download or transcription to
/// stop (paused as it was removed) before leaving it to the next start.
const WORK_STOPPING: Duration = Duration::from_secs(10);

/// The sources nothing uses (`Db::unused_sources`) erased one at a time, a
/// queue beside the clips' (the disk's work, not the processor's): their
/// transcript, chat, files, footage and pictures. Each is queued as it's
/// removed, and at start whatever was left (stopped, or the app quit
/// first); quitting doesn't wait for it.
pub(crate) struct Sweeper {
    jobs: crate::jobs::Jobs<Sweep>,
}

impl Sweeper {
    pub(crate) fn new() -> Self {
        Sweeper { jobs: crate::jobs::Jobs::new() }
    }

    /// Starts erasing, with what was left from before.
    pub(crate) fn run(&self, app: AppHandle) -> anyhow::Result<()> {
        let left = app.state::<AppState>().db.lock().unwrap().unused_sources()?;
        let news = app.clone();
        self.jobs.watch(move || crate::job_list::changed(&news));
        self.jobs.run("sweep", move |job, stop| {
            if let Err(e) = erase_unused(&app, job, stop) {
                eprintln!("erasing unused VOD {}: {e:#}", job.vod);
            }
        })?;
        for vod in left {
            self.jobs.push_new(Sweep { vod, kept: false }, |j| j.vod == vod);
        }
        Ok(())
    }

    /// VOD `id` was removed and nothing uses it: it's erased.
    pub(crate) fn removed(&self, id: i64) {
        self.jobs.push_new(Sweep { vod: id, kept: true }, |j| j.vod == id);
    }

    /// Stops erasing VOD `id`, or takes it out of the queue: what's left of
    /// it is erased at the next start.
    pub(crate) fn cancel(&self, id: i64) {
        self.jobs.take_waiting(|j| j.vod == id);
        self.jobs.stop_running(|j| j.vod == id);
    }

    /// The VOD being erased, if one is, and those waiting, in order.
    pub(crate) fn listed(&self) -> (Option<i64>, Vec<i64>) {
        let (running, waiting) = self.jobs.snapshot();
        (running.map(|j| j.vod), waiting.into_iter().map(|j| j.vod).collect())
    }
}

/// Erases the VOD `job` names, unless something uses it (again): gone
/// first (so adding it back adds it anew: `Db::add_vod`), then its chat and
/// transcript a part at a time, the library free in between, then its
/// files once its work has stopped. Stopped (`stop`), it's left as it is,
/// for the next start.
fn erase_unused(app: &AppHandle, job: Sweep, stop: &AtomicBool) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let id = job.vod;
    let unused = |db: &Db| -> anyhow::Result<bool> { Ok(db.vod(id)?.is_some() && db.uses(id)? <= 0) };
    crate::job_list::begun(app, crate::job_list::Queue::Erase, id);
    let total = {
        let db = state.db.lock().unwrap();
        if !unused(&db)? {
            return Ok(());
        }
        db.set_gone(id)?;
        db.erase_left(id)?
    };
    let mut done = 0;
    loop {
        let db = state.db.lock().unwrap();
        if !unused(&db)? {
            return Ok(());
        }
        let n = db.delete_some_of(id, ERASE_PART)?;
        drop(db);
        if n == 0 {
            break;
        }
        done += n;
        // The files are the rest.
        let progress = 0.95 * (done as f64 / total.max(1) as f64).min(1.0);
        crate::job_list::progressed_on(app, crate::job_list::Queue::Erase, id, progress);
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        std::thread::yield_now();
    }
    // Its storyboard's ffmpeg lets go of its files first (on Windows a file
    // open elsewhere can't be deleted): it may have been at work on it as
    // it was removed.
    state.storyboards.let_go(id);
    let began = Instant::now();
    loop {
        let db = state.db.lock().unwrap();
        let mut erased = false;
        state.worker.control_vod(id, Stop::Pause, |running| {
            // Used again meanwhile (an undo): kept.
            if !running.is_empty() || !unused(&db)? {
                return Ok(());
            }
            erased = true;
            erase_source(app, &db, id, job.kept)
        })?;
        drop(db);
        if erased || !unused(&state.db.lock().unwrap())? {
            return Ok(());
        }
        if stop.load(std::sync::atomic::Ordering::Relaxed) || began.elapsed() > WORK_STOPPING {
            // Its work still stopping: the next start erases it.
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Deletes the video kept for VOD `id`, keeping its transcript. It then
/// plays in its site's player where the app shows one, and elsewhere is
/// downloaded again when played (`pipeline::keeps_video`).
#[tauri::command]
pub(crate) async fn delete_video(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let remove = |work: &Path, id| remove_kept(&state.storyboards, work, id);
    delete_kept_video(&state.db, &state.videos, &state.paths.work(), id, remove)?;
    crate::storyboards::forget_gone(&app, id);
    emit_vod(&app, &state.db.lock().unwrap(), id)
}

/// Deletes VOD `id`'s kept video and what's left of downloading it
/// (`pipeline::remove_media`), once its storyboard's ffmpeg has let go of
/// them: a file another process has open can't be deleted on Windows, and
/// would stay, its record gone. Called with the record forgotten already
/// (no job starts on the file again) and the library let go of.
pub(crate) fn remove_kept(storyboards: &crate::storyboards::Storyboards, work: &Path, id: i64) {
    storyboards.let_go(id);
    pipeline::remove_media(work, id);
}

/// `delete_video`'s work: `remove` deletes the files (`remove_kept`).
pub(crate) fn delete_kept_video(
    db: &std::sync::Mutex<Db>,
    videos: &crate::videos::Videos,
    work: &Path,
    id: i64,
    remove: impl FnOnce(&Path, i64),
) -> CmdResult<()> {
    // Through to the files' deletion: a download recorded meanwhile, or one
    // a "Keep video" begins meanwhile, waits for it (`Videos::files`).
    let _files = videos.files();
    let had = {
        let db = db.lock().unwrap();
        let vod = db.vod(id).map_err(err)?.ok_or("That VOD is gone.")?;
        if busy(vod.status) {
            return Err("Its video is still in use: wait until it's transcribed.".into());
        }
        forget_video(&db, work, id).map_err(err)?;
        vod.media
    };
    // Forgotten first, so nothing starts on it; deleted with the library let
    // go (a big video can take a moment). Only with a video kept (`had`):
    // Delete video is offered only then, and a paused download's parts are
    // Delete all's to clear. A download that recorded it has written all it
    // will; a "Keep video" queued meanwhile begins after.
    if had {
        remove(work, id);
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

/// Puts VOD `id`'s requests in one of `which` (paused, failed) back in their
/// queues, at whichever stage each needs next. With no request at all and
/// some of it not transcribed, asks for each part that isn't.
pub(crate) fn requeue(db: &Db, id: i64, which: &[Status]) -> anyhow::Result<()> {
    let requests = db.requests(id)?;
    if requests.is_empty() {
        let Some(vod) = db.vod(id)? else { return Ok(()) };
        for (start, end) in stretches::gaps(&db.stretches(id)?, vod.info.duration) {
            db.request(id, start, end)?;
        }
        return Ok(());
    }
    // The VOD's whole audio serves any request of it.
    let whole = db.audio_path(id)?.is_some_and(|p| Path::new(&p).is_file());
    for stretch in requests.into_iter().filter(|s| which.contains(&s.status)) {
        let downloaded = whole || stretch.audio.as_ref().is_some_and(|p| Path::new(p).is_file());
        let progress = if stretch.status == Status::Paused { stretch.progress } else { 0.0 };
        let status = if downloaded { Status::Ready } else { Status::Queued };
        db.set_stretch_status(stretch.id, status, progress, None)?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn retry_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    requeue(&db, id, &[Status::Paused, Status::Error]).map_err(err)?;
    emit_vod(&app, &db, id)?;
    state.worker.wake();
    Ok(())
}

/// Transcribes a finished VOD again, e.g. with a better model: a request for
/// all of it, whose lines replace the old ones as they come (a failed
/// request it replaces goes). With `outdated`, only the parts transcribed
/// by a model this version no longer uses, each a request.
#[tauri::command]
pub(crate) async fn retranscribe_vod(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    outdated: Option<bool>,
) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    match db.vod(id).map_err(err)? {
        Some(vod) if matches!(vod.status, Status::Done | Status::Error | Status::None) => {}
        _ => return Err("Only finished VODs can be transcribed again.".into()),
    }
    db.atomically(|db| {
        for failed in db.requests(id)?.into_iter().filter(|s| s.status == Status::Error) {
            db.delete_stretch(failed.id)?;
        }
        db.set_note(id, None)?;
        if outdated == Some(true) {
            for (start, end) in db.outdated_ranges(id)? {
                db.request(id, start, end)?;
            }
        } else {
            db.request(id, 0.0, None)?;
        }
        anyhow::Ok(())
    })
    .map_err(err)?;
    emit_vod(&app, &db, id)?;
    state.worker.wake();
    Ok(())
}

/// Stops a VOD's requests, keeping their partial downloads and transcripts.
#[tauri::command]
pub(crate) async fn pause_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    // A running job is paused by its stage once its process has stopped.
    state
        .worker
        .control_vod(id, Stop::Pause, |running| {
            let db = state.db.lock().unwrap();
            for stretch in db.requests(id)? {
                if !running.contains(&stretch.id) && matches!(stretch.status, Status::Queued | Status::Ready) {
                    db.set_stretch_status(stretch.id, Status::Paused, stretch.progress, None)?;
                }
            }
            Ok(())
        })
        .map_err(err)?;
    emit_vod(&app, &state.db.lock().unwrap(), id)
}

#[tauri::command]
pub(crate) async fn resume_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    if db.requests(id).map_err(err)?.iter().any(|s| s.status == Status::Paused) {
        requeue(&db, id, &[Status::Paused]).map_err(err)?;
        emit_vod(&app, &db, id)?;
        state.worker.wake();
    }
    Ok(())
}

/// Ends a VOD's requests, keeping the lines they wrote (`worker::cancel`);
/// Retry then asks for the rest.
#[tauri::command]
pub(crate) async fn cancel_vod(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state
        .worker
        .control_vod(id, Stop::Cancel, |running| {
            let db = state.db.lock().unwrap();
            for stretch in db.requests(id)?.into_iter().filter(|s| !running.contains(&s.id)) {
                worker::cancel(&db, &state.paths.work(), stretch.id)?;
            }
            Ok(())
        })
        .map_err(err)?;
    emit_vod(&app, &state.db.lock().unwrap(), id)
}

/// Ends request `id` alone (the transcript's Stop), keeping the lines it
/// wrote, as `cancel_vod` does for all of a VOD's.
#[tauri::command]
pub(crate) async fn cancel_stretch(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let Some(vod_id) = state.db.lock().unwrap().stretch(id).map_err(err)?.map(|s| s.vod_id) else { return Ok(()) };
    state
        .worker
        .control(id, Stop::Cancel, || worker::cancel(&state.db.lock().unwrap(), &state.paths.work(), id))
        .map_err(err)?;
    emit_vod(&app, &state.db.lock().unwrap(), vod_id)
}

/// Asks for `start`..`end` seconds of VOD `vod_id` (`end` None: to its end)
/// to be transcribed, after the requests already made. Its lines replace
/// those there as they come.
#[tauri::command]
pub(crate) async fn transcribe_stretch(
    app: AppHandle,
    state: State<'_, AppState>,
    vod_id: i64,
    start: f64,
    end: Option<f64>,
) -> CmdResult<Stretch> {
    let db = state.db.lock().unwrap();
    let vod = db.vod(vod_id).map_err(err)?.filter(|v| v.listed).ok_or("That VOD is gone.")?;
    check_stretch(vod.info.duration, start, end)?;
    let stretch = db.request(vod_id, start, end).map_err(err)?;
    emit_vod(&app, &db, vod_id)?;
    state.worker.wake();
    Ok(stretch)
}

/// Why `start`..`end` (None: to the end) of a VOD `duration` long can't be
/// transcribed, if it can't.
fn check_stretch(duration: Option<f64>, start: f64, end: Option<f64>) -> Result<(), &'static str> {
    if !start.is_finite() || start < 0.0 || end.is_some_and(|end| !end.is_finite() || end <= start) {
        return Err("That stretch is empty.");
    }
    // A length of 0 (as a site or ffmpeg may give one it doesn't know) isn't one.
    if duration.is_some_and(|d| d > 0.0 && start >= d) {
        return Err("That stretch is past the end of the VOD.");
    }
    Ok(())
}

/// VOD `vod_id`'s stretches, transcribed and asked for, in time order.
#[tauri::command]
pub(crate) async fn transcript_stretches(app: AppHandle, vod_id: i64) -> CmdResult<Vec<Stretch>> {
    crate::with_reads(app, move |db| db.stretches(vod_id)).await
}

#[derive(Serialize)]
pub(crate) struct SearchResponse {
    hits: Vec<SearchHit>,
    truncated: bool,
    /// What the typed text set: its grouping, filters, or what's wrong with it.
    query: QueryInfo,
}

/// What a search typed into the box answers when a newer one typed has
/// come: the page asks again if it was its newest (two sent close together
/// can arrive the other way round).
const OVERTAKEN: &str = "overtaken by a newer search";

/// Searches the library, on search's own connection (`Searcher`): `typed`
/// (the search box's, one per pause in the typing) gives way to the next
/// one typed, which the page wants instead.
#[tauri::command]
pub(crate) async fn search(
    app: AppHandle,
    query: String,
    filter: SearchFilter,
    typed: Option<bool>,
) -> CmdResult<SearchResponse> {
    let found = tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().searches.run(typed == Some(true), |db| db.search(&query, &filter, 500))
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let r = found.ok_or(OVERTAKEN)?;
    Ok(SearchResponse { hits: r.hits, truncated: r.truncated, query: r.query })
}

/// Names `from:` could mean, for the box to suggest as they're typed.
#[tauri::command]
pub(crate) async fn author_suggestions(
    app: AppHandle,
    prefix: String,
    vod_id: Option<i64>,
) -> CmdResult<Vec<AuthorSuggestion>> {
    crate::with_reads(app, move |db| db.author_suggestions(&prefix, vod_id)).await
}

#[derive(Serialize)]
pub(crate) struct Line {
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) text: String,
    /// Who says it, when an import named them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) speaker: Option<String>,
}

/// VOD `vod_id`'s lines: all of them, or with `from`, those starting from it
/// to before `to` (None: on to the end), the range of a stretch being
/// transcribed, whose lines replace those there.
#[tauri::command]
pub(crate) async fn transcript(
    app: AppHandle,
    vod_id: i64,
    from: Option<f64>,
    to: Option<f64>,
) -> CmdResult<Vec<Line>> {
    let lines = crate::with_reads(app, move |db| match from {
        Some(from) => db.lines_between(vod_id, from, to),
        None => db.lines(vod_id),
    })
    .await?;
    Ok(lines.into_iter().map(|l| Line { start: l.start, end: l.end, text: l.text, speaker: l.speaker }).collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stretch_asked_for_is_within_the_vod() {
        assert_eq!(check_stretch(Some(600.0), 100.0, Some(200.0)), Ok(()));
        assert_eq!(check_stretch(Some(600.0), 100.0, None), Ok(()));
        assert_eq!(check_stretch(None, 100.0, None), Ok(()), "a length not known");
        assert!(check_stretch(Some(600.0), 600.0, None).is_err(), "past the end");
        assert!(check_stretch(Some(600.0), 200.0, Some(100.0)).is_err(), "empty");
        assert!(check_stretch(Some(600.0), f64::NAN, None).is_err());
        // A length its site or ffmpeg gave as 0 isn't known: all of it can be asked for.
        assert_eq!(check_stretch(Some(0.0), 0.0, None), Ok(()));
    }
}
