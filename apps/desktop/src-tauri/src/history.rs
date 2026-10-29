//! Undo and redo. Each document (an edit; a VOD's marks and clip bar) has
//! its own history, kept as long as the app is open: every change to one
//! records what it was before and after. Undo in a view puts back the
//! newest change to its document, or one made from it to another document
//! (a clip dropped on an edit from a VOD's view) as long as that document
//! hasn't changed since; redo puts back what was undone last, the same way.
//! Renders, downloads and deleting a VOD or an edit aren't undone.
//!
//! A document is a clip (an edit, or a VOD's own clip with its marks and
//! clip bar), and every change to one is a revision of it (`Db::change`):
//! each change here records the revision it left, and undo and redo say
//! which they leave. A snapshot is the whole document, so putting one back
//! is right whatever revision the clip is at.

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
    /// An edit, and the clips on its clip bar, each as it is.
    Edit {
        state: EditState,
        bar: Vec<(i64, EditState)>,
    },
    Vod {
        marks: Vec<MarkState>,
        clips: Vec<BarClip>,
    },
}

struct Entry {
    doc: Doc,
    /// The view it was made from, if another document's (`Doc`) was open.
    origin: Option<Doc>,
    /// What it was, as people would say it: "Piece removed".
    label: &'static str,
    before: Snapshot,
    after: Snapshot,
    /// The clip's revision once it was made, or last undone or redone.
    revision: i64,
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
    /// The revision it left the document at.
    revision: i64,
}

/// News that `doc` was put back as it was (`restored`), at `revision`.
#[derive(Serialize, Clone)]
struct Restored {
    #[serde(flatten)]
    doc: Doc,
    revision: i64,
}

/// The revision of `doc`'s clip now (0 if it's gone).
pub(crate) fn revision(db: &Db, doc: Doc) -> i64 {
    let clip = match doc {
        Doc::Edit(id) => Some(id),
        Doc::Vod(id) => db.vod_clip(id).ok().flatten(),
    };
    clip.and_then(|c| db.revision(c).ok().flatten()).unwrap_or(0)
}

/// What `doc` holds now; None if it's gone.
pub(crate) fn snapshot(db: &Db, doc: Doc) -> Result<Option<Snapshot>> {
    Ok(match doc {
        Doc::Edit(id) => match db.edit_state(id)? {
            Some(state) => Some(Snapshot::Edit { state, bar: edit_bar(db, id)? }),
            None => None,
        },
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

/// Records a change to `doc` in its history (`History::record`), and makes
/// an edit's snippet again if the change reaches its file, as undoing or
/// redoing it does (`changed`): changes to an edit are recorded through here,
/// so the two directions agree.
pub(crate) fn record(app: &AppHandle, db: &Db, doc: Doc, label: &'static str, before: Option<Snapshot>) {
    app.state::<AppState>().history.record(db, doc, label, before);
    changed(app, db, doc);
}

/// `doc` changed (a change, or its undo or redo): an edit's snippet is made
/// again (`edits::remake_snippet`, which leaves it if its file is as it was).
pub(crate) fn changed(app: &AppHandle, db: &Db, doc: Doc) {
    if let Doc::Edit(id) = doc {
        crate::edits::remake_snippet(app, db, id);
    }
}

impl History {
    /// Records a change to `doc` (`label`), from `before` to what it holds now;
    /// nothing if it didn't change. Takes back what could be redone of it.
    pub(crate) fn record(&self, db: &Db, doc: Doc, label: &'static str, before: Option<Snapshot>) {
        let (Some(before), Ok(Some(after))) = (before, snapshot(db, doc)) else { return };
        if before == after {
            return;
        }
        let revision = revision(db, doc);
        let mut s = self.0.lock().unwrap();
        let origin = s.view.filter(|v| *v != doc);
        s.add(doc, origin, label, before, after, revision);
    }

    /// Forgets `doc`'s newest change if what it holds now is what it was
    /// before that change (the change taken back), as if it had never been
    /// made; false, and nothing forgotten, otherwise.
    pub(crate) fn unrecord(&self, db: &Db, doc: Doc) -> bool {
        let Ok(Some(now)) = snapshot(db, doc) else { return false };
        self.0.lock().unwrap().take_back(doc, &now)
    }

    /// How many changes to `doc` undo can take back.
    #[cfg(test)]
    pub(crate) fn steps(&self, doc: Doc) -> usize {
        self.0.lock().unwrap().undo.get(&doc).map_or(0, Vec::len)
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
    fn add(
        &mut self,
        doc: Doc,
        origin: Option<Doc>,
        label: &'static str,
        before: Snapshot,
        after: Snapshot,
        revision: i64,
    ) {
        self.count += 1;
        let cleared = self.redo.remove(&doc).unwrap_or_default();
        let entry = Entry { doc, origin, label, before, after, revision, at: self.count, cleared };
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

/// Puts back in `doc` what one change did to it, taking it from `from` (as
/// the change left it, or as an undo of it did) to `to`. Only that: what the
/// change didn't touch stays as it is now, so a clip that went meanwhile
/// without a change of its own (its file deleted outside the app) stays gone.
fn restore(app: &AppHandle, db: &Db, doc: Doc, from: &Snapshot, to: &Snapshot) -> Result<()> {
    match (doc, from, to) {
        (Doc::Edit(id), Snapshot::Edit { state: was, bar: had }, Snapshot::Edit { state, bar }) => {
            if was != state {
                db.restore_edit(id, state)?;
            }
            restore_edit_bar(app, db, id, had, bar)?;
        }
        (Doc::Vod(id), Snapshot::Vod { marks: was, clips: had }, Snapshot::Vod { marks, clips }) => {
            if was != marks {
                db.restore_marks(id, marks)?;
            }
            restore_bar(app, db, id, had, clips)?;
        }
        _ => {}
    }
    Ok(())
}

/// The clips on edit `id`'s clip bar (those in it, not only dragged out),
/// each as it is.
fn edit_bar(db: &Db, id: i64) -> Result<Vec<(i64, EditState)>> {
    let mut out = Vec::new();
    for clip in db.bar_clips(id)?.into_iter().filter(|c| c.shown) {
        if let Some(state) = db.edit_state(clip.id)? {
            out.push((clip.id, state));
        }
    }
    Ok(out)
}

/// Puts back what one change did to edit `id`'s clip bar: each clip it
/// made, changed or took away, as `to` has it (by its id, or else by what
/// it holds: a clip made again has a new id). Clips the change didn't touch
/// stay as they are now.
fn restore_edit_bar(
    app: &AppHandle,
    db: &Db,
    id: i64,
    from: &[(i64, EditState)],
    to: &[(i64, EditState)],
) -> Result<()> {
    let state = app.state::<AppState>();
    let now = edit_bar(db, id)?;
    let find = |list: &[(i64, EditState)], (id, s): &(i64, EditState)| {
        list.iter().position(|(i, _)| i == id).or_else(|| list.iter().position(|(_, t)| t == s))
    };
    for want in to {
        if find(from, want).is_some_and(|i| from[i].1 == want.1) {
            continue;
        }
        match find(&now, want) {
            Some(i) if now[i].1 == want.1 => {}
            Some(i) => {
                db.restore_edit(now[i].0, &want.1)?;
                state.clips.stop(now[i].0);
                state.clips.queue_once(now[i].0);
            }
            None => state.clips.queue(db.add_bar_clip(id, &want.1)?),
        }
    }
    for was in from.iter().filter(|was| find(to, was).is_none()) {
        if let Some(i) = find(&now, was) {
            crate::clips::remove(app, db, now[i].0)?;
        }
    }
    Ok(())
}

/// One step in putting a clip bar back (`bar_steps`).
#[derive(Debug, PartialEq)]
enum BarStep<'a> {
    /// Clip `id` back to the stretch and title it had.
    Set(i64, &'a BarClip),
    /// Clip `id` taken off.
    Remove(i64),
    /// A clip made again.
    Add(&'a BarClip),
}

/// The clip in `list` that is `clip`: of its id, or else of the same stretch
/// (a clip made again by an undo has a new id).
fn find<'a>(list: &'a [BarClip], clip: &BarClip) -> Option<&'a BarClip> {
    list.iter().find(|c| c.id == clip.id).or_else(|| list.iter().find(|c| c.same_stretch(clip)))
}

impl BarClip {
    fn same_stretch(&self, other: &BarClip) -> bool {
        (self.start - other.start).abs() < 1e-6 && (self.end - other.end).abs() < 1e-6
    }
}

/// What takes clip bar `now` from `from` to `to` for one change: each clip
/// the change made, changed or took away, put back as `to` has it. Clips the
/// change didn't touch aren't steps, whatever's become of them since.
fn bar_steps<'a>(from: &[BarClip], to: &'a [BarClip], now: &[BarClip]) -> Vec<BarStep<'a>> {
    let mut steps = Vec::new();
    for want in to {
        if find(from, want).is_some_and(|was| was.same_stretch(want) && was.title == want.title) {
            continue;
        }
        match find(now, want) {
            Some(clip) if clip.same_stretch(want) && clip.title == want.title => {}
            Some(clip) => steps.push(BarStep::Set(clip.id, want)),
            None => steps.push(BarStep::Add(want)),
        }
    }
    for was in from.iter().filter(|was| find(to, was).is_none()) {
        if let Some(clip) = find(now, was) {
            steps.push(BarStep::Remove(clip.id));
        }
    }
    steps
}

/// Puts back what one change did to VOD `vod_id`'s clip bar (`bar_steps`):
/// a clip moved or renamed back, taken off (its file too, unless dragged
/// out) or made again.
fn restore_bar(app: &AppHandle, db: &Db, vod_id: i64, from: &[BarClip], to: &[BarClip]) -> Result<()> {
    let state = app.state::<AppState>();
    let now: Vec<BarClip> = db
        .clips(vod_id)?
        .into_iter()
        .filter(|c| c.shown)
        .map(|c| BarClip { id: c.id, start: c.start, end: c.end, title: c.title })
        .collect();
    for step in bar_steps(from, to, &now) {
        match step {
            BarStep::Set(id, want) => {
                let clip = now.iter().find(|c| c.id == id).expect("a clip of now");
                if !clip.same_stretch(want) {
                    db.set_clip_range(id, want.start, want.end)?;
                    state.clips.stop(id);
                    state.clips.queue_once(id);
                }
                if clip.title != want.title {
                    db.rename_clip(id, &want.title)?;
                }
            }
            BarStep::Remove(id) => crate::clips::remove(app, db, id)?,
            BarStep::Add(want) => {
                let clip = db.add_clip(vod_id, want.start, want.end, &want.title, true)?;
                state.clips.queue(clip.id);
            }
        }
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
    let (from, to) = if undo { (&entry.after, &entry.before) } else { (&entry.before, &entry.after) };
    if let Err(e) = restore(app, &db, entry.doc, from, to) {
        // Kept, to try again: it's the newest of its stack still.
        let stacks = if undo { &mut s.undo } else { &mut s.redo };
        stacks.entry(entry.doc).or_default().push(entry);
        return Err(err(e));
    }
    s.count += 1;
    entry.at = s.count;
    entry.revision = revision(&db, entry.doc);
    // Undone or redone, it's no longer the change that took those away.
    entry.cleared.clear();
    let done = Done { doc: entry.doc, label: entry.label, revision: entry.revision };
    let other = if undo { &mut s.redo } else { &mut s.undo };
    other.entry(entry.doc).or_default().push(entry);
    changed(app, &db, done.doc);
    let _ = app.emit("restored", Restored { doc: done.doc, revision: done.revision });
    Ok(Some(done))
}

/// The view open now (an edit, a VOD, or neither), the origin of changes made.
#[tauri::command]
pub(crate) fn set_view(app: AppHandle, view: Option<Doc>) {
    app.state::<AppState>().history.0.lock().unwrap().view = view;
    crate::storyboards::opened(&app, view);
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
        let entry = Entry {
            doc,
            origin,
            label,
            before: empty(),
            after: empty(),
            revision: 0,
            at: s.count,
            cleared: Vec::new(),
        };
        s.undo.entry(doc).or_default().push(entry);
    }

    #[test]
    fn a_change_taken_back_gives_back_what_could_be_redone() {
        let vod = Doc::Vod(1);
        let marked = Snapshot::Vod {
            marks: vec![MarkState { kind: "cut".into(), start: 1.0, text: None, detail: Default::default() }],
            clips: Vec::new(),
        };
        let mut s = Stacks::default();
        s.add(vod, None, "Cut added", empty(), marked.clone(), 1);
        // Undone: the cut can be redone.
        let undone = History::take(&mut s, vod, true).unwrap();
        s.redo.entry(vod).or_default().push(undone);
        // A chapter started, which takes the redo away...
        let chapter = Snapshot::Vod {
            marks: vec![MarkState { kind: "chapter".into(), start: 2.0, text: None, detail: Default::default() }],
            clips: Vec::new(),
        };
        s.add(vod, None, "Chapter added", empty(), chapter, 2);
        assert!(s.redo.get(&vod).is_none_or(Vec::is_empty));
        // ...then Escape takes it back: the cut can be redone again.
        assert!(s.take_back(vod, &empty()));
        assert_eq!(History::take(&mut s, vod, false).map(|e| e.label), Some("Cut added"));
        assert!(s.undo.get(&vod).is_none_or(Vec::is_empty));
    }

    fn bar(clips: &[(i64, f64, f64, &str)]) -> Vec<BarClip> {
        clips.iter().map(|&(id, start, end, title)| BarClip { id, start, end, title: title.into() }).collect()
    }

    #[test]
    fn undo_puts_back_only_what_its_change_did_to_the_clip_bar() {
        // A chapter added with clip 1 in the bar; clip 1 gone since with its
        // file: undoing the chapter leaves the bar as it is.
        let with = bar(&[(1, 1.0, 3.0, "a")]);
        assert_eq!(bar_steps(&with, &with, &[]), []);
        // Clip 2 deleted: undone, it's made again; redone, the one made again goes.
        let both = bar(&[(1, 1.0, 3.0, "a"), (2, 5.0, 6.0, "b")]);
        assert_eq!(bar_steps(&with, &both, &with), [BarStep::Add(&both[1])]);
        let again = bar(&[(1, 1.0, 3.0, "a"), (7, 5.0, 6.0, "b")]);
        assert_eq!(bar_steps(&both, &with, &again), [BarStep::Remove(7)]);
        // Clip 1 resized and renamed: undone, back as it was; others untouched.
        let resized = bar(&[(1, 1.0, 4.0, "c"), (2, 5.0, 6.0, "b")]);
        let now = bar(&[(1, 1.0, 4.0, "c")]);
        assert_eq!(bar_steps(&resized, &both, &now), [BarStep::Set(1, &both[0])]);
        // Clip 3 added: undone, it goes, if it's still there.
        let added = bar(&[(1, 1.0, 3.0, "a"), (3, 8.0, 9.0, "d")]);
        assert_eq!(bar_steps(&added, &with, &added), [BarStep::Remove(3)]);
        assert_eq!(bar_steps(&added, &with, &with), []);
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
