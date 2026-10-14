//! Chat replays: fetched one VOD at a time in the background, when a VOD is
//! added, and at startup for VODs that don't have theirs yet (or whose fetch
//! failed). Separate from the download and transcription queue: chat doesn't
//! need the audio, and a slow chat shouldn't hold up a transcript.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vodscribe_core::db::{ChatLine, ChatStatus, ChatWindow, Db, LibraryChat};
use vodscribe_core::layers::{self, Timeline};
use vodscribe_core::word_emotes::{self, WordEmote};
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
    // Saved a batch at a time, so the view can show chat as it arrives
    // (`chat-saved`) and an interrupted fetch carries on where it got to.
    let mut store = LibraryChat {
        db: &state.db,
        vod_id: id,
        on_save: Box::new(|| {
            let _ = app.emit("chat-saved", id);
        }),
    };
    let result = chat::fetch(&vod.info, yt_dlp.as_ref(), &state.paths.work(), id, &mut store, &flag, |f| {
        let percent = (f * 100.0) as i32;
        if percent != shown {
            shown = percent;
            let _ = app.emit("chat-progress", (id, f));
        }
    });
    drop(store);
    *state.chat.current.lock().unwrap() = None;
    let db = state.db.lock().unwrap();
    // The VOD may have been deleted meanwhile, which is no error. A failure
    // keeps what was saved, and where it got to, for the next try.
    let saved = match result {
        Ok(true) => db.finish_chat(id),
        Ok(false) => db.set_chat_status(id, ChatStatus::None, None),
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

// The transcript view asks for the chat it shows, not all of it: a busy VOD's
// is a hundred thousand messages and more. Each runs on a blocking thread and
// holds the library's lock only for its query.

/// Runs `f` with the library on a blocking thread.
async fn with_db<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&Db) -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>().db.lock().unwrap()))
        .await
        .map_err(err)?
        .map_err(err)
}

/// What the minimap and header need of VOD `vod_id`'s chat.
#[derive(Serialize)]
pub(crate) struct ChatOverview {
    count: u32,
    /// Messages in each of `bins` equal stretches of `duration`.
    histogram: Vec<u32>,
    /// The subs, gifts and tips.
    events: Vec<ChatLine>,
}

#[tauri::command]
pub(crate) async fn chat_overview(app: AppHandle, vod_id: i64, duration: f64, bins: usize) -> CmdResult<ChatOverview> {
    with_db(app, move |db| {
        Ok(ChatOverview {
            count: db.chat_count(vod_id)?,
            histogram: db.chat_histogram(vod_id, duration, bins.min(10_000))?,
            events: db.chat_events(vod_id)?,
        })
    })
    .await
}

/// For each stretch (from, to: seconds), its first `cap` messages and how many it has.
#[tauri::command]
pub(crate) async fn chat_windows(
    app: AppHandle,
    vod_id: i64,
    ranges: Vec<(f64, f64)>,
    cap: usize,
) -> CmdResult<Vec<ChatWindow>> {
    with_db(app, move |db| db.chat_windows(vod_id, &ranges, cap)).await
}

/// The emotes in VOD `vod_id`'s chat, as (name, image URL).
#[tauri::command]
pub(crate) async fn chat_emotes(app: AppHandle, vod_id: i64) -> CmdResult<Vec<(String, String)>> {
    with_db(app, move |db| db.chat_emotes(vod_id)).await
}

/// The emotes VOD `vod_id`'s chat writes as words (7TV, BTTV, FFZ; see
/// `word_emotes`): the channel's list, from the copy kept for a day.
#[tauri::command]
pub(crate) async fn chat_word_emotes(app: AppHandle, vod_id: i64) -> CmdResult<Vec<WordEmote>> {
    let state = app.state::<AppState>();
    let Some(vod) = state.db.lock().unwrap().vod(vod_id).map_err(err)? else { return Ok(Vec::new()) };
    let dir = state.paths.emotes().join("words");
    tauri::async_runtime::spawn_blocking(move || word_emotes::for_vod(&vod.info, &dir)).await.map_err(err)?.map_err(err)
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

/// The timeline's layers and event toggles (see `layers`), or the starting ones.
#[tauri::command]
pub(crate) async fn get_timeline(state: State<'_, AppState>) -> CmdResult<Timeline> {
    Ok(layers::load(&state.paths.layers()))
}

#[tauri::command]
pub(crate) async fn set_timeline(state: State<'_, AppState>, timeline: Timeline) -> CmdResult<()> {
    layers::save(&state.paths.layers(), &timeline).map_err(err)
}

/// For each list of terms, the ids and times of VOD `vod_id`'s chat messages that contain any of them.
#[tauri::command]
pub(crate) async fn chat_matches(
    app: AppHandle,
    vod_id: i64,
    terms: Vec<Vec<String>>,
) -> CmdResult<Vec<Vec<(i64, f64)>>> {
    with_db(app, move |db| terms.iter().map(|t| db.chat_matches(vod_id, t)).collect()).await
}
