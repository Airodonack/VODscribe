//! Ctrl+C and Ctrl+V (core `clipboard`): what's copied is kept here, in the
//! app, until the next copy or the app quits (a reload of the page keeps
//! it); the system's clipboard gets a line of text for each thing copied,
//! which the page writes. A paste goes ahead only while the system's
//! clipboard still holds that text: something copied since, in any app, is
//! what the user means to paste, not this.
//!
//! A clip copied whose file was made keeps where that file is: pasted, as
//! it was, its copy gets a copy of the file rather than being made again
//! (`Cutter::copy_file`), the same frames at once.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use vodscribe_core::clipboard::{Copied, CopiedKind, Paste};
use vodscribe_core::db::{Clip, ClipStatus, Db};
use vodscribe_core::edits::{self, ItemState, Put};

use crate::history::{self, Doc};
use crate::{AppState, CmdResult, err};

/// What Ctrl+C copied last.
pub(crate) struct Clipboard {
    items: Vec<Held>,
    /// What the page wrote to the system's clipboard for it.
    text: String,
}

#[derive(Clone)]
struct Held {
    copied: Copied,
    file: Option<CopiedFile>,
}

/// A copied clip's made file, and what it was made of then.
#[derive(Clone, Debug)]
pub(crate) struct CopiedFile {
    pub(crate) path: PathBuf,
    pub(crate) thumb: Option<PathBuf>,
    /// The clip's items when it was copied: the file is of these.
    pub(crate) items: Vec<ItemState>,
    /// The file as it was then: one changed or replaced since isn't copied.
    size: u64,
    modified: Option<SystemTime>,
}

impl CopiedFile {
    /// Whether the file at `path` is still the one recorded.
    pub(crate) fn unchanged(&self) -> bool {
        std::fs::metadata(&self.path).is_ok_and(|m| m.len() == self.size && m.modified().ok() == self.modified)
    }
}

/// A thing copied, for the page to say and to write as text: its title,
/// the first of its pieces (a stretch of a VOD, in its seconds) and how
/// many it has.
#[derive(Serialize)]
pub(crate) struct CopiedSaid {
    kind: &'static str,
    title: String,
    first: Option<Stretch>,
    pieces: usize,
    /// How long it plays, in seconds.
    length: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Stretch {
    vod_id: i64,
    vod_title: String,
    start: f64,
    end: f64,
}

/// Copies `items` (what's selected or picked) as they are now, in their
/// order, in place of what was copied before. What the page needs to say
/// so and to write the text (`copied_text`).
#[tauri::command]
pub(crate) async fn copy_items(app: AppHandle, items: Vec<Put>) -> CmdResult<Vec<CopiedSaid>> {
    let thumbs = app.clone();
    let (held, said) = crate::with_reads(app.clone(), move |db| {
        let held = items.iter().map(|item| held(&thumbs, db, item)).collect::<Result<Vec<_>>>()?;
        let said = held.iter().map(|h| said(db, &h.copied)).collect::<Result<Vec<_>>>()?;
        Ok((held, said))
    })
    .await?;
    *app.state::<AppState>().clipboard.lock().unwrap() = Some(Clipboard { items: held, text: String::new() });
    Ok(said)
}

/// What the page wrote to the system's clipboard for the last copy.
#[tauri::command]
pub(crate) fn copied_text(app: AppHandle, text: String) {
    if let Some(clipboard) = app.state::<AppState>().clipboard.lock().unwrap().as_mut() {
        clipboard.text = text;
    }
}

/// `item` copied, with its file if it's a clip whose file is made.
fn held(app: &AppHandle, db: &Db, item: &Put) -> Result<Held> {
    let copied = db.copy_of(item)?;
    let file = match item {
        Put::Clip { id } => db.clip(*id)?.filter(|c| c.status == ClipStatus::Ready).and_then(|c| {
            let path = PathBuf::from(c.file_path?);
            let meta = std::fs::metadata(&path).ok()?;
            let thumb = Some(crate::clips::thumb_path(app, c.id)).filter(|t| t.is_file());
            Some(CopiedFile {
                path,
                thumb,
                items: copied.state.items.clone(),
                size: meta.len(),
                modified: meta.modified().ok(),
            })
        }),
        _ => None,
    };
    Ok(Held { copied, file })
}

/// What the page is told of `copied` (`CopiedSaid`).
fn said(db: &Db, copied: &Copied) -> Result<CopiedSaid> {
    let pieces: Vec<&ItemState> = copied.state.items.iter().filter(|i| i.vod_id.is_some()).collect();
    let mut length = 0.0;
    for p in &pieces {
        let start = p.start.map_or(0.0, |s| s.seconds());
        let end = match p.end {
            Some(end) => end.seconds(),
            None => db.vod(p.vod_id.expect("a piece"))?.and_then(|v| v.info.duration).unwrap_or(start),
        };
        let struck: Vec<(f64, f64)> = p.struck.iter().map(|(a, b)| (a.seconds(), b.seconds())).collect();
        length += edits::spans(start, end, &struck).iter().map(|(a, b)| b - a).sum::<f64>();
    }
    let first = match pieces.first() {
        Some(p) => {
            let vod_id = p.vod_id.expect("a piece");
            let vod = db.vod(vod_id)?;
            let start = p.start.map_or(0.0, |s| s.seconds());
            let end = match p.end {
                Some(end) => end.seconds(),
                None => vod.as_ref().and_then(|v| v.info.duration).unwrap_or(start),
            };
            let vod_title = vod.map(|v| v.info.title).unwrap_or_default();
            Some(Stretch { vod_id, vod_title, start, end })
        }
        None => None,
    };
    let kind = match copied.kind {
        CopiedKind::Clip => "clip",
        CopiedKind::Stretch => "stretch",
    };
    Ok(CopiedSaid { kind, title: copied.state.title.clone(), first, pieces: pieces.len(), length })
}

/// What a paste made, for the page to show it.
#[derive(Serialize)]
pub(crate) struct Pasted {
    /// The clips made on the bar, in order.
    clips: Vec<Clip>,
    /// On a timeline, the edit as it is now, and the stretch the paste plays.
    view: Option<crate::edits::EditView>,
    span: Option<(f64, f64)>,
    /// Pieces left out, their VOD gone; copies left out, of another VOD.
    dropped: usize,
    refused: usize,
}

/// Pastes what was copied last into `target` (a VOD or an edit, or a clip
/// open in the editor): on its clip bar if `bar`, else in its timeline at
/// `at` seconds, in place of `replace` if given; as one change undo takes
/// back whole. Nothing (None) if nothing was copied, or the system's
/// clipboard now holds something other than the copy's `text`.
#[tauri::command]
pub(crate) async fn paste_items(
    app: AppHandle,
    target: Doc,
    bar: bool,
    at: Option<f64>,
    replace: Option<(f64, f64)>,
    text: String,
) -> CmdResult<Option<Pasted>> {
    let state = app.state::<AppState>();
    let held = match state.clipboard.lock().unwrap().as_ref() {
        Some(clipboard) if clipboard.text == text && !clipboard.items.is_empty() => clipboard.items.clone(),
        _ => return Ok(None),
    };
    let db = state.db.lock().unwrap();
    let onto = match (target, bar) {
        (Doc::Vod(vod), true) => Paste::Bar(db.vod_clip(vod).map_err(err)?.ok_or("That VOD is gone.")?),
        (Doc::Edit(id), true) => Paste::Bar(id),
        (Doc::Edit(id), false) => Paste::Timeline { clip: id, at: at.unwrap_or(0.0), replace },
        (Doc::Vod(_), false) => return Err("A VOD's timeline takes no pieces: paste on its clip bar.".into()),
    };
    let label = match (bar, held.len()) {
        (true, 1) => "Clip pasted",
        (true, _) => "Clips pasted",
        (false, _) => "Pasted",
    };
    let copies: Vec<Copied> = held.iter().map(|h| h.copied.clone()).collect();
    let pasted = history::change(&app, &db, target, label, |db| db.paste(onto, &copies)).map_err(err)?;
    let mut clips = Vec::with_capacity(pasted.clips.len());
    for &(id, of) in &pasted.clips {
        if let Some(file) = &held[of].file {
            state.clips.copy_file(id, file.clone());
        }
        state.clips.queue(id);
        clips.extend(db.clip(id).map_err(err)?);
    }
    let view = match onto {
        Paste::Timeline { clip, .. } => Some(crate::edits::view(&app, &db, clip)?),
        Paste::Bar(_) => None,
    };
    Ok(Some(Pasted { clips, view, span: pasted.span, dropped: pasted.dropped, refused: pasted.refused }))
}

/// Gives clip `id` (just pasted, waiting) a copy of `file` as its own,
/// rather than making it, if it holds what the file was made of and the
/// file is as it was. True if it did, or there's nothing left to do (the
/// clip gone, or made meanwhile); false to make it the usual way.
pub(crate) fn copy_file(app: &AppHandle, id: i64, file: &CopiedFile) -> Result<bool> {
    if !file.unchanged() {
        return Ok(false);
    }
    let state = app.state::<AppState>();
    let (out, whole, clip) = {
        let db = state.db.lock().unwrap();
        let Some(clip) = db.clip(id)? else { return Ok(true) };
        if clip.status == ClipStatus::Ready {
            return Ok(true);
        }
        let Some(now) = db.edit_state(id)? else { return Ok(true) };
        if now.items != file.items {
            return Ok(false);
        }
        let name = file.path.file_name().context("a clip's file has a name")?;
        let dir = file.path.parent().unwrap_or(Path::new("."));
        let whole = edits::cut_whole(&db.edit_pieces(id)?);
        (vodscribe_core::clips::unique_path(dir, &name.to_string_lossy()), whole, clip)
    };
    crate::files::writing(&out);
    if let Err(e) = std::fs::copy(&file.path, &out) {
        eprintln!("clip {id}: copying {}: {e}", file.path.display());
        crate::files::delete_file(&out);
        return Ok(false);
    }
    let db = state.db.lock().unwrap();
    let path = out.to_string_lossy();
    let recorded = if db.clip(id)?.is_none() {
        false
    } else if whole {
        db.set_clip_file(id, clip.start, clip.end, ClipStatus::Ready, Some(&path), None)?
    } else {
        db.set_edit_render(id, ClipStatus::Ready, Some(&path), None, db.revision(id)?)?;
        true
    };
    if !recorded {
        // Deleted, or changed, while the file was copied.
        crate::files::delete_file(&out);
        return Ok(db.clip(id)?.is_none());
    }
    db.set_made_now(id)?;
    if let Some(thumb) = &file.thumb
        && let Err(e) = std::fs::copy(thumb, crate::clips::thumb_path(app, id))
    {
        eprintln!("clip {id}: its picture: {e}");
    }
    crate::clips::emit(app, &db, id);
    Ok(true)
}
