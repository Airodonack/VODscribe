//! Clips (`vodscribe_core::clips`): the commands the interface makes and
//! changes them with, and the queue that cuts their files, one at a time. A
//! clip is cut from its VOD's footage, as an edit is rendered from it
//! (`footage::find`): the VOD's file where it plays from one here, or else a
//! section of it downloaded into the footage folder and kept there while
//! clips use it, so a clip made a little longer, or put in an edit, needs no
//! new download.
//!
//! Clip files live in the library, each named by its clip's id and which
//! make it is (`Paths::clip_make`): a clip whose range or encoding changes
//! is made again as a new file, and the one before goes unless another app
//! was given it. The name a clip goes by elsewhere (`drag_name`) is given when it
//! is dragged out, copied or shown: a second name for the file, in a folder
//! beside it (`handed_out`, `Paths::clip_names`), which goes with the clip.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{Emitter, Manager, State};
use vodscribe_core::db::{Clip, ClipBar, ClipStatus, Db, Mark, MarkDoc};
use vodscribe_core::settings::{ClipEncoding, Settings};
use vodscribe_core::{clips, tools};

use crate::AppHandle;
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
    /// Pictures of clips to make again (`picture_again`), a listing's at a
    /// time: each clip's id and its file.
    pictures: Jobs<Vec<(i64, String)>>,
    /// The clips' files whose picture has been tried again since the app
    /// started (`picture_again`): one that failed isn't tried at each listing.
    pictured: Mutex<HashSet<String>>,
}

impl Cutter {
    pub(crate) fn new() -> Self {
        Self {
            jobs: Jobs::new(),
            removing: Arc::default(),
            canceled: Arc::default(),
            copies: Arc::default(),
            out: Mutex::default(),
            pictures: Jobs::new(),
            pictured: Mutex::default(),
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
            tidy_at_start(&state.paths, &db)?;
            // Edits left rendering when the app stopped wait to be asked again.
            db.reset_renders()?;
            db.unfinished_clips()?
        };
        {
            // Pictures of clips no longer there (deleted with their VOD).
            let thumbs = app.state::<AppState>().paths.clip_thumbs();
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
        let picturing = app.clone();
        self.pictures.run("clip pictures", move |clips, stop| make_pictures(&picturing, clips, stop))?;
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

    /// Whether no picture is being made again, or waits to be (`picture_again`).
    #[cfg(test)]
    pub(crate) fn pictures_idle(&self) -> bool {
        self.pictures.snapshot() == (None, Vec::new())
    }

    /// Ends the queues for good (`Jobs::close`).
    #[cfg(test)]
    pub(crate) fn close(&self) {
        self.jobs.close();
        self.pictures.close();
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
    let snippet = is_cut(&app.state::<AppState>().db.lock().unwrap(), id);
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

/// Whether clip or edit `id` is a snippet of one piece as it came, which is
/// cut (`cut`), not rendered; None if it's gone.
fn is_cut(db: &Db, id: i64) -> Result<Option<bool>> {
    db.is_snippet(id).map(|s| s.map(|s| s && db.edit_pieces(id).is_ok_and(|p| vodscribe_core::edits::cut_whole(&p))))
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
    } else if let Ok(Some(edit)) = db.edit(id, crate::edits::made_now(app))
        && unmade(edit.status)
        && db.set_edit_render(id, ClipStatus::Waiting, None, None, None).is_ok()
    {
        crate::edits::emit(app, &db, id);
    }
}

/// The error a clip canceled from the list of jobs shows.
pub(crate) const CANCELED: &str = "Canceled";

/// Clip `id`'s first frame, shown while it's dragged out.
pub(crate) fn thumb_path(app: &AppHandle, id: i64) -> PathBuf {
    app.state::<AppState>().paths.clip_thumb(id)
}

/// Clip `id`'s file `file` as it's handed out (dragged to another app,
/// shown in its folder): under `name`, the name a user knows it by (its
/// VOD's title and range, or its edit's title), beside the clip's own file
/// (`Paths::clip_names`). A hard link, the same file under a second name,
/// where the disk allows (no space taken); a copy where it doesn't; never a
/// symbolic link, which would break when the clip's file is made again. One
/// handed out before under that name is given again while it's still this
/// file; once the clip is made again (a new file) or retitled, the old name
/// stays as it was, for the editor whose project uses it, and the next hand
/// out gets a name of its own. The clip's own file if none can be made: the
/// caller notes which went (`hand_out`), as the clip's own file is then the
/// other app's, never deleted.
pub(crate) fn handed_out(paths: &vodscribe_core::Paths, id: i64, file: &Path, name: &str) -> PathBuf {
    named(paths, id, file, name, true).unwrap_or_else(|e| {
        eprintln!("clip {id}: handing out its file as {name}: {e}");
        file.to_path_buf()
    })
}

/// `handed_out`, but never a copy: where the disk takes no hard link, the
/// clip's own file. For a drag, whose receiver asks on the main thread: a
/// copy of the whole clip there froze the window till it was written.
fn linked_out(paths: &vodscribe_core::Paths, id: i64, file: &Path, name: &str) -> PathBuf {
    named(paths, id, file, name, false).unwrap_or_else(|e| {
        eprintln!("clip {id}: handing out its file as {name}: {e}");
        file.to_path_buf()
    })
}

fn named(paths: &vodscribe_core::Paths, id: i64, file: &Path, name: &str, may_copy: bool) -> std::io::Result<PathBuf> {
    let dir = paths.clip_names(id);
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let names = std::iter::once(name.to_string()).chain((2..).map(|n| format!("{stem} ({n}).{ext}")));
    for candidate in names {
        let at = dir.join(candidate);
        if std::fs::symlink_metadata(&at).is_ok() {
            if same_file(&at, file) {
                return Ok(at);
            }
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        if let Err(e) = hard_link(file, &at) {
            if !may_copy {
                // (The folder only if it's empty: made for this name alone.)
                let _ = std::fs::remove_dir(&dir);
                return Err(e);
            }
            // Copied under a name of its own first, so a copy cut short is
            // never handed out; its time kept, so it's known as this file.
            let part = dir.join(format!(".{}.part", at.file_name().unwrap_or_default().to_string_lossy()));
            std::fs::copy(file, &part)?;
            if let Ok(modified) = std::fs::metadata(file).and_then(|m| m.modified()) {
                std::fs::File::options().write(true).open(&part)?.set_modified(modified)?;
            }
            std::fs::rename(&part, &at)?;
        }
        return Ok(at);
    }
    unreachable!("names never run out")
}

/// A hard link from `from` at `to`; in a test, none where its library's disk
/// takes none (`TestApp::takes_no_hard_links`), as on exFAT.
fn hard_link(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if crate::testkit::refuses_links(to) {
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "this disk takes no hard links"));
    }
    std::fs::hard_link(from, to)
}

/// Notes that clip (or edit) `id`'s file went to another app as `given`
/// (`handed_out`): under a name of its own, which stays when the clip goes;
/// or, where none could be made, the clip's own file itself, which the app
/// then never deletes (`file_to_let_go`).
pub(crate) fn hand_out(db: &Db, paths: &vodscribe_core::Paths, id: i64, given: &Path) -> Result<()> {
    if given.starts_with(paths.clip_names(id)) { db.hand_out_clip(id) } else { db.hand_out_clip_file(id) }
}

/// Clip (or edit) `id`'s file `file` as its deletion lets go of it
/// (`let_go_files`): none if that file itself went to another app
/// (`hand_out`), so it's neither deleted nor left to delete later
/// (`files::delete_file`), as the other app's project uses it.
pub(crate) fn file_to_let_go(db: &Db, id: i64, file: Option<String>) -> Result<Option<String>> {
    Ok(if db.clip_file_handed_out(id)? { None } else { file })
}

/// Whether `a` and `b` are the same file, or copies of it as it is: the
/// same length and time changed (a link shares both).
fn same_file(a: &Path, b: &Path) -> bool {
    let stat = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok()));
    stat(a).is_some_and(|a| Some(a) == stat(b))
}

/// Whether `file`, a clip's or an edit's, is the library's own, which goes
/// with it: not one read from 0.7.1's clip folder (`Paths::old_clips`),
/// which stays there.
pub(crate) fn own_file(paths: &vodscribe_core::Paths, file: &str) -> bool {
    Path::new(file).starts_with(paths.clips())
}

/// What the clips' files need at start, before any is made: the clips made
/// only to be dragged out, and those a cut held when the app last stopped,
/// forgotten, the names they were handed out by kept for whoever took them
/// (`let_go_files`); then the files the app couldn't delete when it last
/// ran tried again, kept in the library's folder from now
/// (`files::keep_leftovers_in`). Nothing else in the clips folder is
/// touched: a file no clip here has may be a library's that 0.7.2 set
/// aside, leaving its clips here.
pub(crate) fn tidy_at_start(paths: &vodscribe_core::Paths, db: &Db) -> Result<()> {
    for (id, file, handed_out) in db.forget_hidden_clips()? {
        let_go_files(paths, id, file.as_deref(), handed_out);
    }
    crate::files::keep_leftovers_in(paths.leftovers());
    Ok(())
}

/// What's left of clip `id`'s files once it's gone: its own file (`file`)
/// deleted; the names it was handed out by kept if another app took them
/// (`handed_out`), as its project may use them, else deleted with it.
pub(crate) fn let_go_files(paths: &vodscribe_core::Paths, id: i64, file: Option<&str>, handed_out: bool) {
    let dir = paths.clip_names(id);
    if let Some(file) = file.filter(|f| own_file(paths, f)) {
        if !handed_out {
            // Only those that are this clip's file: a library from before
            // 0.7.3 may give its last deleted clip's id once more, and that
            // clip may have left names another app took.
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if same_file(&entry.path(), Path::new(file)) {
                    delete_file(entry.path());
                }
            }
        }
        delete_file(file);
    }
    let _ = std::fs::remove_dir(&dir);
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
    // Before its footage is fetched; failed here, it's shown by `fail`.
    clips::can_make(&ffmpeg, encoding, !vod.audio_only)?;
    // A file of its own for each make (`Db::next_clip_file`): the one before,
    // which another app may have been given, is never written over.
    let ext = clips::extension(vod.audio_only, encoding.format());
    let out = state.db.lock().unwrap().next_clip_file(id, ext)?;
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
            // chapter, which a cut file hasn't) doesn't make it again; and
            // what with, so a change of codec or quality does (dragout.ts).
            if recorded {
                db.set_made_now(id)?;
                db.set_made_as(id, Some(encoding))?;
            }
            recorded
        }
        // Stopped: moved (it's queued again) or deleted.
        Err(e) if vodscribe_core::is_canceled(e) => false,
        Err(e) => db.set_clip_file(id, range.0, range.1, ClipStatus::Error, None, Some(&format!("{e:#}")))?,
    };
    if result.is_ok() && !recorded {
        // Deleted while it was being cut, or moved as ffmpeg ended (it's
        // queued again): this make is nobody's, and none will record it.
        delete_file(&out);
    }
    // Its file in the format before goes (those it was handed out by stay,
    // and so does that file itself if it was what went: the new one is the
    // app's own).
    let old = clip.file_path.as_deref().filter(|p| own_file(&state.paths, p));
    if let Some(old) = old.filter(|p| recorded && result.is_ok() && Path::new(p) != out) {
        if db.clip_file_handed_out(id)? {
            db.take_back_clip(id)?;
        } else {
            delete_file(old);
        }
    }
    if recorded && result.is_ok() && !vod.audio_only {
        drop(db);
        if let Err(e) = clips::thumbnail(&ffmpeg, &out, &thumb_path(app, id)) {
            eprintln!("clip {id}: its picture: {e:#}");
            // None rather than one of what it was before: its drag shows the
            // app's icon, and its picture is tried again when it's next
            // shown (`picture_again`).
            let _ = std::fs::remove_file(thumb_path(app, id));
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
pub(crate) fn bar_doc(clip: &Clip) -> Doc {
    match clip.bar {
        Some(ClipBar::Edit { id }) => Doc::Edit(id),
        Some(ClipBar::Vod { id }) => Doc::Vod(id),
        None => Doc::Vod(clip.vod_id),
    }
}

/// The clips on edit `id`'s clip bar, in their order.
#[tauri::command]
pub(crate) async fn edit_clips(app: AppHandle, id: i64) -> CmdResult<Vec<Clip>> {
    listed(app, move |db| db.bar_clips(id)).await
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
/// clip already (C pressed twice) gives that one, made again only if it
/// failed or was made otherwise than clips are made now (`again`).
fn make_edit_clips(app: &AppHandle, edit_id: i64, ranges: Vec<(f64, f64)>, shown: bool) -> CmdResult<Vec<Clip>> {
    let state = app.state::<AppState>();
    let now = Settings::load(&state.paths.settings()).clip_encoding();
    let (made, queued) = {
        let db = state.db.lock().unwrap();
        let doc = Doc::Edit(edit_id);
        let made = make_edit_clips_in(&db, &state.history, edit_id, ranges, shown, now).map_err(err)?;
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
    now: ClipEncoding,
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
                Some(had) if again(db, &had, now)? => {
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

/// Whether clip `had`, asked for again (C on its stretch), is made again:
/// if it failed, or if it was made otherwise than clips are made now (`now`,
/// `clips::made_as`); it's then waiting to be. One waiting or being made, or
/// made as wanted, is given as it is.
fn again(db: &Db, had: &Clip, now: ClipEncoding) -> Result<bool> {
    let older = |file: &str| !clips::made_as(Path::new(file), had.encoding, now);
    let again = match had.status {
        ClipStatus::Error => true,
        ClipStatus::Ready => had.file_path.as_deref().is_some_and(older),
        ClipStatus::Waiting | ClipStatus::Cutting => false,
    };
    if again {
        db.set_edit_render(had.id, ClipStatus::Waiting, None, None, None)?;
    }
    Ok(again)
}

/// VOD `vod_id`'s clips, on its clip bar or only dragged out, in their order.
#[tauri::command]
pub(crate) async fn list_clips(app: AppHandle, vod_id: i64) -> CmdResult<Vec<Clip>> {
    listed(app, move |db| db.clips(vod_id)).await
}

/// A clip bar's clips, as `list` gives them, for the page to show: those
/// whose picture couldn't be made when they were have it tried again
/// (`picture_again`).
async fn listed(app: AppHandle, list: impl FnOnce(&Db) -> Result<Vec<Clip>> + Send + 'static) -> CmdResult<Vec<Clip>> {
    let paths = app.state::<AppState>().paths.clone();
    let handle = app.clone();
    let (clips, unpictured) = crate::with_reads(app.clone(), move |db| {
        let clips = list(db)?;
        let tried = handle.state::<AppState>().clips.pictured.lock().unwrap().clone();
        let unpictured = clips.iter().filter(|c| lacks_picture(db, &paths, &tried, c)).filter_map(made_file).collect();
        Ok((clips, unpictured))
    })
    .await?;
    picture_again(&app, unpictured);
    Ok(clips)
}

/// Clip `clip`'s id and file, if it's made.
fn made_file(clip: &Clip) -> Option<(i64, String)> {
    Some((clip.id, clip.file_path.clone().filter(|_| clip.status == ClipStatus::Ready)?))
}

/// Whether clip `clip` is made, and cut (`cut` makes a picture of a cut
/// clip's first frame), with a picture in its file, but has no picture, and
/// its file isn't one of those `tried` again already (`picture_again`).
fn lacks_picture(db: &Db, paths: &vodscribe_core::Paths, tried: &HashSet<String>, clip: &Clip) -> bool {
    let sound_alone = |file: &str| {
        let ext = Path::new(file).extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
        matches!(ext.as_deref(), Some("m4a" | "wav"))
    };
    // The library asked last: of a long clip bar, few lack one.
    made_file(clip).is_some_and(|(_, file)| !sound_alone(&file) && !tried.contains(&file))
        && !paths.clip_thumb(clip.id).exists()
        && is_cut(db, clip.id).is_ok_and(|cut| cut == Some(true))
}

/// Queues the pictures of `clips` (each id and its file), which couldn't
/// be made when they were, to be made again (`make_pictures`): each file
/// once while the app runs, so one that fails for good (a file ffmpeg can't
/// read) isn't tried at each listing; the page lists a VOD's clips every
/// tenth of a second while a drag waits on one (clips.svelte.ts `ready`).
/// A clip made again has a new file, which `cut` gives its own picture.
fn picture_again(app: &AppHandle, clips: Vec<(i64, String)>) {
    let cutter = &app.state::<AppState>().clips;
    let clips: Vec<_> = {
        let mut pictured = cutter.pictured.lock().unwrap();
        clips.into_iter().filter(|(_, file)| pictured.insert(file.clone())).collect()
    };
    if !clips.is_empty() {
        cutter.pictures.push(clips);
    }
}

/// Makes the pictures of `clips` (each id and its file) again, as
/// `picture_again` queued them, till `stop`: each kept only if its clip's
/// file is still that one (`cut` makes a picture for a new one).
fn make_pictures(app: &AppHandle, clips: Vec<(i64, String)>, stop: &AtomicBool) {
    let state = app.state::<AppState>();
    let Some(ffmpeg) = tools::find_ffmpeg(&state.paths.bin()) else {
        eprintln!("clips' pictures: {}", crate::worker::NO_FFMPEG);
        return;
    };
    for (id, file) in clips {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let thumb = thumb_path(app, id);
        let part = thumb.with_extension("again.png");
        let made = clips::thumbnail(&ffmpeg, Path::new(&file), &part);
        // Under the library's lock, as `cut` records a new file under it
        // before it makes that one's picture.
        let db = state.db.lock().unwrap();
        let same = db.clip(id).ok().flatten().is_some_and(|c| made_file(&c).is_some_and(|(_, f)| f == file));
        match made {
            Ok(()) if same => {
                if let Err(e) = std::fs::rename(&part, &thumb) {
                    eprintln!("clip {id}: its picture: {e}");
                }
            }
            Ok(()) => {
                let _ = std::fs::remove_file(&part);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                eprintln!("clip {id}: its picture again: {e:#}");
            }
        }
    }
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
    let made = make_clips(&app, vod_id, vec![(start, end)], shown.unwrap_or(true), Same::Listed)?;
    Ok(made.into_iter().next().expect("a clip for its one range"))
}

/// Makes a clip of a piece of an edit carried out of it (dragout.ts
/// `carry`): `start`..`end` seconds of VOD `vod_id`, in that VOD's clip bar
/// unless `shown` is false. As `add_clip`, but the clip made already that it
/// gives is one that plays just that stretch (`Same::Plays`): the piece is
/// what goes where it's dropped, and a clip listed at its stretch may have
/// been changed in the editor. Its own command, not a flag of `add_clip`'s,
/// as the two callers mean different things by the same stretch.
#[tauri::command]
pub(crate) async fn add_piece_clip(
    app: AppHandle,
    vod_id: i64,
    start: f64,
    end: f64,
    shown: Option<bool>,
) -> CmdResult<Clip> {
    let made = make_clips(&app, vod_id, vec![(start, end)], shown.unwrap_or(true), Same::Plays)?;
    Ok(made.into_iter().next().expect("a clip for its one range"))
}

/// Makes each of `ranges` (`start`..`end` seconds of VOD `vod_id`) a clip in
/// its clip bar, in that order, as one change undo takes back whole (several
/// stretches selected, clipped at once).
#[tauri::command]
pub(crate) async fn add_clips(app: AppHandle, vod_id: i64, ranges: Vec<(f64, f64)>) -> CmdResult<Vec<Clip>> {
    make_clips(&app, vod_id, ranges, true, Same::Listed)
}

/// Which clip made already a stretch asked for again gives (`make_clips`).
#[derive(Clone, Copy)]
enum Same {
    /// The one its clip bar lists at that stretch, as the page finds it:
    /// C, and a stretch of the VOD shown dragged out (`Db::vod_clip_of`).
    Listed,
    /// One that plays just that stretch: a piece of an edit carried out
    /// (`Db::vod_clip_playing`).
    Plays,
}

/// What `add_clip` and `add_clips` do: a clip of each range (its end kept
/// within the VOD), each titled with what's said in it, as one change (all of
/// them, or none if one can't be made); then each is queued to be made. A
/// range with a clip already (C pressed twice) gives that one, as for an
/// edit (`make_edit_clips`), put in the clip bar if `shown`.
fn make_clips(app: &AppHandle, vod_id: i64, ranges: Vec<(f64, f64)>, shown: bool, same: Same) -> CmdResult<Vec<Clip>> {
    let state = app.state::<AppState>();
    let now = Settings::load(&state.paths.settings()).clip_encoding();
    let db = state.db.lock().unwrap();
    let (made, queued) = make_clips_in(&db, &state.history, vod_id, ranges, shown, now, same).map_err(err)?;
    drop(db);
    state.clips.queue_made(&queued, shown);
    Ok(made)
}

/// What `make_clips` does in the library, recorded in `history`: the clips,
/// and those of them to be made.
fn make_clips_in(
    db: &Db,
    history: &history::History,
    vod_id: i64,
    ranges: Vec<(f64, f64)>,
    shown: bool,
    now: ClipEncoding,
    same: Same,
) -> Result<(Vec<Clip>, Vec<i64>)> {
    db.vod(vod_id)?.context("That VOD is gone.")?;
    let label = if ranges.len() == 1 { "Clip added" } else { "Clips added" };
    history.change(db, Doc::Vod(vod_id), label, |db| {
        let (mut made, mut queued) = (Vec::new(), Vec::new());
        for (start, end) in ranges {
            let had = match same {
                Same::Listed => db.vod_clip_of(vod_id, start, end)?,
                Same::Plays => db.vod_clip_playing(vod_id, start, end)?,
            };
            let clip = match had {
                Some(had) => {
                    if shown && !had.shown {
                        db.show_clip(had.id)?;
                    }
                    if again(db, &had, now)? {
                        queued.push(had.id);
                    }
                    db.clip(had.id)?.context("That clip is gone.")?
                }
                None => {
                    let words = db.words_between(vod_id, start, end)?;
                    let clip = db.add_clip(vod_id, start, end, &clips::title(&words, start), shown)?;
                    queued.push(clip.id);
                    clip
                }
            };
            made.push(clip);
        }
        Ok((made, queued))
    })
}

/// Moves clip `id` to `start`..`end`; its file is made again, as a new file
/// (`Db::next_clip_file`). The name it's handed out by follows the new range.
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
    // Made once, at its new range: one waiting keeps its place, and isn't
    // listed with the jobs as still waiting once made.
    state.clips.stop(id);
    state.clips.queue_once(id);
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
    let gone = delete_clips_in(&db, &state.history, &ids, |id| {
        state.clips.cancel(id);
    })
    .map_err(err)?;
    // Their files once they're gone from the library, as `remove` does for one.
    for (clip, handed_out) in gone {
        after_delete(&app, clip.id, clip.file_path, handed_out);
    }
    Ok(())
}

/// Ctrl+X's taking off of clips `ids` (copied just before, `copy_items`),
/// as one change in each clip bar's history, as `delete_clips` does: a clip
/// whose file the copy has is held for the cut's pastes (`clipboard::hold`),
/// off its bar with its file; one not made yet is deleted, as there's
/// nothing to keep.
#[tauri::command]
pub(crate) async fn cut_clips(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<()> {
    let files = crate::clipboard::copied_files(&app);
    let db = state.db.lock().unwrap();
    let holds = |clip: &Clip| held_by(clip, &files);
    let take_out = |id| {
        state.clips.cancel(id);
    };
    let (gone, held) = take_off_in(&db, &state.history, &ids, holds, CUT, take_out).map_err(err)?;
    crate::clipboard::hold(&app, &held);
    for id in held {
        emit(&app, &db, id);
    }
    for (clip, handed_out) in gone {
        after_delete(&app, clip.id, clip.file_path, handed_out);
    }
    Ok(())
}

/// What a cut is called in undo, of one clip or several.
pub(crate) const CUT: (&str, &str) = ("Clip cut", "Clips cut");

/// Whether a cut holds `clip`: made, with its file one of `files` (those the
/// cut's copy has).
pub(crate) fn held_by(clip: &Clip, files: &[PathBuf]) -> bool {
    clip.status == ClipStatus::Ready && clip.file_path.as_ref().is_some_and(|f| files.iter().any(|c| c == Path::new(f)))
}

/// What `delete_clips` does in the library: clips `ids` deleted in one
/// transaction, each clip bar's change recorded in `history`. One that's
/// gone, or one that can't be deleted, leaves all of them as they were and
/// records nothing. `take_out` is given each one's id once it's gone, to take
/// it out of the queue, or stop its file being made, as `remove` does for one.
/// Returns those deleted, each with whether its file
/// was dragged out of the app (its names not the app's to delete then), and
/// with no file where that itself went to another app (`file_to_let_go`).
fn delete_clips_in(
    db: &Db,
    history: &history::History,
    ids: &[i64],
    take_out: impl Fn(i64),
) -> Result<Vec<(Clip, bool)>> {
    Ok(take_off_in(db, history, ids, |_| false, ("Clip deleted", "Clips deleted"), take_out)?.0)
}

/// A clip deleted, with whether its file was dragged out of the app.
type Gone = (Clip, bool);

/// `delete_clips_in`, but the clips `hold` says are held instead (a cut):
/// off their bar, kept with their files and left in the queue (`take_out`
/// is given those deleted, once they are). Each bar's change
/// is recorded as `labels` says, for one clip or several. Those deleted, as
/// `delete_clips_in` gives them, and the ids of those held.
fn take_off_in(
    db: &Db,
    history: &history::History,
    ids: &[i64],
    hold: impl Fn(&Clip) -> bool,
    labels: (&'static str, &'static str),
    take_out: impl Fn(i64),
) -> Result<(Vec<Gone>, Vec<i64>)> {
    let mut gone = Vec::new();
    for &id in ids {
        let mut clip = db.clip(id)?.context("That clip is gone.")?;
        let handed_out = db.clip_handed_out(id)?;
        if !hold(&clip) {
            clip.file_path = file_to_let_go(db, id, clip.file_path)?;
        }
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
    let (held, gone): (Vec<_>, Vec<_>) = gone.into_iter().partition(|(clip, _)| hold(clip));
    db.atomically(|db| {
        gone.iter().try_for_each(|(clip, _)| db.delete_clip(clip.id))?;
        held.iter().try_for_each(|(clip, _)| db.hide_clip(clip.id))
    })?;
    // Out of the queue once they're gone, so a delete that fails leaves them
    // queued. The library is held, so one whose turn has just come finds
    // itself gone.
    for (clip, _) in &gone {
        take_out(clip.id);
    }
    for (doc, before) in docs {
        let of_doc = |list: &[Gone]| list.iter().filter(|(c, _)| bar_doc(c) == doc).count();
        let label = if of_doc(&gone) + of_doc(&held) == 1 { labels.0 } else { labels.1 };
        history.record(db, doc, label, before);
    }
    Ok((gone, held.into_iter().map(|(clip, _)| clip.id).collect()))
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
    let file = file_to_let_go(db, id, clip.and_then(|c| c.file_path))?;
    db.delete_clip(id)?;
    after_delete(app, id, file, handed_out);
    Ok(())
}

/// What's left of clip `id` once it's gone from the library: its files
/// (`let_go_files`) and its thumbnail deleted, and the page told.
pub(crate) fn after_delete(app: &AppHandle, id: i64, file: Option<String>, handed_out: bool) {
    crate::clipboard::forget(app, id);
    let_go_files(&app.state::<AppState>().paths, id, file.as_deref(), handed_out);
    let _ = std::fs::remove_file(thumb_path(app, id));
    crate::page::emit(app, "clip-removed", id);
}

/// Shows clip `id`'s file in the system file manager, under the name it's
/// handed out by (`handed_out`): from there it's the user's to take.
#[tauri::command]
pub(crate) async fn show_clip(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let path = {
        let db = state.db.lock().unwrap();
        let clip = db.clip(id).map_err(err)?;
        let path = clip.and_then(|c| c.file_path).filter(|p| Path::new(p).is_file());
        let path = PathBuf::from(path.ok_or("That clip's file isn't made yet.")?);
        let format = Settings::load(&state.paths.settings()).clip_encoding().format();
        let shown = handed_out(&state.paths, id, &path, &drag_name(&db, format, id));
        hand_out(&db, &state.paths, id, &shown).map_err(err)?;
        shown
    };
    crate::system::reveal(&app, &path)
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

/// Shown while a clip with no picture (sound alone) is dragged.
const DRAG_ICON: &[u8] = include_bytes!("../icons/64x64.png");

/// Whether a clip whose file isn't made yet can be dragged out of the window:
/// the system's drag begins at once and the file goes to where it's dropped
/// once it's made (a file manager waits for it; an app that takes only files
/// already made gets none, and the page says so: `DragEnded`). Linux, macOS
/// and Windows; there VODSCRIBE_DRAG_WAITS=1 goes back to waiting for the
/// files at the window's edge before the drag begins (dragout.ts). That
/// switch is a developer's fallback, not a setting: no changelog, support
/// page or dialog names it.
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
    let db = briefly(&state.db)?;
    Some(made_in(&db, id))
}

/// The library, unless it's busy for more than a moment (`made`).
fn briefly(db: &Mutex<Db>) -> Option<std::sync::MutexGuard<'_, Db>> {
    let end = Instant::now() + Duration::from_millis(50);
    loop {
        match db.try_lock() {
            Ok(db) => return Some(db),
            Err(std::sync::TryLockError::Poisoned(e)) => return Some(e.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(std::sync::TryLockError::WouldBlock) => return None,
        }
    }
}

/// Clip `id`'s file as a drag hands it out, under `name` (`linked_out`), if
/// it's made; still being made while the library is busy (`made`). Named
/// under the library's lock: a delete lets go of the clip's names under it
/// too (`remove`), so it comes wholly before or after. Between them, a name
/// linked after the delete had looked for them stayed, a full-size file that
/// no clip named and nothing removed. Where no name can be made, the clip's
/// own file is handed out and noted so at once (`Db::hand_out_clip_file`),
/// not when the drop is over: a remake ending in between would delete it
/// from under the app that has it. If the drop never comes, it's kept all
/// the same.
fn hand_out_made(db: &Mutex<Db>, paths: &vodscribe_core::Paths, id: i64, name: &str) -> Made {
    let Some(db) = briefly(db) else { return Made::Making };
    match made_in(&db, id) {
        Made::Ready(file) => {
            let given = linked_out(paths, id, &file, name);
            if !given.starts_with(paths.clip_names(id))
                && let Err(e) = db.hand_out_clip_file(id)
            {
                eprintln!("clip {id}: noting its file handed out: {e:#}");
            }
            Made::Ready(given)
        }
        other => other,
    }
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
    window: crate::WebviewWindow,
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
    let (names, resolve) = {
        let state = app.state::<AppState>();
        let settings = Settings::load(&state.paths.settings());
        let format = settings.clip_encoding().format();
        let db = state.db.lock().unwrap();
        let names: Vec<_> =
            ids.iter().map(|&id| Outgoing { name: drag_name(&db, format, id), job: Some(id) }).collect();
        let named: Vec<_> = names.iter().map(|o| o.name.as_str()).collect();
        let resolve = crate::resolve::Watch::new(crate::resolve::applies(&settings, &named));
        (names, resolve)
    };
    let listed: Vec<(i64, String)> = names.iter().zip(&ids).map(|(o, &id)| (id, o.name.clone())).collect();
    let handing = Arc::new(DragOut::new(app.clone(), listed));
    let files = {
        let handing = handing.clone();
        move || handing.files()
    };
    let taken = move || handing.taken();
    let out = ids.clone();
    let cutter = app.clone();
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
    drag_file(window, number, names, files, image, resolve, taken, dropped, waited).await
}

/// Clips' files as a drag out of the window hands them out (`drag_clip`).
pub(crate) struct DragOut {
    app: AppHandle,
    /// Each clip's id, and the name it goes by.
    clips: Vec<(i64, String)>,
}

impl DragOut {
    pub(crate) fn new(app: AppHandle, clips: Vec<(i64, String)>) -> Self {
        DragOut { app, clips }
    }

    /// Where each clip's file is, as the receiver asks: under the name it
    /// goes by (`linked_out`), for an app that takes the file itself (Linux's
    /// addresses, Windows' CF_HDROP, macOS's file address); one that copies
    /// it names its copy so anyway. A clip's own file, where no name could
    /// be made, is noted as handed out as it's given (`hand_out_made`).
    pub(crate) fn files(&self) -> Vec<Made> {
        let state = self.app.state::<AppState>();
        self.clips.iter().map(|(id, name)| hand_out_made(&state.db, &state.paths, *id, name)).collect()
    }

    /// Another app has them (not when the drag begins: called off, or dropped
    /// on nothing, the names are still the app's to delete): noted
    /// (`Db::hand_out_clip`).
    pub(crate) fn taken(&self) {
        let state = self.app.state::<AppState>();
        let db = state.db.lock().unwrap();
        for &(id, _) in &self.clips {
            let _ = db.hand_out_clip(id);
        }
    }
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
    /// Let go on DaVinci Resolve, or perhaps (`resolve`): the page asks
    /// whether to make clips as MOV.
    pub(crate) resolve: Option<crate::resolve::ResolveDrop>,
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

/// The name clip `id`'s file goes by where it's handed out: an edit's (or a
/// snippet changed in the editor) its title, a clip its VOD's date, title
/// and range (`clips::file_name`). With its file's extension if it's made,
/// else the one it will have, made in `format`.
pub(crate) fn drag_name(db: &Db, format: vodscribe_core::settings::ClipFormat, id: i64) -> String {
    let named = || -> Result<Option<String>> {
        let Some(clip) = db.clip(id)? else { return Ok(None) };
        let own = clip.file_path.as_deref().filter(|_| clip.status == ClipStatus::Ready);
        let made = own.and_then(|p| Path::new(p).extension()).map(|e| e.to_string_lossy().into_owned());
        let pieces = db.edit_pieces(id)?;
        if !vodscribe_core::edits::cut_whole(&pieces) {
            let audio_only = pieces.iter().all(|p| db.vod(p.vod_id).ok().flatten().is_some_and(|v| v.audio_only));
            let title = db.clip_title(id)?.unwrap_or_default();
            let ext = made.unwrap_or_else(|| clips::extension(audio_only, format).into());
            return Ok(Some(format!("{}.{ext}", clips::safe_name(&title))));
        }
        let Some(vod) = db.vod(clip.vod_id)? else { return Ok(None) };
        let name = clips::file_name(
            vod.info.published.as_deref(),
            &vod.info.title,
            (clip.start, clip.end),
            vod.audio_only,
            format,
        );
        Ok(Some(match made {
            Some(ext) => format!("{}.{ext}", name.rsplit_once('.').map_or(&*name, |(stem, _)| stem)),
            None => name,
        }))
    };
    named().ok().flatten().unwrap_or_else(|| format!("Clip.{}", clips::extension(false, format)))
}

/// Starts the system's drag of the files `files` says are where (ones the
/// app made, or is making; `names` says what each goes by where it's
/// dropped), showing `image` (a PNG) or the app's icon. When
/// it's over (once the files went where they were dropped, or couldn't),
/// `taken` runs if another app took them, then the page hears "drag-ended"
/// (`DragEnded`, with the page's `number` for the drag), so what it does next
/// (deleting a clip made only for the drag) knows. `resolve` watches it for
/// DaVinci Resolve (resolve.rs). `dropped(true)` once it's
/// let go on another app, `dropped(false)` once it's over; `waited()` once
/// that app waits for a file still being made (it may be called again). False
/// if none began, the button up before it could (`ReleasedEarly`).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn drag_file(
    window: crate::WebviewWindow,
    number: Option<u64>,
    names: Vec<Outgoing>,
    files: impl Fn() -> Vec<Made> + Send + Sync + 'static,
    image: Option<Vec<u8>>,
    resolve: crate::resolve::Watch,
    taken: impl Fn() + Send + Sync + 'static,
    dropped: impl Fn(bool) + Send + Sync + 'static,
    waited: impl Fn() + Send + Sync + 'static,
) -> CmdResult<bool> {
    let image = image.unwrap_or_else(|| DRAG_ICON.to_vec());
    let (tx, rx) = mpsc::channel();
    let on_main = window.clone();
    window
        .run_on_main_thread(move || {
            let started = start_drag(&on_main, number, names, files, image, resolve, taken, dropped, waited);
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
/// files only, and its name where the system says. On Linux, `resolve`
/// refuses them to DaVinci Resolve as it asks, and the page hears that it
/// did (resolve.rs).
#[allow(clippy::too_many_arguments)]
fn start_drag(
    window: &crate::WebviewWindow,
    number: Option<u64>,
    names: Vec<Outgoing>,
    files: impl Fn() -> Vec<Made> + Send + Sync + 'static,
    image: Vec<u8>,
    resolve: crate::resolve::Watch,
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
    let resolve = Arc::new(resolve);
    let watched = resolve.clone();
    let on = move |event| match event {
        // Let go on another app: listed with the jobs from now, until it's over.
        Event::LetGo => dropped(true),
        // (Off the main thread: the library's lock.)
        Event::Ended(ended) => {
            let failure = ended.failure.map(|failure| match failure {
                Failure::Cancelled => CANCELED.to_string(),
                Failure::Failed(why) => why,
            });
            // Refused to Resolve: nothing else to say of it.
            let outcome = match watched.heard(ended.ending, ended.delivered) {
                Some(heard @ crate::resolve::ResolveDrop::Refused) => {
                    DragEnded { resolve: Some(heard), ..Default::default() }
                }
                heard => DragEnded {
                    resolve: heard,
                    ..drag_outcome(ended.ending, ended.delivered, failure, ended.made, ended.receiver)
                },
            };
            let outcome = DragEnded { drag: number, ..outcome };
            drag_source::trace(&format!("ended: {outcome:?}"));
            // Taken, then the page told, so a drag of its own coming back in
            // isn't taken for one to add. Copies written where it was dropped
            // are the other app's own: the app's files aren't handed out.
            if outcome.delivered && !ended.copied {
                taken();
            }
            dropped(false);
            // Through Tauri's own queue, not `page::emit`: brought back and
            // let go on the window, the web view's drop is in that queue
            // already, and the page must hear it first, while it still knows
            // where the drag would go. On Linux `page::emit` would overtake
            // it, and the drop would land nowhere. The drag is over, so
            // nothing holds that queue now.
            let _ = told.emit("drag-ended", outcome);
        }
    };
    #[cfg(target_os = "linux")]
    return drag_source::start(window, items, &image, resolve, on);
    #[cfg(not(target_os = "linux"))]
    {
        drop(resolve);
        drag_source::start(window, items, &image, on)
    }
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

#[cfg(test)]
mod tests;
