//! Chat replays: fetched one VOD at a time in the background, when a VOD is
//! added, and at startup for VODs that don't have theirs yet (or whose fetch
//! failed). Separate from the download and transcription queue: chat doesn't
//! need the audio, and a slow chat shouldn't hold up a transcript.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{ChatLine, ChatStatus};
use vodscribe_core::layers::{self, Layer};
use vodscribe_core::{chat, emotes, tools};

use crate::{AppState, CmdResult, emit_vod, err};

pub(crate) struct ChatQueue {
    tx: Mutex<Sender<i64>>,
    /// The VOD being fetched, and the flag that stops it.
    current: Mutex<Option<(i64, Arc<AtomicBool>)>>,
}

impl ChatQueue {
    /// The queue, and the receiving end for `run`.
    pub(crate) fn new() -> (Self, Receiver<i64>) {
        let (tx, rx) = channel();
        (ChatQueue { tx: Mutex::new(tx), current: Mutex::new(None) }, rx)
    }

    /// Fetches VOD `id`'s chat, unless it has been already.
    pub(crate) fn push(&self, id: i64) {
        let _ = self.tx.lock().unwrap().send(id);
    }

    /// Stops fetching VOD `id`'s chat, if that's under way (it's being deleted).
    pub(crate) fn stop(&self, id: i64) {
        if let Some((current, flag)) = &*self.current.lock().unwrap()
            && *current == id
        {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// Fetches the chat of every VOD that needs it, then of each VOD pushed.
pub(crate) fn run(app: AppHandle, rx: Receiver<i64>) {
    let pending = app.state::<AppState>().db.lock().unwrap().chat_pending().unwrap_or_default();
    for id in pending.into_iter().chain(rx) {
        fetch(&app, id);
    }
}

fn fetch(app: &AppHandle, id: i64) {
    let state = app.state::<AppState>();
    let Ok(Some(vod)) = state.db.lock().unwrap().vod(id) else { return };
    if matches!(vod.chat, ChatStatus::Done | ChatStatus::None) {
        return;
    }
    let flag = Arc::new(AtomicBool::new(false));
    *state.chat.current.lock().unwrap() = Some((id, flag.clone()));
    let yt_dlp = tools::find_yt_dlp(&state.paths.bin());
    let mut shown = -1;
    let result = chat::fetch(&vod.info, yt_dlp.as_ref(), &state.paths.work(), id, &flag, |f| {
        let percent = (f * 100.0) as i32;
        if percent != shown {
            shown = percent;
            let _ = app.emit("chat-progress", (id, f));
        }
    });
    *state.chat.current.lock().unwrap() = None;
    let mut db = state.db.lock().unwrap();
    // The VOD may have been deleted meanwhile, which is no error.
    let saved = match result {
        Ok(Some(messages)) => db.set_chat(id, &messages),
        Ok(None) => db.set_chat_status(id, ChatStatus::None, None),
        Err(e) if vodscribe_core::is_canceled(&e) => return,
        Err(e) => {
            eprintln!("chat for VOD {id}: {e:#}");
            db.set_chat_status(id, ChatStatus::Failed, Some(&format!("{e:#}")))
        }
    };
    if saved.is_ok() {
        let _ = emit_vod(app, &db, id);
    }
}

/// VOD `vod_id`'s chat, oldest first.
#[tauri::command]
pub(crate) async fn chat(state: State<'_, AppState>, vod_id: i64) -> CmdResult<Vec<ChatLine>> {
    state.db.lock().unwrap().chat(vod_id).map_err(err)
}

/// Fetches VOD `id`'s chat again, after a failure.
#[tauri::command]
pub(crate) async fn retry_chat(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    db.set_chat_status(id, ChatStatus::Pending, None).map_err(err)?;
    state.chat.push(id);
    emit_vod(&app, &db, id)
}

/// An emote's image, by its URL on the emote provider's CDN (see `emotes`).
#[tauri::command]
pub(crate) async fn emote(state: State<'_, AppState>, url: String) -> CmdResult<tauri::ipc::Response> {
    let dir = state.paths.emotes();
    let (bytes, _) =
        tauri::async_runtime::spawn_blocking(move || emotes::get(&url, &dir)).await.map_err(err)?.map_err(err)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// The timeline layers (see `layers`), or the starting ones.
#[tauri::command]
pub(crate) async fn get_layers(state: State<'_, AppState>) -> CmdResult<Vec<Layer>> {
    Ok(layers::load(&state.paths.layers()))
}

#[tauri::command]
pub(crate) async fn set_layers(state: State<'_, AppState>, layers: Vec<Layer>) -> CmdResult<()> {
    layers::save(&state.paths.layers(), &layers).map_err(err)
}

/// For each list of terms, the ids of VOD `vod_id`'s chat messages that contain any of them.
#[tauri::command]
pub(crate) async fn chat_matches(
    state: State<'_, AppState>,
    vod_id: i64,
    terms: Vec<Vec<String>>,
) -> CmdResult<Vec<Vec<i64>>> {
    let db = state.db.lock().unwrap();
    terms.iter().map(|t| db.chat_matches(vod_id, t).map_err(err)).collect()
}
