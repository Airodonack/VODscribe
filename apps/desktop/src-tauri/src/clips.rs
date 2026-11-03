//! Clips (`vodscribe_core::clips`): the commands the interface makes and
//! changes them with, and the queue that cuts their files, one at a time. A
//! clip is cut from its VOD's footage, as an edit is rendered from it
//! (`footage::find`): the VOD's file where it plays from one here, or else a
//! section of it downloaded into the footage folder and kept there while
//! clips use it, so a clip made a little longer, or put in an edit, needs no
//! new download.
//!
//! Clip files go in the computer's Videos folder, under VODscribe: they're for
//! taking elsewhere. Each is named for its range, so a clip whose range
//! changes is a new file; the one before goes, unless it was dragged out to
//! another app (`handed_out`), which may link to it where it is.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{Clip, ClipBar, ClipStatus, Db, Mark, MarkDoc};
use vodscribe_core::settings::Settings;
use vodscribe_core::{clips, tools};

use crate::file_drag::{Ending, ReleasedEarly};
use crate::files::{delete_file, delete_leftovers, writing};
use crate::history::{self, Doc};
use crate::jobs::Jobs;
use crate::{AppState, CmdResult, err};

/// How often a cut's progress is sent.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Clips made one at a time, by id (`jobs`): snippets cut, edits rendered
/// (`edits::render_edit`), one encode at a time between them.
pub(crate) struct Cutter {
    jobs: Jobs<i64>,
    /// VODs being removed: their clips still queued aren't cut (the rows go
    /// last, after the transcript and chat, which can take a while).
    removing: Arc<Mutex<HashSet<i64>>>,
    /// Clips and edits canceled while being made (`cancel_job`): shown as
    /// canceled once their job has ended.
    canceled: Arc<Mutex<HashSet<i64>>>,
    /// Clips pasted from a copy whose file was made: given a copy of that
    /// file when their turn comes, rather than made again (`copy_file`).
    copies: Arc<Mutex<HashMap<i64, crate::clipboard::CopiedFile>>>,
    /// Clips made only for a drag out of the window, let go on another app
    /// and not yet handed over: listed with the jobs (`job_list`), which
    /// leaves out those still in the window's drag, or over.
    out: Mutex<HashSet<i64>>,
}

impl Cutter {
    pub(crate) fn new() -> Self {
        Self {
            jobs: Jobs::new(),
            removing: Arc::default(),
            canceled: Arc::default(),
            copies: Arc::default(),
            out: Mutex::default(),
        }
    }

    /// Clips `ids`, dragged out of the window, were let go on another app
    /// (`out_now`), or the drag is over (not).
    pub(crate) fn dragged_out(&self, ids: &[i64], out_now: bool) {
        let mut out = self.out.lock().unwrap();
        for &id in ids {
            if out_now {
                out.insert(id);
            } else {
                out.remove(&id);
            }
        }
    }

    /// Whether clip `id`, made only for a drag out, was let go on another app
    /// and is still being handed over (`dragged_out`).
    pub(crate) fn is_out(&self, id: i64) -> bool {
        self.out.lock().unwrap().contains(&id)
    }

    /// Starts cutting, once the app's state is in place, beginning with the
    /// clips left unfinished when the app last stopped.
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        let removing = self.removing.clone();
        let canceled = self.canceled.clone();
        let copies = self.copies.clone();
        let news = app.clone();
        self.jobs.watch(move || crate::job_list::changed(&news));
        let unfinished = {
            let state = app.state::<AppState>();
            let db = state.db.lock().unwrap();
            // Clips made only to be dragged out: their files are someone else's now.
            db.forget_hidden_clips()?;
            // Edits left rendering when the app stopped wait to be asked again.
            db.reset_renders()?;
            db.unfinished_clips()?
        };
        {
            // Pictures of clips no longer there (deleted with their VOD).
            let thumbs = thumbs_dir(&app);
            let state = app.state::<AppState>();
            let db = state.db.lock().unwrap();
            for entry in std::fs::read_dir(&thumbs).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let id = name.strip_suffix(".png").and_then(|id| id.parse::<i64>().ok());
                if id.is_none_or(|id| db.clip(id).ok().flatten().is_none()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let settling = app.clone();
        self.jobs.run_ended(
            "clips",
            move |id, stop| {
                crate::job_list::begun(&app, crate::job_list::Queue::Render, id);
                // A pasted clip whose copy had its file: a copy of that, if it's still the same.
                let copy = copies.lock().unwrap().remove(&id);
                let copied = copy.is_some_and(|file| {
                    crate::clipboard::copy_file(&app, id, &file).unwrap_or_else(|e| {
                        eprintln!("clip {id}: copying its file: {e:#}");
                        false
                    })
                });
                if !copied {
                    make(&app, id, stop, &removing);
                }
            },
            // Canceled while it was being made (`cancel_job`): shown so now it
            // has stopped. Taken as the job ends, under the queue's lock, so a
            // cancel as it ends either finds it running and marks it, or doesn't.
            move |id| canceled.lock().unwrap().remove(id),
            move |id, canceled| {
                if canceled {
                    settle_canceled(&settling, id);
                }
            },
        )?;
        for clip in unfinished {
            self.queue(clip.id);
        }
        Ok(())
    }

    pub(crate) fn queue(&self, id: i64) {
        self.jobs.push(id);
    }

    /// Clip `id`, just pasted, is to be given a copy of `file` when its turn
    /// comes (queue it after), not made: the copy's file, made already.
    pub(crate) fn copy_file(&self, id: i64, file: crate::clipboard::CopiedFile) {
        self.copies.lock().unwrap().insert(id, file);
    }

    /// Queues clips `ids`, just made: in the clip bar (`shown`), last; made
    /// only to be dragged out of the window, first, ahead of those waiting,
    /// in their order, since the drag waits for their files (the one being
    /// made goes on: one encode at a time).
    pub(crate) fn queue_made(&self, ids: &[i64], shown: bool) {
        if shown {
            for &id in ids {
                self.queue(id);
            }
        } else {
            for &id in ids.iter().rev() {
                self.queue_first(id);
            }
        }
    }

    /// Queues clip `id` first, or moves it there if it's waiting.
    pub(crate) fn queue_first(&self, id: i64) {
        self.jobs.push_front(id, |&j| j == id);
    }

    /// Queues clip `id` unless it's waiting already; once is enough.
    pub(crate) fn queue_once(&self, id: i64) {
        self.jobs.push_once(id, |&j| j == id);
    }

    /// Whether clip `id` is waiting its turn.
    pub(crate) fn is_waiting(&self, id: i64) -> bool {
        self.jobs.is_waiting(|&j| j == id)
    }

    /// Takes clip `id` out of the queue, or stops it if it's being made;
    /// true if it was waiting, not begun.
    pub(crate) fn cancel(&self, id: i64) -> bool {
        if !self.jobs.take_waiting(|&j| j == id).is_empty() {
            return true;
        }
        self.jobs.stop_running(|&j| j == id);
        false
    }

    /// Stops the cut of a clip of VOD `vod_id`, if one's being cut, and cuts
    /// none of its others (its VOD is being deleted: a download from its site
    /// could take a while).
    pub(crate) fn stop_vod(&self, db: &Db, vod_id: i64) {
        self.removing.lock().unwrap().insert(vod_id);
        self.jobs.stop_running(|&id| on_bar_of(db, id, vod_id));
    }

    /// VOD `vod_id`'s removal is over: gone, or still there if it failed. Its
    /// clips are no longer held back, and any left to make (it stayed) are
    /// queued again rather than waiting for the next start.
    pub(crate) fn vod_removed(&self, db: &Db, vod_id: i64) {
        self.removing.lock().unwrap().remove(&vod_id);
        for clip in db.clips(vod_id).into_iter().flatten() {
            if matches!(clip.status, ClipStatus::Waiting | ClipStatus::Cutting) {
                self.jobs.push_new(clip.id, |&j| j == clip.id);
            }
        }
    }

    /// Stops clip `id`'s cut, if it's the one being cut.
    pub(crate) fn stop(&self, id: i64) {
        self.jobs.stop_running(|&j| j == id);
    }

    /// Cancels clip or edit `id` from the list of jobs: taken out of the
    /// queue, or stopped if it's being made (its ffmpeg ended and its part
    /// file deleted: `tools::encode`). A clip then shows it was canceled,
    /// and an edit as not rendered (`settle_canceled`): at once if it was
    /// waiting, once its job has ended if it was running.
    pub(crate) fn cancel_job(&self, app: &AppHandle, id: i64) {
        let taken = !self.jobs.take_waiting(|&j| j == id).is_empty();
        let running = self.jobs.stop_running_then(
            |&j| j == id,
            |&j| {
                self.canceled.lock().unwrap().insert(j);
            },
        );
        if !running && taken {
            settle_canceled(app, id);
        }
    }

    /// Puts the clips and edits waiting in the order of `ids` (dragged in the list of jobs).
    pub(crate) fn reorder(&self, ids: &[i64]) {
        self.jobs.reorder(ids, |&j| j);
    }

    /// The clip or edit being made, if one is, and those waiting, in order.
    pub(crate) fn listed(&self) -> (Option<i64>, Vec<i64>) {
        self.jobs.snapshot()
    }
}

/// What the clips' queue does with clip or edit `id`: a snippet of one
/// piece as it came is cut; one changed in the editor (more pieces, lines
/// struck, groups), or a listed edit, is rendered as an edit is.
fn make(app: &AppHandle, id: i64, stop: &AtomicBool, removing: &Mutex<HashSet<i64>>) {
    let snippet = {
        let state = app.state::<AppState>();
        let db = state.db.lock().unwrap();
        db.is_snippet(id)
            .map(|s| s.map(|s| s && db.edit_pieces(id).is_ok_and(|p| vodscribe_core::edits::cut_whole(&p))))
    };
    match snippet {
        Ok(Some(false)) => {
            if let Err(e) = crate::edits::render_edit(app, id, stop) {
                eprintln!("edit {id}: {e:#}");
            }
            return;
        }
        Ok(Some(true)) => {}
        // Gone meanwhile.
        _ => return,
    }
    if let Err(e) = cut(app, id, stop, removing) {
        eprintln!("clip {id}: {e:#}");
        fail(app, id, &e);
    }
}

/// What a clip or edit canceled from the list of jobs shows, unless it was
/// made meanwhile: a clip (a snippet) failed, "Canceled", so it isn't made
/// again at the next start and Clip again makes it; a listed edit as before
/// it was asked for, not rendered (as Stop render leaves it).
fn settle_canceled(app: &AppHandle, id: i64) {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let Ok(Some(snippet)) = db.is_snippet(id) else { return };
    let unmade = |status| matches!(status, ClipStatus::Waiting | ClipStatus::Cutting);
    if snippet {
        if let Ok(Some(clip)) = db.clip(id)
            && unmade(clip.status)
            && db.set_edit_render(id, ClipStatus::Error, None, Some(CANCELED), None).is_ok()
        {
            emit(app, &db, id);
        }
    } else if let Ok(Some(edit)) = db.edit(id)
        && unmade(edit.status)
        && db.set_edit_render(id, ClipStatus::Waiting, None, None, None).is_ok()
    {
        crate::edits::emit(app, &db, id);
    }
}

/// The error a clip canceled from the list of jobs shows.
pub(crate) const CANCELED: &str = "Canceled";

/// Where clip files go: VODscribe in the Videos folder, or in the app's data
/// where there's no Videos folder.
pub(crate) fn clips_dir(app: &AppHandle) -> PathBuf {
    match app.path().video_dir() {
        Ok(videos) => videos.join("VODscribe"),
        Err(_) => app.state::<AppState>().paths.local.join("clips"),
    }
}

/// Each clip's first frame, shown while it's dragged out (`<clip id>.png`).
fn thumbs_dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.local.join("clip-thumbs")
}

pub(crate) fn thumb_path(app: &AppHandle, id: i64) -> PathBuf {
    thumbs_dir(app).join(format!("{id}.png"))
}

#[derive(Serialize, Clone)]
struct Progress {
    id: i64,
    progress: f64,
}

pub(crate) fn emit(app: &AppHandle, db: &Db, id: i64) {
    if let Ok(Some(clip)) = db.clip(id) {
        let _ = app.emit("clip", clip);
    }
}

/// Shows clip `id` as failed with `e`, if it failed before its file was
/// begun (no ffmpeg, say) and so still says it's waiting.
fn fail(app: &AppHandle, id: i64, e: &anyhow::Error) {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    if let Ok(Some(clip)) = db.clip(id)
        && clip.status == ClipStatus::Waiting
        && db.set_clip_file(id, clip.start, clip.end, ClipStatus::Error, None, Some(&format!("{e:#}"))).is_ok()
    {
        emit(app, &db, id);
    }
}

/// Makes clip `id`'s file, if it's waiting and its VOD plays from a file here
/// (and isn't being removed: `removing`).
fn cut(app: &AppHandle, id: i64, stop: &AtomicBool, removing: &Mutex<HashSet<i64>>) -> Result<()> {
    let state = app.state::<AppState>();
    let (clip, vod, source) = {
        let db = state.db.lock().unwrap();
        let removed = |c: &Clip| removing.lock().unwrap().contains(&c.vod_id);
        let Some(clip) = db.clip(id)?.filter(|c| c.status != ClipStatus::Ready && !removed(c)) else { return Ok(()) };
        let Some(vod) = db.vod(clip.vod_id)? else { return Ok(()) };
        let source = crate::footage::find(app, &db, vod.id, clip.start, clip.end);
        (clip, vod, source)
    };
    let range = (clip.start, clip.end);
    let ffmpeg = tools::find_ffmpeg(&state.paths.bin()).context(crate::worker::NO_FFMPEG)?;
    let format = Settings::load(&state.paths.settings()).clip_format;
    // Named for its range: one already made for this range is made again in
    // place; a range moved gets a new file, and the old one stays where it is.
    let (folder, name) = clips::file_path(
        vod.info.channel.as_deref(),
        vod.info.published.as_deref(),
        &vod.info.title,
        range,
        vod.audio_only,
        format,
    );
    let dir = folder.map_or_else(|| clips_dir(app), |f| clips_dir(app).join(f));
    let out = match &clip.file_path {
        Some(path) if Path::new(path).file_name().is_some_and(|n| *n == *name) => PathBuf::from(path),
        _ => clips::unique_path(&dir, &name),
    };
    writing(&out);
    {
        let db = state.db.lock().unwrap();
        if !db.set_clip_file(id, range.0, range.1, ClipStatus::Cutting, None, None)? {
            return Ok(());
        }
        emit(app, &db, id);
    }
    let last = Mutex::new(Instant::now());
    let report = |progress: f64| {
        let mut last = last.lock().unwrap();
        if last.elapsed() >= PROGRESS_EVERY {
            *last = Instant::now();
            let _ = app.emit("clip-progress", Progress { id, progress });
            crate::job_list::progressed(app, id, progress);
        }
    };
    let result = (|| {
        // Its footage downloaded first, if it isn't here: the bulk of the time,
        // so most of the progress.
        let (source, cut_from) = match source {
            Some(source) => (source, 0.0),
            None => {
                crate::footage::ensure(app, vod.id, range.0, range.1, stop, &|p| report(p * 0.8))?;
                let db = state.db.lock().unwrap();
                let source = crate::footage::find(app, &db, vod.id, range.0, range.1);
                (source.context("Its footage didn't download.")?, 0.8)
            }
        };
        let within = (range.0 - source.offset, range.1 - source.offset);
        let report = |p: f64| report(cut_from + p * (1.0 - cut_from));
        clips::cut(&ffmpeg, &source.path, &out, within, vod.audio_only, format, stop, report)
    })();
    let db = state.db.lock().unwrap();
    let path = out.to_string_lossy();
    let recorded = match &result {
        Ok(()) => {
            let recorded = db.set_clip_file(id, range.0, range.1, ClipStatus::Ready, Some(&path), None)?;
            // What it's made of, so a change that leaves that as it was (a
            // chapter, which a cut file hasn't) doesn't make it again.
            if recorded {
                db.set_made_now(id)?;
            }
            recorded
        }
        // Stopped: moved (it's queued again) or deleted.
        Err(e) if vodscribe_core::is_canceled(e) => false,
        Err(e) => db.set_clip_file(id, range.0, range.1, ClipStatus::Error, None, Some(&format!("{e:#}")))?,
    };
    if result.is_ok() && db.clip(id)?.is_none() {
        // Deleted while it was being cut.
        delete_file(&out);
    }
    // Its file for the range before goes, unless another app took it (then
    // it stays there, and the new one is the app's own).
    if let Some(old) = clip.file_path.as_deref().filter(|p| recorded && result.is_ok() && Path::new(p) != out) {
        if db.clip_handed_out(id)? {
            db.take_back_clip(id)?;
        } else {
            delete_file(old);
        }
    }
    if recorded && result.is_ok() && !vod.audio_only {
        drop(db);
        if let Err(e) = clips::thumbnail(&ffmpeg, &out, &thumb_path(app, id)) {
            eprintln!("clip {id}: its picture: {e:#}");
        }
        emit(app, &state.db.lock().unwrap(), id);
    } else if recorded {
        emit(app, &db, id);
    }
    delete_leftovers();
    Ok(())
}

/// Whether clip `id` goes with VOD `vod_id` as it's taken out of the
/// library (`Cutter::stop_vod`).
fn on_bar_of(db: &Db, id: i64, vod_id: i64) -> bool {
    // The clips of its own clip bar only, which go with it: edits keep their
    // pieces of it, and their clip bars' clips too.
    db.clip(id).ok().flatten().is_some_and(|c| c.bar == Some(ClipBar::Vod { id: vod_id }))
}

/// The history a change to `clip` goes in: that of the VOD or edit whose
/// clip bar it's on.
fn bar_doc(clip: &Clip) -> Doc {
    match clip.bar {
        Some(ClipBar::Edit { id }) => Doc::Edit(id),
        Some(ClipBar::Vod { id }) => Doc::Vod(id),
        None => Doc::Vod(clip.vod_id),
    }
}

/// The clips on edit `id`'s clip bar, in their order.
#[tauri::command]
pub(crate) async fn edit_clips(app: AppHandle, id: i64) -> CmdResult<Vec<Clip>> {
    crate::with_reads(app, move |db| db.bar_clips(id)).await
}

/// Makes `from`..`to` seconds of edit `edit_id`'s timeline a clip, titled
/// with what's said at its start; on the edit's clip bar unless `shown` is
/// false (one only dragged out of the window).
#[tauri::command]
pub(crate) async fn add_edit_clip(
    app: AppHandle,
    edit_id: i64,
    from: f64,
    to: f64,
    shown: Option<bool>,
) -> CmdResult<Clip> {
    let made = make_edit_clips(&app, edit_id, vec![(from, to)], shown.unwrap_or(true))?;
    Ok(made.into_iter().next().expect("a clip for its one range"))
}

/// Makes each of `ranges` (`from`..`to` seconds of edit `edit_id`'s
/// timeline) a clip on the edit's clip bar, in that order, as one change
/// undo takes back whole (several stretches selected, clipped at once).
#[tauri::command]
pub(crate) async fn add_edit_clips(app: AppHandle, edit_id: i64, ranges: Vec<(f64, f64)>) -> CmdResult<Vec<Clip>> {
    make_edit_clips(&app, edit_id, ranges, true)
}

/// What `add_edit_clip` and `add_edit_clips` do: a clip of each range, each
/// titled with what's said at its start, as one change (all of them, or none
/// if one can't be made); then each is queued to be made. A range with a
/// clip already (C pressed twice) gives that one, made again only if it failed.
fn make_edit_clips(app: &AppHandle, edit_id: i64, ranges: Vec<(f64, f64)>, shown: bool) -> CmdResult<Vec<Clip>> {
    let state = app.state::<AppState>();
    let (made, queued) = {
        let db = state.db.lock().unwrap();
        let doc = Doc::Edit(edit_id);
        let made = make_edit_clips_in(&db, &state.history, edit_id, ranges, shown).map_err(err)?;
        history::changed(app, &db, doc);
        made
    };
    state.clips.queue_made(&queued, shown);
    Ok(made)
}

/// What `make_edit_clips` does in the library, recorded in `history`: the
/// clips, and those of them to be made.
fn make_edit_clips_in(
    db: &Db,
    history: &history::History,
    edit_id: i64,
    ranges: Vec<(f64, f64)>,
    shown: bool,
) -> Result<(Vec<Clip>, Vec<i64>)> {
    let label = if ranges.len() == 1 { "Clip added" } else { "Clips added" };
    history.change(db, Doc::Edit(edit_id), label, |db| {
        let (mut made, mut queued) = (Vec::new(), Vec::new());
        for (from, to) in ranges {
            let title = db.span_title(edit_id, from, to)?;
            let had = db.edit_clip_of(edit_id, from, to)?;
            let mut clip = db.add_edit_clip(edit_id, from, to, &title, shown)?;
            match had {
                None => queued.push(clip.id),
                Some(had) if had.status == ClipStatus::Error => {
                    db.set_edit_render(clip.id, ClipStatus::Waiting, None, None, None)?;
                    clip = db.clip(clip.id)?.context("That clip is gone.")?;
                    queued.push(clip.id);
                }
                Some(_) => {}
            }
            made.push(clip);
        }
        Ok((made, queued))
    })
}

#[tauri::command]
pub(crate) async fn list_clips(app: AppHandle, vod_id: i64) -> CmdResult<Vec<Clip>> {
    crate::with_reads(app, move |db| db.clips(vod_id)).await
}

/// Makes a clip of VOD `vod_id` from `start` to `end` seconds, titled
/// with what's said in it; in the clip bar unless `shown` is false (one only
/// dragged out of the window).
#[tauri::command]
pub(crate) async fn add_clip(
    app: AppHandle,
    vod_id: i64,
    start: f64,
    end: f64,
    shown: Option<bool>,
) -> CmdResult<Clip> {
    let made = make_clips(&app, vod_id, vec![(start, end)], shown.unwrap_or(true))?;
    Ok(made.into_iter().next().expect("a clip for its one range"))
}

/// Makes each of `ranges` (`start`..`end` seconds of VOD `vod_id`) a clip in
/// its clip bar, in that order, as one change undo takes back whole (several
/// stretches selected, clipped at once).
#[tauri::command]
pub(crate) async fn add_clips(app: AppHandle, vod_id: i64, ranges: Vec<(f64, f64)>) -> CmdResult<Vec<Clip>> {
    make_clips(&app, vod_id, ranges, true)
}

/// What `add_clip` and `add_clips` do: a clip of each range (its end kept
/// within the VOD), each titled with what's said in it, as one change (all of
/// them, or none if one can't be made); then each is queued to be made.
fn make_clips(app: &AppHandle, vod_id: i64, ranges: Vec<(f64, f64)>, shown: bool) -> CmdResult<Vec<Clip>> {
    let state = app.state::<AppState>();
    let made = make_clips_in(&state.db.lock().unwrap(), &state.history, vod_id, ranges, shown).map_err(err)?;
    state.clips.queue_made(&made.iter().map(|c| c.id).collect::<Vec<_>>(), shown);
    Ok(made)
}

/// What `make_clips` does in the library, recorded in `history`.
fn make_clips_in(
    db: &Db,
    history: &history::History,
    vod_id: i64,
    ranges: Vec<(f64, f64)>,
    shown: bool,
) -> Result<Vec<Clip>> {
    db.vod(vod_id)?.context("That VOD is gone.")?;
    let label = if ranges.len() == 1 { "Clip added" } else { "Clips added" };
    history.change(db, Doc::Vod(vod_id), label, |db| {
        let mut made = Vec::new();
        for (start, end) in ranges {
            let words = db.words_between(vod_id, start, end)?;
            made.push(db.add_clip(vod_id, start, end, &clips::title(&words, start), shown)?);
        }
        Ok(made)
    })
}

/// Moves clip `id` to `start`..`end`; its file is made again, named for the new range.
#[tauri::command]
pub(crate) async fn set_clip_range(app: AppHandle, id: i64, start: f64, end: f64) -> CmdResult<Clip> {
    let state = app.state::<AppState>();
    let clip = {
        let db = state.db.lock().unwrap();
        let clip = db.clip(id).map_err(err)?.ok_or("That clip is gone.")?;
        history::change(&app, &db, bar_doc(&clip), "Clip resized", |db| db.set_clip_range(id, start, end))
            .map_err(err)?;
        db.clip(id).map_err(err)?.ok_or("That clip is gone.")?
    };
    state.clips.stop(id);
    state.clips.queue(id);
    Ok(clip)
}

/// Puts clip `id` (one dragged out of the window) in the clip bar.
#[tauri::command]
pub(crate) async fn keep_clip(app: AppHandle, id: i64) -> CmdResult<Clip> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let doc = bar_doc(&db.clip(id).map_err(err)?.ok_or("That clip is gone.")?);
    history::change(&app, &db, doc, "Clip added", |db| db.show_clip(id)).map_err(err)?;
    // News of it too, in order with the cutter's (the page keeps the newest).
    emit(&app, &db, id);
    db.clip(id).map_err(err)?.ok_or_else(|| "That clip is gone.".into())
}

#[tauri::command]
pub(crate) async fn rename_clip(app: AppHandle, id: i64, title: String) -> CmdResult<Clip> {
    let title = clips::written_title(&title).ok_or("A clip needs a name.")?;
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let doc = bar_doc(&db.clip(id).map_err(err)?.ok_or("That clip is gone.")?);
    history::change(&app, &db, doc, "Clip renamed", |db| db.rename_clip(id, &title)).map_err(err)?;
    emit(&app, &db, id);
    db.clip(id).map_err(err)?.ok_or_else(|| "That clip is gone.".into())
}

/// Deletes clip `id`, and its file unless that was dragged out of the app
/// (it's someone else's then). One only dragged out, not in the clip bar,
/// leaves no step to undo: the bar is as it was.
#[tauri::command]
pub(crate) async fn delete_clip(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    let Some(clip) = db.clip(id).map_err(err)? else { return Ok(()) };
    history::change(&app, &db, bar_doc(&clip), "Clip deleted", |db| remove(&app, db, id)).map_err(err)
}

/// Deletes clips `ids` (several selected), as one change in each clip bar's
/// history: undone, they all come back. All of them go, or none do.
#[tauri::command]
pub(crate) async fn delete_clips(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    let gone = delete_clips_in(&db, &state.history, &ids, |id| state.clips.stop(id)).map_err(err)?;
    // Their files once they're gone from the library, as `remove` does for one.
    for (clip, handed_out) in gone {
        after_delete(&app, clip.id, clip.file_path, handed_out);
    }
    Ok(())
}

/// What `delete_clips` does in the library: clips `ids` deleted in one
/// transaction, each clip bar's change recorded in `history`. One that's
/// gone, or one that can't be deleted, leaves all of them as they were and
/// records nothing. `stop` is given each one's id before it goes (its file
/// being made is stopped). Returns those deleted, each with whether its file
/// was dragged out of the app (not the app's to delete then).
fn delete_clips_in(db: &Db, history: &history::History, ids: &[i64], stop: impl Fn(i64)) -> Result<Vec<(Clip, bool)>> {
    let mut gone = Vec::new();
    for &id in ids {
        let clip = db.clip(id)?.context("That clip is gone.")?;
        let handed_out = db.clip_handed_out(id)?;
        gone.push((clip, handed_out));
    }
    // Each clip bar as it was before any of them went.
    let mut docs: Vec<(Doc, Option<history::Snapshot>)> = Vec::new();
    for (clip, _) in &gone {
        let doc = bar_doc(clip);
        if !docs.iter().any(|(d, _)| *d == doc) {
            docs.push((doc, history::snapshot(db, doc).ok().flatten()));
        }
    }
    for (clip, _) in &gone {
        stop(clip.id);
    }
    db.atomically(|db| gone.iter().try_for_each(|(clip, _)| db.delete_clip(clip.id)))?;
    for (doc, before) in docs {
        let label =
            if gone.iter().filter(|(c, _)| bar_doc(c) == doc).count() == 1 { "Clip deleted" } else { "Clips deleted" };
        history.record(db, doc, label, before);
    }
    Ok(gone)
}

/// Takes off a clip bar (VOD `vod_id`'s, or edit `edit_id`'s) the clips
/// whose file was deleted outside the app (in a file manager), as one change
/// undo can take back. Only where
/// the file's folder is still there: a drive that's away (unmounted,
/// unplugged) takes none of them.
#[tauri::command]
pub(crate) async fn forget_gone_clips(
    app: AppHandle,
    state: State<'_, AppState>,
    vod_id: Option<i64>,
    edit_id: Option<i64>,
) -> CmdResult<()> {
    let doc = match (vod_id, edit_id) {
        (Some(vod), None) => Doc::Vod(vod),
        (None, Some(edit)) => Doc::Edit(edit),
        _ => return Err("Say whose clip bar: a VOD's or an edit's.".into()),
    };
    let finished = |clip: &Clip| clip.shown && clip.status == ClipStatus::Ready;
    let files: Vec<(i64, String)> = {
        let db = state.db.lock().unwrap();
        let clips = match doc {
            Doc::Vod(vod) => db.clips(vod),
            Doc::Edit(edit) => db.bar_clips(edit),
        }
        .map_err(err)?;
        clips.into_iter().filter(finished).filter_map(|c| Some((c.id, c.file_path?))).collect()
    };
    // Looked at without the library held: a sleeping disk or a network folder
    // can take a while to answer.
    let gone = tauri::async_runtime::spawn_blocking(move || {
        files
            .into_iter()
            .filter(|(_, path)| {
                let path = Path::new(path);
                !path.exists() && path.parent().is_some_and(Path::is_dir)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(err)?;
    let db = state.db.lock().unwrap();
    // A change of its own in the bar's history: undone, the clips come back
    // (made again), apart from what was done before or after.
    // Only those as they were when looked at (not made again meanwhile).
    let mut taken = Vec::new();
    for (id, path) in gone {
        if db.clip(id).map_err(err)?.is_some_and(|c| finished(&c) && c.file_path.as_deref() == Some(&*path)) {
            taken.push(id);
        }
    }
    let label = if taken.len() == 1 { "Clip's file deleted" } else { "Clips' files deleted" };
    history::change(&app, &db, doc, label, |db| taken.iter().try_for_each(|&id| remove(&app, db, id))).map_err(err)
}

/// Deletes clip `id` (taking it out of the queue, or stopping its cut), and its file unless that was
/// dragged out of the app, and tells the page (`clip-removed`).
pub(crate) fn remove(app: &AppHandle, db: &Db, id: i64) -> Result<()> {
    let state = app.state::<AppState>();
    // Out of the queue, or its encode stopped (its part file deleted).
    state.clips.cancel(id);
    let clip = db.clip(id)?;
    let handed_out = db.clip_handed_out(id)?;
    db.delete_clip(id)?;
    after_delete(app, id, clip.and_then(|c| c.file_path), handed_out);
    Ok(())
}

/// What's left of clip `id` once it's gone from the library: its file
/// (unless it was dragged out of the app, `handed_out`: another app's then)
/// and its thumbnail deleted, and the page told.
pub(crate) fn after_delete(app: &AppHandle, id: i64, file: Option<String>, handed_out: bool) {
    if let Some(path) = file.filter(|_| !handed_out) {
        delete_file(path);
    }
    let _ = std::fs::remove_file(thumb_path(app, id));
    let _ = app.emit("clip-removed", id);
}

/// Shows clip `id`'s file in the system file manager.
#[tauri::command]
pub(crate) async fn show_clip(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let clip = state.db.lock().unwrap().clip(id).map_err(err)?;
    let path = clip.and_then(|c| c.file_path).filter(|p| Path::new(p).is_file());
    app.opener().reveal_item_in_dir(path.ok_or("That clip's file isn't made yet.")?).map_err(err)
}

/// VOD `vod_id`'s marks on its timeline, in time order.
#[tauri::command]
pub(crate) async fn list_marks(app: AppHandle, vod_id: i64) -> CmdResult<Vec<Mark>> {
    crate::with_reads(app, move |db| db.marks(vod_id)).await
}

/// Edit `id`'s own marks on its timeline, in time order: each where its
/// moment plays (`vodscribe_core::edits::place_marks`).
#[tauri::command]
pub(crate) async fn edit_marks(app: AppHandle, id: i64) -> CmdResult<Vec<Mark>> {
    crate::with_reads(app, move |db| Ok(vodscribe_core::edits::place_marks(&db.edit_pieces(id)?, &db.clip_marks(id)?)))
        .await
}

/// Puts a mark (a cut, chapter or highlight) in edit `edit_id` at `time`
/// seconds of its timeline: on the moment of the VOD playing there.
#[tauri::command]
pub(crate) async fn add_edit_mark(
    app: AppHandle,
    state: State<'_, AppState>,
    edit_id: i64,
    time: f64,
    kind: String,
    text: Option<String>,
) -> CmdResult<Mark> {
    if !MARK_KINDS.contains(&kind.as_str()) {
        return Err("That isn't a kind of mark.".into());
    }
    let db = state.db.lock().unwrap();
    let pieces = db.edit_pieces(edit_id).map_err(err)?;
    let (vod_id, at) = vodscribe_core::edits::source_at(&pieces, time).ok_or("A mark has to be within the edit.")?;
    let text = mark_text(text);
    let add = |db: &Db| db.add_clip_mark(edit_id, vod_id, &kind, at, text.as_deref());
    let id = history::change(&app, &db, Doc::Edit(edit_id), added(&kind), add).map_err(err)?;
    Ok(Mark { id, vod_id, kind, start: time, text, detail: Default::default() })
}

/// The history a mark's changes go in: its VOD's, or its edit's.
fn mark_history(db: &Db, id: i64) -> CmdResult<Option<Doc>> {
    Ok(db.mark_doc(id).map_err(err)?.map(|d| match d {
        MarkDoc::Vod(vod) => Doc::Vod(vod),
        MarkDoc::Edit(edit) => Doc::Edit(edit),
    }))
}

/// How undo names adding a mark of `kind`.
fn added(kind: &str) -> &'static str {
    match kind {
        "chapter" => "Chapter added",
        "highlight" => "Highlight added",
        _ => "Cut added",
    }
}

/// What a mark can be (`db::Mark`).
const MARK_KINDS: [&str; 3] = ["cut", "chapter", "highlight"];

/// A mark's title or note (`clips::written_title`).
fn mark_text(text: Option<String>) -> Option<String> {
    text.as_deref().and_then(clips::written_title)
}

/// Puts a mark on VOD `vod_id`'s timeline at `time` seconds: a cut (between
/// two frames) unless `kind` says a chapter or a highlight, with its `text`.
#[tauri::command]
pub(crate) async fn add_mark(
    app: AppHandle,
    state: State<'_, AppState>,
    vod_id: i64,
    time: f64,
    kind: Option<String>,
    text: Option<String>,
) -> CmdResult<Mark> {
    if !(time.is_finite() && time >= 0.0) {
        return Err("A mark has to be within the video.".into());
    }
    let kind = kind.unwrap_or_else(|| "cut".into());
    if !MARK_KINDS.contains(&kind.as_str()) {
        return Err("That isn't a kind of mark.".into());
    }
    let db = state.db.lock().unwrap();
    db.vod(vod_id).map_err(err)?.ok_or("That VOD is gone.")?;
    let add = |db: &Db| db.add_mark(vod_id, &kind, time, mark_text(text).as_deref());
    history::change(&app, &db, Doc::Vod(vod_id), added(&kind), add).map_err(err)
}

/// Sets mark `id`'s title (a chapter's) or note (a highlight's).
#[tauri::command]
pub(crate) async fn set_mark_text(app: AppHandle, id: i64, text: Option<String>) -> CmdResult<()> {
    marking(&app, id, "Text changed", |db, _| db.set_mark_text(id, mark_text(text).as_deref()))
}

/// Runs `change` on mark `id` (false if it's gone), recording it in its
/// VOD's or edit's history as `label` (`history::change`).
fn marking(
    app: &AppHandle,
    id: i64,
    label: &'static str,
    change: impl FnOnce(&Db, Doc) -> anyhow::Result<bool>,
) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let Some(doc) = mark_history(&db, id)? else { return Err("That mark is gone.".into()) };
    let work = |db: &Db| match change(db, doc)? {
        true => Ok(()),
        false => anyhow::bail!("That mark is gone."),
    };
    history::change(app, &db, doc, label, work).map_err(err)
}

#[tauri::command]
pub(crate) async fn move_mark(app: AppHandle, id: i64, time: f64) -> CmdResult<()> {
    if !(time.is_finite() && time >= 0.0) {
        return Err("A mark has to be within the video.".into());
    }
    // In an edit, `time` is of its timeline: the mark goes to the moment playing there.
    marking(&app, id, "Mark moved", |db, doc| match doc {
        Doc::Vod(_) => db.move_mark(id, time),
        Doc::Edit(edit) => match vodscribe_core::edits::source_at(&db.edit_pieces(edit)?, time) {
            Some((vod, at)) => db.move_mark_to(id, vod, at),
            None => anyhow::bail!("A mark has to be within the edit."),
        },
    })
}

#[tauri::command]
pub(crate) async fn delete_mark(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    if state.db.lock().unwrap().mark_doc(id).map_err(err)?.is_none() {
        return Ok(());
    }
    marking(&app, id, "Mark removed", |db, _| db.delete_mark(id).map(|()| true))
}

/// Takes away mark `id`, just added and cancelled (Escape in its title box),
/// as if it had never been made: its adding leaves undo's history too. With
/// anything changed since, it's an ordinary removal.
#[tauri::command]
pub(crate) async fn cancel_mark(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    let Some(doc) = mark_history(&db, id)? else { return Ok(()) };
    let before = history::snapshot(&db, doc).ok().flatten();
    db.delete_mark(id).map_err(err)?;
    if state.history.unrecord(&db, doc) {
        history::changed(&app, &db, doc);
    } else {
        history::record(&app, &db, doc, "Mark removed", before);
    }
    Ok(())
}

/// Whether DaVinci Resolve is installed where its Linux installer puts it:
/// it can't read MP4 clips there, so the interface points to the setting.
#[tauri::command]
pub(crate) fn resolve_installed() -> bool {
    cfg!(target_os = "linux") && Path::new("/opt/resolve/bin/resolve").is_file()
}

/// Clip `id`'s file, if it's made.
fn ready_file(app: &AppHandle, id: i64) -> CmdResult<PathBuf> {
    let clip = app.state::<AppState>().db.lock().unwrap().clip(id).map_err(err)?.ok_or("That clip is gone.")?;
    clip.file_path
        .filter(|_| clip.status == ClipStatus::Ready)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or_else(|| "That clip's file isn't made yet.".into())
}

/// Shown while a clip with no picture (sound alone) is dragged.
const DRAG_ICON: &[u8] = include_bytes!("../icons/64x64.png");

/// Whether a clip whose file isn't made yet can be dragged out of the window:
/// the system's drag begins at once and the file goes to where it's dropped
/// once it's made (a file manager waits for it; an app that takes only files
/// already made gets none, and the page says so: `DragEnded`). Linux, macOS
/// and Windows; there VODSCRIBE_DRAG_WAITS=1 goes back to waiting for the
/// files at the window's edge before the drag begins (dragout.ts).
pub(crate) fn drags_before_made() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos", windows))
        && std::env::var("VODSCRIBE_DRAG_WAITS").as_deref() != Ok("1")
}

/// Whether the page may drag clips out before their files are made
/// (`drags_before_made`); else it waits for them first.
#[tauri::command]
pub(crate) fn drag_before_made() -> bool {
    drags_before_made()
}

/// Where a file being dragged out is: made, at its path; still being made;
/// or not going to be (its clip failed or is gone), and why.
pub(crate) enum Made {
    Ready(PathBuf),
    Making,
    Failed(String),
}

/// Where clip `id`'s file is (`Made`), or None if the library is busy just
/// now (asked on the main thread, which never waits on it for long:
/// elsewhere, `made_in`).
fn made(app: &AppHandle, id: i64) -> Option<Made> {
    let state = app.state::<AppState>();
    let end = Instant::now() + Duration::from_millis(50);
    let db = loop {
        match state.db.try_lock() {
            Ok(db) => break db,
            Err(std::sync::TryLockError::Poisoned(e)) => break e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(std::sync::TryLockError::WouldBlock) => return None,
        }
    };
    Some(made_in(&db, id))
}

/// Where clip `id`'s file is, as `db` has it.
fn made_in(db: &Db, id: i64) -> Made {
    match db.clip(id) {
        Ok(Some(clip)) => match clip.status {
            ClipStatus::Ready => match clip.file_path.map(PathBuf::from).filter(|p| p.is_file()) {
                Some(path) => Made::Ready(path),
                None => Made::Failed("Its file is gone.".into()),
            },
            ClipStatus::Waiting | ClipStatus::Cutting => Made::Making,
            ClipStatus::Error => Made::Failed(clip.error.unwrap_or_else(|| "unknown error".into())),
        },
        Ok(None) => Made::Failed("That clip is gone.".into()),
        Err(e) => Made::Failed(format!("{e:#}")),
    }
}

/// Starts dragging clip `id`'s file out of the window, into another app (a
/// video editor, a file manager), or several clips' files in one drag (the
/// header names them, `3,5,8`, in their order). Takes the clips' ids, not
/// paths, so the interface can't offer any other file. Called while the
/// mouse button is still down; the drag then belongs to the system. A clip
/// whose file is still being made can go (`drags_before_made`): it's made
/// next, and handed over once it's there. False if the drag didn't begin,
/// the button up before it could.
#[tauri::command]
pub(crate) async fn drag_clip(
    app: AppHandle,
    window: tauri::WebviewWindow,
    request: tauri::ipc::Request<'_>,
) -> CmdResult<bool> {
    // The clips in a header, the picture as the request's bytes (a PNG is
    // tens of KB: a JSON array of its numbers took a moment to send).
    let ids: Vec<i64> = request
        .headers()
        .get("clip")
        .and_then(|v| v.to_str().ok()?.split(',').map(|id| id.trim().parse().ok()).collect())
        .filter(|ids: &Vec<i64>| !ids.is_empty())
        .ok_or("No clip named.")?;
    let number = request.headers().get("drag").and_then(|v| v.to_str().ok()?.parse().ok());
    let image = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) if !bytes.is_empty() => Some(bytes.clone()),
        _ => None,
    };
    // (Off the main thread: waits for the library, as long as a cut holds it.)
    // One that failed (its download blocked, say) still goes where the drag
    // can wait for its files: why is said only if it's let go on another app
    // (`start_drag`), and nothing lands there.
    for &id in &ids {
        match made_in(&app.state::<AppState>().db.lock().unwrap(), id) {
            Made::Ready(_) => {}
            Made::Making | Made::Failed(_) if drags_before_made() => {}
            Made::Making => return Err("That clip's file isn't made yet.".into()),
            Made::Failed(why) => return Err(format!("Couldn't make the clip: {why}")),
        }
    }
    // Those still to make go next, in their order: the drag waits for them.
    let cutter = &app.state::<AppState>().clips;
    for &id in ids.iter().rev() {
        if cutter.is_waiting(id) {
            cutter.queue_first(id);
        }
    }
    // The picture the interface drew (the frame over its waveform), if it's a
    // PNG of reasonable size; else the first clip's first frame.
    let drawn = image.filter(|png| png.len() < (2 << 20) && png.starts_with(b"\x89PNG"));
    let image = drawn.or_else(|| std::fs::read(thumb_path(&app, ids[0])).ok());
    let names = {
        let state = app.state::<AppState>();
        let format = Settings::load(&state.paths.settings()).clip_format;
        let db = state.db.lock().unwrap();
        ids.iter().map(|&id| Outgoing { name: drag_name(&db, format, id), job: Some(id) }).collect()
    };
    let asking = app.clone();
    let listed = ids.clone();
    let files = move || listed.iter().map(|&id| made(&asking, id).unwrap_or(Made::Making)).collect();
    // Another app's once one takes them (not when the drag begins: called
    // off, or dropped on nothing, the files are still the app's to delete).
    let out = ids.clone();
    let cutter = app.clone();
    let taken = move || {
        let state = app.state::<AppState>();
        let db = state.db.lock().unwrap();
        for &id in &ids {
            let _ = db.hand_out_clip(id);
        }
    };
    // Let go on another app: their making is listed with the jobs (Cancel
    // there refuses them), until the drag is over.
    let dropped = move |out_now: bool| {
        cutter.state::<AppState>().clips.dragged_out(&out, out_now);
        crate::job_list::changed(&cutter);
    };
    drag_file(window, number, names, files, image, taken, dropped).await
}

/// How the system's drag of files out of the window ended, as the page hears
/// it ("drag-ended"): whether they went to another app; if they never came
/// because one couldn't be made, why (not for one canceled); and whether they
/// didn't because the app they were let go on takes only files already made
/// (`finished_only`, `receiver` its name where the system says it), so the
/// page says so and keeps them in the clip bar to drag again once they are.
#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub(crate) struct DragEnded {
    /// The page's number for the drag (`drag_clip`'s "drag" header), so it
    /// knows its end from another's still pending; none for an edit's.
    pub(crate) drag: Option<u64>,
    pub(crate) delivered: bool,
    pub(crate) failed: Option<String>,
    pub(crate) finished_only: bool,
    pub(crate) receiver: Option<String>,
}

/// A file in a drag out of the window: the name it goes by where it's
/// dropped, known before it's made (macOS names a file promised then), and
/// the job making it, whose progress is shown there meanwhile.
// (macOS's drag reads both, Windows' the name, Linux's neither.)
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct Outgoing {
    pub(crate) name: String,
    pub(crate) job: Option<i64>,
}

/// The name clip `id`'s file goes by, as it's made in `format` (`cut`, or
/// `edits::render_edit` for one changed in the editor): its file's own, if
/// it's made.
fn drag_name(db: &Db, format: vodscribe_core::settings::ClipFormat, id: i64) -> String {
    let named = || -> Result<Option<String>> {
        let Some(clip) = db.clip(id)? else { return Ok(None) };
        let own = clip.file_path.as_deref().filter(|_| clip.status == ClipStatus::Ready);
        if let Some(name) = own.and_then(|p| Path::new(p).file_name()) {
            return Ok(Some(name.to_string_lossy().into_owned()));
        }
        let pieces = db.edit_pieces(id)?;
        if !vodscribe_core::edits::cut_whole(&pieces) {
            let audio_only = pieces.iter().all(|p| db.vod(p.vod_id).ok().flatten().is_some_and(|v| v.audio_only));
            let title = db.edit(id)?.map(|e| e.title).unwrap_or_default();
            return Ok(Some(format!("{}.{}", clips::safe_name(&title), clips::extension(audio_only, format))));
        }
        let Some(vod) = db.vod(clip.vod_id)? else { return Ok(None) };
        let (_, name) = clips::file_path(
            vod.info.channel.as_deref(),
            vod.info.published.as_deref(),
            &vod.info.title,
            (clip.start, clip.end),
            vod.audio_only,
            format,
        );
        Ok(Some(name))
    };
    named().ok().flatten().unwrap_or_else(|| format!("Clip.{}", clips::extension(false, format)))
}

/// Starts the system's drag of the files `files` says are where (ones the
/// app made, or is making; `names` says what each goes by where it's
/// dropped), showing `image` (a PNG) or the app's icon. When
/// it's over (once the files went where they were dropped, or couldn't),
/// `taken` runs if another app took them, then the page hears "drag-ended"
/// (`DragEnded`, with the page's `number` for the drag), so what it does next
/// (deleting a clip made only for the drag) knows. `dropped(true)` once it's
/// let go on another app, `dropped(false)` once it's over. False if none
/// began, the button up before it could (`ReleasedEarly`).
pub(crate) async fn drag_file(
    window: tauri::WebviewWindow,
    number: Option<u64>,
    names: Vec<Outgoing>,
    files: impl Fn() -> Vec<Made> + Send + Sync + 'static,
    image: Option<Vec<u8>>,
    taken: impl Fn() + Send + Sync + 'static,
    dropped: impl Fn(bool) + Send + Sync + 'static,
) -> CmdResult<bool> {
    let image = image.unwrap_or_else(|| DRAG_ICON.to_vec());
    let (tx, rx) = mpsc::channel();
    let on_main = window.clone();
    window
        .run_on_main_thread(move || {
            let started = start_drag(&on_main, number, names, files, image, taken, dropped);
            let _ = tx.send(began(started));
        })
        .map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || rx.recv()).await.map_err(err)?.map_err(err)?
}

/// Whether a drag out of the window began (`start_drag`'s result, as the
/// page hears it): refused because the button came up first, it didn't, which
/// is no error to show; refused otherwise, why.
fn began(started: Result<()>) -> CmdResult<bool> {
    match started {
        Ok(()) => Ok(true),
        Err(e) if e.is::<ReleasedEarly>() => Ok(false),
        Err(e) => Err(format!("{e:#}")),
    }
}

/// What the end of a drag out of the window comes to (`DragEnded`), from what
/// was seen of it: where it ended (`ending`), whether the files were given to
/// the app it was let go on as addresses (`given`), why one couldn't be made
/// (`failure`), whether they were all made as it ended (`made`), and the
/// receiving app's name (`receiver`). Why one failed is said once it's left
/// the window, not called off; why nothing landed, only if let go on another
/// app before they were made.
fn drag_outcome(
    ending: Ending,
    given: bool,
    failure: Option<String>,
    made: bool,
    receiver: Option<String>,
) -> DragEnded {
    let failing = failure.is_some();
    let failed = failure.filter(|why| why != CANCELED);
    match ending {
        Ending::CalledOff | Ending::Back => DragEnded::default(),
        Ending::Nowhere => DragEnded { failed, ..Default::default() },
        Ending::Elsewhere if given => DragEnded { delivered: true, ..Default::default() },
        Ending::Elsewhere if failing => DragEnded { failed, ..Default::default() },
        // It took nothing, though they were all there: its own business.
        Ending::Elsewhere if made => DragEnded::default(),
        Ending::Elsewhere => DragEnded { finished_only: true, receiver, ..Default::default() },
    }
}

/// The files `made` says are where, if all are made: their paths; else, if
/// one won't be, why; else (some still being made) None. (Linux's drag; the
/// others ask each file's.)
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn settled(made: Vec<Made>) -> Option<Result<Vec<PathBuf>, String>> {
    let mut paths = Vec::new();
    let mut making = false;
    for m in made {
        match m {
            Made::Ready(path) => paths.push(path),
            Made::Making => making = true,
            Made::Failed(why) => return Some(Err(why)),
        }
    }
    (!making).then_some(Ok(paths))
}

/// How often a drag waiting for its files looks at them.
#[cfg(target_os = "linux")]
const HOLD_TICK: Duration = Duration::from_millis(100);

#[cfg(target_os = "linux")]
thread_local! {
    /// Asks for a drag's files being held (`start_drag`), one inside another.
    static HOLDING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// A line about a drag's progress, in the tests' builds only.
#[cfg(all(any(target_os = "linux", windows), feature = "e2e"))]
fn trace(what: &str) {
    eprintln!("drag: {what}");
}
#[cfg(all(any(target_os = "linux", windows), not(feature = "e2e")))]
fn trace(_: &str) {}

/// The window a drag is over, asking for its data or let go on (`context`'s
/// destination), if it's another app's: X11 says; Wayland doesn't.
#[cfg(target_os = "linux")]
fn other_window(context: &gtk::gdk::DragContext) -> Option<gtk::gdk::Window> {
    use gtk::glib::translate::{FromGlibPtrNone, ToGlibPtr};
    // SAFETY: the context is alive for the call; the window it gives may be
    // null (none known), checked before it's taken.
    let window = unsafe {
        let window = gtk::gdk::ffi::gdk_drag_context_get_dest_window(context.to_glib_none().0);
        if window.is_null() {
            return None;
        }
        gtk::gdk::Window::from_glib_none(window)
    };
    (window.window_type() == gtk::gdk::WindowType::Foreign).then_some(window)
}

/// Whether the window asking a drag for its data (`context`'s destination)
/// is one of the app's own (X11 says; elsewhere there may be none).
#[cfg(target_os = "linux")]
fn asked_by_own_window(context: &gtk::gdk::DragContext) -> bool {
    use gtk::glib::translate::ToGlibPtr;
    // SAFETY: the context is alive for the call; the window it gives may be
    // null (none known), checked before its type is read.
    unsafe {
        let window = gtk::gdk::ffi::gdk_drag_context_get_dest_window(context.to_glib_none().0);
        !window.is_null() && gtk::gdk::ffi::gdk_window_get_window_type(window) != gtk::gdk::ffi::GDK_WINDOW_FOREIGN
    }
}

/// Whether an ask for a drag's files (from `context`'s destination, before
/// the drop if not `dropped`) waits for them, while they're still being made:
/// held until they are, or answered at once with none. Apps ask in three
/// ways. Nautilus 43 and later (GTK 4) asks as the drag goes over it and
/// keeps that answer for the drop, asking no more: that ask is held. Qt's
/// apps give up on an answer that waits (after 5 s on X11, 1 on Wayland), and
/// Dolphin, which asks as the drag goes over it, then refuses the drop (so
/// never takes KDE's promise, `ark`): never held. The others (GTK 3's, as
/// Nautilus 42, which asks at each move and takes the drop only once
/// answered) ask again once it's let go there: held only then. Where X11
/// names the window asking, by the toolkit its process runs; where it
/// doesn't (Wayland), as Nautilus 43 is answered, unless the session is
/// KDE's, whose apps are Qt's.
#[cfg(target_os = "linux")]
fn holds_ask(context: &gtk::gdk::DragContext, dropped: bool) -> bool {
    let toolkit = match other_window(context) {
        Some(window) => window_pid(&window).and_then(toolkit_of),
        None if kde_session() => Some(Toolkit::Qt),
        None => Some(Toolkit::Gtk4),
    };
    match toolkit {
        Some(Toolkit::Qt) => false,
        Some(Toolkit::Gtk4) | None => true,
        Some(Toolkit::Other) => dropped,
    }
}

/// The toolkit an app asking for a drag's files is made with, as it asks
/// (`holds_ask`).
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Debug)]
enum Toolkit {
    Qt,
    Gtk4,
    Other,
}

/// Whether this is a KDE session.
#[cfg(target_os = "linux")]
fn kde_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("kde")))
}

/// The process another app's window `window` is of, as it says (_NET_WM_PID).
#[cfg(target_os = "linux")]
fn window_pid(window: &gtk::gdk::Window) -> Option<u32> {
    let atom = gtk::gdk::Atom::intern;
    let (_, _, bytes) = gtk::gdk::property_get(window, &atom("_NET_WM_PID"), &atom("CARDINAL"), 0, 1, 0)?;
    // A 32-bit property comes as the machine's longs.
    match bytes.len() {
        8.. => u64::from_ne_bytes(bytes[..8].try_into().ok()?).try_into().ok(),
        4.. => Some(u32::from_ne_bytes(bytes[..4].try_into().ok()?)),
        _ => None,
    }
}

/// The toolkit process `pid` runs, by the libraries it has loaded; None if
/// that can't be read (another user's, or in a sandbox of its own).
#[cfg(target_os = "linux")]
fn toolkit_of(pid: u32) -> Option<Toolkit> {
    let maps = std::fs::read_to_string(format!("/proc/{pid}/maps")).ok()?;
    Some(if maps.contains("/libQt") {
        Toolkit::Qt
    } else if maps.contains("/libgtk-4") {
        Toolkit::Gtk4
    } else {
        Toolkit::Other
    })
}

/// Another app's window `window`'s class (WM_CLASS's second part: "dolphin",
/// "org.gnome.Nautilus").
#[cfg(target_os = "linux")]
fn window_class(window: &gtk::gdk::Window) -> Option<String> {
    let atom = gtk::gdk::Atom::intern;
    let (_, _, bytes) = gtk::gdk::property_get(window, &atom("WM_CLASS"), &atom("STRING"), 0, 1024, 0)?;
    let class = bytes.split(|&b| b == 0).nth(1).filter(|c| !c.is_empty())?;
    Some(String::from_utf8_lossy(class).into_owned())
}

/// The name an app whose windows are of class `class` goes by: its desktop
/// entry's (the one naming that class, or named for it: "Files" for
/// org.gnome.Nautilus), else the class (its last part, if it's a reverse
/// domain name).
#[cfg(target_os = "linux")]
fn app_name(class: &str) -> String {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let dirs = home.into_iter().chain(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    let entries = dirs.flat_map(|d| std::fs::read_dir(d.join("applications")).into_iter().flatten().flatten());
    let mut named = None;
    for entry in entries {
        let path = entry.path();
        let Some(stem) =
            path.file_stem().and_then(|s| s.to_str()).filter(|_| path.extension().is_some_and(|e| e == "desktop"))
        else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let (name, wm_class) = desktop_entry(&text);
        let Some(name) = name else { continue };
        if wm_class.is_some_and(|c| c.eq_ignore_ascii_case(class)) {
            return name;
        }
        let stem = stem.to_ascii_lowercase();
        let class = class.to_ascii_lowercase();
        if named.is_none() && (stem == class || stem.ends_with(&format!(".{class}"))) {
            named = Some(name);
        }
    }
    named.unwrap_or_else(|| match class.matches('.').count() {
        2.. => class.rsplit('.').next().unwrap_or(class).to_string(),
        _ => class.to_string(),
    })
}

/// A desktop entry's name and the window class it says its windows have (its
/// [Desktop Entry] group's Name and StartupWMClass).
#[cfg(target_os = "linux")]
fn desktop_entry(text: &str) -> (Option<String>, Option<&str>) {
    let (mut name, mut class) = (None, None);
    let mut main = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
        } else if main && let Some(v) = line.strip_prefix("Name=") {
            name = Some(v.trim().to_string());
        } else if main && let Some(v) = line.strip_prefix("StartupWMClass=") {
            class = Some(v.trim());
        }
    }
    (name.filter(|n| !n.is_empty()), class)
}

/// The system's drag (GTK's) of the files `files` says are where. Each is
/// offered two ways, both answered as they're asked for, so the app it's let
/// go on chooses: their addresses (text/uri-list, any app that takes files),
/// and KDE's promise (`ark`, Dolphin's: it asks the app to put them in its
/// folder, and they're copied there as they're made). An app asks for their
/// addresses as the drag goes over it (a file manager) or once it's let go
/// there. Answered at once when they're all made, and always to the app's own
/// window (the drag brought back over it: it needs no files, and its ask
/// comes inside its own handling of the drag); at once with none to an app
/// that gives up on an answer that waits, or asks again at the drop, until
/// then (`holds_ask`: Qt's; Dolphin then takes the promise); else the answer
/// waits for them, the app going on
/// meanwhile in a main loop run inside the ask (`HOLDING`): a file manager
/// waits as long as it takes. (Meanwhile what other threads send the page,
/// and closures run on the main thread through Tauri, wait for the end of it:
/// Tauri's event loop takes them only between its own turns, not in a loop
/// run inside one; the player's time and the list of jobs are told from the
/// wait itself.) The wait ends when they're made (answered), one fails or
/// goes (no answer: nothing lands), the drag is called off or dropped on
/// nothing, or the app it was let go on gives up waiting (its drop over with
/// nothing taken), with no limit of its own. The drag's end comes only after
/// that (GTK waits for the answer), so the page hears "drag-ended" once the
/// files went, or didn't; for Dolphin, once its copies are written. Let go
/// on an app that took nothing because they weren't made yet, the page hears
/// that it takes finished files only, and its name where X11 gives it.
#[cfg(target_os = "linux")]
fn start_drag(
    window: &tauri::WebviewWindow,
    number: Option<u64>,
    _names: Vec<Outgoing>,
    files: impl Fn() -> Vec<Made> + Send + Sync + 'static,
    image: Vec<u8>,
    taken: impl Fn() + Send + Sync + 'static,
    dropped: impl Fn(bool) + Send + Sync + 'static,
) -> Result<()> {
    use gtk::prelude::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    // A drag begun while the last one's answer waits would run inside it.
    if HOLDING.with(Cell::get) > 0 {
        anyhow::bail!("Still handing over the last drag.");
    }
    /// What's known of the drag as it goes.
    #[derive(Default)]
    struct Drag {
        /// Another app was given all the files' addresses.
        given: Cell<bool>,
        /// Called off, or dropped where nothing took it.
        failed: Cell<bool>,
        /// Called off (Escape).
        called_off: Cell<bool>,
        /// Over (GTK's drag-end), its end put off while an answer waits.
        ended: Cell<bool>,
        /// Answers waiting (`hold`), one inside another.
        holding: Cell<u32>,
        /// Why the files never came, if one couldn't be made.
        failure: RefCell<Option<String>>,
        /// The drag is over the app's own page (its drag-motion, drag-drop).
        over_page: Cell<bool>,
        /// The class of the other app's window it was let go on, where X11 says.
        receiver: RefCell<Option<String>>,
        /// The files were all made as it ended.
        made: Cell<bool>,
        /// Let go (GTK's drop-performed), anywhere.
        dropped: Cell<bool>,
    }
    let drag = Rc::new(Drag::default());
    let source = window.gtk_window()?;
    let page = crate::webview::page(window).ok();
    let files: Arc<dyn Fn() -> Vec<Made> + Send + Sync> = Arc::new(files);
    let dropped = Arc::new(dropped);
    let targets = gtk::TargetList::new(&[]);
    targets.add_uri_targets(0);
    // (Apps that don't know it never ask: Nautilus asks only for the addresses.)
    let ark = ark::Offer::new(files.clone());
    if ark.is_some() {
        let other = gtk::TargetFlags::OTHER_APP.bits();
        targets.add(&gtk::gdk::Atom::intern("application/x-kde-ark-dndextract-service"), other, ark::SERVICE);
        targets.add(&gtk::gdk::Atom::intern("application/x-kde-ark-dndextract-path"), other, ark::OBJECT);
    }
    let handlers: Rc<RefCell<Vec<(gtk::Widget, glib::SignalHandlerId)>>> = Rc::default();
    let widget: gtk::Widget = source.clone().upcast();
    // Escaped, so names with spaces and other characters arrive whole; one
    // address for each file, as a list of them.
    let answer = |data: &gtk::SelectionData, paths: &[PathBuf]| {
        let uris: Vec<_> = paths.iter().filter_map(|p| glib::filename_to_uri(p, None).ok()).collect();
        data.set_uris(&uris.iter().map(|u| u.as_str()).collect::<Vec<_>>());
    };
    // Waits, running the app's main loop, until the files are all made or
    // one won't be, or the drag is over.
    let hold = {
        let (drag, files, app) = (drag.clone(), files.clone(), window.app_handle().clone());
        move || {
            HOLDING.with(|h| h.set(h.get() + 1));
            drag.holding.set(drag.holding.get() + 1);
            let main = glib::MainLoop::new(None, false);
            let (quit, looked, watched, app) = (main.clone(), files.clone(), drag.clone(), app.clone());
            let ticks = Cell::new(0u32);
            // (The tick ends itself as it ends the wait.)
            glib::timeout_add_local(HOLD_TICK, move || {
                if watched.failed.get() || watched.ended.get() || settled(looked()).is_some() {
                    quit.quit();
                    return glib::ControlFlow::Break;
                }
                crate::player::retell(&app);
                ticks.set(ticks.get() + 1);
                if ticks.get().is_multiple_of(5) {
                    crate::job_list::retell(&app);
                }
                glib::ControlFlow::Continue
            });
            main.run();
            drag.holding.set(drag.holding.get() - 1);
            HOLDING.with(|h| h.set(h.get() - 1));
        }
    };
    let asks = Rc::new(Cell::new(0u32));
    let data = {
        let (drag, files, ark) = (drag.clone(), files.clone(), ark.clone());
        source.connect_drag_data_get(move |_, context, data, info, _| {
            asks.set(asks.get() + 1);
            let n = asks.get();
            if drag.failed.get() || drag.ended.get() {
                return;
            }
            if let Some(ark) = ark.as_ref().filter(|_| info == ark::SERVICE || info == ark::OBJECT) {
                let named = if info == ark::SERVICE { &ark.service } else { &ark.path };
                data.set(&data.target(), 8, named.as_bytes());
                return;
            }
            // The app's own page: what's made so far, at once.
            if drag.over_page.get() || asked_by_own_window(context) {
                let ready: Vec<_> =
                    files().into_iter().filter_map(|m| if let Made::Ready(p) = m { Some(p) } else { None }).collect();
                trace(&format!(
                    "ask {n} ({}) by the app's own window: answered with the {} made",
                    data.target().name(),
                    ready.len()
                ));
                answer(data, &ready);
                return;
            }
            let mut now = settled(files());
            if now.is_none() && !holds_ask(context, drag.dropped.get()) {
                trace(&format!("ask {n} ({}) answered at once with none: not one to hold", data.target().name()));
                answer(data, &[]);
                return;
            }
            if now.is_none() {
                trace(&format!("ask {n} ({}): held until the files are made", data.target().name()));
                hold();
                now = if drag.failed.get() || drag.ended.get() { None } else { settled(files()) };
            }
            match now {
                Some(Ok(paths)) => {
                    trace(&format!("ask {n} ({}): answered with {} files", data.target().name(), paths.len()));
                    answer(data, &paths);
                    drag.given.set(true);
                }
                Some(Err(why)) => {
                    trace(&format!("ask {n}: not answered, a file couldn't be made: {why}"));
                    *drag.failure.borrow_mut() = Some(why);
                }
                None => trace(&format!("ask {n}: not answered, the drag is over")),
            }
        })
    };
    handlers.borrow_mut().push((widget.clone(), data));
    // Over the app's own page: an ask then is its own (as X11 tells by the
    // window asking; Wayland doesn't). GTK leaves the page just before a drop
    // on it, so the drop says so again.
    if let Some(page) = &page {
        let over = drag.clone();
        let motion = page.connect_drag_motion(move |_, _, _, _, _| {
            over.over_page.set(true);
            false
        });
        let over = drag.clone();
        let dropped = page.connect_drag_drop(move |_, _, _, _, _| {
            over.over_page.set(true);
            false
        });
        let over = drag.clone();
        let left = page.connect_drag_leave(move |_, _, _| over.over_page.set(false));
        handlers.borrow_mut().extend([(page.clone(), motion), (page.clone(), dropped), (page.clone(), left)]);
    }
    // Dropped where nothing takes it: no animation of it flying back, which
    // holds on to the pointer (losing the next press) until it's done.
    let failing = drag.clone();
    let on_failed = source.connect_drag_failed(move |_, _, result| {
        trace(&format!("failed: {result:?}"));
        failing.failed.set(true);
        failing.called_off.set(result == gtk::DragResult::UserCancelled);
        glib::Propagation::Stop
    });
    handlers.borrow_mut().push((widget.clone(), on_failed));
    // Once it's over, dropped or not, the window stops answering for it,
    // the files went (`taken`), and the page is told.
    let finish = {
        let (drag, handlers, told, ark) = (drag.clone(), handlers.clone(), window.clone(), ark.clone());
        let dropped = dropped.clone();
        let taken = Arc::new(taken);
        move || {
            for (widget, handler) in handlers.borrow_mut().drain(..) {
                widget.disconnect(handler);
            }
            let page = crate::webview::page(&told);
            let ending = match () {
                _ if drag.called_off.get() => Ending::CalledOff,
                _ if drag.over_page.get() => Ending::Back,
                _ if drag.failed.get() => Ending::Nowhere,
                _ => Ending::Elsewhere,
            };
            // Dolphin asked for them (`ark`): the page told once its copies
            // are written. They're its own: the app's aren't handed out.
            let copying = ark.as_ref().is_some_and(|ark| {
                let (told, dropped) = (told.clone(), dropped.clone());
                ark.ended(move |failed| {
                    let failed = failed.filter(|why| why != CANCELED);
                    trace(&format!("over: copied for Dolphin, failed {failed:?}"));
                    dropped(false);
                    let ended = DragEnded { drag: number, delivered: failed.is_none(), failed, ..Default::default() };
                    let _ = told.emit("drag-ended", ended);
                })
            });
            if !copying {
                let (given, failure) = (drag.given.get(), drag.failure.borrow_mut().take());
                let (receiver, made) = (drag.receiver.borrow_mut().take(), drag.made.get());
                // Taken (above), then the page told, so a drag of its own
                // coming back in isn't taken for one to add. Off the main
                // thread, for the library's lock.
                let (taken, told, dropped) = (taken.clone(), told.clone(), dropped.clone());
                std::thread::spawn(move || {
                    let outcome = drag_outcome(ending, given, failure, made, receiver.as_deref().map(app_name));
                    let ended = DragEnded { drag: number, ..outcome };
                    trace(&format!("over: {ended:?}"));
                    if ended.delivered {
                        taken();
                    }
                    dropped(false);
                    let _ = told.emit("drag-ended", ended);
                });
            }
            // The press that started the drag never comes back up in the page:
            // the drag took the release. Without one, its next press is lost.
            if let Ok(page) = page {
                release_button(&page);
            }
        }
    };
    let finish = Rc::new(finish);
    let ending = (drag.clone(), finish.clone(), files.clone());
    let end = source.connect_drag_end(move |_, context| {
        let (drag, finish, files) = (&ending.0, &ending.1, &ending.2);
        drag.ended.set(true);
        // The app it was let go on, while the drag still says; and whether
        // the files were made by then (taken, or not, as they were).
        if !drag.failed.get() && !drag.over_page.get() {
            *drag.receiver.borrow_mut() = other_window(context).and_then(|w| window_class(&w));
            drag.made.set(settled(files()).is_some_and(|r| r.is_ok()));
        }
        if drag.holding.get() == 0 {
            finish();
        } else {
            // Over while an answer waits (called off during it, or the app it
            // was let go on gave up): finished once that wait is, outside it.
            let (drag, finish) = (drag.clone(), finish.clone());
            glib::timeout_add_local(HOLD_TICK, move || {
                if drag.holding.get() > 0 {
                    return glib::ControlFlow::Continue;
                }
                finish();
                glib::ControlFlow::Break
            });
        }
    });
    handlers.borrow_mut().push((widget.clone(), end));
    // Its picture, set as it begins: else GTK sets its own icon then, loading
    // it from the icon theme through gdk-pixbuf, and aborts the app if no
    // loader can read it (an SVG, where the loaders are sandboxed away).
    // (One that can't be decoded: none to see, rather than a theme icon.)
    let picture =
        drag_picture(&image).or_else(|| gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 1, 1).ok());
    let over = drag.clone();
    let begin = source.connect_drag_begin(move |_, context| {
        if let Some(surface) = &picture {
            context.drag_set_icon_surface(surface);
        }
        // Let go on another app: listed with the jobs from now (`dropped`).
        let (over, dropped) = (over.clone(), dropped.clone());
        context.connect_drop_performed(move |context, _| {
            over.dropped.set(true);
            if !over.over_page.get() && !asked_by_own_window(context) {
                dropped(true);
            }
        });
    });
    handlers.borrow_mut().push((widget, begin));
    let undo = || {
        for (widget, handler) in handlers.borrow_mut().drain(..) {
            widget.disconnect(handler);
        }
        // Never asked for: off the bus.
        if let Some(ark) = &ark {
            ark.ended(|_| {});
        }
    };
    // A drag begun with no button held never ends (nothing releases it), and
    // the file it was for is never forgotten: the release may have come in
    // the moment between the page asking and this. Refused, so the page
    // treats it as a drag that didn't happen.
    let held = source.window().and_then(|w| {
        let pointer = w.display().default_seat()?.pointer()?;
        let (_, _, _, state) = w.device_position(&pointer);
        Some(state.contains(gtk::gdk::ModifierType::BUTTON1_MASK))
    });
    if held == Some(false) {
        undo();
        return Err(ReleasedEarly.into());
    }
    if source.drag_begin_with_coordinates(&targets, gtk::gdk::DragAction::COPY, 1, None, -1, -1).is_none() {
        undo();
        anyhow::bail!("The drag couldn't start.");
    }
    Ok(())
}

/// KDE's way of taking files not made yet (Ark's, which Dolphin takes): the
/// drag names a D-Bus object of the app's (two more targets: the app's name
/// on the session bus, the object's path); let go on Dolphin, it asks the
/// object to put the files in its folder (`extractSelectedFilesTo`), its
/// drop done at once, and the app copies each file there as it's made, one
/// by one. Offered in every drag: apps that don't know it never ask for it.
#[cfg(target_os = "linux")]
mod ark {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use gtk::gio;

    use super::{HOLD_TICK, Made};

    /// The drag's targets for it, by their `info`.
    pub(super) const SERVICE: u32 = 1;
    pub(super) const OBJECT: u32 = 2;

    const INTERFACE: &str = r#"<node><interface name="org.kde.ark.DndExtract"><method name="extractSelectedFilesTo"><arg type="s" name="path" direction="in"/></method></interface></node>"#;

    /// What the copies came to, once they're over: why not, if one failed.
    type Then = Box<dyn FnOnce(Option<String>) + Send>;

    #[derive(Default)]
    struct State {
        /// The receiver asked for the files.
        asked: bool,
        /// The copies are over (why not, if they failed), and nobody told yet.
        over: Option<Option<String>>,
        /// What to do once they are (the drag over by then).
        then: Option<Then>,
    }

    /// One drag's object on the bus, while it's offered.
    pub(super) struct Offer {
        conn: gio::DBusConnection,
        id: Mutex<Option<gio::RegistrationId>>,
        /// The app's name on the session bus.
        pub(super) service: String,
        /// The object's path, this drag's own.
        pub(super) path: String,
        state: Arc<Mutex<State>>,
    }

    impl Offer {
        /// Puts the drag's object on the bus: asked, it copies what `files` says
        /// is made into the folder named, one by one. None without a bus.
        pub(super) fn new(files: Arc<dyn Fn() -> Vec<Made> + Send + Sync>) -> Option<Arc<Offer>> {
            static DRAGS: AtomicU32 = AtomicU32::new(0);
            let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
            let service = conn.unique_name()?.to_string();
            let path = format!("/DndExtract/{}", DRAGS.fetch_add(1, Ordering::Relaxed) + 1);
            let node = gio::DBusNodeInfo::for_xml(INTERFACE).ok()?;
            let interface = node.lookup_interface("org.kde.ark.DndExtract")?;
            let state = Arc::new(Mutex::new(State::default()));
            let asked = state.clone();
            let id = conn
                .register_object(
                    &path,
                    &interface,
                    move |_, _, _, _, _, params, invocation| {
                        let folder: String = params.child_value(0).get().unwrap_or_default();
                        // Once: a second call would copy them all again.
                        let first = !std::mem::replace(&mut asked.lock().unwrap().asked, true);
                        invocation.return_value(None);
                        if first {
                            super::trace(&format!("Ark: asked to put them in {folder}"));
                            let (files, state) = (files.clone(), asked.clone());
                            std::thread::spawn(move || {
                                let result = copy_all(&*files, Path::new(&folder));
                                let mut s = state.lock().unwrap();
                                match s.then.take() {
                                    Some(then) => {
                                        drop(s);
                                        then(result.err());
                                    }
                                    None => s.over = Some(result.err()),
                                }
                            });
                        }
                    },
                    |_, _, _, _, _| glib::Variant::from(""),
                    |_, _, _, _, _, _| false,
                )
                .ok()?;
            Some(Arc::new(Offer { conn, id: Mutex::new(Some(id)), service, path, state }))
        }

        /// The drag is over: `then` runs once the files went (why not, if they
        /// didn't), if the receiver asked for them, and true; else false, and
        /// the object goes from the bus.
        pub(super) fn ended(self: &Arc<Self>, then: impl FnOnce(Option<String>) + Send + 'static) -> bool {
            let mut s = self.state.lock().unwrap();
            if !s.asked {
                drop(s);
                self.withdraw();
                return false;
            }
            let me = self.clone();
            let then = move |failed| {
                me.withdraw();
                then(failed);
            };
            match s.over.take() {
                Some(failed) => {
                    drop(s);
                    then(failed);
                }
                None => s.then = Some(Box::new(then)),
            }
            true
        }

        /// Takes the object off the bus.
        fn withdraw(&self) {
            if let Some(id) = self.id.lock().unwrap().take() {
                let _ = self.conn.unregister_object(id);
            }
        }
    }

    /// Copies each of the files `files` says are where into `folder` once it's
    /// made, in their order, each written beside and renamed, named as the
    /// app's (another name if one's there); stops at one that can't be.
    fn copy_all(files: &dyn Fn() -> Vec<Made>, folder: &Path) -> Result<(), String> {
        let mut done = 0;
        loop {
            let now = files();
            let count = now.len();
            for made in now.into_iter().skip(done) {
                match made {
                    Made::Ready(path) => {
                        copy_one(&path, folder).map_err(|e| format!("Couldn't put it in {}: {e}", folder.display()))?;
                        done += 1;
                    }
                    Made::Failed(why) => return Err(why),
                    Made::Making => break,
                }
            }
            if done == count {
                return Ok(());
            }
            std::thread::sleep(HOLD_TICK);
        }
    }

    fn copy_one(from: &Path, folder: &Path) -> std::io::Result<PathBuf> {
        let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "clip".into());
        let part = folder.join(format!(".{name}.part"));
        std::fs::copy(from, &part)?;
        let to = vodscribe_core::clips::unique_path(folder, &name);
        std::fs::rename(&part, &to).inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })?;
        super::trace(&format!("Ark: {} put in the folder", to.display()));
        Ok(to)
    }
}

/// The PNG `png` as a cairo surface for a drag's picture, decoded here.
#[cfg(target_os = "linux")]
fn drag_picture(png: &[u8]) -> Option<gtk::cairo::ImageSurface> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return None,
    };
    let (width, height) = (i32::try_from(info.width).ok()?, i32::try_from(info.height).ok()?);
    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, width, height).ok()?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().ok()?;
        for (y, row) in pixels.chunks(info.line_size).take(info.height as usize).enumerate() {
            for (x, px) in row.chunks(channels).take(info.width as usize).enumerate() {
                let (r, g, b, a) =
                    if channels == 4 { (px[0], px[1], px[2], px[3]) } else { (px[0], px[0], px[0], px[1]) };
                // Cairo's ARGB32: premultiplied, in the machine's byte order (little-endian here: B, G, R, A).
                let pre = |c: u8| (u16::from(c) * u16::from(a) / 255) as u8;
                let at = y * stride + x * 4;
                data[at..at + 4].copy_from_slice(&[pre(b), pre(g), pre(r), a]);
            }
        }
    }
    Some(surface)
}

/// Sends `page` the release of the first mouse button, where the pointer is.
#[cfg(target_os = "linux")]
fn release_button(page: &gtk::Widget) {
    use gtk::glib::translate::{ToGlibPtr, ToGlibPtrMut};
    use gtk::prelude::*;
    let (Some(window), Some(device)) = (page.window(), page.display().default_seat().and_then(|s| s.pointer())) else {
        return;
    };
    let (_, x, y, _) = window.device_position_double(&device);
    let (_, root_x, root_y) = device.position_double();
    let mut event = gtk::gdk::Event::new(gtk::gdk::EventType::ButtonRelease);
    // SAFETY: a button event just made, of the type its fields are written as;
    // the window is referenced for the event, which unreferences it when freed.
    unsafe {
        let button = ToGlibPtrMut::<*mut gtk::gdk::ffi::GdkEvent>::to_glib_none_mut(&mut event).0
            as *mut gtk::gdk::ffi::GdkEventButton;
        (*button).window = ToGlibPtr::<*mut gtk::gdk::ffi::GdkWindow>::to_glib_full(&window);
        (*button).send_event = 1;
        (*button).time = gtk::gdk::ffi::GDK_CURRENT_TIME as u32;
        (*button).x = x;
        (*button).y = y;
        (*button).x_root = root_x;
        (*button).y_root = root_y;
        (*button).button = 1;
        (*button).state = gtk::gdk::ffi::GDK_BUTTON1_MASK;
    }
    event.set_device(Some(&device));
    gtk::main_do_event(&mut event);
}

/// The system's drag (OLE's) of the files `files` says are where, made or
/// not (`drag_source`): it begins at once, each file offered as a virtual
/// file (Explorer copies it once it's made, showing its own dialog) and as
/// itself (given to an app that takes only that once all are made). The page
/// hears "drag-ended" once they went (for Explorer, once its copies are
/// written), or didn't; let go on an app that asked for the files themselves
/// while one was still being made, that it takes finished files only, named
/// as its program describes itself.
#[cfg(windows)]
fn start_drag(
    window: &tauri::WebviewWindow,
    number: Option<u64>,
    names: Vec<Outgoing>,
    files: impl Fn() -> Vec<Made> + Send + Sync + 'static,
    image: Vec<u8>,
    taken: impl Fn() + Send + Sync + 'static,
    dropped: impl Fn(bool) + Send + Sync + 'static,
) -> Result<()> {
    use crate::drag_source::{self, State};

    /// One of the drag's files, the `at`th that `files` says is where.
    struct Polled {
        files: Arc<dyn Fn() -> Vec<Made> + Send + Sync>,
        at: usize,
        name: String,
    }
    impl drag_source::Item for Polled {
        fn name(&self) -> String {
            self.name.clone()
        }
        fn state(&self) -> State {
            match (self.files)().into_iter().nth(self.at) {
                Some(Made::Ready(path)) => State::Ready(path),
                Some(Made::Making) => State::Making,
                Some(Made::Failed(why)) => State::Failed(why),
                None => State::Failed("That clip is gone.".into()),
            }
        }
        // (Explorer's Cancel stops nothing here: the page forgets a clip
        // made only for the drag once it hears it went nowhere.)
    }
    let files: Arc<dyn Fn() -> Vec<Made> + Send + Sync> = Arc::new(files);
    let items = (names.into_iter().enumerate())
        .map(|(at, Outgoing { name, .. })| {
            Box::new(Polled { files: files.clone(), at, name }) as Box<dyn drag_source::Item>
        })
        .collect();
    let hwnd = window.hwnd()?.0 as isize;
    let told = window.clone();
    drag_source::run(
        hwnd,
        items,
        &image,
        // Let go on another app: listed with the jobs from now, until it's over.
        dropped,
        move |outcome| {
            // Taken, then the page told (so a drag of its own coming back in
            // isn't taken for one to add), off the UI thread (the library's lock).
            std::thread::spawn(move || {
                let made = !outcome.refused_unmade;
                let ended = drag_outcome(outcome.ending, outcome.delivered, outcome.failure, made, outcome.receiver);
                let ended = DragEnded { drag: number, ..ended };
                trace(&format!("ended: {ended:?}"));
                if ended.delivered && outcome.taken {
                    taken();
                }
                let _ = told.emit("drag-ended", ended);
            });
        },
        trace,
    )
}

/// The system's drag (AppKit's) of the files `files` says are where, made or
/// being made (`file_drag`): Finder, and any app that takes a file promise,
/// gets each file written where it was dropped once it's made, with its
/// progress and cancel meanwhile; an app that takes files already made gets
/// those that are. Once it's over, `taken` runs if another app was given the
/// app's own files (not copies written for it), then the page is told
/// (`drag_outcome`, as on Linux).
#[cfg(target_os = "macos")]
fn start_drag(
    window: &tauri::WebviewWindow,
    number: Option<u64>,
    names: Vec<Outgoing>,
    files: impl Fn() -> Vec<Made> + Send + Sync + 'static,
    image: Vec<u8>,
    taken: impl Fn() + Send + Sync + 'static,
    dropped: impl Fn(bool) + Send + Sync + 'static,
) -> Result<()> {
    use crate::file_drag::{self, Event, Failure};

    // The window itself (not its web view) takes file drops in tao, which
    // unwraps the dropped files' names: a file not made yet has none, and the
    // app panicked as the drag crossed the window's edge (tao 0.37.1's
    // draggingEntered). Tauri hears drops from the web view, which stays
    // registered for them.
    let ns_window = window.ns_window()? as *const objc2_app_kit::NSWindow;
    // SAFETY: Tauri's window, alive through this call on the main thread.
    unsafe { (*ns_window).unregisterDraggedTypes() };
    let files: Arc<dyn Fn() -> Vec<Made> + Send + Sync> = Arc::new(files);
    let items = names
        .into_iter()
        .enumerate()
        .map(|(index, Outgoing { name, job })| {
            let file = DragFile { app: window.app_handle().clone(), index, name, job, files: files.clone() };
            Arc::new(file) as Arc<dyn file_drag::Item>
        })
        .collect();
    let told = window.clone();
    file_drag::start(window, items, &image, move |event| match event {
        Event::LetGo => dropped(true),
        Event::Ended(ended) => {
            let failure = ended.failure.map(|failure| match failure {
                Failure::Cancelled => CANCELED.to_string(),
                Failure::Failed(why) => why,
            });
            let outcome = drag_outcome(ended.ending, ended.given, failure, ended.made, ended.receiver);
            // Copies written where it was dropped are the other app's own:
            // the app's files aren't handed out.
            if outcome.delivered && !ended.written {
                taken();
            }
            dropped(false);
            let _ = told.emit("drag-ended", DragEnded { drag: number, ..outcome });
        }
    })
}

/// A clip's (or an edit's) file in a drag out of the window, as macOS's drag
/// asks after it (`file_drag::Item`).
#[cfg(target_os = "macos")]
struct DragFile {
    app: AppHandle,
    index: usize,
    name: String,
    job: Option<i64>,
    files: Arc<dyn Fn() -> Vec<Made> + Send + Sync>,
}

#[cfg(target_os = "macos")]
impl crate::file_drag::Item for DragFile {
    fn file_name(&self) -> String {
        self.name.clone()
    }

    fn file_type(&self) -> String {
        let ext = Path::new(&self.name).extension().and_then(|e| e.to_str()).unwrap_or_default();
        match ext.to_ascii_lowercase().as_str() {
            "mp4" => "public.mpeg-4",
            "m4a" => "public.mpeg-4-audio",
            "mov" => "com.apple.quicktime-movie",
            "wav" => "com.microsoft.waveform-audio",
            _ => "public.data",
        }
        .into()
    }

    fn state(&self) -> crate::file_drag::State {
        use crate::file_drag::State;
        match (self.files)().into_iter().nth(self.index) {
            Some(Made::Ready(path)) => State::Ready(path),
            Some(Made::Making) => State::Making(self.job.and_then(|id| crate::job_list::progress_of(&self.app, id))),
            Some(Made::Failed(why)) => State::Failed(why),
            None => State::Failed("That clip is gone.".into()),
        }
    }
}

/// Saves a copy of clip `id`'s file where the person picks, in the system's
/// save dialog (opened here, so the interface can't name a place to write).
/// False if they canceled.
#[tauri::command]
pub(crate) async fn save_clip(app: AppHandle, window: tauri::WebviewWindow, id: i64) -> CmdResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    let from = ready_file(&app, id)?;
    let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = from.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(&name)
            .add_filter(if matches!(ext.as_str(), "m4a" | "wav") { "Audio" } else { "Video" }, &[ext.as_str()])
            .blocking_save_file();
        let Some(to) = picked else { return Ok(false) };
        let to = to.into_path().map_err(err)?;
        if to != from {
            std::fs::copy(&from, &to).map_err(|e| format!("Couldn't save the clip there: {e}"))?;
        }
        Ok(true)
    })
    .await
    .map_err(err)?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip made only to be dragged out was queued last, behind every clip
    /// and edit render waiting, and the drag waited for all of them.
    #[test]
    fn a_clip_made_for_a_drag_out_is_made_next() {
        let cutter = Cutter::new();
        let (began, order) = mpsc::channel();
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        cutter
            .jobs
            .run("cutter-test", move |id, _| {
                began.send(id).unwrap();
                if id == 1 {
                    wait.lock().unwrap().recv().unwrap();
                }
            })
            .unwrap();
        cutter.queue_made(&[1], true);
        assert_eq!(order.recv_timeout(Duration::from_secs(5)), Ok(1));
        // While 1 is being made: two for the clip bar, then two for a drag.
        cutter.queue_made(&[2, 3], true);
        cutter.queue_made(&[8, 9], false);
        go.send(()).unwrap();
        let next: Vec<_> = (0..4).map(|_| order.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
        assert_eq!(next, [8, 9, 2, 3], "the drag's in the order picked, then the rest as they were");
    }

    /// A drag's answer waits while any of its files is still being made,
    /// and is none once one can't be: all or nothing, as one drop.
    #[test]
    fn a_drag_is_answered_once_all_its_files_are_made() {
        let ready = |p: &str| Made::Ready(PathBuf::from(p));
        assert!(settled(vec![ready("/a"), Made::Making]).is_none());
        assert_eq!(settled(vec![ready("/a"), ready("/b")]), Some(Ok(vec![PathBuf::from("/a"), PathBuf::from("/b")])));
        assert_eq!(settled(vec![Made::Making, Made::Failed("no".into())]), Some(Err("no".into())));
    }

    /// What a drag out of the window comes to, by what was seen of it: let go
    /// on an app that took nothing while the files were still being made, it
    /// takes finished files only (named, where the system says); one that
    /// failed is said only once it's let go on another app; called off or
    /// dropped on the app's own window, nothing.
    #[test]
    fn a_drag_ends_as_its_receiver_took_it() {
        use Ending::*;
        let named = || Some("Kdenlive".to_string());
        let blocked = || Some("blocked".to_string());
        let ended = |delivered, failed: Option<&str>, finished_only, receiver: Option<&str>| DragEnded {
            drag: None,
            delivered,
            failed: failed.map(String::from),
            finished_only,
            receiver: receiver.map(String::from),
        };
        assert_eq!(drag_outcome(Elsewhere, true, None, true, named()), ended(true, None, false, None), "given");
        assert_eq!(
            drag_outcome(Elsewhere, false, None, false, named()),
            ended(false, None, true, Some("Kdenlive")),
            "let go before it was made, nothing taken"
        );
        assert_eq!(drag_outcome(Elsewhere, false, None, false, None), ended(false, None, true, None), "name unknown");
        assert_eq!(drag_outcome(Elsewhere, false, None, true, named()), DragEnded::default(), "made, not taken");
        assert_eq!(
            drag_outcome(Elsewhere, false, blocked(), false, named()),
            ended(false, Some("blocked"), false, None)
        );
        assert_eq!(drag_outcome(Nowhere, false, blocked(), false, None), ended(false, Some("blocked"), false, None));
        assert_eq!(drag_outcome(Nowhere, false, None, false, None), DragEnded::default(), "dropped on nothing");
        assert_eq!(drag_outcome(Elsewhere, false, Some(CANCELED.into()), false, named()), DragEnded::default());
        for ending in [CalledOff, Back] {
            for (given, failure) in [(true, None), (false, None), (false, blocked())] {
                assert_eq!(drag_outcome(ending, given, failure, false, named()), DragEnded::default(), "{ending:?}");
            }
        }
    }

    /// The receiving app is named as its desktop entry names it: the entry
    /// naming its window's class, else the class.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_app_is_named_by_its_desktop_entry() {
        let entry = "[Desktop Entry]\nName=Kdenlive\nStartupWMClass=kdenlive\n[Desktop Action new]\nName=New\n";
        assert_eq!(desktop_entry(entry), (Some("Kdenlive".into()), Some("kdenlive")));
        assert_eq!(desktop_entry("[Desktop Entry]\nExec=x\n"), (None, None));
        assert_eq!(app_name("org.example.NoSuchApp"), "NoSuchApp", "a reverse domain name's last part");
        assert_eq!(app_name("No-such-app"), "No-such-app");
    }

    /// A clip made only for a drag out of the window showed in the list of
    /// jobs while the drag was still in the window, where nothing had been let
    /// go yet: it's listed once it's let go on another app, until it's over.
    #[test]
    fn a_drag_clip_is_listed_with_the_jobs_only_once_let_go_elsewhere() {
        let (db, vod) = library();
        let hidden = db.add_clip(vod, 1.0, 2.0, "dragged", false).unwrap().id;
        let pill = db.add_clip(vod, 3.0, 4.0, "pill", true).unwrap().id;
        let cutter = Cutter::new();
        let listed = |id| crate::job_list::describe(&db, crate::job_list::Queue::Render, id, |id| cutter.is_out(id));
        assert!(listed(hidden).is_none(), "listed while in the window's drag");
        assert!(listed(pill).is_some(), "a pill's making");
        cutter.dragged_out(&[hidden], true);
        assert_eq!(listed(hidden).map(|(_, label)| label).as_deref(), Some("dragged"));
        cutter.dragged_out(&[hidden], false);
        assert!(listed(hidden).is_none(), "listed once the drag was over");
    }

    /// Several clips made at once went in one at a time: one that failed
    /// part-way left those before it in the library, waiting to be made, with
    /// no step to undo them.
    #[test]
    fn making_several_clips_is_all_or_none() {
        let (db, vod) = library();
        let history = history::History::default();
        assert!(make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (-1.0, 3.0)], true).is_err());
        assert!(db.clips(vod).unwrap().is_empty(), "none made");
        assert_eq!(history.steps(Doc::Vod(vod)), 0, "no step recorded");
        let made = make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (3.0, 90.0)], true).unwrap();
        assert_eq!(made.iter().map(|c| (c.start, c.end)).collect::<Vec<_>>(), [(1.0, 2.0), (3.0, 60.0)]);
        assert_eq!(history.steps(Doc::Vod(vod)), 1, "one step for both");

        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
        let doc = Doc::Edit(edit);
        let before = history.steps(doc);
        assert!(make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (40.0, 50.0)], true).is_err());
        assert!(db.bar_clips(edit).unwrap().is_empty(), "none made on the edit's bar");
        assert_eq!(history.steps(doc), before, "no step recorded");
        let (made, queued) = make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (3.0, 4.0)], true).unwrap();
        assert_eq!(made.iter().map(|c| c.id).collect::<Vec<_>>(), queued);
        assert_eq!(history.steps(doc), before + 1);
    }

    fn library() -> (Db, i64) {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let info = vodscribe_core::fetch::VodInfo {
            platform: "youtube".into(),
            platform_id: "x".into(),
            url: "https://youtu.be/x".into(),
            title: "t".into(),
            channel: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let vod = db.add_vod(&info).unwrap().0.id;
        (db, vod)
    }

    /// A clip on an edit's clip bar of a VOD taken out of the library stays,
    /// as the edit's pieces of it do: its cut went on stopped, and was never
    /// queued again (only the VOD's own bar is), so it showed being cut until
    /// the app next started.
    #[test]
    fn taking_a_vod_out_stops_only_the_clips_of_its_own_bar() {
        let (db, vod) = library();
        let of_vod = db.add_clip(vod, 1.0, 2.0, "v", true).unwrap().id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
        let of_edit = db.add_edit_clip(edit, 1.0, 2.0, "e", true).unwrap().id;
        assert!(on_bar_of(&db, of_vod, vod), "a clip of the VOD's bar");
        assert!(!on_bar_of(&db, of_edit, vod), "a clip of the edit's bar");
        assert!(!on_bar_of(&db, edit, vod), "the edit");
    }

    /// Deleting an edit took the clips on its clip bar out of the library
    /// (the database's cascade) but left their files in the clips folder,
    /// their cuts running and the page not told: they go as a VOD's do.
    #[test]
    fn deleting_an_edit_releases_its_bar_clips() {
        let (db, vod) = library();
        let of_vod = db.add_clip(vod, 1.0, 2.0, "v", true).unwrap().id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
        let a = db.add_edit_clip(edit, 1.0, 2.0, "a", true).unwrap().id;
        let b = db.add_edit_clip(edit, 3.0, 4.0, "b", true).unwrap();
        assert!(db.set_clip_file(b.id, b.start, b.end, ClipStatus::Ready, Some("/clips/b.mp4"), None).unwrap());
        let b = b.id;
        db.hand_out_clip(b).unwrap();
        let stopped = std::cell::RefCell::new(Vec::new());
        let stop = |id| stopped.borrow_mut().push(id);

        // Not an edit (a VOD's own clip): nothing goes, none stopped.
        assert!(crate::edits::delete_edit_in(&db, of_vod, stop).is_err());
        assert_eq!(db.bar_clips(edit).unwrap().len(), 2);
        assert!(stopped.borrow().is_empty());

        let gone = crate::edits::delete_edit_in(&db, edit, stop).unwrap();
        let clips: Vec<_> = gone.clips.iter().map(|(c, out)| (c.id, c.file_path.as_deref(), *out)).collect();
        assert_eq!(clips, [(a, None, false), (b, Some("/clips/b.mp4"), true)], "its clips, b dragged out");
        assert_eq!(*stopped.borrow(), [a, b], "their cuts stopped");
        assert!(db.edit(edit).unwrap().is_none() && db.clip(a).unwrap().is_none() && db.clip(b).unwrap().is_none());
        assert_eq!(db.clips(vod).unwrap().len(), 1, "the VOD's own clip stays");
    }

    /// Several clips deleted at once went one by one, a clip that was gone
    /// skipped: one that couldn't go, in the middle, left those before it
    /// deleted with no step to undo them.
    #[test]
    fn deleting_several_clips_is_all_or_none() {
        let (db, vod) = library();
        let a = db.add_clip(vod, 1.0, 2.0, "a", true).unwrap().id;
        let b = db.add_clip(vod, 3.0, 4.0, "b", true).unwrap().id;
        let history = history::History::default();
        let stopped = std::cell::RefCell::new(Vec::new());
        let stop = |id| stopped.borrow_mut().push(id);
        assert!(delete_clips_in(&db, &history, &[a, 9999, b], stop).is_err());
        assert_eq!(db.clips(vod).unwrap().len(), 2, "every clip in place");
        assert_eq!(history.steps(Doc::Vod(vod)), 0, "no step recorded");
        assert!(stopped.borrow().is_empty(), "none stopped");

        let gone = delete_clips_in(&db, &history, &[b, a], stop).unwrap();
        assert_eq!(gone.iter().map(|(c, _)| c.id).collect::<Vec<_>>(), [b, a]);
        assert!(db.clips(vod).unwrap().is_empty());
        assert_eq!(history.steps(Doc::Vod(vod)), 1, "one step for both");
    }

    /// The page told an early release from a refused drag by the word
    /// "released" in its error: a reworded sentence would have shown an
    /// ordinary release, too soon, as an error.
    #[test]
    fn a_drag_refused_for_an_early_release_is_none_begun_not_an_error() {
        assert_eq!(began(Ok(())), Ok(true));
        assert_eq!(began(Err(ReleasedEarly.into())), Ok(false));
        assert_eq!(began(Err(anyhow::Error::new(ReleasedEarly).context("Not now."))), Ok(false), "said otherwise");
        assert_eq!(began(Err(anyhow::anyhow!("no view"))), Err("no view".into()));
        assert_eq!(
            began(Err(anyhow::anyhow!("The button was released before the drag could begin."))),
            Err("The button was released before the drag could begin.".into()),
            "only the type tells",
        );
    }
}
