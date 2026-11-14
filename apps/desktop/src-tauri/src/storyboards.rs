//! Storyboards (crates/player `storyboard`) of the footage on this computer:
//! a file added, a video kept, a fetched section, a clip's file. Each file
//! has its own, `storyboards/<vod id>-<path's hash>.sb` (the VOD's id first,
//! so all of a VOD's go with it). They're made one at a time on a queue of
//! VODs, each job making those of the VOD's files that aren't made yet;
//! the queue waits while the player plays other footage (another VOD, or
//! an edit not made of this one), so playing stays smooth. A file's library
//! picture is a frame of its storyboard.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::Manager;
use vodscribe_core::db::Db;
use vodscribe_core::{thumbs, tools};
use vodscribe_player::storyboard::{self, Fingerprint, Storyboard};
use vodscribe_player::timeline::Timeline;

use crate::AppHandle;
use crate::history::Doc;
use crate::jobs::Jobs;
use crate::{AppState, emit_vod};

/// How long a deletion waits for a storyboard's batch to end (`Storyboards::let_go`):
/// a batch of a big file on a slow computer takes a few seconds.
const LET_GO_WITHIN: Duration = Duration::from_secs(20);

pub(crate) struct Storyboards {
    jobs: Jobs<i64>,
    /// Storyboards read, by their file: the timeline asks again at every move.
    boards: Mutex<HashMap<PathBuf, Storyboard>>,
    /// Edits' timelines, by edit, as `storyboard_frame` last made them.
    edits: Mutex<HashMap<i64, Planned>>,
    /// The start's `sweep`, for the tests' app to wait for.
    #[cfg(test)]
    sweeping: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Storyboards {
    pub(crate) fn new() -> Self {
        Storyboards {
            jobs: Jobs::new(),
            boards: Mutex::default(),
            edits: Mutex::default(),
            #[cfg(test)]
            sweeping: Mutex::default(),
        }
    }

    /// Ends the queue for good (`Jobs::close`), once the start's sweep has.
    #[cfg(test)]
    pub(crate) fn close(&self) {
        self.swept();
        self.jobs.close();
    }

    /// Waits for the start's sweep to end: what it asks for is asked.
    #[cfg(test)]
    pub(crate) fn swept(&self) {
        if let Some(sweep) = self.sweeping.lock().unwrap().take() {
            let _ = sweep.join();
        }
    }

    /// Starts the queue's thread, once the app's state is in place, and
    /// beside it the start's `sweep`, which asks it for what a quit (or a
    /// crash) left unmade.
    pub(crate) fn run(&self, app: AppHandle) -> anyhow::Result<()> {
        let sweeping = app.clone();
        self.jobs.run("storyboards", move |id, stop| make(&app, id, stop))?;
        let thread = std::thread::Builder::new().name("storyboards-sweep".into()).spawn(move || sweep(&sweeping))?;
        #[cfg(test)]
        {
            *self.sweeping.lock().unwrap() = Some(thread);
        }
        #[cfg(not(test))]
        drop(thread);
        Ok(())
    }

    /// Makes VOD `id`'s storyboards, after those asked for already.
    pub(crate) fn push(&self, id: i64) {
        self.jobs.push_new(id, |&j| j == id);
    }

    /// Makes VOD `id`'s storyboards next: it's open.
    pub(crate) fn push_front(&self, id: i64) {
        self.jobs.push_front(id, |&j| j == id);
    }

    /// Stops making VOD `id`'s storyboards, and waits (a while at most) for
    /// the batch under way to end: its ffmpeg lets go of the VOD's files, which
    /// can then be deleted. On Windows a file another process has open can't
    /// be. Not under the library's lock: the job may wait for it.
    pub(crate) fn let_go(&self, id: i64) {
        self.jobs.take_waiting(|&j| j == id);
        if !self.jobs.stop_and_wait(|&j| j == id, LET_GO_WITHIN) {
            eprintln!("VOD {id}: its storyboard still reads its files");
        }
    }

    /// Stops making VOD `id`'s storyboards, waiting or under way.
    fn stop(&self, id: i64) {
        self.jobs.take_waiting(|&j| j == id);
        self.jobs.stop_running(|&j| j == id);
    }
}

fn dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.storyboards()
}

/// Where the storyboard of `source`, a file of VOD `vod_id`'s, is kept.
fn file(app: &AppHandle, vod_id: i64, source: &Path) -> PathBuf {
    app.state::<AppState>().paths.source_storyboard(vod_id, source)
}

/// The VOD a storyboard file is of, by its name.
fn vod_of(name: &str) -> Option<i64> {
    name.strip_suffix(".sb")?.split_once('-')?.0.parse().ok()
}

/// VOD `vod_id`'s files on this computer: its own (a file added, or a
/// video kept), its clips' files and the sections of it fetched.
fn files_of(app: &AppHandle, db: &Db, vod_id: i64) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = db.playable_path(vod_id).ok().flatten().map(PathBuf::from).into_iter().collect();
    for clip in db.clips(vod_id).unwrap_or_default() {
        if clip.status == vodscribe_core::db::ClipStatus::Ready
            && let Some(path) = clip.file_path
        {
            files.push(path.into());
        }
    }
    let footage = app.state::<AppState>().paths.source_footage();
    files.extend(vodscribe_core::footage::sections(&footage, vod_id).into_iter().map(|(path, _, _)| path));
    files.retain(|f| f.is_file());
    files
}

/// Whether the storyboard of `source` in `file` is made, of the file as it is.
fn made(file: &Path, source: &Path) -> bool {
    let Ok(fingerprint) = Fingerprint::of(source) else { return true };
    Storyboard::open(file).is_ok_and(|b| b.of(&fingerprint) && b.complete())
}

/// The job: VOD `id`'s files' storyboards made, its own file's picture (a
/// file added) taken from its storyboard.
fn make(app: &AppHandle, id: i64, stop: &AtomicBool) {
    let state = app.state::<AppState>();
    let Some(ffmpeg) = tools::find_ffmpeg(&state.paths.bin()) else { return };
    let (files, vod) = {
        let db = state.db.lock().unwrap();
        (files_of(app, &db, id), db.vod(id).ok().flatten())
    };
    let Some(vod) = vod else { return };
    for source in files {
        let own = vod.source_path.as_deref().is_some_and(|p| Path::new(p) == source);
        let mut pictured = false;
        let made = storyboard::make(&ffmpeg, &source, &file(app, id, &source), stop, |board| {
            if own && !pictured {
                pictured = picture(app, id, board);
            }
            // Playing something else (not this VOD, nor an edit of it): it waits.
            while !stop.load(Ordering::Relaxed) && state.player.playing_other(&state.db, id) {
                std::thread::sleep(Duration::from_millis(250));
            }
            !stop.load(Ordering::Relaxed)
        });
        match made {
            // Made before: its picture is taken again only if it's missing.
            Ok(Some(board)) if own && !pictured && !has_picture(app, id) => {
                picture(app, id, &board);
            }
            // No picture to make one of (a sound file): no picture.
            Ok(None) if own && !has_picture(app, id) => set_picture(app, id, ""),
            Ok(_) => {}
            Err(e) if vodscribe_core::is_canceled(&e) => return,
            Err(e) => eprintln!("VOD {id}: storyboard of {}: {e:#}", tools::shown(&source)),
        }
    }
}

/// Whether VOD `id` has its picture (or is known to have none).
fn has_picture(app: &AppHandle, id: i64) -> bool {
    let state = app.state::<AppState>();
    let thumb = state.db.lock().unwrap().vod(id).ok().flatten().and_then(|v| v.thumb);
    thumb.is_some_and(|name| name.is_empty() || state.paths.source_thumb(&name).is_file())
}

/// Makes `board`'s frame an eighth of the way in VOD `id`'s picture, once
/// it's made; whether it was.
fn picture(app: &AppHandle, id: i64, board: &Storyboard) -> bool {
    let Some(jpeg) = board.slot(board.layout.picture_slot()).and_then(|pts| board.frame(pts).ok()) else {
        return false;
    };
    let thumbs = app.state::<AppState>().paths.thumbs();
    thumbs::remove(&thumbs, id);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let name = format!("{id}-{stamp}.jpg");
    if std::fs::create_dir_all(&thumbs)
        .and_then(|()| std::fs::write(app.state::<AppState>().paths.source_thumb(&name), jpeg))
        .is_err()
    {
        return false;
    }
    set_picture(app, id, &name);
    true
}

fn set_picture(app: &AppHandle, id: i64, name: &str) {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    if db.set_thumb(id, name).is_ok() {
        let _ = emit_vod(app, &db, id);
    }
}

/// `doc` is open: its footage's storyboards are made next (an edit's in the
/// order of its pieces).
pub(crate) fn opened(app: &AppHandle, doc: Option<Doc>) {
    let state = app.state::<AppState>();
    let vods = match doc {
        None => return,
        Some(Doc::Vod(id)) => vec![id],
        Some(Doc::Edit(id)) => {
            let pieces = state.db.lock().unwrap().edit_pieces(id).unwrap_or_default();
            let mut vods: Vec<i64> = Vec::new();
            for p in pieces {
                if !vods.contains(&p.vod_id) {
                    vods.push(p.vod_id);
                }
            }
            vods
        }
    };
    for id in vods.into_iter().rev() {
        state.storyboards.push_front(id);
    }
}

/// Deletes VOD `id`'s storyboards (it's erased), stopping what's making them.
pub(crate) fn remove(app: &AppHandle, id: i64) {
    app.state::<AppState>().storyboards.stop(id);
    delete(&dir(app), |vod, _| vod == id);
}

/// Deletes VOD `id`'s storyboards of files no longer here (its kept video
/// deleted), stopping what's making them; what's still here of it is made
/// on.
pub(crate) fn forget_gone(app: &AppHandle, id: i64) {
    let state = app.state::<AppState>();
    state.storyboards.stop(id);
    delete(&dir(app), |vod, board| vod == id && board.is_none_or(|b| !Path::new(&b.fingerprint.path).is_file()));
    // Again, though the one stopped may not have let go yet.
    state.storyboards.jobs.push_once(id, |&j| j == id);
}

/// Deletes the storyboards in `dir` that `which` picks, by their VOD and
/// what they are (None: unreadable).
fn delete(dir: &Path, which: impl Fn(i64, Option<&Storyboard>) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(vod) = vod_of(&name) else { continue };
        if which(vod, Storyboard::open(&entry.path()).ok().as_ref()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// At startup: deletes the storyboards of VODs and files no longer here (a
/// section tidied away, a clip's file deleted; not those on a drive that's
/// away), and asks for those missing, of files changed since, or unfinished.
pub(crate) fn sweep(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(vods) = state.db.lock().unwrap().vods() else { return };
    let library: Vec<i64> = vods.iter().map(|v| v.id).collect();
    // (A test holds it here, the library read and its files not yet gone through.)
    #[cfg(test)]
    crate::testkit::sweep_read(app);
    delete(&dir(app), |vod, board| {
        let gone = |b: &Storyboard| {
            let path = Path::new(&b.fingerprint.path);
            !path.exists() && path.parent().is_some_and(Path::is_dir)
        };
        // A VOD not in the library as it was at the start may have been
        // added since: asked again now. Ids are never used again, so one
        // not there now is gone for good.
        let erased = || !library.contains(&vod) && matches!(state.db.lock().unwrap().vod(vod), Ok(None));
        board.is_none_or(gone) || erased()
    });
    for id in library {
        let files = files_of(app, &state.db.lock().unwrap(), id);
        if files.iter().any(|f| !made(&file(app, id, f), f)) {
            state.storyboards.push(id);
        }
    }
}

/// At startup, before the storyboards' queue runs: deletes the leftovers
/// of a header being written (`.sb-new`) that a quit or a crash left.
/// Later, the queue may be writing one.
pub(crate) fn tidy(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().ends_with(".sb-new") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// An edit's timeline, and its pieces' VODs, at a revision of it.
struct Planned {
    revision: Option<i64>,
    made: Instant,
    timeline: Timeline,
    vods: HashMap<i64, i64>,
}

/// How long an edit's timeline is used for its thumbnails, the edit
/// unchanged: footage comes in and goes meanwhile.
const PLANNED_FOR: Duration = Duration::from_secs(2);
/// Storyboards kept read at most.
const BOARDS_KEPT: usize = 32;

/// Where a moment of a VOD or edit is in its footage: VOD `vod_id`'s file
/// `path`, `file_time` seconds in. A time in the file plus `offset` is
/// the VOD's or edit's.
#[derive(Debug, PartialEq)]
struct Found {
    vod_id: i64,
    path: PathBuf,
    file_time: f64,
    offset: f64,
}

/// Where timeline time `time` of an edit (`timeline`, its pieces' VODs
/// `vods`) is in its footage, if that's a file here (not footage
/// downloading, read through the media server).
fn locate(timeline: &Timeline, vods: &HashMap<i64, i64>, time: f64) -> Option<Found> {
    let stretch = timeline.at(time)?;
    let source = stretch.source.as_ref().filter(|s| !tools::served(&s.path))?;
    Some(Found {
        vod_id: *vods.get(&stretch.piece_id?)?,
        path: source.path.clone(),
        file_time: stretch.file_time(time),
        offset: stretch.at - stretch.from,
    })
}

/// Where `time` of `doc` is in its footage here, if it is.
fn find(app: &AppHandle, doc: Doc, time: f64) -> Option<Found> {
    let state = app.state::<AppState>();
    match doc {
        Doc::Vod(id) => {
            let source = crate::footage::find(app, &state.db.lock().unwrap(), id, time, time + 0.001)?;
            (!tools::served(&source.path)).then_some(Found {
                vod_id: id,
                file_time: time - source.offset,
                offset: source.offset,
                path: source.path,
            })
        }
        Doc::Edit(id) => {
            let mut edits = state.storyboards.edits.lock().unwrap();
            let db = state.db.lock().unwrap();
            let revision = db.revision(id).ok().flatten();
            let fresh = edits.get(&id).is_some_and(|p| p.revision == revision && p.made.elapsed() < PLANNED_FOR);
            if !fresh {
                let pieces = db.edit_pieces(id).ok()?;
                let timeline = Timeline::of(&pieces, |vod, a, b| crate::footage::cover(app, &db, vod, a, b));
                let vods = pieces.iter().map(|p| (p.id, p.vod_id)).collect();
                edits.insert(id, Planned { revision, made: Instant::now(), timeline, vods });
            }
            let planned = edits.get(&id)?;
            locate(&planned.timeline, &planned.vods, time)
        }
    }
}

/// The storyboard frame to show for `time` of `doc`: the keyframe at or
/// before it in its footage, however far back (`Storyboard::shown_at`; for
/// an edit, that can be just before its piece starts), or while the
/// storyboard is still being made, the nearest one within its interval or
/// `within` seconds (the timeline's few pixels). Where it shows in `doc`,
/// and its JPEG. A storyboard missing or unfinished is asked for first.
fn frame_at(app: &AppHandle, doc: Doc, time: f64, within: f64) -> Option<(f64, Vec<u8>)> {
    let state = app.state::<AppState>();
    let found = find(app, doc, time)?;
    let file = file(app, found.vod_id, &found.path);
    let fingerprint = Fingerprint::of(&found.path).ok()?;
    let mut boards = state.storyboards.boards.lock().unwrap();
    if boards.len() > BOARDS_KEPT && !boards.contains_key(&file) {
        boards.clear();
    }
    let board = match boards.entry(file.clone()) {
        std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
        std::collections::hash_map::Entry::Vacant(e) => match Storyboard::open(&file) {
            Ok(board) => e.insert(board),
            Err(_) => {
                state.storyboards.push_front(found.vod_id);
                return None;
            }
        },
    };
    if !board.complete() {
        let _ = board.refresh();
    }
    if !board.of(&fingerprint) {
        // Of the file as it was: made again.
        boards.remove(&file);
        state.storyboards.push_front(found.vod_id);
        return None;
    }
    if !board.complete() {
        state.storyboards.push_front(found.vod_id);
    }
    let pts = board.shown_at(found.file_time, within)?;
    Some((pts + found.offset, board.frame(pts).ok()?))
}

/// The storyboard frame to show for `time` of `doc` (a VOD or an edit, in
/// its own time): the keyframe at or before it, or while its storyboard is
/// made, the nearest within `within` seconds or the storyboard's interval.
/// The time it shows at (8 bytes, a little-endian f64), then its JPEG.
/// Empty if there's none: no footage here, its storyboard not made yet, or
/// nothing near yet.
#[tauri::command]
pub(crate) async fn storyboard_frame(
    app: AppHandle,
    doc: Doc,
    time: f64,
    within: f64,
) -> crate::CmdResult<tauri::ipc::Response> {
    let found = tauri::async_runtime::spawn_blocking(move || frame_at(&app, doc, time, within.max(0.0)))
        .await
        .map_err(crate::err)?;
    let bytes = found.map_or_else(Vec::new, |(at, jpeg)| {
        let mut out = at.to_le_bytes().to_vec();
        out.extend(jpeg);
        out
    });
    Ok(tauri::ipc::Response::new(bytes))
}

#[cfg(test)]
mod tests;
