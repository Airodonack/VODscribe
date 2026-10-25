//! Importing annotations for a VOD (`vodscribe_core::annotations`): from the
//! page's parsed file, from a file picked, or from the file host where
//! they're published; asking the file host whether a VOD has some; and
//! reading a VOD's cast and activity back for the page.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::annotations::{self, Found, Offer, VodAnnotations};
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

/// Makes `annotations` (an annotations file as the page parsed it) the
/// VOD's it names, with `chat` (its chat file) as its chat if given: adding
/// the VOD if the library doesn't have it. What an earlier import brought is
/// replaced; the user's own marks stay.
#[tauri::command]
pub(crate) async fn import_annotations(
    app: AppHandle,
    annotations: serde_json::Value,
    chat: Option<Vec<ChatMessage>>,
) -> CmdResult<Imported> {
    let annotations = annotations::from_value(annotations).map_err(err)?;
    import(app, annotations, chat).await
}

/// As `import_annotations`, from file `path` (picked in the file dialog),
/// with the chat file beside it if there's one.
#[tauri::command]
pub(crate) async fn import_annotations_file(app: AppHandle, path: String) -> CmdResult<Imported> {
    let read = tauri::async_runtime::spawn_blocking(move || annotations::read(Path::new(&path), None));
    let (annotations, chat) = read.await.map_err(err)?.map_err(err)?;
    import(app, annotations, chat).await
}

async fn import(
    app: AppHandle,
    annotations: annotations::Annotations,
    chat: Option<Vec<ChatMessage>>,
) -> CmdResult<Imported> {
    let task_app = app.clone();
    let brings_chat = chat.is_some();
    let (vod, created) = tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        let db = state.db.lock().unwrap();
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
    /// False when the user turned asking off (Settings): nothing was asked.
    asked: bool,
    offer: Offer,
}

/// Asks the file host again whether VOD `vod_id`'s annotations are
/// published, whatever it said before, unless the user turned asking off.
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
        let asked = annotations::can_be_published(&vod.info.platform) && may_ask(&app);
        if asked {
            look_up(&app, vod_id, &vod.info.platform, &vod.info.platform_id)
                .map_err(|e| anyhow::anyhow!("Couldn't check for annotations: {e:#}"))?;
        }
        let offer = app.state::<AppState>().db.lock().unwrap().annotations_offer(vod_id)?;
        Ok(Checked { asked, offer })
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// What's known of VOD `vod_id`'s published annotations, to offer them.
#[tauri::command]
pub(crate) async fn annotations_offer(app: AppHandle, vod_id: i64) -> CmdResult<Offer> {
    crate::with_reads(app, move |db| db.annotations_offer(vod_id)).await
}

/// Fetches VOD `vod_id`'s published annotations and imports them: its
/// transcript and the marks the last import brought are replaced, the
/// user's own marks stay.
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
    let imported = import(app.clone(), annotations, None).await?;
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    db.set_annotations_imported(vod_id, version.as_deref()).map_err(err)?;
    emit_vod(&app, &db, vod_id)?;
    Ok(imported)
}
