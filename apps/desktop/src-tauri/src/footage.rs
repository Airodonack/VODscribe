//! Footage for edits (`vodscribe_core::footage`): where a stretch of a VOD
//! plays from, and a queue that downloads, one at a time, the sections of
//! VODs from sites that edits need (into the app's data, `footage/`), telling
//! the page how far each is (`footage-progress`) and when it's in (`footage`).
//! What's in of the one downloading plays already (`footage::Growing`).
//! Sections not used for a while, or past a cap, go when the app starts.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::Db;
use vodscribe_core::footage::{self, Growing, Source};
use vodscribe_core::settings::Settings;
use vodscribe_core::{clips, fetch, mkv, preview, tools};

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

fn dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.local.join("footage")
}

/// The footage of `start`..`end` of VOD `vod_id`, if it's here (or in already
/// of the section downloading).
pub(crate) fn find(app: &AppHandle, db: &Db, vod_id: i64, start: f64, end: f64) -> Option<Source> {
    footage::find(db, &dir(app), vod_id, start, end)
        .ok()
        .flatten()
        .or_else(|| app.state::<AppState>().footage.growing.lock().unwrap().as_ref()?.find(vod_id, start, end))
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
            preview::tidy(&dir(&app), KEEP_FOR, MAX_BYTES);
            let state = app.state::<AppState>();
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
                let result = ensure(&app, vod_id, start, end, &AtomicBool::new(false));
                if let Err(e) = &result {
                    eprintln!("footage of {vod_id} at {start}: {e:#}");
                }
                let _ = app.emit("footage", Fetched { vod_id, error: result.err().map(|e| format!("{e:#}")) });
            }
        })?;
        Ok(())
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
/// downloading a section (`footage::section`) of it if not.
pub(crate) fn ensure(app: &AppHandle, vod_id: i64, start: f64, end: f64, stop: &AtomicBool) -> Result<()> {
    let state = app.state::<AppState>();
    let _one = state.footage.fetching.lock().unwrap();
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
            let mut last = None;
            loop {
                let usable = part_file(&work, &prefix).and_then(|path| {
                    let whole = from + mkv::written(&path)?;
                    let growing = Growing { vod_id, from, to, path, whole };
                    let usable = growing.usable();
                    *state.footage.growing.lock().unwrap() = Some(growing);
                    Some(usable)
                });
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
            let report = |p: f64| guess.store(p.to_bits(), Ordering::Relaxed);
            let file =
                fetch::download_or_reprobe(&yt_dlp, &vod.info.url, &info, &work.join(&prefix), wanted, stop, report)?;
            let dir = dir(app);
            std::fs::create_dir_all(&dir)?;
            let out = dir.join(format!("{name}.mkv"));
            std::fs::rename(&file, &out).or_else(|_| std::fs::copy(&file, &out).map(|_| ()))?;
            // Dated now, not when the site made it, for tidying.
            preview::touch(&out);
            anyhow::Ok(())
        })();
        done.store(true, Ordering::Relaxed);
        result
    });
    *state.footage.growing.lock().unwrap() = None;
    // Whatever was downloaded goes, done or not.
    if let Ok(entries) = std::fs::read_dir(&work) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.strip_prefix(&prefix).is_some_and(|rest| rest.starts_with('.')) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    result
}

/// The file a section downloads into (yt-dlp's part file, under `prefix` in `work`), once it's begun.
fn part_file(work: &std::path::Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(work).ok()?.flatten().map(|e| e.path()).find(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        name.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('.')) && name.ends_with(".part")
    })
}
