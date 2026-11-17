//! The specs of undo and redo (`undo`, `redo`, `set_view`), through the
//! commands as the page calls them: which change a view takes back or puts
//! back, what it answers and tells the page, what's kept and what's
//! forgotten. Then what putting a change back does to a clip bar, at
//! `restore` and its steps, where the library alone is enough.

use super::*;

use serde_json::{Value, json};

use crate::testkit::TestApp;

fn vod_view(id: i64) -> Value {
    json!({ "kind": "vod", "id": id })
}

fn edit_view(id: i64) -> Value {
    json!({ "kind": "edit", "id": id })
}

/// Undo in `view`, as Ctrl+Z asks it: what it did (`Done`), or null.
fn undo_in(app: &TestApp, view: Value) -> Result<Value, Value> {
    app.invoke("undo", json!({ "view": view }))
}

/// Redo in `view`, as Ctrl+Shift+Z asks it.
fn redo_in(app: &TestApp, view: Value) -> Result<Value, Value> {
    app.invoke("redo", json!({ "view": view }))
}

/// Undo in `view`: the label of what it took back, if anything.
fn undo(app: &TestApp, view: Value) -> Option<String> {
    undo_in(app, view).unwrap()["label"].as_str().map(String::from)
}

/// Redo in `view`: the label of what it put back, if anything.
fn redo(app: &TestApp, view: Value) -> Option<String> {
    redo_in(app, view).unwrap()["label"].as_str().map(String::from)
}

/// The page opens `view` (a VOD's, an edit's, or none).
fn open(app: &TestApp, view: Option<Value>) {
    app.invoke("set_view", json!({ "view": view })).unwrap();
}

/// A cut added at `time` of VOD `vod` (`add_mark`, no kind given).
fn add_cut(app: &TestApp, vod: i64, time: f64) {
    app.invoke("add_mark", json!({ "vodId": vod, "time": time, "kind": null, "text": null })).unwrap();
}

/// VOD `vod`'s marks' moments.
fn marks(app: &TestApp, vod: i64) -> Vec<f64> {
    let marks = app.invoke("list_marks", json!({ "vodId": vod })).unwrap();
    marks.as_array().unwrap().iter().map(|m| m["start"].as_f64().unwrap()).collect()
}

fn rename_edit(app: &TestApp, edit: i64, title: &str) {
    app.invoke("rename_edit", json!({ "id": edit, "title": title })).unwrap();
}

/// Edit `edit` as the Edit view loads it (`get_edit`).
fn get_edit(app: &TestApp, edit: i64) -> Value {
    app.invoke("get_edit", json!({ "id": edit })).unwrap()
}

fn edit_title(app: &TestApp, edit: i64) -> String {
    get_edit(app, edit)["edit"]["title"].as_str().unwrap().to_string()
}

/// Edit `edit`'s pieces, each its VOD's stretch.
fn pieces(app: &TestApp, edit: i64) -> Vec<(f64, f64)> {
    let view = get_edit(app, edit);
    view["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["start"].as_f64().unwrap(), p["end"].as_f64().unwrap()))
        .collect()
}

/// A VOD of a minute and an edit of its first ten seconds, already there.
fn vod_and_edit(app: &TestApp) -> (i64, i64) {
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    (vod, app.edit_of(&[(vod, 0.0, 10.0)]))
}

/// Undo takes back the newest change first, then the one before it
/// (history.rs `undo`: "Undoes the newest change in `view`'s history"; 0.7.1
/// Fixes: "Undo undoes the latest change.").
#[test]
fn undo_takes_back_the_newest_change_first() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    rename_edit(&app, edit, "Highlights");
    app.invoke("add_range_to_edit", json!({ "editId": edit, "vodId": vod, "start": 20.0, "end": 30.0 })).unwrap();
    assert_eq!(undo(&app, edit_view(edit)).as_deref(), Some("Piece added"));
    assert_eq!(pieces(&app, edit), [(0.0, 10.0)]);
    assert_eq!(edit_title(&app, edit), "Highlights", "only the newest taken back");
    assert_eq!(undo(&app, edit_view(edit)).as_deref(), Some("Edit renamed"));
    assert_eq!(edit_title(&app, edit), "Best of");
}

/// Redo puts back what was undone last, then what was undone before it
/// (history.rs, its module doc: "redo puts back what was undone last, the
/// same way"; 0.4.0: "Undo and redo (Ctrl+Z and Ctrl+Shift+Z, or ⌘Z and
/// ⇧⌘Z) for edits, marks and the clip bar.").
#[test]
fn redo_puts_back_what_was_undone_last_first() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    add_cut(&app, vod, 5.0);
    add_cut(&app, vod, 9.0);
    undo(&app, vod_view(vod));
    undo(&app, vod_view(vod));
    assert!(marks(&app, vod).is_empty());
    assert_eq!(redo(&app, vod_view(vod)).as_deref(), Some("Cut added"));
    assert_eq!(marks(&app, vod), [5.0], "the one undone last, the first cut");
    assert_eq!(redo(&app, vod_view(vod)).as_deref(), Some("Cut added"));
    assert_eq!(marks(&app, vod), [5.0, 9.0]);
    assert_eq!(redo_in(&app, vod_view(vod)), Ok(Value::Null), "nothing more to redo");
}

/// Each document has its own history: undo in a VOD's view takes back the
/// VOD's change, not a newer one made to an edit in the edit's view
/// (history.rs, its module doc: "Each document (an edit; a VOD's marks and
/// clip bar) has its own history").
#[test]
fn each_document_has_its_own_history() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    open(&app, Some(vod_view(vod)));
    add_cut(&app, vod, 5.0);
    open(&app, Some(edit_view(edit)));
    rename_edit(&app, edit, "Highlights");
    open(&app, Some(vod_view(vod)));
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Cut added"));
    assert_eq!(edit_title(&app, edit), "Highlights", "the edit's change is the edit's");
    assert_eq!(undo_in(&app, vod_view(vod)), Ok(Value::Null));
    assert_eq!(undo(&app, edit_view(edit)).as_deref(), Some("Edit renamed"));
}

/// Undo in a VOD's view takes back a clip dropped from it on an edit: the
/// newest thing done there, before the VOD's own change (history.rs, its
/// module doc: undo "puts back the newest change to its document, or one
/// made from it to another document (a clip dropped on an edit from a VOD's
/// view)"; `set_view`: "the origin of changes made").
#[test]
fn undo_in_a_view_takes_back_a_clip_dropped_from_it_on_an_edit() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    open(&app, Some(vod_view(vod)));
    let clip = app.make_clip(vod, "0:20", "0:30");
    add_cut(&app, vod, 5.0);
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    assert_eq!(pieces(&app, edit).len(), 2);
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Piece added"));
    assert_eq!(pieces(&app, edit), [(0.0, 10.0)], "the drop taken back");
    assert_eq!(marks(&app, vod), [5.0], "the VOD's own change still there");
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Cut added"));
    // Redone from the VOD's view too: the drop is the newest undone there.
    assert_eq!(redo(&app, vod_view(vod)).as_deref(), Some("Cut added"));
    assert_eq!(redo(&app, vod_view(vod)).as_deref(), Some("Piece added"));
    assert_eq!(pieces(&app, edit).len(), 2);
}

/// Once the edit has changed since a clip was dropped on it from a VOD's
/// view, the drop is no longer the VOD's to undo but the edit's, after the
/// edit's newer change (history.rs, its module doc: "as long as that
/// document hasn't changed since").
#[test]
fn a_drop_on_an_edit_changed_since_is_the_edits_to_undo() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    open(&app, Some(vod_view(vod)));
    let clip = app.make_clip(vod, "0:20", "0:30");
    add_cut(&app, vod, 5.0);
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    open(&app, Some(edit_view(edit)));
    rename_edit(&app, edit, "Highlights");
    open(&app, Some(vod_view(vod)));
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Cut added"), "not the drop under the rename");
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Clip added"));
    assert_eq!(undo_in(&app, vod_view(vod)), Ok(Value::Null), "nor the drop after the VOD's own");
    assert_eq!(pieces(&app, edit).len(), 2, "the drop still in");
    assert_eq!(undo(&app, edit_view(edit)).as_deref(), Some("Edit renamed"));
    assert_eq!(undo(&app, edit_view(edit)).as_deref(), Some("Piece added"));
    assert_eq!(pieces(&app, edit), [(0.0, 10.0)]);
}

/// A change made after a redo is newer than what was redone: undo in the
/// VOD's view takes back the new cut before the drop redone just before it
/// (history.rs, its module doc: undo "puts back the newest change to its
/// document, or one made from it to another document").
#[test]
fn a_change_made_after_a_redo_is_undone_before_what_was_redone() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    open(&app, Some(vod_view(vod)));
    let clip = app.make_clip(vod, "0:20", "0:30");
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Piece added"));
    assert_eq!(redo(&app, vod_view(vod)).as_deref(), Some("Piece added"));
    add_cut(&app, vod, 5.0);
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Cut added"), "the cut, made last");
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Piece added"));
}

/// With no view open, a change has no origin: it's its document's alone, and
/// undo in a VOD's view doesn't take it back (history.rs `set_view`: "The
/// view open now (an edit, a VOD, or neither), the origin of changes made").
#[test]
fn a_change_made_with_no_view_open_is_its_documents_alone() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    let clip = app.make_clip(vod, "0:20", "0:30");
    open(&app, Some(vod_view(vod)));
    open(&app, None);
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Clip added"), "the VOD's own, not the drop");
    assert_eq!(pieces(&app, edit).len(), 2);
    assert_eq!(undo(&app, edit_view(edit)).as_deref(), Some("Piece added"));
}

/// A new change takes away what could be redone (history.rs
/// `History::record`: "Takes back what could be redone of it.").
#[test]
fn a_new_change_takes_away_what_could_be_redone() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    add_cut(&app, vod, 5.0);
    undo(&app, vod_view(vod));
    add_cut(&app, vod, 9.0);
    assert_eq!(redo_in(&app, vod_view(vod)), Ok(Value::Null));
    assert_eq!(marks(&app, vod), [9.0]);
}

/// With nothing to undo or redo, each answers nothing and changes nothing,
/// so the page can say so (history.rs `undo`: "what it was, if there was
/// one"; 0.7.0 Fixes: "Undo in the message offers Redo": it "says when
/// there's nothing left to undo").
#[test]
fn with_nothing_to_undo_or_redo_each_answers_nothing() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    let was = get_edit(&app, edit);
    assert_eq!(undo_in(&app, edit_view(edit)), Ok(Value::Null));
    assert_eq!(redo_in(&app, edit_view(edit)), Ok(Value::Null));
    assert_eq!(undo_in(&app, vod_view(vod)), Ok(Value::Null));
    assert_eq!(get_edit(&app, edit), was, "the edit as it was");
    assert!(app.heard("restored").is_empty(), "nothing put back");
}

/// Undo and redo answer what they did: the document, the change as people
/// would say it, and the revision they left it at, a new one, as every
/// change to a document is (history.rs, its module doc: "undo and redo say
/// which they leave"; `Done`).
#[test]
fn undo_and_redo_answer_the_document_the_change_and_the_new_revision() {
    let app = TestApp::new();
    let (_, edit) = vod_and_edit(&app);
    rename_edit(&app, edit, "Highlights");
    let changed = get_edit(&app, edit)["edit"]["revision"].as_i64().unwrap();
    let undone = undo_in(&app, edit_view(edit)).unwrap();
    let now = get_edit(&app, edit)["edit"]["revision"].as_i64().unwrap();
    assert!(now > changed, "undone at revision {now}, not past {changed}");
    assert_eq!(undone, json!({ "doc": edit_view(edit), "label": "Edit renamed", "revision": now }));
    let redone = redo_in(&app, edit_view(edit)).unwrap();
    let again = get_edit(&app, edit)["edit"]["revision"].as_i64().unwrap();
    assert!(again > now, "redone at revision {again}, not past {now}");
    assert_eq!(redone, json!({ "doc": edit_view(edit), "label": "Edit renamed", "revision": again }));
}

/// The page hears which document undo or redo put back, and its revision
/// now, so what shows it loads it again (history.rs `step`: "tells the page
/// which document changed (`restored`)"; api.ts `restored`).
#[test]
fn the_page_hears_which_document_undo_and_redo_put_back() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    add_cut(&app, vod, 5.0);
    let undone = undo_in(&app, vod_view(vod)).unwrap();
    rename_edit(&app, edit, "Highlights");
    let renamed = undo_in(&app, edit_view(edit)).unwrap();
    let redone = redo_in(&app, edit_view(edit)).unwrap();
    let said =
        |done: &Value| json!({ "kind": done["doc"]["kind"], "id": done["doc"]["id"], "revision": done["revision"] });
    assert_eq!(app.heard("restored"), [said(&undone), said(&renamed), said(&redone)]);
    assert_eq!(said(&undone), json!({ "kind": "vod", "id": vod, "revision": undone["revision"] }));
}

/// An undo that can't be put back (its clip deleted since) is refused, and
/// kept: once the clip is back, the same undo goes through (history.rs
/// `step`: "Kept, to try again: it's the newest of its stack still").
#[test]
fn an_undo_that_cant_be_put_back_is_refused_and_kept_to_try_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = app.make_clip(vod, "0:10", "0:20");
    // Opened in the editor, a stretch of it taken out: a step in its own history.
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 2.0, "to": 3.0 })).unwrap();
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(undo_in(&app, edit_view(clip.id)).is_err(), "nothing to put it back on");
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Clip deleted"));
    app.made(clip.id);
    assert_eq!(undo(&app, edit_view(clip.id)).as_deref(), Some("Stretch removed"), "kept, and tried again");
    assert_eq!(pieces(&app, clip.id), [(10.0, 20.0)], "the clip whole again");
    app.made(clip.id);
}

/// The most changes undo keeps for a document is 200: the oldest beyond
/// that go (history.rs `KEEP`: "The most changes kept for each document").
#[test]
fn undo_keeps_the_newest_200_changes_of_a_document() {
    let app = TestApp::new();
    let (_, edit) = vod_and_edit(&app);
    for n in 1..=201 {
        rename_edit(&app, edit, &format!("Take {n}"));
    }
    for n in (1..=200).rev() {
        assert_eq!(undo(&app, edit_view(edit)).as_deref(), Some("Edit renamed"));
        assert_eq!(edit_title(&app, edit), format!("Take {n}"));
    }
    assert_eq!(undo_in(&app, edit_view(edit)), Ok(Value::Null), "the first rename's step gone");
    assert_eq!(edit_title(&app, edit), "Take 1");
}

/// A deleted edit isn't undone: its history goes with it, the clips dropped
/// on it from a VOD's view too (history.rs, its module doc: "deleting a VOD
/// or an edit aren't undone"; `forget`: "Forgets `doc`'s history (it's
/// deleted)"). A removed VOD's is vods/tests/removing.rs
/// `a_removed_vods_undo_history_goes_with_it`.
#[test]
fn a_deleted_edits_history_goes_with_it() {
    let app = TestApp::new();
    let (vod, edit) = vod_and_edit(&app);
    open(&app, Some(vod_view(vod)));
    let clip = app.make_clip(vod, "0:20", "0:30");
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    rename_edit(&app, edit, "Highlights");
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    assert_eq!(undo_in(&app, edit_view(edit)), Ok(Value::Null));
    assert_eq!(undo(&app, vod_view(vod)).as_deref(), Some("Clip added"), "the VOD's own, not the drop");
    assert!(app.invoke("get_edit", json!({ "id": edit })).is_err(), "still deleted");
}

/// A render isn't a change undo takes back: undo after it takes back the
/// change before it (history.rs, its module doc: "Renders, downloads and
/// deleting a VOD or an edit aren't undone").
#[test]
fn a_render_is_no_step_to_undo() {
    let app = TestApp::new();
    let (_, edit) = vod_and_edit(&app);
    rename_edit(&app, edit, "Highlights");
    let steps = app.state().history.steps(Doc::Edit(edit));
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let now = crate::edits::made_now(app.handle());
    vodscribe_core::testkit::eventually("the edit rendered", || {
        app.db().edit(edit, now).unwrap().is_some_and(|e| e.status == ClipStatus::Ready)
    });
    assert_eq!(app.state().history.steps(Doc::Edit(edit)), steps, "no step for the render");
    assert_eq!(undo(&app, edit_view(edit)).as_deref(), Some("Edit renamed"));
    assert_eq!(edit_title(&app, edit), "Best of");
}

/// Undo in a clip opened from the clip bar puts back what was done to
/// that clip, not to one deleted before it: the new clip's id is one
/// never given before (core db.rs `clip_ids_are_never_reused`), so the
/// deleted clip's steps, kept under its id, aren't the new one's (0.7.3
/// Fixes: "Undo no longer changes another clip.").
#[test]
fn undo_in_a_new_clip_never_brings_back_a_deleted_one() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let old = app.make_clip(vod.id, "0:10", "0:20");
    // Opened in the editor, a stretch of it taken out: a step to undo.
    app.invoke("remove_stretch", json!({ "id": old.id, "from": 2.0, "to": 3.0 })).unwrap();
    app.invoke("delete_clip", json!({ "id": old.id })).unwrap();
    let new = app.make_clip(vod.id, "0:30", "0:40");
    let undone = app.invoke("undo", json!({ "view": { "kind": "edit", "id": new.id } })).unwrap();
    assert_eq!(undone, Value::Null, "nothing to undo in the new clip");
    let clip = app.clip(new.id).unwrap();
    assert_eq!((clip.start, clip.end), (30.0, 40.0), "the new clip as it was made");
    assert_ne!(new.id, old.id, "the deleted clip's id isn't given out again");
}

/// Undo of a delete brings the clip back, made anew under its own id, and
/// an undo past it of a change to that clip changes the clip brought
/// back, rather than making another beside it (the clip e2e scenario:
/// resized, deleted, then both undone; 0.7.3 Fixes: "Undo past a delete
/// keeps one clip.").
#[test]
fn undo_past_a_delete_changes_the_clip_it_brought_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:01", "0:03");
    app.invoke("set_clip_range", json!({ "id": clip.id, "start": 1.0, "end": 4.0 })).unwrap();
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    let view = json!({ "view": { "kind": "vod", "id": vod.id } });
    app.invoke("undo", view.clone()).unwrap();
    let on = || -> Vec<_> { app.clips(vod.id).into_iter().map(|c| (c.start, c.end)).collect() };
    assert_eq!(on(), [(1.0, 4.0)], "the delete undone");
    app.invoke("undo", view.clone()).unwrap();
    assert_eq!(on(), [(1.0, 3.0)], "the resize undone, on the clip brought back");
    // Redone twice: resized again, then deleted.
    app.invoke("redo", view.clone()).unwrap();
    assert_eq!(on(), [(1.0, 4.0)], "the resize redone");
    app.invoke("redo", view).unwrap();
    assert_eq!(on(), [], "the delete redone");
    // Its files made or stopped before the test ends.
    for c in app.clips(vod.id) {
        app.made(c.id);
    }
}

/// As above with a clip that isn't the newest, which an undo of its
/// delete made anew under a new id in 0.7.2 already: undone past the
/// delete, the resize changes the clip brought back, and the other
/// clip stays as it is (0.7.3 Fixes: "Undo past a delete keeps one clip.").
#[test]
fn undo_past_the_delete_of_an_older_clip_changes_the_clip_it_brought_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let older = app.make_clip(vod.id, "0:01", "0:03");
    app.make_clip(vod.id, "0:10", "0:12");
    app.invoke("set_clip_range", json!({ "id": older.id, "start": 1.0, "end": 4.0 })).unwrap();
    app.invoke("delete_clip", json!({ "id": older.id })).unwrap();
    let view = json!({ "view": { "kind": "vod", "id": vod.id } });
    app.invoke("undo", view.clone()).unwrap();
    app.invoke("undo", view).unwrap();
    let mut on: Vec<_> = app.clips(vod.id).into_iter().map(|c| (c.start, c.end)).collect();
    on.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(on, [(1.0, 3.0), (10.0, 12.0)], "the resize undone, on the clip brought back");
    // Their files made before the test ends.
    for c in app.clips(vod.id) {
        app.made(c.id);
    }
}

fn library() -> (Db, i64) {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let info = vodscribe_core::fetch::VodInfo {
        platform: "youtube".into(),
        platform_id: "x".into(),
        url: "https://youtu.be/x".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let vod = db.add_vod(&info).unwrap().0.id;
    (db, vod)
}

/// A change that fails part-way leaves the library as it was and records
/// nothing; one that changes nothing records no step (history.rs
/// `History::change`: "One that fails part-way leaves the library as it was
/// and records nothing"; `History::record`: "nothing if it didn't change").
/// At `History::change`, as no command fails part-way on purpose: two clips
/// added at once went in one at a time, and one that failed after the first
/// left that one in the library, with no step to undo it.
#[test]
fn a_change_that_fails_part_way_leaves_nothing() {
    let (db, vod) = library();
    let history = History::default();
    let doc = Doc::Vod(vod);
    let failed: Result<()> = history.change(&db, doc, "Clips added", |db| {
        db.add_clip(vod, 1.0, 2.0, "a", true)?;
        anyhow::bail!("the second one couldn't go in");
    });
    assert!(failed.is_err());
    assert!(db.clips(vod).unwrap().is_empty(), "the first one taken back");
    assert_eq!(history.steps(doc), 0, "no step recorded");

    let made = history.change(&db, doc, "Clips added", |db| {
        db.add_clip(vod, 1.0, 2.0, "a", true)?;
        db.add_clip(vod, 3.0, 4.0, "b", true)
    });
    assert_eq!(made.unwrap().title, "b");
    assert_eq!(db.clips(vod).unwrap().len(), 2);
    assert_eq!(history.steps(doc), 1, "one step for both");
    // Nothing changed (a clip only dragged out isn't in the bar): no step.
    history.change(&db, doc, "Clip added", |db| db.add_clip(vod, 5.0, 6.0, "c", false)).unwrap();
    assert_eq!(history.steps(doc), 1);
}

/// A paste after a stretch selected splits the piece at its end and
/// puts the copy there, and the split is part of the paste: one step,
/// whose undo is the edit as it was, its one piece whole (history.rs
/// `History::change`: "`work`'s writes, as one transaction, then recorded
/// as `label`").
#[test]
fn a_paste_after_a_selection_is_one_step_with_its_split() {
    use vodscribe_core::clipboard::Paste;
    use vodscribe_core::edits::Put;
    let (db, vod) = library();
    let history = History::default();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let doc = Doc::Edit(edit);
    let was = db.edit_state(edit).unwrap().unwrap();
    let copy = db.copy_of(&Put::Range { vod_id: vod, start: 1.0, end: 2.0, title: None }).unwrap();
    // 2-4 s selected: pasted at its end.
    let pasted = history.change(&db, doc, "Pasted", |db| db.paste(Paste::Timeline { clip: edit, at: 4.0 }, &[copy]));
    assert_eq!(pasted.unwrap().span, Some((4.0, 5.0)));
    let ranges: Vec<(f64, f64)> = db.edit_pieces(edit).unwrap().iter().map(|p| (p.start, p.end)).collect();
    assert_eq!(ranges, [(10.0, 14.0), (1.0, 2.0), (14.0, 20.0)]);
    assert_eq!(history.steps(doc), 1, "the split and the copy, one step");
    let mut s = history.0.lock().unwrap();
    let Some(Entry { before: Snapshot::Edit { state, .. }, .. }) = History::take(&mut s, doc, true) else {
        panic!("no edit step to undo")
    };
    assert_eq!(state, was, "undone, the one piece whole");
}

/// A clip by its stretch alone, for the steps' cases.
impl Made for (f64, f64) {
    fn same(&self, other: &Self) -> bool {
        (self.0 - other.0).abs() < 1e-6 && (self.1 - other.1).abs() < 1e-6
    }
}

fn bar(clips: &[(i64, f64, f64, &str)]) -> Vec<BarClip<(f64, f64)>> {
    clips.iter().map(|&(id, start, end, title)| BarClip { id, made: (start, end), title: title.into() }).collect()
}

/// Undo puts back only what its change did to the clip bar: clips the
/// change didn't touch stay as they are now, whatever's become of them
/// (history.rs `restore`: "what the change didn't touch stays as it is now,
/// so a clip that went meanwhile without a change of its own (its file
/// deleted outside the app) stays gone"; `bar_steps`; 0.5.0 Fixes: "The clip
/// going is now its own step").
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
    assert_eq!(bar_steps(&resized, &both, &now), [BarStep::Remake(1, &both[0])]);
    // Clip 1 only renamed: undone, renamed back, its file as it is.
    let renamed = bar(&[(1, 1.0, 3.0, "c"), (2, 5.0, 6.0, "b")]);
    assert_eq!(bar_steps(&renamed, &both, &renamed), [BarStep::Rename(1, "a")]);
    // Clip 3 added: undone, it goes, if it's still there.
    let added = bar(&[(1, 1.0, 3.0, "a"), (3, 8.0, 9.0, "d")]);
    assert_eq!(bar_steps(&added, &with, &added), [BarStep::Remove(3)]);
    assert_eq!(bar_steps(&added, &with, &with), []);
}

/// A second clip of the same stretch and title on one bar (a pill
/// pasted beside itself): undone, the copy goes and the first stays;
/// redone, a copy is made again. Matched by what it's made of alone,
/// the copy was taken for the first and neither step happened (history.rs
/// `find`: "two clips of one stretch, a pill pasted beside itself, are two").
#[test]
fn a_copy_beside_its_original_is_undone_and_redone_on_its_own() {
    let one = bar(&[(1, 1.0, 2.0, "a")]);
    let two = bar(&[(1, 1.0, 2.0, "a"), (2, 1.0, 2.0, "a")]);
    assert_eq!(bar_steps(&two, &one, &two), [BarStep::Remove(2)]);
    assert_eq!(bar_steps(&one, &two, &one), [BarStep::Add(&two[1])]);
    // Brought back under another id (its own taken): undone again, that one goes.
    let again = bar(&[(1, 1.0, 2.0, "a"), (5, 1.0, 2.0, "a")]);
    assert_eq!(bar_steps(&two, &one, &again), [BarStep::Remove(5)]);
}

/// The edit's clip bar had an undo of its own, which made a clip's file
/// again on any change, a rename too; the VOD's renamed it back alone
/// (history.rs `bar_steps`: "A clip only renamed is renamed back, its file
/// left as it is").
#[test]
fn undo_on_an_edits_clip_bar_goes_by_the_same_steps() {
    let (db, vod) = library();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
    let made = db.add_edit_clip(edit, 1.0, 5.0, "one", true).unwrap().id;
    let one = edit_bar(&db, edit).unwrap();

    // Renamed: undone, renamed back, its file as it is.
    db.rename_clip(made, "two").unwrap();
    let renamed = edit_bar(&db, edit).unwrap();
    assert_eq!(bar_steps(&renamed, &one, &renamed), [BarStep::Rename(made, "one")]);
    // What it plays changed: undone, made of what it was, and made again.
    let piece = db.piece_rows(made).unwrap()[0].id;
    db.set_piece_range(piece, 2.0, 5.0).unwrap();
    let trimmed = edit_bar(&db, edit).unwrap();
    assert_eq!(bar_steps(&trimmed, &renamed, &trimmed), [BarStep::Remake(made, &renamed[0])]);
    // Deleted: undone, made anew; redone, the one made anew goes.
    db.delete_clip(made).unwrap();
    assert!(edit_bar(&db, edit).unwrap().is_empty());
    assert_eq!(bar_steps(&[], &trimmed, &[]), [BarStep::Add(&trimmed[0])]);
    assert_eq!(bar_steps(&trimmed, &[], &trimmed), [BarStep::Remove(made)]);
}

/// The library alone, for `restore`: no files to make or delete.
struct NoQueue;

impl BarQueue for NoQueue {
    fn remade(&self, _: &Db, _: i64) {}
    fn added(&self, _: &Db, _: i64) {}
    fn remove(&self, db: &Db, id: i64) -> Result<()> {
        db.delete_clip(id)
    }
}

/// Steps `doc`'s history back (`undo`) or forward in the library, as
/// `step` does.
fn step_library(history: &History, db: &Db, doc: Doc, undo: bool) {
    let mut s = history.0.lock().unwrap();
    let entry = History::take(&mut s, doc, undo).expect("a step to take");
    let (from, to) = if undo { (&entry.after, &entry.before) } else { (&entry.before, &entry.after) };
    restore(&NoQueue, db, ClipEncoding::default(), doc, from, to).unwrap();
    let other = if undo { &mut s.redo } else { &mut s.undo };
    other.entry(doc).or_default().push(entry);
}

/// VOD `vod`'s clip bar, each clip as it is.
fn vod_bar_states(db: &Db, vod: i64) -> Vec<EditState> {
    db.clips(vod).unwrap().iter().map(|c| db.edit_state(c.id).unwrap().unwrap()).collect()
}

/// A clip of VOD `vod`'s clip bar changed in the editor (a second clip
/// dropped in); its id and what it holds.
fn changed_vod_clip(db: &Db, vod: i64) -> (i64, EditState) {
    let clip = db.add_clip(vod, 1.0, 3.0, "a", true).unwrap().id;
    db.add_piece(clip, vod, (10.0, 12.0), "b", None).unwrap();
    (clip, db.edit_state(clip).unwrap().unwrap())
}

/// A clip of a VOD's clip bar changed in the editor, deleted: undone, it
/// comes back whole (0.7.1 Fixes: "Undo brings a clip back whole. Editor
/// changes to a clip on a VOD's clip bar, a line struck or a clip dropped
/// in, survive Delete then Undo"; it used to come back as its first piece's
/// stretch alone, what was dropped in lost).
#[test]
fn undo_of_a_delete_brings_a_vod_bar_clip_back_whole() {
    let (db, vod) = library();
    let history = History::default();
    let doc = Doc::Vod(vod);
    let (clip, was) = changed_vod_clip(&db, vod);
    history.change(&db, doc, "Clip deleted", |db| db.delete_clip(clip)).unwrap();
    assert!(vod_bar_states(&db, vod).is_empty());
    step_library(&history, &db, doc, true);
    assert_eq!(vod_bar_states(&db, vod), [was], "undone, the clip as it was");
}

/// The same clip copied and pasted beside itself: redone after an undo,
/// the paste is whole (0.7.1 Fixes: "Undo brings a clip back whole": they
/// survive "a paste's Undo then Redo"; it used to be its first piece's
/// stretch alone).
#[test]
fn redo_of_a_paste_brings_a_vod_bar_clip_back_whole() {
    use vodscribe_core::clipboard::Paste;
    use vodscribe_core::edits::Put;
    let (db, vod) = library();
    let history = History::default();
    let doc = Doc::Vod(vod);
    let (clip, was) = changed_vod_clip(&db, vod);
    let on_bar = |db: &Db| vod_bar_states(db, vod);
    let copy = db.copy_of(&Put::Clip { id: clip }).unwrap();
    let bar = db.vod_clip(vod).unwrap().unwrap();
    history.change(&db, doc, "Clip pasted", |db| db.paste(Paste::Bar { bar, after: None }, &[copy])).unwrap();
    assert_eq!(on_bar(&db), [was.clone(), was.clone()]);
    step_library(&history, &db, doc, true);
    assert_eq!(on_bar(&db), std::slice::from_ref(&was), "the paste undone");
    step_library(&history, &db, doc, false);
    assert_eq!(on_bar(&db), [was.clone(), was], "redone, the copy as it was");
}

/// A clip pasted between two on a VOD's bar, then the middle one
/// deleted: undone or redone, it comes back where it was, not last (0.7.2
/// Fixes: "Undo keeps the clip bar's order.").
#[test]
fn undo_and_redo_keep_the_bars_order() {
    use vodscribe_core::clipboard::Paste;
    use vodscribe_core::edits::Put;
    let (db, vod) = library();
    let history = History::default();
    let doc = Doc::Vod(vod);
    let bar = db.vod_clip(vod).unwrap().unwrap();
    let a = db.add_clip(vod, 1.0, 2.0, "a", true).unwrap().id;
    db.add_clip(vod, 3.0, 4.0, "b", true).unwrap();
    let starts = |db: &Db| db.clips(vod).unwrap().iter().map(|c| c.start).collect::<Vec<_>>();
    let copy = db.copy_of(&Put::Range { vod_id: vod, start: 5.0, end: 6.0, title: None }).unwrap();
    history.change(&db, doc, "Clip pasted", |db| db.paste(Paste::Bar { bar, after: Some(a) }, &[copy])).unwrap();
    assert_eq!(starts(&db), [1.0, 5.0, 3.0], "pasted after the first");
    step_library(&history, &db, doc, true);
    assert_eq!(starts(&db), [1.0, 3.0]);
    step_library(&history, &db, doc, false);
    assert_eq!(starts(&db), [1.0, 5.0, 3.0], "redone, in the middle again");

    // The middle one deleted: undone, it's back in the middle.
    let middle = db.clips(vod).unwrap()[1].id;
    history.change(&db, doc, "Clip deleted", |db| db.delete_clip(middle)).unwrap();
    assert_eq!(starts(&db), [1.0, 3.0]);
    step_library(&history, &db, doc, true);
    assert_eq!(starts(&db), [1.0, 5.0, 3.0], "undone, in the middle again");
}

/// Clip `clip` of `doc`'s clip bar, made of `made`, resized and its
/// file made of that: undone, it's back to `made` and waits to be made
/// again. The cutter makes only a clip that isn't ready (`clips::cut`),
/// so one left ready would keep the resized file.
fn undone_resize_is_made_again(db: &Db, doc: Doc, clip: i64, made: (f64, f64)) {
    let history = History::default();
    let ready = |(start, end): (f64, f64)| {
        let file = db.files().clip_file(clip, "mp4").to_string_lossy().into_owned();
        assert!(db.set_clip_file(clip, start, end, ClipStatus::Ready, Some(&file), None).unwrap());
        db.set_made_now(clip).unwrap();
    };
    ready(made);
    let resized = (made.0, made.0 + 1.0);
    history.change(db, doc, "Clip resized", |db| db.set_clip_range(clip, resized.0, resized.1)).unwrap();
    ready(resized);
    step_library(&history, db, doc, true);
    let now = db.clip(clip).unwrap().unwrap();
    assert_eq!((now.start, now.end), made, "back as it was");
    assert_eq!(now.status, ClipStatus::Waiting, "its file, of the resize, to be made again");
    // Redone, the same.
    step_library(&history, db, doc, false);
    let now = db.clip(clip).unwrap().unwrap();
    assert_eq!((now.start, now.end), resized, "resized again");
    assert_eq!(now.status, ClipStatus::Waiting, "its file, of the undo, to be made again");
}

/// A resize undone on a VOD's clip bar waits to be made again (history.rs
/// `remake_clip`: "waiting to be made again unless its file is of that
/// already").
#[test]
fn a_resize_undone_on_a_vod_bar_is_made_again() {
    let (db, vod) = library();
    let clip = db.add_clip(vod, 1.0, 3.0, "a", true).unwrap().id;
    undone_resize_is_made_again(&db, Doc::Vod(vod), clip, (1.0, 3.0));
}

/// A resize undone on an edit's clip bar waits to be made again (0.7.1
/// Fixes: "Undoing a resize makes the clip again. On an edit's clip bar,
/// Undo or Redo of a clip's new length left its video as it was.").
#[test]
fn a_resize_undone_on_an_edits_bar_is_made_again() {
    let (db, vod) = library();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
    let clip = db.add_edit_clip(edit, 5.0, 9.0, "one", true).unwrap().id;
    undone_resize_is_made_again(&db, Doc::Edit(edit), clip, (5.0, 9.0));
}
