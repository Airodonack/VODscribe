//! Importing annotations for a VOD (`vodscribe_core::annotations`): from the
//! file host where they're published, or from a file (the tests'); asking
//! the file host whether a VOD has some; and reading a VOD's cast and
//! activity back for the page, and where its pictures come from.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::annotations::{self, Found, Offer, VodAnnotations};
use vodscribe_core::annotations_filter;
use vodscribe_core::chat::ChatMessage;
use vodscribe_core::db::Vod;
use vodscribe_core::settings::Settings;

use crate::history::Doc;
use crate::{AppState, CmdResult, emit_vod, err};

#[derive(Serialize)]
pub(crate) struct Imported {
    vod: Vod,
    /// Added to the library by the import.
    created: bool,
}

/// Imports annotations file `path` for the VOD it names, with the chat file
/// beside it if there's one, adding the VOD if the library doesn't have it.
/// What an earlier import brought is replaced; the user's own marks stay.
/// The app brings annotations in by the offer (`fetch_annotations`); this
/// is the end-to-end tests' way in, on builds with and without their feature.
#[tauri::command]
pub(crate) async fn import_annotations_file(app: AppHandle, path: String) -> CmdResult<Imported> {
    let read = tauri::async_runtime::spawn_blocking(move || annotations::read(Path::new(&path), None));
    let (annotations, chat) = read.await.map_err(err)?.map_err(err)?;
    import(app, annotations, chat, None).await
}

/// Imports `annotations` (see `import_annotations`); with `onto`, only while
/// that VOD is still in the library, checked under the same lock as the
/// import: one taken out while its file was on its way stays out.
async fn import(
    app: AppHandle,
    annotations: annotations::Annotations,
    chat: Option<Vec<ChatMessage>>,
    onto: Option<i64>,
) -> CmdResult<Imported> {
    let task_app = app.clone();
    let brings_chat = chat.is_some();
    let (vod, created) = tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        let db = state.db.lock().unwrap();
        if let Some(id) = onto
            && !db.vod(id)?.is_some_and(|vod| vod.listed)
        {
            anyhow::bail!("That VOD is gone.");
        }
        db.import_annotations(&annotations, chat.as_deref())
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let state = app.state::<AppState>();
    // Undo there would put marks back from before the import, beside a
    // transcript it doesn't bring back.
    state.history.forget(Doc::Vod(vod.id));
    if brings_chat {
        // A fetch of the site's chat under way would add to the one imported.
        state.chat.stop(vod.id);
    } else if created && vod.chat == vodscribe_core::db::ChatStatus::Pending {
        // No chat came with it: fetched from the site, as for a VOD added by its link.
        state.chat.push(vod.id);
    }
    emit_vod(&app, &state.db.lock().unwrap(), vod.id)?;
    // Whatever started the import (the offer, a file, a test), the page
    // re-reads what it shows of the VOD from here.
    let _ = app.emit("annotations-imported", vod.id);
    Ok(Imported { vod, created })
}

/// VOD `vod_id`'s cast and activity, if annotations were imported for it.
#[tauri::command]
pub(crate) async fn vod_annotations(app: AppHandle, vod_id: i64) -> CmdResult<Option<VodAnnotations>> {
    crate::with_reads(app, move |db| db.vod_annotations(vod_id)).await
}

/// Whether the user lets the app ask the file host about the VODs they add (Settings).
fn may_ask(app: &AppHandle) -> bool {
    Settings::load(&app.state::<AppState>().paths.settings()).ask_for_annotations
}

/// Asks the file host whether `vod`'s annotations are published, keeps the
/// answer on it, and shows the VOD as it is then. A failure to ask is kept
/// quiet: nothing is offered, and nothing is known.
fn look_up(app: &AppHandle, vod_id: i64, platform: &str, platform_id: &str) -> anyhow::Result<Found> {
    let found = annotations::look_up(platform, platform_id)?;
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    db.set_annotations_found(vod_id, &found)?;
    let _ = emit_vod(app, &db, vod_id);
    Ok(found)
}

/// For a VOD just added from a site: asks the file host about its
/// annotations, on a thread of its own so adding never waits, unless the
/// user turned that off. Published, the page offers them
/// (`annotations-found`).
pub(crate) fn look_up_added(app: &AppHandle, vod: &Vod) {
    if !annotations::can_be_published(&vod.info.platform) {
        return;
    }
    let (app, id) = (app.clone(), vod.id);
    let (platform, platform_id) = (vod.info.platform.clone(), vod.info.platform_id.clone());
    std::thread::spawn(move || {
        if !may_ask(&app) {
            return;
        }
        match look_up(&app, id, &platform, &platform_id) {
            Ok(Found::Available { .. }) => {
                let _ = app.emit("annotations-found", id);
            }
            Ok(Found::None) => {}
            Err(e) => {
                if cfg!(debug_assertions) {
                    eprintln!("annotations for {platform} {platform_id}: {e:#}");
                }
            }
        }
    });
}

#[derive(Serialize)]
pub(crate) struct Checked {
    /// False when nothing was asked: the VOD can't have any
    /// (`can_have`), or the user turned asking off (Settings).
    asked: bool,
    /// Whether the file host can hold annotations for a VOD of its site
    /// (`annotations::can_be_published`).
    can_have: bool,
    offer: Offer,
}

/// Asks the file host again whether VOD `vod_id`'s annotations are
/// published, whatever it said before, unless it can't have any or the user
/// turned asking off.
#[tauri::command]
pub(crate) async fn check_annotations(app: AppHandle, vod_id: i64) -> CmdResult<Checked> {
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Checked> {
        let app = task_app;
        let vod = {
            let state = app.state::<AppState>();
            let db = state.db.lock().unwrap();
            db.vod(vod_id)?.ok_or_else(|| anyhow::anyhow!("That VOD is gone."))?
        };
        let can_have = annotations::can_be_published(&vod.info.platform);
        let asked = can_have && may_ask(&app);
        if asked {
            look_up(&app, vod_id, &vod.info.platform, &vod.info.platform_id)
                .map_err(|e| anyhow::anyhow!("Couldn't check for annotations: {e:#}"))?;
        }
        let offer = app.state::<AppState>().db.lock().unwrap().annotations_offer(vod_id)?;
        Ok(Checked { asked, can_have, offer })
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// Checks every VOD in the library against the file host's filter of
/// published annotations (`annotations_filter`), which tells the host
/// nothing of the library, and asks about only the VODs it has: each found
/// shows its gift, one with newer annotations than it brought in marks it.
/// Nothing when the user turned asking off; offline, an error.
fn check_library(app: &AppHandle) -> anyhow::Result<annotations_filter::Checked> {
    let state = app.state::<AppState>();
    annotations_filter::check_library(&state.db, &state.paths, |id| {
        let _ = emit_vod(app, &state.db.lock().unwrap(), id);
    })
}

/// At every start, on a thread of its own: the filter is asked for only if it
/// changed, so a start with nothing new published costs one small request.
/// A failure (offline) is kept quiet.
pub(crate) fn check_library_at_start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = check_library(&app)
            && cfg!(debug_assertions)
        {
            eprintln!("annotations filter: {e:#}");
        }
    });
}

/// The library's "Check for annotations": the same check, now.
#[tauri::command]
pub(crate) async fn check_library_annotations(app: AppHandle) -> CmdResult<annotations_filter::Checked> {
    tauri::async_runtime::spawn_blocking(move || check_library(&app))
        .await
        .map_err(err)?
        .map_err(|e| err(anyhow::anyhow!("Couldn't check for annotations: {e:#}")))
}

/// What's known of VOD `vod_id`'s published annotations, to offer them.
#[tauri::command]
pub(crate) async fn annotations_offer(app: AppHandle, vod_id: i64) -> CmdResult<Offer> {
    crate::with_reads(app, move |db| db.annotations_offer(vod_id)).await
}

/// Fetches VOD `vod_id`'s published annotations and imports them: its
/// transcript and the marks the last import brought are replaced, the
/// user's own marks stay. Taken out of the library while the file came, it
/// stays out: nothing is imported.
#[tauri::command]
pub(crate) async fn fetch_annotations(app: AppHandle, vod_id: i64) -> CmdResult<Imported> {
    let task_app = app.clone();
    let (annotations, version) = tauri::async_runtime::spawn_blocking(move || {
        let vod = {
            let state = task_app.state::<AppState>();
            let db = state.db.lock().unwrap();
            db.vod(vod_id)?.ok_or_else(|| anyhow::anyhow!("That VOD is gone."))?
        };
        annotations::download(&vod.info.platform, &vod.info.platform_id)
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let imported = import(app.clone(), annotations, None, Some(vod_id)).await?;
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    db.set_annotations_imported(vod_id, version.as_deref()).map_err(err)?;
    emit_vod(&app, &db, vod_id)?;
    Ok(imported)
}

/// The server the page asks for the cast's pictures when the app is pointed
/// at another (`annotations::AVATAR_ENV`, the end-to-end tests'); None for
/// the page's own.
#[tauri::command]
pub(crate) fn avatar_base() -> Option<String> {
    annotations::avatar_base()
}
