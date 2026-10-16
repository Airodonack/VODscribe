//! Footage for edits (`vodscribe_core::footage`): where a stretch of a VOD
//! plays from, and a queue that downloads, one at a time, the sections of
//! VODs from sites that edits need (into the app's data, `footage/`), telling
//! the page as each is in (`footage`). Sections not used for a while, or
//! past a cap, go when the app starts.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::Db;
use vodscribe_core::footage::{self, Source};
use vodscribe_core::settings::Settings;
use vodscribe_core::{clips, fetch, preview, tools};

use crate::AppState;

/// Sections older than this go when the app starts.
const KEEP_FOR: Duration = Duration::from_secs(30 * 24 * 3600);
/// The most the sections may take; the oldest go past it.
const MAX_BYTES: u64 = 10 << 30;

#[derive(Default)]
pub(crate) struct Footage {
    /// Stretches to fetch: VOD, start, end.
    waiting: Mutex<VecDeque<(i64, f64, f64)>>,
    wake: Condvar,
    /// Held while a section downloads, so the queue and a render don't fetch the same one.
    fetching: Mutex<()>,
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

/// The footage of `start`..`end` of VOD `vod_id`, if it's here.
pub(crate) fn find(app: &AppHandle, db: &Db, vod_id: i64, start: f64, end: f64) -> Option<Source> {
    footage::find(db, &dir(app), vod_id, start, end).ok().flatten()
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
    let wanted = fetch::Wanted::Section { selector: &selector, ffmpeg: &ffmpeg, start: from, end: to };
    let result = (|| {
        fetch::probe(&yt_dlp, &vod.info.url, &info, stop)?;
        let file =
            fetch::download_or_reprobe(&yt_dlp, &vod.info.url, &info, &work.join(&prefix), wanted, stop, |_| {})?;
        let ext = file.extension().map_or("mp4".into(), |e| e.to_string_lossy().into_owned());
        let dir = dir(app);
        std::fs::create_dir_all(&dir)?;
        let out = dir.join(format!("{name}.{ext}"));
        std::fs::rename(&file, &out).or_else(|_| std::fs::copy(&file, &out).map(|_| ()))?;
        // Dated now, not when the site made it, for tidying.
        preview::touch(&out);
        anyhow::Ok(())
    })();
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
