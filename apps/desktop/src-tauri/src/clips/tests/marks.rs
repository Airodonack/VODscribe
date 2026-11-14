use super::*;

use crate::testkit::TestApp;
use serde_json::{Value, json};

/// A mark as the page hears it: its kind, moment and text.
type Seen = (String, f64, Option<String>);

fn seen(mark: &Value) -> Seen {
    (
        mark["kind"].as_str().unwrap().to_string(),
        mark["start"].as_f64().unwrap(),
        mark["text"].as_str().map(String::from),
    )
}

fn vod_doc(vod: i64) -> Value {
    json!({ "kind": "vod", "id": vod })
}

fn edit_doc(edit: i64) -> Value {
    json!({ "kind": "edit", "id": edit })
}

impl TestApp {
    /// `add_mark` as the page asks it: a mark of `kind` (a cut if None) at
    /// `time` seconds of VOD `vod`.
    fn add_mark(&self, vod: i64, time: f64, kind: Option<&str>, text: Option<&str>) -> Result<Value, Value> {
        self.invoke("add_mark", json!({ "vodId": vod, "time": time, "kind": kind, "text": text }))
    }

    /// VOD `vod`'s marks as `list_marks` lists them.
    fn marks(&self, vod: i64) -> Vec<Seen> {
        let marks = self.invoke("list_marks", json!({ "vodId": vod })).unwrap();
        marks.as_array().unwrap().iter().map(seen).collect()
    }

    /// Edit `edit`'s marks as `edit_marks` lists them, on its timeline.
    fn edit_marks(&self, edit: i64) -> Vec<Seen> {
        let marks = self.invoke("edit_marks", json!({ "id": edit })).unwrap();
        marks.as_array().unwrap().iter().map(seen).collect()
    }

    /// Undo in `doc`'s history: the label of what it took back, if anything.
    fn undo(&self, doc: Value) -> Option<String> {
        let done = self.invoke("undo", json!({ "view": doc })).unwrap();
        done["label"].as_str().map(String::from)
    }

    /// Redo in `doc`'s history: the label of what it brought back, if anything.
    fn redo(&self, doc: Value) -> Option<String> {
        let done = self.invoke("redo", json!({ "view": doc })).unwrap();
        done["label"].as_str().map(String::from)
    }

    /// How many changes undo can take back in `doc`.
    fn steps(&self, doc: history::Doc) -> usize {
        self.state().history.steps(doc)
    }
}

fn mark_id(mark: &Value) -> i64 {
    mark["id"].as_i64().unwrap()
}

fn is(error: Result<Value, Value>, words: &str) {
    assert_eq!(error.unwrap_err(), json!(words));
}

/// A mark is a cut unless it's said to be a chapter or highlight (app clips.rs
/// `add_mark`: "a cut (between two frames) unless `kind` says a chapter or a
/// highlight").
#[test]
fn a_mark_is_a_cut_unless_it_is_said_to_be_a_chapter_or_highlight() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.add_mark(vod, 5.0, None, None).unwrap();
    app.add_mark(vod, 6.0, Some("chapter"), Some("Start")).unwrap();
    app.add_mark(vod, 7.0, Some("highlight"), None).unwrap();
    app.add_mark(vod, 8.0, Some("cut"), None).unwrap();
    let kinds: Vec<String> = app.marks(vod).into_iter().map(|m| m.0).collect();
    assert_eq!(kinds, ["cut", "chapter", "highlight", "cut"]);
}

/// A mark of another kind is refused: "That isn't a kind of mark." (app
/// clips.rs `add_mark` and `add_edit_mark`), in a VOD and in an edit.
#[test]
fn a_mark_of_another_kind_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 30.0)]);
    is(app.add_mark(vod, 5.0, Some("bookmark"), None), "That isn't a kind of mark.");
    is(
        app.invoke("add_edit_mark", json!({ "editId": edit, "time": 5.0, "kind": "bookmark" })),
        "That isn't a kind of mark.",
    );
    assert!(app.marks(vod).is_empty());
    assert!(app.edit_marks(edit).is_empty());
}

/// A mark before the start, or at no real time, is refused: "A mark has to be
/// within the video." (core clips.rs `MARK_OUTSIDE`, from `check_mark`).
#[test]
fn a_mark_before_the_start_or_at_no_real_time_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let handle = || app.handle().clone();
    let within = "A mark has to be within the video.";
    is(app.add_mark(vod, -1.0, None, None), within);
    for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let added = app.run(super::super::add_mark(handle(), app.state(), vod, time, None, None));
        assert_eq!(added.unwrap_err(), within, "{time}");
    }
    assert!(app.marks(vod).is_empty());
    // The very start is in the video.
    app.add_mark(vod, 0.0, None, None).unwrap();
    assert_eq!(app.marks(vod).len(), 1);
}

/// A mark past the video's end is refused too, the same words: "A mark has to
/// be within the video." (crates/core/src/clips.rs `check_mark`). Marks past
/// the end were a bug. A file's and a site's VOD alike, added or moved there.
#[test]
fn a_mark_past_the_video_s_end_is_refused() {
    let app = TestApp::new();
    let file = app.add_video_file("Day 3.mp4", 60.0).id;
    let site = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0).id;
    for vod in [file, site] {
        for time in [9999.0, 60.001] {
            is(app.add_mark(vod, time, Some("chapter"), None), "A mark has to be within the video.");
        }
        let early = app.add_mark(vod, 10.0, None, None).unwrap();
        is(
            app.invoke("move_mark", json!({ "id": mark_id(&early), "time": 9999.0 })).map(|_| Value::Null),
            "A mark has to be within the video.",
        );
        assert_eq!(app.marks(vod), [("cut".to_string(), 10.0, None)], "where it was");
        assert_eq!(app.steps(history::Doc::Vod(vod)), 1, "only the mark at 10 s");
    }
}

/// A mark at the video's very end is in it: the timeline reaches it, and a
/// mark nudged a frame on stops there (media.ts `nudge`).
#[test]
fn a_mark_at_the_video_s_very_end_is_taken() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.add_mark(vod, 60.0, Some("highlight"), None).unwrap();
    let cut = app.add_mark(vod, 10.0, None, None).unwrap();
    app.invoke("move_mark", json!({ "id": mark_id(&cut), "time": 60.0 })).unwrap();
    assert_eq!(app.marks(vod).iter().map(|m| m.1).collect::<Vec<_>>(), [60.0, 60.0]);
}

/// A VOD whose length isn't known (a site that didn't say, or gave 0) has no
/// end a mark can be past: the page goes by its player's length then. Its
/// start still bounds it.
#[test]
fn a_mark_on_a_vod_of_no_known_length_is_bounded_only_by_its_start() {
    let app = TestApp::new();
    for (id, duration) in [("1", None), ("2", Some(0.0))] {
        let info = vodscribe_core::fetch::VodInfo {
            platform: "example".into(),
            platform_id: id.into(),
            url: format!("https://example.example/{id}"),
            title: "No length".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration,
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let vod = app.db().add_vod(&info).unwrap().0.id;
        let mark = app.add_mark(vod, 9999.0, None, None).unwrap();
        app.invoke("move_mark", json!({ "id": mark_id(&mark), "time": 12345.0 })).unwrap();
        assert_eq!(app.marks(vod), [("cut".to_string(), 12345.0, None)], "{duration:?}");
        is(app.add_mark(vod, -1.0, None, None), "A mark has to be within the video.");
    }
}

/// A mark on a VOD that is gone says so: "That VOD is gone." (app clips.rs
/// `add_mark`).
#[test]
fn a_mark_on_a_vod_that_is_gone_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    is(app.add_mark(vod + 100, 5.0, None, None), "That VOD is gone.");
}

/// A mark's text is trimmed, at most 200 characters, and none when nothing is
/// left (app clips.rs `mark_text`; crates/core/src/clips.rs `written_title`),
/// when it is added and when it is changed.
#[test]
fn a_mark_s_text_is_trimmed_and_capped_and_none_when_empty() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let text = |app: &TestApp| app.marks(vod).into_iter().map(|m| m.2).collect::<Vec<_>>();
    app.add_mark(vod, 1.0, Some("chapter"), Some("  The landing \n")).unwrap();
    app.add_mark(vod, 2.0, Some("chapter"), Some("   ")).unwrap();
    app.add_mark(vod, 3.0, Some("chapter"), Some(&"é".repeat(250))).unwrap();
    app.add_mark(vod, 4.0, Some("chapter"), None).unwrap();
    assert_eq!(text(&app), [Some("The landing".into()), None, Some("é".repeat(200)), None]);

    let id = mark_id(&app.invoke("list_marks", json!({ "vodId": vod })).unwrap()[3]);
    app.invoke("set_mark_text", json!({ "id": id, "text": "  Later  " })).unwrap();
    assert_eq!(text(&app)[3], Some("Later".into()));
    app.invoke("set_mark_text", json!({ "id": id, "text": "  " })).unwrap();
    assert_eq!(text(&app)[3], None);
    app.invoke("set_mark_text", json!({ "id": id, "text": "x".repeat(300) })).unwrap();
    assert_eq!(text(&app)[3], Some("x".repeat(200)));
}

/// A VOD's marks are listed in time order (app clips.rs `list_marks`: "in time
/// order").
#[test]
fn a_vod_s_marks_are_listed_in_time_order() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    for time in [30.0, 10.0, 20.0] {
        app.add_mark(vod, time, None, None).unwrap();
    }
    let times: Vec<f64> = app.marks(vod).into_iter().map(|m| m.1).collect();
    assert_eq!(times, [10.0, 20.0, 30.0]);
}

/// Adding a mark is one step, named for its kind (app clips.rs `added`;
/// marks-undo.mjs: "each told in a toast").
#[test]
fn adding_a_mark_is_one_step_named_for_its_kind() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let doc = history::Doc::Vod(vod);
    for (kind, label) in
        [(None, "Cut added"), (Some("chapter"), "Chapter added"), (Some("highlight"), "Highlight added")]
    {
        let before = app.steps(doc);
        app.add_mark(vod, 5.0, kind, None).unwrap();
        assert_eq!(app.steps(doc), before + 1, "{label}");
        assert_eq!(app.undo(vod_doc(vod)).as_deref(), Some(label));
    }
    assert!(app.marks(vod).is_empty());
}

/// A moved mark is where it was moved, and undo puts it back (app clips.rs
/// `move_mark`; 0.7.1 Fixes: "Undo undoes the latest change. Undo (Ctrl+Z)
/// pressed right after a change, such as moving a mark, could undo the one
/// before it instead.").
#[test]
fn a_moved_mark_is_where_it_was_moved_and_undo_puts_it_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.add_mark(vod, 5.0, Some("highlight"), Some("a")).unwrap();
    let other = app.add_mark(vod, 40.0, Some("chapter"), Some("b")).unwrap();
    app.invoke("move_mark", json!({ "id": mark_id(&other), "time": 12.5 })).unwrap();
    let times = |app: &TestApp| app.marks(vod).into_iter().map(|m| m.1).collect::<Vec<_>>();
    assert_eq!(times(&app), [5.0, 12.5]);
    assert_eq!(app.undo(vod_doc(vod)).as_deref(), Some("Mark moved"));
    assert_eq!(times(&app), [5.0, 40.0], "the move, not the adding before it");
}

/// A mark moved in an edit goes to the moment playing there (app clips.rs, the
/// comment in `move_mark`; 0.5.0: "Edits have chapters and highlights of their
/// own ... Each stays on its moment of the video, so it moves with its piece").
#[test]
fn a_mark_moved_in_an_edit_goes_to_the_moment_playing_there() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    // Its timeline: 0:00-0:10 plays 0:00-0:10 of the VOD, 0:10-0:20 plays 0:30-0:40.
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 30.0, 40.0)]);
    let mark = app.invoke("add_edit_mark", json!({ "editId": edit, "time": 2.0, "kind": "highlight" })).unwrap();
    app.invoke("move_mark", json!({ "id": mark_id(&mark), "time": 15.0 })).unwrap();
    let moment = app.db().clip_marks(edit).unwrap()[0].1.at.seconds();
    assert_eq!(moment, 35.0, "of the VOD");
    assert_eq!(app.edit_marks(edit)[0].1, 15.0, "of the edit");
}

/// A mark moved outside its edit, before its start or past its end, is refused:
/// "A mark has to be within the edit." (app clips.rs `move_mark`).
#[test]
fn a_mark_moved_outside_its_edit_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 30.0, 40.0)]);
    let mark = app.invoke("add_edit_mark", json!({ "editId": edit, "time": 2.0, "kind": "highlight" })).unwrap();
    for time in [25.0, 20.5, -3.0] {
        let moved = app.invoke("move_mark", json!({ "id": mark_id(&mark), "time": time })).map(|_| Value::Null);
        is(moved, "A mark has to be within the edit.");
    }
    assert_eq!(app.edit_marks(edit)[0].1, 2.0, "where it was");
    // Its very end is in it.
    app.invoke("move_mark", json!({ "id": mark_id(&mark), "time": 20.0 })).unwrap();
}

/// A mark moved before the start is refused (app clips.rs `move_mark`, by core
/// `check_mark`: "A mark has to be within the video.").
#[test]
fn a_mark_moved_before_the_start_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let mark = app.add_mark(vod, 5.0, None, None).unwrap();
    let moved = app.invoke("move_mark", json!({ "id": mark_id(&mark), "time": -3.0 })).map(|_| Value::Null);
    is(moved, "A mark has to be within the video.");
    assert_eq!(app.marks(vod)[0].1, 5.0);
}

/// A mark that is gone, moved or retitled, says so: "That mark is gone." (app
/// clips.rs `marking`).
#[test]
fn a_mark_that_is_gone_moved_or_retitled_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let mark = mark_id(&app.add_mark(vod, 5.0, Some("chapter"), None).unwrap());
    app.invoke("delete_mark", json!({ "id": mark })).unwrap();
    let moved = app.invoke("move_mark", json!({ "id": mark, "time": 7.0 })).map(|_| Value::Null);
    is(moved, "That mark is gone.");
    let retitled = app.invoke("set_mark_text", json!({ "id": mark, "text": "x" })).map(|_| Value::Null);
    is(retitled, "That mark is gone.");
}

/// A deleted mark comes back with undo and goes again with redo
/// (marks-undo.mjs: "the chapter deleted (Delete), brought back with Ctrl+Z,
/// and deleted again with Ctrl+Shift+Z, each told in a toast").
#[test]
fn a_deleted_mark_comes_back_with_undo_and_goes_again_with_redo() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let mark = mark_id(&app.add_mark(vod, 5.0, Some("chapter"), Some("The landing")).unwrap());
    app.invoke("delete_mark", json!({ "id": mark })).unwrap();
    assert!(app.marks(vod).is_empty());
    assert_eq!(app.undo(vod_doc(vod)).as_deref(), Some("Mark removed"));
    assert_eq!(app.marks(vod), [("chapter".to_string(), 5.0, Some("The landing".to_string()))]);
    assert_eq!(app.redo(vod_doc(vod)).as_deref(), Some("Mark removed"));
    assert!(app.marks(vod).is_empty());
}

/// Deleting a mark already gone is no error (app clips.rs `delete_mark`).
#[test]
fn deleting_a_mark_already_gone_is_no_error() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let mark = mark_id(&app.add_mark(vod, 5.0, None, None).unwrap());
    app.invoke("delete_mark", json!({ "id": mark })).unwrap();
    let steps = app.steps(history::Doc::Vod(vod));
    app.invoke("delete_mark", json!({ "id": mark })).unwrap();
    app.invoke("delete_mark", json!({ "id": mark + 100 })).unwrap();
    assert_eq!(app.steps(history::Doc::Vod(vod)), steps, "no step for nothing");
}

/// A mark canceled just after it was added leaves no step: it is gone and not
/// in undo's history, so undo undoes the change before it (app clips.rs
/// `cancel_mark`; marks-undo.mjs: "gone, and not in undo's history (Ctrl+Z
/// undoes the change before it)"; 0.4.0: "Pressed by mistake, Escape takes it
/// back.").
#[test]
fn a_mark_canceled_just_after_it_was_added_leaves_no_step() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let doc = history::Doc::Vod(vod);
    app.add_mark(vod, 5.0, Some("highlight"), None).unwrap();
    let before = app.steps(doc);
    let chapter = mark_id(&app.add_mark(vod, 9.0, Some("chapter"), None).unwrap());
    app.invoke("cancel_mark", json!({ "id": chapter })).unwrap();
    assert_eq!(app.marks(vod).len(), 1, "gone");
    assert_eq!(app.steps(doc), before, "no step");
    assert_eq!(app.undo(vod_doc(vod)).as_deref(), Some("Highlight added"), "the change before it");
}

/// What undo took back before a canceled mark can still be redone
/// (marks-undo.mjs: "what that undid can still be redone"; 0.4.1 Fixes: "A
/// chapter or highlight taken back with Escape left nothing to redo.").
#[test]
fn what_undo_took_back_before_a_canceled_mark_can_still_be_redone() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.add_mark(vod, 5.0, Some("highlight"), Some("kept")).unwrap();
    assert_eq!(app.undo(vod_doc(vod)).as_deref(), Some("Highlight added"));
    let chapter = mark_id(&app.add_mark(vod, 9.0, Some("chapter"), None).unwrap());
    app.invoke("cancel_mark", json!({ "id": chapter })).unwrap();
    assert!(app.marks(vod).is_empty());
    assert_eq!(app.redo(vod_doc(vod)).as_deref(), Some("Highlight added"));
    assert_eq!(app.marks(vod), [("highlight".to_string(), 5.0, Some("kept".to_string()))]);
}

/// A mark canceled after other changes is an ordinary removal (app clips.rs
/// `cancel_mark`: "With anything changed since, it's an ordinary removal"): one
/// step of its own, which undo brings back.
#[test]
fn a_mark_canceled_after_other_changes_is_an_ordinary_removal() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let doc = history::Doc::Vod(vod);
    let first = mark_id(&app.add_mark(vod, 5.0, Some("highlight"), None).unwrap());
    let chapter = mark_id(&app.add_mark(vod, 9.0, Some("chapter"), None).unwrap());
    app.invoke("move_mark", json!({ "id": first, "time": 6.0 })).unwrap();
    let before = app.steps(doc);
    app.invoke("cancel_mark", json!({ "id": chapter })).unwrap();
    assert_eq!(app.marks(vod).len(), 1);
    assert_eq!(app.steps(doc), before + 1, "a step of its own");
    assert_eq!(app.undo(vod_doc(vod)).as_deref(), Some("Mark removed"));
    assert_eq!(app.marks(vod).len(), 2, "brought back");
}

/// An edit's mark goes on the moment of the VOD playing there (app clips.rs
/// `add_edit_mark`; 0.5.0: "Each stays on its moment of the video, so it moves
/// with its piece").
#[test]
fn an_edit_s_mark_goes_on_the_moment_of_the_vod_playing_there() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 30.0, 40.0)]);
    let mark = app
        .invoke("add_edit_mark", json!({ "editId": edit, "time": 12.0, "kind": "chapter", "text": " Back " }))
        .unwrap();
    assert_eq!(
        (mark["vod_id"].as_i64(), mark["start"].as_f64(), mark["text"].as_str()),
        (Some(vod), Some(12.0), Some("Back"))
    );
    let (_, at) = &app.db().clip_marks(edit).unwrap()[0];
    assert_eq!(at.at.seconds(), 32.0, "of the VOD");
    assert_eq!(app.steps(history::Doc::Edit(edit)), 1);
    assert_eq!(app.undo(edit_doc(edit)).as_deref(), Some("Chapter added"));
    assert!(app.edit_marks(edit).is_empty());
}

/// An edit's mark outside the edit is refused: "A mark has to be within the
/// edit." (app clips.rs `add_edit_mark`).
#[test]
fn an_edit_s_mark_outside_it_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 30.0, 40.0)]);
    for time in [25.0, 20.5, -1.0] {
        is(
            app.invoke("add_edit_mark", json!({ "editId": edit, "time": time, "kind": "cut" })),
            "A mark has to be within the edit.",
        );
    }
    // Its very end is in it.
    app.invoke("add_edit_mark", json!({ "editId": edit, "time": 20.0, "kind": "cut" })).unwrap();
    assert!(app.edit_marks(edit).len() == 1);
}

/// An edit's marks show where their moments play (app clips.rs `edit_marks`:
/// "each where its moment plays"; 0.5.0: "Each stays on its moment of the
/// video, so it moves with its piece"): one in a stretch taken out of the edit
/// is not shown.
#[test]
fn an_edit_s_marks_show_where_their_moments_play() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    // The VOD's own marks come with the piece of the edit that holds their moments.
    app.add_mark(vod, 25.0, Some("highlight"), Some("before")).unwrap();
    app.add_mark(vod, 35.0, Some("highlight"), Some("taken out")).unwrap();
    app.add_mark(vod, 45.0, Some("highlight"), Some("after")).unwrap();
    let edit = app.edit_of(&[(vod, 20.0, 50.0)]);
    // 0:10-0:20 of the edit, 0:30-0:40 of the VOD, is taken out: 0:20-0:30 of
    // the VOD plays at 0:00-0:10, and 0:40-0:50 at 0:10-0:20.
    app.invoke("remove_stretch", json!({ "id": edit, "from": 10.0, "to": 20.0 })).unwrap();
    let said = |text: &str| Some(text.to_string());
    assert_eq!(
        app.edit_marks(edit),
        [("highlight".to_string(), 5.0, said("before")), ("highlight".to_string(), 15.0, said("after"))]
    );
}

/// A chapter whose start is cut away starts where its piece plays again (0.5.0:
/// "a chapter whose start is cut away starts where its piece plays again"; app
/// clips.rs `edit_marks`, crates/core/src/edits.rs `place_marks`). Here the cut
/// is the editor's Remove, which splits the piece in two. Decided: the chapter
/// then starts where the next piece begins.
#[test]
fn a_chapter_whose_start_is_cut_away_starts_where_its_piece_plays_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.add_mark(vod, 36.0, Some("chapter"), Some("cut away")).unwrap();
    let edit = app.edit_of(&[(vod, 20.0, 50.0)]);
    app.invoke("remove_stretch", json!({ "id": edit, "from": 10.0, "to": 20.0 })).unwrap();
    assert_eq!(app.edit_marks(edit), [("chapter".to_string(), 10.0, Some("cut away".to_string()))]);
}

/// A chapter cut away after the edit's last piece shows nowhere: no piece
/// plays again after it (crates/core/src/edits.rs `place_marks`: "one past
/// them all shows nowhere").
#[test]
fn a_chapter_cut_away_after_the_last_piece_shows_nowhere() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.add_mark(vod, 45.0, Some("chapter"), Some("cut away")).unwrap();
    let edit = app.edit_of(&[(vod, 20.0, 50.0)]);
    app.invoke("remove_stretch", json!({ "id": edit, "from": 20.0, "to": 30.0 })).unwrap();
    assert_eq!(app.edit_marks(edit), []);
}

/// A mark's text change is one step (app clips.rs `set_mark_text`: "Text
/// changed").
#[test]
fn a_mark_s_text_change_is_one_step() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let doc = history::Doc::Vod(vod);
    let mark = mark_id(&app.add_mark(vod, 5.0, Some("chapter"), Some("Old")).unwrap());
    let before = app.steps(doc);
    app.invoke("set_mark_text", json!({ "id": mark, "text": "New" })).unwrap();
    assert_eq!(app.steps(doc), before + 1);
    assert_eq!(app.undo(vod_doc(vod)).as_deref(), Some("Text changed"));
    assert_eq!(app.marks(vod)[0].2.as_deref(), Some("Old"));
}
