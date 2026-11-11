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
//!
//! Ctrl+X is a cut, meant to be pasted: the clips it takes off their bar
//! are held (`cut_clips`), still in the library with their files but in no
//! clip bar, so every paste of the cut gets their frames. They go as Delete
//! takes a clip once the copy is over: at the next copy or cut, or as the
//! app quits (and at the next start, for one it didn't quit cleanly:
//! `Db::forget_hidden_clips`). Undo of the cut puts the held clip itself
//! back on its bar (`unhold`); an undo or redo that takes a clip off while
//! the last copy has its file holds it rather than deleting it
//! (`hold_again`), so a redone cut is held again.

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::Manager;
use vodscribe_core::clipboard::{Copied, CopiedKind, Paste};
use vodscribe_core::db::{Clip, ClipStatus, Db};
use vodscribe_core::edits::{self, EditState, ItemState, Put};
use vodscribe_core::settings::ClipEncoding;

use crate::AppHandle;
use crate::history::{self, Doc};
use crate::{AppState, CmdResult, err};

/// What Ctrl+C copied last.
pub(crate) struct Clipboard {
    items: Vec<Held>,
    /// What the page wrote to the system's clipboard for it.
    text: String,
    /// The clips a cut of this copy took off their bar, kept for its
    /// pastes: held while they're off it.
    cut: Vec<i64>,
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
    /// What the file was made with, if that's known.
    pub(crate) encoding: Option<ClipEncoding>,
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
    let new = Clipboard { items: held, text: String::new(), cut: Vec::new() };
    let old = app.state::<AppState>().clipboard.lock().unwrap().replace(new);
    // The copy before is over: what its cut held goes.
    if let Some(old) = old {
        let state = app.state::<AppState>();
        let_go(&app, &state.db.lock().unwrap(), old);
    }
    Ok(said)
}

/// The files of the clips copied last, as they were copied: a cut holds
/// only a clip whose file its copy has.
pub(crate) fn copied_files(app: &AppHandle) -> Vec<PathBuf> {
    let state = app.state::<AppState>();
    let clipboard = state.clipboard.lock().unwrap();
    clipboard.iter().flat_map(|c| &c.items).filter_map(|h| Some(h.file.as_ref()?.path.clone())).collect()
}

/// Clips `ids`, taken off their bar by a cut of what was copied last, held
/// for its pastes.
pub(crate) fn hold(app: &AppHandle, ids: &[i64]) {
    if let Some(clipboard) = app.state::<AppState>().clipboard.lock().unwrap().as_mut() {
        for &id in ids {
            if !clipboard.cut.contains(&id) {
                clipboard.cut.push(id);
            }
        }
    }
}

/// Whether clip `id` is one the cut of what was copied last took.
fn is_cut(app: &AppHandle, id: i64) -> bool {
    app.state::<AppState>().clipboard.lock().unwrap().as_ref().is_some_and(|c| c.cut.contains(&id))
}

/// Clip `id` is gone from the library: not held any more, so a cut lets go
/// of only the clips still in it.
pub(crate) fn forget(app: &AppHandle, id: i64) {
    if let Some(clipboard) = app.state::<AppState>().clipboard.lock().unwrap().as_mut() {
        clipboard.cut.retain(|&c| c != id);
    }
}

/// Puts clip `id` back on `doc`'s bar if the cut holds it and it's the clip
/// an undo of the cut wants, on that bar and made of `made`: the clip
/// itself, with its file. False otherwise, for the undo to make the clip
/// anew. The check is only a guard, now that a deleted clip's id is never
/// given to another (`MIGRATE_V42` in core db.rs).
pub(crate) fn unhold(app: &AppHandle, db: &Db, doc: Doc, id: i64, made: &EditState) -> Result<bool> {
    let Some(clip) = db.clip(id)?.filter(|c| !c.shown) else { return Ok(false) };
    let wanted = crate::clips::bar_doc(&clip) == doc
        && db.edit_state(id)?.is_some_and(|s| s.items == made.items && s.marks == made.marks);
    if !is_cut(app, id) || !wanted {
        return Ok(false);
    }
    db.show_clip(id)?;
    crate::clips::emit(app, db, id);
    Ok(true)
}

/// Holds clip `id` again, off its bar, rather than letting a redo of its
/// cut delete it, while what was copied last has its file (`clips::held_by`:
/// the cut's own copy, or a copy of the clip made since): false if not, for
/// the redo to delete it.
pub(crate) fn hold_again(app: &AppHandle, db: &Db, id: i64) -> Result<bool> {
    let Some(clip) = db.clip(id)?.filter(|c| c.shown) else { return Ok(false) };
    if !crate::clips::held_by(&clip, &copied_files(app)) {
        return Ok(false);
    }
    db.hide_clip(id)?;
    hold(app, &[id]);
    crate::clips::emit(app, db, id);
    Ok(true)
}

/// The clips `old`'s cut holds, deleted as Delete takes a clip: those back
/// on a bar (undone, or made again there) stay.
fn let_go(app: &AppHandle, db: &Db, old: Clipboard) {
    for id in old.cut {
        if db.clip(id).ok().flatten().is_some_and(|c| !c.shown)
            && let Err(e) = crate::clips::remove(app, db, id)
        {
            eprintln!("clip {id}, cut and not pasted: {e:#}");
        }
    }
}

/// As the app quits (`db` held): the copy is over, and what its cut holds goes.
pub(crate) fn quit(app: &AppHandle, db: &Db) {
    let old = app.state::<AppState>().clipboard.lock().unwrap().take();
    if let Some(old) = old {
        let_go(app, db, old);
    }
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
                encoding: c.encoding,
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
    let length = db.played_of(&copied.state)?;
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
    /// The clips already on the bar that stretches pasted are: none made of those.
    found: Vec<Clip>,
    /// On a timeline, the edit as it is now, and the stretch the paste plays.
    view: Option<crate::edits::EditView>,
    span: Option<(f64, f64)>,
    /// Pieces left out, their VOD gone; copies left out, of another VOD.
    dropped: usize,
    refused: usize,
}

/// Pastes what was copied last into `target` (a VOD or an edit, or a clip
/// open in the editor): on its clip bar if `bar` (after its clip `after`,
/// else last), else in its timeline at `at` seconds (where.ts
/// `insertionPoint`: after what's selected, never in its place, else the
/// playhead); as one change undo takes back whole. Nothing (None) if
/// nothing was copied, its text is empty (not written yet), or the
/// system's clipboard now holds something other than the copy's `text`.
#[tauri::command]
pub(crate) async fn paste_items(
    app: AppHandle,
    target: Doc,
    bar: bool,
    at: Option<f64>,
    after: Option<i64>,
    text: String,
) -> CmdResult<Option<Pasted>> {
    let state = app.state::<AppState>();
    let held = match state.clipboard.lock().unwrap().as_ref() {
        // An empty text is no copy: the page hasn't written it yet, and an
        // empty system clipboard isn't this copy.
        Some(clipboard) if !clipboard.text.is_empty() && clipboard.text == text && !clipboard.items.is_empty() => {
            clipboard.items.clone()
        }
        _ => return Ok(None),
    };
    let db = state.db.lock().unwrap();
    let onto = match (target, bar) {
        (Doc::Vod(vod), true) => Paste::Bar { bar: db.vod_clip(vod).map_err(err)?.ok_or("That VOD is gone.")?, after },
        (Doc::Edit(id), true) => Paste::Bar { bar: id, after },
        (Doc::Edit(id), false) => Paste::Timeline { clip: id, at: at.unwrap_or(0.0) },
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
        Paste::Bar { .. } => None,
    };
    let mut found = Vec::with_capacity(pasted.found.len());
    for &(id, _) in &pasted.found {
        found.extend(db.clip(id).map_err(err)?);
    }
    Ok(Some(Pasted { clips, found, view, span: pasted.span, dropped: pasted.dropped, refused: pasted.refused }))
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
    let (out, whole, clip, made_of, version) = {
        let db = state.db.lock().unwrap();
        let Some(clip) = db.clip(id)? else { return Ok(true) };
        if clip.status == ClipStatus::Ready {
            return Ok(true);
        }
        let Some(now) = db.edit_state(id)? else { return Ok(true) };
        if now.items != file.items {
            return Ok(false);
        }
        let ext = file.path.extension().and_then(|e| e.to_str()).context("a clip's file has an extension")?;
        let whole = edits::cut_whole(&db.edit_pieces(id)?);
        // What it's made of, and its revision, as the copy begins: recorded
        // with the file, so a change in the editor while it's copied leaves
        // it not current, and the remake queued with that change makes it
        // again (`edits::render_edit`).
        (db.next_clip_file(id, ext)?, whole, clip, db.made_of_now(id)?, db.revision(id)?)
    };
    crate::files::writing(&out);
    if let Err(e) = std::fs::copy(&file.path, &out) {
        eprintln!("clip {id}: copying {}: {e}", file.path.display());
        crate::files::delete_file(&out);
        return Ok(false);
    }
    // (A test holds it here, as a large clip on a slow disk takes a while.)
    #[cfg(test)]
    crate::testkit::copied(&out);
    let db = state.db.lock().unwrap();
    let path = out.to_string_lossy();
    // A snippet changed in the editor meanwhile keeps the file but waits for
    // its remake, so nothing hands this one out as if it were made
    // (`clips::status_once_made`). A listed edit renders only when asked,
    // so it's left made, and not current, as a render of it would be.
    let status = match db.is_snippet(id)? {
        Some(true) => crate::clips::status_once_made(app, &db, id, &made_of)?,
        _ => ClipStatus::Ready,
    };
    let recorded = if db.clip(id)?.is_none() {
        false
    } else if whole {
        db.set_clip_file(id, clip.start, clip.end, status, Some(&path), None)?
    } else {
        db.set_edit_render(id, status, Some(&path), None, version)?;
        true
    };
    if !recorded {
        // Deleted, or changed, while the file was copied.
        crate::files::delete_file(&out);
        return Ok(db.clip(id)?.is_none());
    }
    db.set_made_of(id, &made_of)?;
    db.set_made_as(id, file.encoding)?;
    if let Some(thumb) = &file.thumb
        && let Err(e) = std::fs::copy(thumb, crate::clips::thumb_path(app, id))
    {
        eprintln!("clip {id}: its picture: {e}");
    }
    crate::clips::emit(app, &db, id);
    Ok(true)
}

#[cfg(test)]
mod tests;
