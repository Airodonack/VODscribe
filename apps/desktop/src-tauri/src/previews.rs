//! Edits' previews (`vodscribe_core::preview`): a queue that makes their
//! segments, one at a time, in order from the open edit's playhead to
//! `AHEAD` past it, and the commands the edit's player asks for them with. Segments are kept in the
//! app's data (`preview/`), the least recently used going past a cap.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::preview::{self, Planned};
use vodscribe_core::render::{self, Piece};
use vodscribe_core::tools;

use crate::{AppState, CmdResult, err};

/// Segments not used for this long go when the app starts.
const KEEP_FOR: Duration = Duration::from_secs(14 * 24 * 3600);
/// The most the segments may take; the least recently used go past it.
const MAX_BYTES: u64 = 2 << 30;
/// How far past the playhead segments are made (the page asks again as it moves).
const AHEAD: f64 = 120.0;

#[derive(Default)]
pub(crate) struct Previews {
    /// Segments to make, by key, in the order they're made.
    waiting: Mutex<VecDeque<(String, Piece)>>,
    wake: Condvar,
}

#[derive(Serialize, Clone)]
struct Made {
    key: String,
    /// Why it couldn't be made, if it couldn't.
    error: Option<String>,
}

fn dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.local.join("preview")
}

impl Previews {
    /// Starts making segments, once the app's state is in place; first, tidies the cache.
    pub(crate) fn run(&self, app: AppHandle) -> Result<()> {
        let dir = dir(&app);
        std::thread::Builder::new().name("previews".into()).spawn(move || {
            preview::tidy(&dir, KEEP_FOR, MAX_BYTES);
            let state = app.state::<AppState>();
            loop {
                let (key, piece) = {
                    let mut waiting = state.previews.waiting.lock().unwrap();
                    loop {
                        if let Some(next) = waiting.pop_front() {
                            break next;
                        }
                        waiting = state.previews.wake.wait(waiting).unwrap();
                    }
                };
                let out = preview::path(&dir, &key);
                // A section downloading moved once it was done: the page asks
                // again then, for the same segment from where it is now.
                if !out.is_file() && !piece.source.is_file() {
                    continue;
                }
                let result = if out.is_file() {
                    Ok(())
                } else {
                    tools::find_ffmpeg(&state.paths.bin())
                        .ok_or_else(|| anyhow::anyhow!(crate::worker::NO_FFMPEG))
                        .and_then(|ffmpeg| render::preview(&ffmpeg, &piece, &out, &AtomicBool::new(false)))
                };
                if let Err(e) = &result {
                    eprintln!("preview {key}: {e:#}");
                }
                let _ = app.emit("preview-made", Made { key, error: result.err().map(|e| format!("{e:#}")) });
            }
        })?;
        Ok(())
    }

    /// Makes, in order, `planned`'s segments not made yet from the one
    /// playing at `at` to `AHEAD` past it, instead of what was asked before.
    fn ask(&self, dir: &std::path::Path, planned: &[Planned], at: f64) {
        let wanted = planned
            .iter()
            .filter(|p| p.at + p.length > at && p.at < at + AHEAD)
            .filter_map(|p| Some((p.key.clone()?, p.source.clone()?)))
            .filter(|(key, _)| !preview::path(dir, key).is_file());
        *self.waiting.lock().unwrap() = wanted.collect();
        self.wake.notify_one();
    }
}

/// Edit `id`'s preview, segment by segment; those not made yet from `at`
/// (the playhead) on are made next, and each says when it is (the
/// `preview-made` event). Footage not here yet is fetched (`footage`), after
/// which the page asks again.
#[tauri::command]
pub(crate) async fn preview_plan(app: AppHandle, id: i64, at: f64) -> CmdResult<Vec<Planned>> {
    let handle = app.clone();
    let (planned, missing) = tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<AppState>();
        let db = state.db.lock().unwrap();
        let pieces = db.edit_pieces(id)?;
        let planned = preview::plan(&pieces, |vod, a, b| crate::footage::find(&handle, &db, vod, a, b));
        anyhow::Ok((planned, crate::edits::missing(&handle, &db, &pieces)))
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let state = app.state::<AppState>();
    state.footage.ask(missing);
    state.previews.ask(&dir(&app), &planned, at);
    Ok(planned)
}

/// Segment `key`'s bytes, if it's made; empty if not.
#[tauri::command]
pub(crate) async fn preview_segment(app: AppHandle, key: String) -> CmdResult<tauri::ipc::Response> {
    if !preview::is_key(&key) {
        return Err("That isn't a preview segment.".into());
    }
    let path = preview::path(&dir(&app), &key);
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let bytes = std::fs::read(&path).unwrap_or_default();
        if !bytes.is_empty() {
            preview::touch(&path);
        }
        bytes
    })
    .await
    .map_err(err)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// What the segments are in, for the player to ask the webview whether it plays them.
#[tauri::command]
pub(crate) fn preview_codecs() -> &'static str {
    render::PREVIEW_CODECS
}
