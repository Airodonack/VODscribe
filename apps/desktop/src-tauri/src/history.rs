//! Undo and redo. Each document (an edit; a VOD's marks and clip bar) has
//! its own history, kept as long as the app is open: every change to one
//! records what it was before and after. Undo in a view puts back the
//! newest change to its document, or one made from it to another document
//! (a clip dropped on an edit from a VOD's view) as long as that document
//! hasn't changed since; redo puts back what was undone last, the same way.
//! Renders, downloads and deleting a VOD or an edit aren't undone.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use vodscribe_core::db::{Db, MarkState};
use vodscribe_core::edits::EditState;

use crate::{AppState, CmdResult, err};

/// The most changes kept for each document.
const KEEP: usize = 200;

/// A document with a history: an edit, or a VOD's marks and clip bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub(crate) enum Doc {
    Edit(i64),
    Vod(i64),
}

/// A clip in a VOD's clip bar, as a snapshot holds it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BarClip {
    id: i64,
    start: f64,
    end: f64,
    title: String,
}

/// What a document holds at one moment.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Snapshot {
    Edit(EditState),
    Vod { marks: Vec<MarkState>, clips: Vec<BarClip> },
}

struct Entry {
    doc: Doc,
    /// The view it was made from, if another document's (`Doc`) was open.
    origin: Option<Doc>,
    /// What it was, as people would say it: "Piece removed".
    label: &'static str,
    before: Snapshot,
    after: Snapshot,
    /// When it was made, or last undone or redone (one count for all documents).
    at: u64,
    /// What could be redone when it was made, which it took away: given back
    /// if it's taken back unmade (`unrecord`).
    cleared: Vec<Entry>,
}

#[derive(Default)]
struct Stacks {
    /// The view open (set by the page), the origin of changes made now.
    view: Option<Doc>,
    count: u64,
    undo: HashMap<Doc, Vec<Entry>>,
    redo: HashMap<Doc, Vec<Entry>>,
}

#[derive(Default)]
pub(crate) struct History(Mutex<Stacks>);

/// What undo or redo did, for the page to say so and show it.
#[derive(Serialize, Clone)]
pub(crate) struct Done {
    doc: Doc,
    label: &'static str,
}

/// What `doc` holds now; None if it's gone.
pub(crate) fn snapshot(db: &Db, doc: Doc) -> Result<Option<Snapshot>> {
    Ok(match doc {
        Doc::Edit(id) => db.edit_state(id)?.map(Snapshot::Edit),
        Doc::Vod(id) => {
            let clips = db
                .clips(id)?
                .into_iter()
                .filter(|c| c.shown)
                .map(|c| BarClip { id: c.id, start: c.start, end: c.end, title: c.title })
                .collect();
            Some(Snapshot::Vod { marks: db.mark_states(id)?, clips })
        }
    })
}

impl History {
    /// Records a change to `doc` (`label`), from `before` to what it holds now;
    /// nothing if it didn't change. Takes back what could be redone of it.
    pub(crate) fn record(&self, db: &Db, doc: Doc, label: &'static str, before: Option<Snapshot>) {
        let (Some(before), Ok(Some(after))) = (before, snapshot(db, doc)) else { return };
        if before == after {
            return;
        }
        let mut s = self.0.lock().unwrap();
        let origin = s.view.filter(|v| *v != doc);
        s.add(doc, origin, label, before, after);
    }

    /// Forgets `doc`'s newest change if what it holds now is what it was
    /// before that change (the change taken back), as if it had never been
    /// made; false, and nothing forgotten, otherwise.
    pub(crate) fn unrecord(&self, db: &Db, doc: Doc) -> bool {
        let Ok(Some(now)) = snapshot(db, doc) else { return false };
        self.0.lock().unwrap().take_back(doc, &now)
    }

    /// Forgets `doc`'s history (it's deleted).
    pub(crate) fn forget(&self, doc: Doc) {
        let mut s = self.0.lock().unwrap();
        s.undo.remove(&doc);
        s.redo.remove(&doc);
    }

    /// Takes the newest change to step back (`undo`) or forward from `stacks`,
    /// as seen from `view`: its document's own, or one made from it to another
    /// whose newest change it is.
    fn take(s: &mut Stacks, view: Doc, undo: bool) -> Option<Entry> {
        let stacks = if undo { &s.undo } else { &s.redo };
        let doc = stacks
            .iter()
            .filter_map(|(doc, list)| Some((*doc, list.last()?)))
            .filter(|(doc, e)| *doc == view || e.origin == Some(view))
            .max_by_key(|(_, e)| e.at)
            .map(|(doc, _)| doc)?;
        let stacks = if undo { &mut s.undo } else { &mut s.redo };
        stacks.get_mut(&doc)?.pop()
    }
}

impl Stacks {
    fn add(&mut self, doc: Doc, origin: Option<Doc>, label: &'static str, before: Snapshot, after: Snapshot) {
        self.count += 1;
        let cleared = self.redo.remove(&doc).unwrap_or_default();
        let entry = Entry { doc, origin, label, before, after, at: self.count, cleared };
        let stack = self.undo.entry(doc).or_default();
        stack.push(entry);
        if stack.len() > KEEP {
            stack.remove(0);
        }
    }

    /// `History::unrecord`, given what `doc` holds now.
    fn take_back(&mut self, doc: Doc, now: &Snapshot) -> bool {
        let Some(stack) = self.undo.get_mut(&doc) else { return false };
        if stack.last().is_none_or(|e| e.before != *now) {
            return false;
        }
        let entry = stack.pop().expect("checked above");
        if !entry.cleared.is_empty() {
            self.redo.insert(doc, entry.cleared);
        }
        true
    }
}

/// Puts `snapshot` back into `doc`.
fn restore(app: &AppHandle, db: &Db, doc: Doc, snapshot: &Snapshot) -> Result<()> {
    match (doc, snapshot) {
        (Doc::Edit(id), Snapshot::Edit(state)) => db.restore_edit(id, state)?,
        (Doc::Vod(id), Snapshot::Vod { marks, clips }) => {
            db.restore_marks(id, marks)?;
            restore_bar(app, db, id, clips)?;
        }
        _ => {}
    }
    Ok(())
}

/// Makes VOD `vod_id`'s clip bar hold `clips`: those still there (by id, or
/// else of the same stretch) kept, with their stretch and title as they were;
/// the rest taken off (their files too, unless dragged out) or made again.
fn restore_bar(app: &AppHandle, db: &Db, vod_id: i64, clips: &[BarClip]) -> Result<()> {
    let state = app.state::<AppState>();
    let mut now: Vec<_> = db.clips(vod_id)?.into_iter().filter(|c| c.shown).collect();
    let mut missing = Vec::new();
    for want in clips {
        let same =
            |c: &vodscribe_core::db::Clip| (c.start - want.start).abs() < 1e-6 && (c.end - want.end).abs() < 1e-6;
        let found = now.iter().position(|c| c.id == want.id).or_else(|| now.iter().position(same));
        let Some(at) = found else {
            missing.push(want);
            continue;
        };
        let clip = now.remove(at);
        if !same(&clip) {
            db.set_clip_range(clip.id, want.start, want.end)?;
            state.clips.stop(clip.id);
            state.clips.queue(clip.id);
        }
        if clip.title != want.title {
            db.rename_clip(clip.id, &want.title)?;
        }
    }
    for gone in now {
        crate::clips::remove(app, db, gone.id)?;
    }
    for want in missing {
        let clip = db.add_clip(vod_id, want.start, want.end, &want.title, true)?;
        state.clips.queue(clip.id);
    }
    Ok(())
}

/// Steps `view`'s history back (`undo`) or forward, and tells the page which
/// document changed (`restored`).
fn step(app: &AppHandle, view: Doc, undo: bool) -> CmdResult<Option<Done>> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let mut s = state.history.0.lock().unwrap();
    let Some(mut entry) = History::take(&mut s, view, undo) else { return Ok(None) };
    if let Err(e) = restore(app, &db, entry.doc, if undo { &entry.before } else { &entry.after }) {
        // Kept, to try again: it's the newest of its stack still.
        let stacks = if undo { &mut s.undo } else { &mut s.redo };
        stacks.entry(entry.doc).or_default().push(entry);
        return Err(err(e));
    }
    s.count += 1;
    entry.at = s.count;
    // Undone or redone, it's no longer the change that took those away.
    entry.cleared.clear();
    let done = Done { doc: entry.doc, label: entry.label };
    let other = if undo { &mut s.redo } else { &mut s.undo };
    other.entry(entry.doc).or_default().push(entry);
    let _ = app.emit("restored", done.doc);
    Ok(Some(done))
}

/// The view open now (an edit, a VOD, or neither), the origin of changes made.
#[tauri::command]
pub(crate) fn set_view(app: AppHandle, view: Option<Doc>) {
    app.state::<AppState>().history.0.lock().unwrap().view = view;
}

/// Undoes the newest change in `view`'s history; what it was, if there was one.
#[tauri::command]
pub(crate) async fn undo(app: AppHandle, view: Doc) -> CmdResult<Option<Done>> {
    step(&app, view, true)
}

/// Redoes what was undone last in `view`'s history.
#[tauri::command]
pub(crate) async fn redo(app: AppHandle, view: Doc) -> CmdResult<Option<Done>> {
    step(&app, view, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Snapshot {
        Snapshot::Vod { marks: Vec::new(), clips: Vec::new() }
    }

    fn push(s: &mut Stacks, doc: Doc, origin: Option<Doc>, label: &'static str) {
        s.count += 1;
        let entry = Entry { doc, origin, label, before: empty(), after: empty(), at: s.count, cleared: Vec::new() };
        s.undo.entry(doc).or_default().push(entry);
    }

    #[test]
    fn a_change_taken_back_gives_back_what_could_be_redone() {
        let vod = Doc::Vod(1);
        let marked =
            Snapshot::Vod { marks: vec![MarkState { kind: "cut".into(), start: 1.0, text: None }], clips: Vec::new() };
        let mut s = Stacks::default();
        s.add(vod, None, "Cut added", empty(), marked.clone());
        // Undone: the cut can be redone.
        let undone = History::take(&mut s, vod, true).unwrap();
        s.redo.entry(vod).or_default().push(undone);
        // A chapter started, which takes the redo away...
        let chapter = Snapshot::Vod {
            marks: vec![MarkState { kind: "chapter".into(), start: 2.0, text: None }],
            clips: Vec::new(),
        };
        s.add(vod, None, "Chapter added", empty(), chapter);
        assert!(s.redo.get(&vod).is_none_or(Vec::is_empty));
        // ...then Escape takes it back: the cut can be redone again.
        assert!(s.take_back(vod, &empty()));
        assert_eq!(History::take(&mut s, vod, false).map(|e| e.label), Some("Cut added"));
        assert!(s.undo.get(&vod).is_none_or(Vec::is_empty));
    }

    #[test]
    fn a_view_undoes_its_own_changes_and_those_made_from_it() {
        let (vod, edit) = (Doc::Vod(1), Doc::Edit(2));
        let mut s = Stacks::default();
        push(&mut s, vod, None, "Cut added");
        // A clip dropped on the edit from the VOD's view.
        push(&mut s, edit, Some(vod), "Piece added");

        // In the VOD's view, the drop is the newest thing done there.
        assert_eq!(History::take(&mut s, vod, true).map(|e| e.label), Some("Piece added"));
        assert_eq!(History::take(&mut s, vod, true).map(|e| e.label), Some("Cut added"));
        assert!(History::take(&mut s, vod, true).is_none());

        // Once the edit has changed since, the drop is the edit's to undo.
        let mut s = Stacks::default();
        push(&mut s, edit, Some(vod), "Piece added");
        push(&mut s, edit, None, "Piece resized");
        push(&mut s, vod, None, "Cut added");
        assert_eq!(History::take(&mut s, vod, true).map(|e| e.label), Some("Cut added"));
        assert!(History::take(&mut s, vod, true).is_none(), "not the drop under the resize");
        assert_eq!(History::take(&mut s, edit, true).map(|e| e.label), Some("Piece resized"));
        assert_eq!(History::take(&mut s, edit, true).map(|e| e.label), Some("Piece added"));
    }
}
