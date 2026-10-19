//! Footage for edits (`vodscribe_core::footage`): where a stretch of a VOD
//! plays from, and a queue that downloads, one at a time, the sections of
//! VODs from sites that edits need (into the app's data, `footage/`), telling
//! the page how far each is (`footage-progress`) and when it's in (`footage`).
//! What's in of the one downloading plays already (`footage::Growing`).
//! Sections not used for a while, or past a cap, go when the app starts.

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
/// The most the sections may take; the oldest go past it.
const MAX_BYTES: u64 = 10 << 30;
const PROGRESS_EVERY: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(crate) struct Footage {
    /// Stretches to fetch: VOD, start, end.
    waiting: Mutex<VecDeque<(i64, f64, f64)>>,
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

/// Whether VOD `vod_id`'s footage is gone for good: it's a file on this
/// computer that isn't where it was (deleted or moved outside the app). Only
/// where its folder is still there: a drive that's away (unmounted,
/// unplugged) loses nothing.
pub(crate) fn lost(db: &Db, vod_id: i64) -> bool {
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

/// The footage of VOD `vod_id` from `start`, and how far towards `end` (the
/// VOD's time) it's here: all the way if it's downloaded, else as far as the
/// section downloading is in.
pub(crate) fn reach(app: &AppHandle, db: &Db, vod_id: i64, start: f64, end: f64) -> Option<(Source, f64)> {
    if let Some(source) = footage::find(db, &dir(app), vod_id, start, end).ok().flatten() {
        return Some((source, end));
    }
    let state = app.state::<AppState>();
    let (source, until) = state.footage.growing.lock().unwrap().as_ref()?.reach(vod_id, start, end)?;
    Some((served(&state, source), until))
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
            // Never what an edit uses: its site may not have the VOD any more.
            let used = state.db.lock().unwrap().piece_ranges().unwrap_or_default();
            footage::tidy(&dir(&app), KEEP_FOR, MAX_BYTES, |path| {
                footage::section_of(path).is_some_and(|(vod, from, to)| {
                    used.iter().any(|&(v, start, end)| v == vod && from < end && to > start)
                })
            });
            loop {
                let (vod_id, start, end) = {
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
        self.waiting.lock().unwrap().retain(|&(v, _, _)| v != vod_id);
        if let Some((v, stop)) = &*self.current.lock().unwrap()
            && *v == vod_id
        {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Fetches the footage of each stretch (VOD, start, end), after what's asked already.
    pub(crate) fn ask(&self, stretches: impl IntoIterator<Item = (i64, f64, f64)>) {
        let mut waiting = self.waiting.lock().unwrap();
        for stretch in stretches {
            if !waiting.contains(&stretch) {
                waiting.push_back(stretch);
            }
        }
        self.wake.notify_one();
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
    let info = work.join(format!("{prefix}.info.json"));
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
            fetch::probe(&yt_dlp, &vod.info.url, &info, stop)?;
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
