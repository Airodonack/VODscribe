//! Footage (`vodscribe_core::footage`): where a stretch of a VOD plays from,
//! and a queue that downloads, one at a time, the sections of VODs from sites
//! that edits need, and that the player needs ahead of where a VOD from a
//! site plays (first, in place of what it asked before), into the app's
//! data (`footage/`), telling the page how far each is (`footage-progress`)
//! and when it's in (`footage`). What's in of the one downloading plays
//! already (`footage::Growing`). Sections not used for a while, or past a
//! cap, go when the app starts.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::Db;
use vodscribe_core::footage::{self, Growing, Source};
use vodscribe_core::settings::Settings;
use vodscribe_core::{clips, fetch, mkv, tools};

use crate::AppState;

/// Sections older than this go when the app starts.
const KEEP_FOR: Duration = Duration::from_secs(30 * 24 * 3600);
const PROGRESS_EVERY: Duration = Duration::from_millis(500);
/// How long what the site said of a VOD is used for its sections.
const INFO_FOR: Duration = Duration::from_secs(20 * 60);

#[derive(Default)]
pub(crate) struct Footage {
    /// Stretches to fetch: VOD, start, end, and whether the player asked
    /// (it's ahead of where a VOD plays).
    waiting: Mutex<VecDeque<(i64, f64, f64, bool)>>,
    wake: Condvar,
    /// Held while a section downloads, so the queue and a render don't fetch the same one.
    fetching: Mutex<()>,
    /// The VOD the queue is fetching for, and what stops it (the VOD removed).
    current: Mutex<Option<(i64, Arc<AtomicBool>)>>,
    /// The section downloading, and how far it's in.
    growing: Mutex<Option<Growing>>,
}

/// How far a section's download is: to `usable` (the VOD's time) its footage
/// can be used already; before that's known, `progress` (0..1) is a guess
/// from what's downloaded.
#[derive(Serialize, Clone)]
struct Fetching {
    vod_id: i64,
    from: f64,
    to: f64,
    progress: f64,
    usable: Option<f64>,
}

#[derive(Serialize, Clone)]
struct Fetched {
    vod_id: i64,
    /// Why it couldn't be fetched, if it couldn't.
    error: Option<String>,
}

/// Deletes VOD `vod_id`'s footage (it's removed).
pub(crate) fn remove(app: &AppHandle, vod_id: i64) {
    footage::remove(&dir(app), vod_id);
}

fn dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.local.join("footage")
}

/// Whether VOD `vod_id`'s footage is gone for good: it was deleted without
/// keeping what clips use (`Db::set_gone`), or it's a file on this computer
/// that isn't where it was (deleted or moved outside the app). Only
/// where its folder is still there: a drive that's away (unmounted,
/// unplugged) loses nothing.
pub(crate) fn lost(db: &Db, vod_id: i64) -> bool {
    if db.is_gone(vod_id).unwrap_or(false) {
        return true;
    }
    let source = db.vod(vod_id).ok().flatten().and_then(|v| v.source_path);
    source.is_some_and(|path| {
        let path = std::path::Path::new(&path);
        !path.exists() && path.parent().is_some_and(std::path::Path::is_dir)
    })
}

/// The footage of `start`..`end` of VOD `vod_id`, if it's here (or in already
/// of the section downloading).
pub(crate) fn find(app: &AppHandle, db: &Db, vod_id: i64, start: f64, end: f64) -> Option<Source> {
    footage::find(db, &dir(app), vod_id, start, end).ok().flatten().or_else(|| {
        let state = app.state::<AppState>();
        let source = state.footage.growing.lock().unwrap().as_ref()?.find(vod_id, start, end)?;
        Some(served(&state, source))
    })
}

/// Deletes the sections not used for a while, then the least recently used
/// past the cap (Settings), or past `max_bytes` if given; never what an edit
/// uses (its site may not have the VOD any more).
fn tidy(app: &AppHandle, max_bytes: Option<u64>) {
    let state = app.state::<AppState>();
    let used = state.db.lock().unwrap().piece_ranges().unwrap_or_default();
    let cap = max_bytes.unwrap_or_else(|| Settings::load(&state.paths.settings()).footage_cap());
    footage::tidy(&dir(app), KEEP_FOR, cap, |path| {
        footage::section_of(path)
            .is_some_and(|(vod, from, to)| used.iter().any(|&(v, start, end)| v == vod && from < end && to > start))
    });
}

/// How many bytes the footage kept takes.
#[tauri::command]
pub(crate) async fn footage_bytes(app: AppHandle) -> u64 {
    let dir = dir(&app);
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
            })
            .unwrap_or(0)
    })
    .await
    .unwrap_or(0)
}

/// Deletes the footage kept, but what edits use; then the cap (Settings)
/// applies again, if it was changed.
#[tauri::command]
pub(crate) async fn clear_footage(app: AppHandle, all: bool) {
    let _ = tauri::async_runtime::spawn_blocking(move || tidy(&app, all.then_some(0))).await;
}

/// Where the footage of `start`..`end` of VOD `vod_id` is here, stretch by
/// stretch (`footage::cover`), the section downloading as far as it's in
/// (read through the media server, as `served`).
pub(crate) fn cover(app: &AppHandle, db: &Db, vod_id: i64, start: f64, end: f64) -> Vec<(f64, f64, Source)> {
    let state = app.state::<AppState>();
    let growing = state.footage.growing.lock().unwrap().clone();
    let found = footage::cover(db, &dir(app), vod_id, start, end, growing.as_ref()).unwrap_or_default();
    found
        .into_iter()
        .map(|(a, b, source)| {
            let downloading = growing.as_ref().is_some_and(|g| g.path == source.path);
            (a, b, if downloading { served(&state, source) } else { source })
        })
        .collect()
}

/// `source`, of the section downloading, read through the media server
/// rather than from its file, which the download renames when it's done:
/// on Windows that fails while ffmpeg has the file open (media.rs).
fn served(state: &AppState, source: Source) -> Source {
    match &source.section {
        Some(name) => Source { path: crate::media::footage_url(state, name), ..source },
        None => source,
    }
}

/// The file footage section `name` is in now (`footage::section_stem`): the
/// part file while it downloads, then yt-dlp's finished file beside it, then
/// the one kept in the footage folder.
pub(crate) fn section_file(app: &AppHandle, name: &str) -> Option<PathBuf> {
    let state = app.state::<AppState>();
    let growing = state
        .footage
        .growing
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|g| (footage::section_stem(g.vod_id, g.from, g.to) == name).then(|| g.path.clone()));
    let prefix = format!("footage-{name}.");
    let finished = || {
        std::fs::read_dir(state.paths.work()).ok()?.flatten().map(|e| e.path()).find(|p| {
            let file = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            file.starts_with(&prefix)
                && !file.ends_with(".part")
                && !file.ends_with(".json")
                && !file.ends_with(".ytdl")
        })
    };
    growing
        .filter(|p| p.is_file())
        .or_else(finished)
        .or_else(|| Some(dir(app).join(format!("{name}.mkv"))).filter(|p| p.is_file()))
}

/// The frame of VOD `vod_id` at `at` seconds, a JPEG `width` wide (at most
/// 960), from its footage here; empty if there's none of that moment yet.
#[tauri::command]
pub(crate) async fn vod_frame(
    app: AppHandle,
    vod_id: i64,
    at: f64,
    width: u32,
) -> crate::CmdResult<tauri::ipc::Response> {
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let source = {
            let db = state.db.lock().unwrap();
            find(&app, &db, vod_id, at, at + 0.001)
        };
        let ffmpeg = tools::find_ffmpeg(&state.paths.bin());
        source
            .zip(ffmpeg)
            .and_then(|(s, ffmpeg)| vodscribe_core::local::frame(&ffmpeg, &s.path, at - s.offset, width.min(960)).ok())
            .unwrap_or_default()
    })
    .await
    .map_err(crate::err)?;
    Ok(tauri::ipc::Response::new(bytes))
}

impl Footage {
    /// Starts fetching what's asked, once the app's state is in place; first, tidies the folder.
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        std::thread::Builder::new().name("footage".into()).spawn(move || {
            let state = app.state::<AppState>();
            tidy(&app, None);
            loop {
                let (vod_id, start, end, _) = {
                    let mut waiting = state.footage.waiting.lock().unwrap();
                    loop {
                        if let Some(next) = waiting.pop_front() {
                            break next;
                        }
                        waiting = state.footage.wake.wait(waiting).unwrap();
                    }
                };
                let stop = Arc::new(AtomicBool::new(false));
                *state.footage.current.lock().unwrap() = Some((vod_id, stop.clone()));
                let result = ensure(&app, vod_id, start, end, &stop, &|_| {});
                *state.footage.current.lock().unwrap() = None;
                // Within the cap still, after each (a VOD playing fetches one every two minutes).
                tidy(&app, None);
                if let Err(e) = &result {
                    eprintln!("footage of {vod_id} at {start}: {e:#}");
                }
                let _ = app.emit("footage", Fetched { vod_id, error: result.err().map(|e| format!("{e:#}")) });
            }
        })?;
        Ok(())
    }

    /// Stops fetching VOD `vod_id`'s footage (it's being removed): what's
    /// waiting, and the download under way.
    pub(crate) fn stop_vod(&self, vod_id: i64) {
        self.waiting.lock().unwrap().retain(|&(v, _, _, _)| v != vod_id);
        if let Some((v, stop)) = &*self.current.lock().unwrap()
            && *v == vod_id
        {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Fetches the footage of each stretch (VOD, start, end), after what's asked already.
    pub(crate) fn ask(&self, stretches: impl IntoIterator<Item = (i64, f64, f64)>) {
        let mut waiting = self.waiting.lock().unwrap();
        for (vod, start, end) in stretches {
            if !waiting.iter().any(|&(v, a, b, _)| (v, a, b) == (vod, start, end)) {
                waiting.push_back((vod, start, end, false));
            }
        }
        self.wake.notify_one();
    }

    /// Fetches the footage of `start`..`end` of VOD `vod_id` for the player,
    /// before anything else, in place of what it asked for before (from
    /// where it was playing then).
    pub(crate) fn ask_first(&self, vod_id: i64, start: f64, end: f64) {
        let mut waiting = self.waiting.lock().unwrap();
        waiting.retain(|&(v, a, b, player)| !player && (v, a, b) != (vod_id, start, end));
        waiting.push_front((vod_id, start, end, true));
        self.wake.notify_one();
    }

    /// Whether the footage of VOD `vod_id` from `at` is being fetched or
    /// waits to be (by a section that holds `at`).
    pub(crate) fn coming(&self, vod_id: i64, at: f64) -> bool {
        let holds = |start: f64, end: f64| {
            let (from, to) = footage::section(start, end, None);
            from <= at && at < to
        };
        self.waiting.lock().unwrap().iter().any(|&(v, a, b, _)| v == vod_id && holds(a, b))
            || self.growing.lock().unwrap().as_ref().is_some_and(|g| g.vod_id == vod_id && g.from <= at && at < g.to)
    }
}

/// Makes sure the footage of `start`..`end` of VOD `vod_id` is here,
/// downloading a section (`footage::section`) of it if not, telling
/// `progress` how far the download is (0 to 1).
pub(crate) fn ensure(
    app: &AppHandle,
    vod_id: i64,
    start: f64,
    end: f64,
    stop: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
) -> Result<()> {
    let state = app.state::<AppState>();
    // Behind another download (the queue's, for an edit playing), stoppable.
    let _one = loop {
        match state.footage.fetching.try_lock() {
            Ok(one) => break one,
            Err(std::sync::TryLockError::Poisoned(e)) => break e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) if stop.load(Ordering::Relaxed) => {
                return Err(vodscribe_core::Canceled.into());
            }
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    let vod = {
        let db = state.db.lock().unwrap();
        if find(app, &db, vod_id, start, end).is_some() {
            return Ok(());
        }
        db.vod(vod_id)?.context("That VOD is gone.")?
    };
    if vod.source_path.is_some() {
        bail!("“{}” isn't where it was on this computer.", vod.info.title);
    }
    let paths = &state.paths;
    let yt_dlp = tools::find_yt_dlp(&paths.bin()).context(fetch::NO_YT_DLP)?;
    let ffmpeg = tools::find_ffmpeg(&paths.bin()).context(crate::worker::NO_FFMPEG)?;
    let (from, to) = footage::section(start, end, vod.info.duration);
    let name = footage::section_stem(vod_id, from, to);
    let work = paths.work();
    std::fs::create_dir_all(&work)?;
    let prefix = format!("footage-{name}");
    // What the site says of the VOD, asked once for a while rather than for
    // each section (a VOD playing asks for one every two minutes): kept
    // beside, outside this section's files. A download it no longer
    // serves asks again (`download_or_reprobe`).
    let info = work.join(format!("footage-{vod_id}.info.json"));
    let fresh = std::fs::metadata(&info)
        .and_then(|m| m.modified())
        .is_ok_and(|at| at.elapsed().is_ok_and(|age| age < INFO_FOR));
    let selector = clips::selector(Settings::load(&paths.settings()).video_height());
    let wanted = fetch::Wanted::Section { selector: &selector, ffmpeg: &ffmpeg, start: from, end: to, matroska: true };
    // The download's guess of how far it is, as f64 bits; and when it ends.
    let guess = AtomicU64::new(0);
    let done = AtomicBool::new(false);
    let result = std::thread::scope(|s| {
        // What's whole of the file so far, read as it comes in: for the page, and to play.
        s.spawn(|| {
            let mut last: Option<(Option<f64>, f64)> = None;
            loop {
                // Once yt-dlp has renamed the part file it's whole: what was
                // usable stays so (the player finds the file by section_file).
                let usable = part_file(&work, &prefix)
                    .and_then(|path| {
                        let whole = from + mkv::written(&path)?;
                        let growing = Growing { vod_id, from, to, path, whole };
                        let usable = growing.usable();
                        *state.footage.growing.lock().unwrap() = Some(growing);
                        Some(usable)
                    })
                    .or(last.and_then(|(usable, _)| usable));
                let progress = f64::from_bits(guess.load(Ordering::Relaxed));
                if last != Some((usable, progress)) {
                    last = Some((usable, progress));
                    let _ = app.emit("footage-progress", Fetching { vod_id, from, to, progress, usable });
                }
                let wait = Instant::now();
                while wait.elapsed() < PROGRESS_EVERY {
                    if done.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        });
        let result = (|| {
            if !fresh {
                fetch::probe(&yt_dlp, &vod.info.url, &info, stop)?;
            }
            let report = |p: f64| {
                guess.store(p.to_bits(), Ordering::Relaxed);
                progress(p);
            };
            let file =
                fetch::download_or_reprobe(&yt_dlp, &vod.info.url, &info, &work.join(&prefix), wanted, stop, report)?;
            let dir = dir(app);
            std::fs::create_dir_all(&dir)?;
            let out = dir.join(format!("{name}.mkv"));
            // Beside its place first (copied, if it can't be moved: a long
            // while), then into place, only while the VOD's still there
            // (removing it deletes its footage, a part file too).
            let part = dir.join(format!("{name}.part.mkv"));
            std::fs::rename(&file, &part).or_else(|_| std::fs::copy(&file, &part).map(|_| ()))?;
            let db = state.db.lock().unwrap();
            if db.vod(vod_id)?.is_none() {
                let _ = std::fs::remove_file(&part);
                anyhow::bail!("That VOD is gone.");
            }
            std::fs::rename(&part, &out)?;
            // Dated now, not when the site made it, for tidying.
            footage::touch(&out);
            anyhow::Ok(())
        })();
        done.store(true, Ordering::Relaxed);
        result
    });
    *state.footage.growing.lock().unwrap() = None;
    // Failed, the next asks the site afresh.
    if result.is_err() {
        let _ = std::fs::remove_file(&info);
    }
    // Whatever was downloaded goes, done or not.
    vodscribe_core::pipeline::remove_work_prefix(&work, &prefix);
    result
}

/// Removes footage sections' files from `work`: a section downloads there,
/// and moves to the footage folder once done, so at startup any there are
/// left by a quit or a crash mid-download (and are fetched again if needed).
pub(crate) fn tidy_work(work: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(work) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("footage-") && entry.path().is_file() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The file a section downloads into (yt-dlp's part file, under `prefix` in `work`), once it's begun.
fn part_file(work: &std::path::Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(work).ok()?.flatten().map(|e| e.path()).find(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        name.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('.')) && name.ends_with(".part")
    })
}
