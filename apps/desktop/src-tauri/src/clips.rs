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
use tauri::{AppHandle, Manager, State};
use vodscribe_core::db::{Clip, ClipBar, ClipStatus, Db, Mark, MarkDoc};
use vodscribe_core::settings::Settings;
use vodscribe_core::{clips, tools};

use crate::drag_source::{Ending, ReleasedEarly};
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
    /// leaves out those still in the window's drag, or over. Each with how
    /// many such drags it's in: the same clip can be dragged out again while
    /// the first still hands it over.
    out: Mutex<HashMap<i64, usize>>,
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
    /// (`out_now`), or that drag, let go there, is over (not).
    pub(crate) fn dragged_out(&self, ids: &[i64], out_now: bool) {
        let mut out = self.out.lock().unwrap();
        for &id in ids {
            if out_now {
                *out.entry(id).or_default() += 1;
            } else if let Some(drags) = out.get_mut(&id) {
                *drags -= 1;
                if *drags == 0 {
                    out.remove(&id);
                }
            }
        }
    }

    /// Whether clip `id`, made only for a drag out, was let go on another app
    /// and is still being handed over (`dragged_out`).
    pub(crate) fn is_out(&self, id: i64) -> bool {
        self.out.lock().unwrap().contains_key(&id)
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
        let running = self.jobs.stop_running_and_mark(
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
        crate::page::emit(app, "clip", clip);
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
    let encoding = Settings::load(&state.paths.settings()).clip_encoding();
    // Named for its range: one already made for this range is made again in
    // place; a range moved gets a new file, and the old one stays where it is.
    let (folder, name) = clips::file_path(
        vod.info.channel.as_deref(),
        vod.info.published.as_deref(),
        &vod.info.title,
        range,
        vod.audio_only,
        encoding.format(),
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
            crate::page::emit(app, "clip-progress", Progress { id, progress });
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
        clips::cut(&ffmpeg, &source.path, &out, within, vod.audio_only, encoding, stop, report)
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
/// false (one only dragged out of the window), after its clip `after` if
/// given (where.ts `insertionPoint`), else last.
#[tauri::command]
pub(crate) async fn add_edit_clip(
    app: AppHandle,
    edit_id: i64,
    from: f64,
    to: f64,
    shown: Option<bool>,
    after: Option<i64>,
) -> CmdResult<Clip> {
    let made = make_edit_clips(&app, edit_id, vec![(from, to)], shown.unwrap_or(true), after)?;
    Ok(made.into_iter().next().expect("a clip for its one range"))
}

/// Makes each of `ranges` (`from`..`to` seconds of edit `edit_id`'s
/// timeline) a clip on the edit's clip bar, in that order, as one change
/// undo takes back whole (several stretches selected, clipped at once);
/// after its clip `after` if given, else last.
#[tauri::command]
pub(crate) async fn add_edit_clips(
    app: AppHandle,
    edit_id: i64,
    ranges: Vec<(f64, f64)>,
    after: Option<i64>,
) -> CmdResult<Vec<Clip>> {
    make_edit_clips(&app, edit_id, ranges, true, after)
}

/// What `add_edit_clip` and `add_edit_clips` do: a clip of each range, each
/// titled with what's said at its start, as one change (all of them, or none
/// if one can't be made); then each is queued to be made. A range with a
/// clip already (C pressed twice) gives that one, made again only if it failed.
fn make_edit_clips(
    app: &AppHandle,
    edit_id: i64,
    ranges: Vec<(f64, f64)>,
    shown: bool,
    after: Option<i64>,
) -> CmdResult<Vec<Clip>> {
    let state = app.state::<AppState>();
    let (made, queued) = {
        let db = state.db.lock().unwrap();
        let doc = Doc::Edit(edit_id);
        let made = make_edit_clips_in(&db, &state.history, edit_id, ranges, shown, after).map_err(err)?;
        history::changed(app, &db, doc);
        made
    };
    state.clips.queue_made(&queued, shown);
    Ok(made)
}

/// What `make_edit_clips` does in the library, recorded in `history`: the
/// clips, and those of them to be made. Those new on the bar go after its
/// clip `after`, if given.
fn make_edit_clips_in(
    db: &Db,
    history: &history::History,
    edit_id: i64,
    ranges: Vec<(f64, f64)>,
    shown: bool,
    after: Option<i64>,
) -> Result<(Vec<Clip>, Vec<i64>)> {
    let label = if ranges.len() == 1 { "Clip added" } else { "Clips added" };
    history.change(db, Doc::Edit(edit_id), label, |db| {
        let (mut made, mut queued, mut fresh) = (Vec::new(), Vec::new(), Vec::new());
        for (from, to) in ranges {
            let title = db.span_title(edit_id, from, to)?;
            let had = db.edit_clip_of(edit_id, from, to)?;
            let mut clip = db.add_edit_clip(edit_id, from, to, &title, shown)?;
            match had {
                None => {
                    queued.push(clip.id);
                    fresh.push(clip.id);
                }
                Some(had) if had.status == ClipStatus::Error => {
                    db.set_edit_render(clip.id, ClipStatus::Waiting, None, None, None)?;
                    clip = db.clip(clip.id)?.context("That clip is gone.")?;
                    queued.push(clip.id);
                }
                Some(_) => {}
            }
            made.push(clip);
        }
        if let Some(after) = after.filter(|_| shown && !fresh.is_empty()) {
            db.place_after_on_bar(edit_id, &fresh, after)?;
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
/// dragged out of the window), after its clip `after` if given (where.ts
/// `insertionPoint`), else last.
#[tauri::command]
pub(crate) async fn add_clip(
    app: AppHandle,
    vod_id: i64,
    start: f64,
    end: f64,
    shown: Option<bool>,
    after: Option<i64>,
) -> CmdResult<Clip> {
    let made = make_clips(&app, vod_id, vec![(start, end)], shown.unwrap_or(true), after)?;
    Ok(made.into_iter().next().expect("a clip for its one range"))
}

/// Makes each of `ranges` (`start`..`end` seconds of VOD `vod_id`) a clip in
/// its clip bar, in that order, as one change undo takes back whole (several
/// stretches selected, clipped at once); after its clip `after` if given,
/// else last.
#[tauri::command]
pub(crate) async fn add_clips(
    app: AppHandle,
    vod_id: i64,
    ranges: Vec<(f64, f64)>,
    after: Option<i64>,
) -> CmdResult<Vec<Clip>> {
    make_clips(&app, vod_id, ranges, true, after)
}

/// What `add_clip` and `add_clips` do: a clip of each range (its end kept
/// within the VOD), each titled with what's said in it, as one change (all of
/// them, or none if one can't be made); then each is queued to be made.
fn make_clips(
    app: &AppHandle,
    vod_id: i64,
    ranges: Vec<(f64, f64)>,
    shown: bool,
    after: Option<i64>,
) -> CmdResult<Vec<Clip>> {
    let state = app.state::<AppState>();
    let made = make_clips_in(&state.db.lock().unwrap(), &state.history, vod_id, ranges, shown, after).map_err(err)?;
    state.clips.queue_made(&made.iter().map(|c| c.id).collect::<Vec<_>>(), shown);
    Ok(made)
}

/// What `make_clips` does in the library, recorded in `history`: on the
/// bar after its clip `after`, if given.
fn make_clips_in(
    db: &Db,
    history: &history::History,
    vod_id: i64,
    ranges: Vec<(f64, f64)>,
    shown: bool,
    after: Option<i64>,
) -> Result<Vec<Clip>> {
    db.vod(vod_id)?.context("That VOD is gone.")?;
    let label = if ranges.len() == 1 { "Clip added" } else { "Clips added" };
    history.change(db, Doc::Vod(vod_id), label, |db| {
        let mut made = Vec::new();
        for (start, end) in ranges {
            let words = db.words_between(vod_id, start, end)?;
            made.push(db.add_clip(vod_id, start, end, &clips::title(&words, start), shown)?);
        }
        if let Some(after) = after.filter(|_| shown && !made.is_empty()) {
            let bar = db.vod_clip(vod_id)?.context("That VOD is gone.")?;
            db.place_after_on_bar(bar, &made.iter().map(|c| c.id).collect::<Vec<_>>(), after)?;
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
    crate::page::emit(app, "clip-removed", id);
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
        let format = Settings::load(&state.paths.settings()).clip_encoding().format();
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
    // Counted once each way, and over only if it was let go there.
    let (asked, telling) = (out.clone(), cutter.clone());
    let let_go = AtomicBool::new(false);
    let dropped = move |out_now: bool| {
        if let_go.swap(out_now, std::sync::atomic::Ordering::SeqCst) == out_now {
            return;
        }
        cutter.state::<AppState>().clips.dragged_out(&out, out_now);
        crate::job_list::changed(&cutter);
    };
    // The app they were let go on waits for some still being made: the page
    // says so ("drag-making"), once, as that app may show nothing till they
    // come.
    let told = AtomicBool::new(false);
    let waited = move || {
        // (The library busy: taken as still being made, as the drag does.)
        let making = asked.iter().filter(|&&id| matches!(made(&telling, id), None | Some(Made::Making))).count();
        if making > 0 && !told.swap(true, std::sync::atomic::Ordering::SeqCst) {
            crate::page::emit(&telling, "drag-making", DragMaking { drag: number, making });
        }
    };
    drag_file(window, number, names, files, image, taken, dropped, waited).await
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

/// A drag of clips' files out of the window let go on another app before
/// they were all made, as the page hears it ("drag-making"): the page's
/// number for the drag, and how many are still being made.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct DragMaking {
    pub(crate) drag: Option<u64>,
    pub(crate) making: usize,
}

/// A file in a drag out of the window: the name it goes by where it's
/// dropped, known before it's made (macOS names a file promised then), and
/// the job making it, whose progress is shown there meanwhile.
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
/// let go on another app, `dropped(false)` once it's over; `waited()` once
/// that app waits for a file still being made (it may be called again). False
/// if none began, the button up before it could (`ReleasedEarly`).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn drag_file(
    window: tauri::WebviewWindow,
    number: Option<u64>,
    names: Vec<Outgoing>,
    files: impl Fn() -> Vec<Made> + Send + Sync + 'static,
    image: Option<Vec<u8>>,
    taken: impl Fn() + Send + Sync + 'static,
    dropped: impl Fn(bool) + Send + Sync + 'static,
    waited: impl Fn() + Send + Sync + 'static,
) -> CmdResult<bool> {
    let image = image.unwrap_or_else(|| DRAG_ICON.to_vec());
    let (tx, rx) = mpsc::channel();
    let on_main = window.clone();
    window
        .run_on_main_thread(move || {
            let started = start_drag(&on_main, number, names, files, image, taken, dropped, waited);
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

/// The system's drag (`drag_source`) of the files `files` says are where,
/// made or being made, each offered as the system's receivers take a file
/// not made yet (`dragndrop_source`): a file manager waits for it, an app that
/// takes files already made gets them if they are. Once it's over, `taken`
/// runs if another app was given the app's own files (not copies written for
/// it), then the page hears "drag-ended" (`drag_outcome`); let go on an app
/// that took nothing because they weren't made yet, that it takes finished
/// files only, and its name where the system says.
#[allow(clippy::too_many_arguments)]
fn start_drag(
    window: &tauri::WebviewWindow,
    number: Option<u64>,
    names: Vec<Outgoing>,
    files: impl Fn() -> Vec<Made> + Send + Sync + 'static,
    image: Vec<u8>,
    taken: impl Fn() + Send + Sync + 'static,
    dropped: impl Fn(bool) + Send + Sync + 'static,
    waited: impl Fn() + Send + Sync + 'static,
) -> Result<()> {
    use crate::drag_source::{self, Event, Failure};

    let files: Arc<dyn Fn() -> Vec<Made> + Send + Sync> = Arc::new(files);
    let waited: Arc<dyn Fn() + Send + Sync> = Arc::new(waited);
    let items = names
        .into_iter()
        .enumerate()
        .map(|(index, Outgoing { name, job })| {
            let (app, files, waited) = (window.app_handle().clone(), files.clone(), waited.clone());
            Box::new(DragFile { app, index, name, job, files, waited }) as Box<dyn drag_source::Item>
        })
        .collect();
    let told = window.app_handle().clone();
    drag_source::start(window, items, &image, move |event| match event {
        // Let go on another app: listed with the jobs from now, until it's over.
        Event::LetGo => dropped(true),
        // (Off the main thread: the library's lock.)
        Event::Ended(ended) => {
            let failure = ended.failure.map(|failure| match failure {
                Failure::Cancelled => CANCELED.to_string(),
                Failure::Failed(why) => why,
            });
            let outcome = drag_outcome(ended.ending, ended.delivered, failure, ended.made, ended.receiver);
            let outcome = DragEnded { drag: number, ..outcome };
            drag_source::trace(&format!("ended: {outcome:?}"));
            // Taken, then the page told, so a drag of its own coming back in
            // isn't taken for one to add. Copies written where it was dropped
            // are the other app's own: the app's files aren't handed out.
            if outcome.delivered && !ended.copied {
                taken();
            }
            dropped(false);
            crate::page::emit(&told, "drag-ended", outcome);
        }
    })
}

/// A clip's (or an edit's) file in a drag out of the window, the `index`th
/// that `files` says is where, as the drag asks after it.
struct DragFile {
    app: AppHandle,
    index: usize,
    name: String,
    /// The job making it, whose progress macOS shows where it's dropped.
    job: Option<i64>,
    files: Arc<dyn Fn() -> Vec<Made> + Send + Sync>,
    /// Told when the receiver waits for the file still being made.
    waited: Arc<dyn Fn() + Send + Sync>,
}

impl crate::drag_source::Item for DragFile {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn state(&self) -> crate::drag_source::State {
        use crate::drag_source::State;
        match (self.files)().into_iter().nth(self.index) {
            Some(Made::Ready(path)) => State::Ready(path),
            Some(Made::Making) => State::Making(self.job.and_then(|id| crate::job_list::progress_of(&self.app, id))),
            Some(Made::Failed(why)) => State::Failed(why),
            None => State::Failed("That clip is gone.".into()),
        }
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

    // (The receiver's cancel stops nothing here: the page forgets a clip
    // made only for the drag once it hears it went nowhere.)

    fn waited_for(&self) {
        (self.waited)();
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

    /// The same clip dragged out again while the first drag still hands it
    /// over (let go on a file manager that copies it as it's made): the
    /// first's end left the second's making out of the list of jobs.
    #[test]
    fn a_drag_clip_stays_listed_till_its_last_drag_out_is_over() {
        let (db, vod) = library();
        let hidden = db.add_clip(vod, 1.0, 2.0, "dragged", false).unwrap().id;
        let cutter = Cutter::new();
        let listed = |id| crate::job_list::describe(&db, crate::job_list::Queue::Render, id, |id| cutter.is_out(id));
        cutter.dragged_out(&[hidden], true);
        cutter.dragged_out(&[hidden], true);
        cutter.dragged_out(&[hidden], false);
        assert!(listed(hidden).is_some(), "left out as the first drag ended, the second still handing it over");
        cutter.dragged_out(&[hidden], false);
        assert!(listed(hidden).is_none(), "listed once both were over");
    }

    /// Several clips made at once went in one at a time: one that failed
    /// part-way left those before it in the library, waiting to be made, with
    /// no step to undo them.
    #[test]
    fn making_several_clips_is_all_or_none() {
        let (db, vod) = library();
        let history = history::History::default();
        assert!(make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (-1.0, 3.0)], true, None).is_err());
        assert!(db.clips(vod).unwrap().is_empty(), "none made");
        assert_eq!(history.steps(Doc::Vod(vod)), 0, "no step recorded");
        let made = make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (3.0, 90.0)], true, None).unwrap();
        assert_eq!(made.iter().map(|c| (c.start, c.end)).collect::<Vec<_>>(), [(1.0, 2.0), (3.0, 60.0)]);
        assert_eq!(history.steps(Doc::Vod(vod)), 1, "one step for both");

        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
        let doc = Doc::Edit(edit);
        let before = history.steps(doc);
        assert!(make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (40.0, 50.0)], true, None).is_err());
        assert!(db.bar_clips(edit).unwrap().is_empty(), "none made on the edit's bar");
        assert_eq!(history.steps(doc), before, "no step recorded");
        let (made, queued) = make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (3.0, 4.0)], true, None).unwrap();
        assert_eq!(made.iter().map(|c| c.id).collect::<Vec<_>>(), queued);
        assert_eq!(history.steps(doc), before + 1);
    }

    /// The Clip key with a clip picked: the clips it makes go after that
    /// one (where.ts `insertionPoint`), as a paste's do, not last.
    #[test]
    fn clips_made_after_a_clip_go_next_to_it() {
        let (db, vod) = library();
        let history = history::History::default();
        let first = make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (3.0, 4.0)], true, None).unwrap();
        make_clips_in(&db, &history, vod, vec![(5.0, 6.0), (7.0, 8.0)], true, Some(first[0].id)).unwrap();
        let starts: Vec<f64> = db.clips(vod).unwrap().iter().map(|c| c.start).collect();
        assert_eq!(starts, [1.0, 5.0, 7.0, 3.0]);

        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
        let (made, _) = make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (3.0, 4.0)], true, None).unwrap();
        make_edit_clips_in(&db, &history, edit, vec![(5.0, 6.0)], true, Some(made[0].id)).unwrap();
        // A stretch with a clip already stays where it is.
        make_edit_clips_in(&db, &history, edit, vec![(3.0, 4.0)], true, Some(made[0].id)).unwrap();
        let starts: Vec<f64> = db.bar_clips(edit).unwrap().iter().map(|c| c.start).collect();
        assert_eq!(starts, [1.0, 5.0, 3.0]);
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
