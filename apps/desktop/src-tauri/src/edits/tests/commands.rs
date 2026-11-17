//! Making and changing edits through their commands, as the page calls them
//! (api.ts): a new edit and its name, the list of edits, an edit opened, its
//! transcript and captions, things put in it, its pieces moved, trimmed,
//! split and taken out, stretches of it taken out, and an edit renamed and
//! deleted. Each test is named for what it says the app does and cites where
//! that's said.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vodscribe_core::testkit::eventually;

use super::*;
use crate::testkit::TestApp;

/// A new edit, as New edit makes it: what `create_edit` answers.
fn new_edit(app: &TestApp) -> Value {
    app.invoke("create_edit", json!({})).unwrap()
}

/// A new edit's id.
fn new_edit_id(app: &TestApp) -> i64 {
    new_edit(app)["id"].as_i64().unwrap()
}

/// Edit (any clip) `id` as the Edit view opens it: `get_edit`'s answer.
fn view(app: &TestApp, id: i64) -> Value {
    app.invoke("get_edit", json!({ "id": id })).unwrap()
}

/// The pieces of an edit's view.
fn pieces(view: &Value) -> &Vec<Value> {
    view["pieces"].as_array().unwrap()
}

/// What an edit's view plays, piece by piece: (VOD, start, end).
fn plays(view: &Value) -> Vec<(i64, f64, f64)> {
    pieces(view)
        .iter()
        .map(|p| (p["vod_id"].as_i64().unwrap(), p["start"].as_f64().unwrap(), p["end"].as_f64().unwrap()))
        .collect()
}

/// What edit `id` plays now (`plays` of its view).
fn plays_now(app: &TestApp, id: i64) -> Vec<(i64, f64, f64)> {
    plays(&view(app, id))
}

/// The ids of edit `id`'s pieces, in order.
fn piece_ids(app: &TestApp, id: i64) -> Vec<i64> {
    pieces(&view(app, id)).iter().map(|p| p["id"].as_i64().unwrap()).collect()
}

/// Each piece of edit `id` by the group it's in: the group's title, or None.
fn groups(app: &TestApp, id: i64) -> Vec<Option<String>> {
    pieces(&view(app, id)).iter().map(|p| p["group"]["title"].as_str().map(String::from)).collect()
}

/// Undo in edit `id`'s view: the label of what it took back, if anything.
fn undo(app: &TestApp, id: i64) -> Option<String> {
    let done = app.invoke("undo", json!({ "view": { "kind": "edit", "id": id } })).unwrap();
    done["label"].as_str().map(String::from)
}

/// `start`..`end` of VOD `vod` put in edit `edit`, as dropping a stretch on
/// it does (`add_range_to_edit`), at `index` if given.
fn put_range(app: &TestApp, edit: i64, vod: i64, start: f64, end: f64, index: Option<usize>) -> Result<Value, Value> {
    let args = json!({ "editId": edit, "vodId": vod, "start": start, "end": end, "index": index });
    app.invoke("add_range_to_edit", args)
}

/// A stretch of VOD `vod`, as `put_in_edit` takes one (`edits::Put::Range`).
fn range(vod: i64, start: f64, end: f64, title: &str) -> Value {
    json!({ "kind": "range", "vodId": vod, "start": start, "end": end, "title": title })
}

/// The clips on edit `edit`'s clip bar, in order.
fn bar(app: &TestApp, edit: i64) -> Vec<vodscribe_core::db::Clip> {
    app.db().bar_clips(edit).unwrap()
}

/// `seconds` to the millisecond, as times are compared here.
fn ms(seconds: f64) -> f64 {
    (seconds * 1000.0).round() / 1000.0
}

/// Lines or captions as the page gets them: (start, end, text), to the
/// millisecond.
fn timed(answer: Value) -> Vec<(f64, f64, String)> {
    let row = |r: &Value| {
        let text = r["text"].as_str().unwrap().to_string();
        (ms(r["start"].as_f64().unwrap()), ms(r["end"].as_f64().unwrap()), text)
    };
    answer.as_array().unwrap().iter().map(row).collect()
}

/// A clip of VOD `vod` from `from` to `to` (seconds), split in two at `at`
/// in the editor and made again: a clip of two pieces.
fn clip_in_two(app: &TestApp, vod: i64, from: &str, to: &str, at: f64) -> vodscribe_core::db::Clip {
    let clip = app.make_clip(vod, from, to);
    let piece = piece_ids(app, clip.id)[0];
    app.invoke("split_piece", json!({ "id": piece, "at": at })).unwrap();
    app.made(clip.id)
}

// A new edit and the list of edits.

/// A new edit is empty, not rendered, and named "Edit 1", then "Edit 2"
/// (edits.rs `create_edit`: "A new, empty edit, named \"Edit 1\", \"Edit 2\"
/// and so on"; core edits.rs `Edit::status`: "waiting (never rendered)").
#[test]
fn a_new_edit_is_empty_not_rendered_and_named_edit_1_then_edit_2() {
    let app = TestApp::new();
    let first = new_edit(&app);
    let second = new_edit(&app);
    assert_eq!((&first["title"], &second["title"]), (&json!("Edit 1"), &json!("Edit 2")));
    assert_eq!((&first["pieces"], &first["length"], &first["ranges"]), (&json!(0), &json!(0.0), &json!([])));
    assert_eq!((&first["status"], &first["file_path"]), (&json!("waiting"), &Value::Null));
}

/// A new edit is never given a name another edit has (edits.rs `create_edit`:
/// "named \"Edit 1\", \"Edit 2\" and so on"): one renamed "Edit 3" keeps the
/// next new one from being named so.
#[test]
fn a_new_edit_is_never_given_a_name_another_edit_has() {
    let app = TestApp::new();
    let first = new_edit_id(&app);
    new_edit(&app);
    app.invoke("rename_edit", json!({ "id": first, "title": "Edit 3" })).unwrap();
    let third = new_edit(&app)["title"].as_str().unwrap().to_string();
    assert!(third.starts_with("Edit ") && third != "Edit 2" && third != "Edit 3", "named {third}");
}

/// The list of edits is the library's edits in its order, and nothing else: no
/// VOD, no VOD's own clip, no clip on a clip bar (core edits.rs `Db::edits`:
/// "Every edit (listed clips other than VODs' own), in their order";
/// edits.rs `list_edits`).
#[test]
fn the_list_of_edits_is_the_library_s_edits_in_its_order_and_nothing_else() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.make_clip(vod, "0:10", "0:20");
    let a = new_edit_id(&app);
    let b = new_edit_id(&app);
    app.invoke("put_in_edit", json!({ "editId": a, "items": [range(vod, 1.0, 2.0, "one")], "bar": true })).unwrap();
    let order =
        [json!({ "kind": "edit", "id": a }), json!({ "kind": "vod", "id": vod }), json!({ "kind": "edit", "id": b })];
    app.invoke("reorder_library", json!({ "items": order })).unwrap();
    let listed = app.invoke("list_edits", json!({})).unwrap();
    let ids: Vec<i64> = listed.as_array().unwrap().iter().map(|e| e["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, [a, b]);
    for clip in bar(&app, a) {
        app.made(clip.id);
    }
}

/// An edit's row says how many pieces it has, how long it plays, and the VODs
/// its pieces are of (core edits.rs `Edit`: "How many pieces it has, and how
/// long it plays, in seconds"; "The VODs its pieces are of").
#[test]
fn an_edit_s_row_says_how_many_pieces_how_long_and_of_which_vods() {
    let app = TestApp::new();
    let day = app.add_video_file("Day 3.mp4", 60.0).id;
    let night = app.add_video_file("Night 1.mp4", 60.0).id;
    let edit = app.edit_of(&[(day, 10.0, 20.0), (night, 0.0, 5.0), (day, 30.0, 32.0)]);
    let listed = app.invoke("list_edits", json!({})).unwrap();
    let row = listed.as_array().unwrap().iter().find(|e| e["id"] == edit).expect("the edit listed");
    assert_eq!((&row["pieces"], ms(row["length"].as_f64().unwrap())), (&json!(3), 17.0));
    let mut vods = vec![day, night];
    vods.sort_unstable();
    assert_eq!(row["vod_ids"], json!(vods));
}

// An edit opened.

/// An edit opened gives its pieces in their order, each with its VOD's title
/// (edits.rs `EditView`: "Edit `id` and its pieces, for the Edit view"; core
/// edits.rs `Db::edit_pieces`: "Edit `id`'s pieces, in order").
#[test]
fn an_edit_opened_gives_its_pieces_in_order_each_with_its_vod_s_title() {
    let app = TestApp::new();
    let day = app.add_video_file("Day 3.mp4", 60.0).id;
    let night = app.add_video_file("Night 1.mp4", 60.0).id;
    let edit = app.edit_of(&[(night, 5.0, 10.0), (day, 10.0, 20.0)]);
    let opened = view(&app, edit);
    assert_eq!(opened["edit"]["title"], "Best of");
    assert_eq!(plays(&opened), [(night, 5.0, 10.0), (day, 10.0, 20.0)]);
    let titles: Vec<&str> = pieces(&opened).iter().map(|p| p["vod_title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Night 1", "Day 3"]);
}

/// An edit that's gone says so, opened or with anything put in it: "That
/// edit is gone." (edits.rs `view`, `add_to_edit`, `add_range_to_edit`; core
/// edits.rs `Db::put`).
#[test]
fn an_edit_that_is_gone_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = app.make_clip(vod, "0:10", "0:20");
    let edit = new_edit_id(&app);
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    let gone = Err(json!("That edit is gone."));
    assert_eq!(app.invoke("get_edit", json!({ "id": edit })), gone);
    assert_eq!(app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })), gone);
    assert_eq!(put_range(&app, edit, vod, 1.0, 2.0, None), gone);
    let items = [range(vod, 1.0, 2.0, "one")];
    assert_eq!(app.invoke("put_in_edit", json!({ "editId": edit, "items": items, "bar": false })), gone);
}

/// A piece whose video file was deleted outside VODscribe is a hole, and the
/// edit's other pieces aren't (CHANGELOG.md, 0.5.0: "In an edit, a piece whose
/// video file you deleted outside VODscribe shows striped in the timeline and
/// plays and renders as black and silence"; tests/e2e/scenarios/holes.mjs:
/// "The piece says it's a hole"; edits.rs `pieces`).
#[test]
fn a_piece_whose_file_was_deleted_outside_vodscribe_is_a_hole() {
    let app = TestApp::new();
    let gone = app.add_video_file("Gone.mp4", 60.0);
    let here = app.add_video_file("Here.mp4", 60.0).id;
    let edit = app.edit_of(&[(gone.id, 0.0, 10.0), (here, 0.0, 10.0)]);
    let holes = |app: &TestApp| pieces(&view(app, edit)).iter().map(|p| p["hole"].clone()).collect::<Vec<_>>();
    assert_eq!(holes(&app), [false, false]);
    std::fs::remove_file(gone.source_path.unwrap()).unwrap();
    assert_eq!(holes(&app), [true, false]);
}

/// A piece of a file on a drive that's away (its folder gone with it) isn't a
/// hole: nothing of it is lost (footage.rs `lost`: "Only where its folder is
/// still there: a drive that's away (unmounted, unplugged) loses nothing";
/// edits.rs `pieces`).
#[test]
fn a_piece_of_a_file_on_a_drive_that_is_away_is_not_a_hole() {
    let app = TestApp::new();
    let away = app.add_video_file("Away.mp4", 60.0);
    let edit = app.edit_of(&[(away.id, 0.0, 10.0)]);
    let file = PathBuf::from(away.source_path.unwrap());
    std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    assert_eq!(pieces(&view(&app, edit))[0]["hole"], false);
}

// Its transcript and captions.

/// An edit's transcript has only what it plays, each piece's VOD's lines where
/// that piece plays on the edit's timeline (CHANGELOG.md, 0.5.0: "An edit's
/// transcript shows under its player as a VOD's does, with only what the edit
/// plays"; edits.rs `edit_lines`: "its pieces' VODs' lines where they
/// play"). A line split between two pieces shows only its words said there:
/// core edits.rs `lines_and_captions_follow_the_pieces_with_the_words_that_play`.
#[test]
fn an_edit_s_transcript_has_only_what_it_plays_on_its_timeline() {
    let app = TestApp::new();
    let day = app.add_video_file("Day 3.mp4", 60.0).id;
    let night = app.add_video_file("Night 1.mp4", 60.0).id;
    app.says(day, 12.0, "Hello there");
    app.says(day, 50.0, "Not in it");
    app.says(night, 31.0, "Good night");
    let edit = app.edit_of(&[(night, 30.0, 40.0), (day, 10.0, 20.0)]);
    let lines = timed(app.invoke("edit_lines", json!({ "id": edit })).unwrap());
    assert_eq!(lines, [(1.0, 2.0, "Good night".into()), (12.0, 13.0, "Hello there".into())]);
}

/// An edit has captions: what's said in each piece, where it plays on the
/// edit's timeline, each within the stretch it plays in (CHANGELOG.md, 0.5.0:
/// "Edits have captions: what's said in each piece, over the picture as it
/// plays"; core edits.rs `clip_cues`: "each within the stretch it plays in,
/// then held a moment as a VOD's are, over no other").
#[test]
fn an_edit_s_captions_are_what_each_piece_says_where_it_plays() {
    let app = TestApp::new();
    let day = app.add_video_file("Day 3.mp4", 60.0).id;
    let night = app.add_video_file("Night 1.mp4", 60.0).id;
    app.says(night, 38.5, "Good night.");
    app.says(day, 12.0, "Hello there.");
    app.says(day, 50.0, "Not in it.");
    let edit = app.edit_of(&[(night, 30.0, 40.0), (day, 10.0, 20.0)]);
    let cues = timed(app.invoke("edit_captions", json!({ "id": edit })).unwrap());
    let said: Vec<(f64, &str)> = cues.iter().map(|(start, _, text)| (*start, text.as_str())).collect();
    assert_eq!(said, [(8.5, "Good night."), (12.0, "Hello there.")]);
    assert!(cues[0].1 <= 10.0, "the first piece's caption runs past where it ends: {cues:?}");
}

/// A clip on a clip bar has its transcript and captions on its own timeline,
/// as an edit's are (edits.rs `edit_lines`: "Edit (any clip) `id`'s
/// transcript, on its timeline"; `edit_captions`: "Edit (any clip) `id`'s
/// captions, on its timeline").
#[test]
fn a_clip_has_its_transcript_and_captions_on_its_own_timeline() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.says(vod, 12.0, "Hello there.");
    let clip = app.make_clip(vod, "0:10", "0:20");
    let lines = timed(app.invoke("edit_lines", json!({ "id": clip.id })).unwrap());
    assert_eq!(lines, [(2.0, 3.0, "Hello there.".into())]);
    let cues = timed(app.invoke("edit_captions", json!({ "id": clip.id })).unwrap());
    assert_eq!(cues.iter().map(|c| (c.0, c.2.as_str())).collect::<Vec<_>>(), [(2.0, "Hello there.")]);
}

// Putting things in it.

/// A change answers the edit as it is now, and the page hears it (edits.rs
/// `changing`: "then gives the edit as it is now"; `view`).
#[test]
fn a_change_answers_the_edit_as_it_is_now_and_the_page_hears_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = new_edit_id(&app);
    let answer = put_range(&app, edit, vod, 10.0, 20.0, None).unwrap();
    eventually("the page hears the edit", || app.heard("edit").last() == Some(&answer["edit"]));
    assert_eq!(answer, view(&app, edit));
    assert_eq!(plays(&answer), [(vod, 10.0, 20.0)]);
}

/// A stretch put in an edit is named for what's said at its start, as a clip
/// of it is (edits.rs `add_range_to_edit`: "or for what's said in it as a clip
/// would be").
#[test]
fn a_stretch_put_in_an_edit_is_named_as_a_clip_of_it_is() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.says(vod, 10.5, "The Eagle has landed");
    let clip = app.make_clip(vod, "0:10", "0:20");
    let edit = new_edit_id(&app);
    let answer = put_range(&app, edit, vod, 10.0, 20.0, None).unwrap();
    assert_eq!(pieces(&answer)[0]["title"], json!(clip.title));
    assert_eq!(clip.title, "The Eagle has landed");
}

/// A stretch put with a clip's name keeps that name (edits.rs
/// `add_range_to_edit`: "named `title` (a clip's it was)").
#[test]
fn a_stretch_put_with_a_clip_s_name_keeps_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.says(vod, 10.5, "The Eagle has landed");
    let edit = new_edit_id(&app);
    let args = json!({ "editId": edit, "vodId": vod, "start": 10.0, "end": 20.0, "title": "Close call" });
    let answer = app.invoke("add_range_to_edit", args).unwrap();
    assert_eq!(pieces(&answer)[0]["title"], "Close call");
}

/// A stretch put at a place in an edit goes there, and with no place, last
/// (edits.rs `add_range_to_edit`: "at `index` (or last)").
#[test]
fn a_stretch_put_at_a_place_goes_there_and_with_none_last() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 20.0, 30.0)]);
    put_range(&app, edit, vod, 40.0, 45.0, Some(1)).unwrap();
    put_range(&app, edit, vod, 50.0, 55.0, None).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 10.0), (vod, 40.0, 45.0), (vod, 20.0, 30.0), (vod, 50.0, 55.0)]);
}

/// A stretch past its VOD's end is cut at the end, one starting past it is
/// refused saying so, and a refused change is no step to undo (core edits.rs
/// `Db::vod_span`: "its end cut at the VOD's"; core clips.rs `check_span`:
/// "That's past the end of the video."; history.rs `History::change`: "One
/// that fails part-way leaves the library as it was and records nothing").
#[test]
fn a_stretch_past_its_vod_s_end_is_cut_there_or_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = new_edit_id(&app);
    assert_eq!(put_range(&app, edit, vod, 70.0, 80.0, None), Err(json!(clips::PAST_THE_END)));
    assert_eq!(undo(&app, edit), None, "a refused stretch recorded a change");
    put_range(&app, edit, vod, 50.0, 80.0, None).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 50.0, 60.0)]);
}

/// A stretch of a VOD that's gone is refused: "That VOD is gone." (edits.rs
/// `add_range_to_edit`).
#[test]
fn a_stretch_of_a_vod_that_is_gone_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = new_edit_id(&app);
    // No VOD of the library has that id.
    assert_eq!(put_range(&app, edit, vod + 1000, 1.0, 2.0, None), Err(json!("That VOD is gone.")));
    assert!(plays_now(&app, edit).is_empty());
}

/// A clip added to an edit arrives whole, as one group named as the clip,
/// its pieces as the clip has them (CHANGELOG.md, 0.5.0: "A clip added to an
/// edit arrives whole, as one band on the timeline that moves and goes as
/// one"; edits.rs `add_to_edit`: "as a group …: shown as one, its pieces as
/// the clip has them").
#[test]
fn a_clip_added_to_an_edit_arrives_whole_as_one_group() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = clip_in_two(&app, vod, "0:10", "0:30", 20.0);
    let edit = app.edit_of(&[(vod, 40.0, 50.0)]);
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id, "index": 0 })).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 10.0, 20.0), (vod, 20.0, 30.0), (vod, 40.0, 50.0)]);
    let named = Some(clip.title.clone());
    assert_eq!(groups(&app, edit), [named.clone(), named, None]);
    let opened = view(&app, edit);
    let group = |i: usize| pieces(&opened)[i]["group"]["id"].clone();
    assert_eq!(group(0), group(1), "the clip's pieces aren't one group");
}

/// What's put in an edit is its own copy: the clip changed after changes
/// nothing in the edit (CHANGELOG.md, 0.4.0: "What's put in an edit is its own
/// copy."; edits.rs `add_to_edit`: "It's the edit's own from then on").
#[test]
fn a_clip_changed_after_it_was_added_changes_nothing_in_the_edit() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = app.make_clip(vod, "0:10", "0:20");
    let edit = new_edit_id(&app);
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    let piece = piece_ids(&app, clip.id)[0];
    app.invoke("trim_piece", json!({ "id": piece, "start": 12.0, "end": 18.0 })).unwrap();
    app.made(clip.id);
    assert_eq!(plays_now(&app, clip.id), [(vod, 12.0, 18.0)], "the clip didn't change");
    assert_eq!(plays_now(&app, edit), [(vod, 10.0, 20.0)]);
}

/// An edit added to another arrives as one group, as a clip does, and an edit
/// can't go inside itself: "A clip can't go inside itself." (CHANGELOG.md,
/// 0.5.0: "An edit added to another arrives the same way."; core edits.rs
/// `Db::add_group`).
#[test]
fn an_edit_added_to_another_arrives_as_one_group_but_not_inside_itself() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let inner = app.edit_of(&[(vod, 0.0, 5.0), (vod, 10.0, 15.0)]);
    let outer = new_edit_id(&app);
    app.invoke("add_to_edit", json!({ "editId": outer, "clipId": inner })).unwrap();
    assert_eq!(plays_now(&app, outer), [(vod, 0.0, 5.0), (vod, 10.0, 15.0)]);
    assert_eq!(groups(&app, outer), [Some("Best of".into()), Some("Best of".into())]);
    let itself = app.invoke("add_to_edit", json!({ "editId": inner, "clipId": inner }));
    assert_eq!(itself, Err(json!("A clip can't go inside itself.")));
    assert_eq!(plays_now(&app, inner), [(vod, 0.0, 5.0), (vod, 10.0, 15.0)]);
}

/// Several things put in an edit go in, in their order, where the marker
/// shows, as one step a single undo takes back (CHANGELOG.md, 0.6.0: "Drag
/// what you picked into an edit." "it all goes in, in the order you picked it,
/// as one step a single undo takes back"; "on its timeline where the marker
/// shows"; edits.rs `put_in_edit`: "as one change undo takes back whole").
#[test]
fn several_things_put_in_an_edit_go_in_their_order_as_one_step() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 20.0, 30.0)]);
    let items = [range(vod, 40.0, 41.0, "b"), range(vod, 50.0, 52.0, "a")];
    app.invoke("put_in_edit", json!({ "editId": edit, "items": items, "bar": false, "index": 1 })).unwrap();
    let put = [(vod, 0.0, 10.0), (vod, 40.0, 41.0), (vod, 50.0, 52.0), (vod, 20.0, 30.0)];
    assert_eq!(plays_now(&app, edit), put);
    assert_eq!(undo(&app, edit).as_deref(), Some("Pieces added"));
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 10.0), (vod, 20.0, 30.0)], "one undo left some");
}

/// Several things put on an edit's clip bar are a clip each, and each is made
/// (CHANGELOG.md, 0.6.0: "or on its clip bar for a clip each"; edits.rs
/// `put_in_edit`: "The clips made on the bar are queued to be made").
#[test]
fn several_things_put_on_an_edit_s_clip_bar_are_a_clip_each_and_made() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 30.0)]);
    let items = [range(vod, 1.0, 2.0, "one"), range(vod, 3.0, 5.0, "two")];
    app.invoke("put_in_edit", json!({ "editId": edit, "items": items, "bar": true })).unwrap();
    let clips = bar(&app, edit);
    assert_eq!(clips.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["one", "two"]);
    for clip in clips {
        let made = app.made(clip.id);
        assert_eq!((made.status, made.error), (ClipStatus::Ready, None), "clip {} not made", clip.title);
    }
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 30.0)], "the timeline changed");
}

/// Several things put in an edit, one of which can't go, put none of them in,
/// and record no step (core edits.rs `Db::put`: "as one change (all of them,
/// or none if one can't go)"; history.rs `History::change`: "One that fails
/// part-way leaves the library as it was and records nothing").
#[test]
fn several_things_put_in_an_edit_one_of_which_cannot_go_put_none() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0)]);
    let items = [range(vod, 20.0, 30.0, "fine"), json!({ "kind": "vod", "id": vod + 1000 })];
    let refused = app.invoke("put_in_edit", json!({ "editId": edit, "items": items, "bar": false }));
    assert_eq!(refused, Err(json!("That VOD is gone.")));
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 10.0)]);
    assert_eq!(undo(&app, edit), None, "a refused change recorded a step");
}

/// Clips, highlights and stretches of the edit's own timeline put in together
/// go in, in their order, where the marker shows: a clip as one group, a
/// highlight as one piece named for it, a stretch of the edit as what plays
/// there (CHANGELOG.md, 0.6.0: "Drag one of the clips or rows picked, Alt-drag
/// a picked stretch, or pull a picked piece off an edit's timeline, and it all
/// goes in, in the order you picked it"; 0.7.0: "Highlights drag into edits."
/// "it goes in as one piece named for it").
#[test]
fn clips_highlights_and_stretches_put_together_go_in_their_order_at_the_marker() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = app.make_clip(vod, "0:40", "0:45");
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 20.0, 30.0)]);
    let items = [
        // 0:09-0:11 of the edit: the end of its first piece and the start of its second.
        json!({ "kind": "span", "clip": edit, "from": 9.0, "to": 11.0 }),
        json!({ "kind": "clip", "id": clip.id }),
        json!({ "kind": "highlight", "vodId": vod, "start": 50.0, "end": 52.0, "title": "Close call" }),
        range(vod, 55.0, 56.0, "last"),
    ];
    app.invoke("put_in_edit", json!({ "editId": edit, "items": items, "bar": false, "index": 1 })).unwrap();
    let put = [
        (vod, 0.0, 10.0),
        (vod, 9.0, 10.0),
        (vod, 20.0, 21.0),
        (vod, 40.0, 45.0),
        (vod, 50.0, 52.0),
        (vod, 55.0, 56.0),
        (vod, 20.0, 30.0),
    ];
    assert_eq!(plays_now(&app, edit), put);
    let (clip_named, lit) = (Some(clip.title.clone()), Some("Close call".to_string()));
    assert_eq!(groups(&app, edit), [None, None, None, clip_named, lit, None, None]);
}

/// Put on an edit's clip bar, a clip of another bar is copied there, and one
/// already on it, or a stretch of the edit it already has a clip of, is left
/// as it is (core edits.rs `Db::put`: "on its clip bar each is a clip of its
/// own, and one already there is left as it is"; CHANGELOG.md, 0.7.0 Fixes:
/// "C twice in an edit makes one clip.").
#[test]
fn what_an_edit_s_clip_bar_has_already_is_left_as_it_is() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let of_vod = app.make_clip(vod, "0:40", "0:45");
    let edit = app.edit_of(&[(vod, 0.0, 30.0)]);
    let put = |items: Value| app.invoke("put_in_edit", json!({ "editId": edit, "items": items, "bar": true })).unwrap();
    put(json!([{ "kind": "clip", "id": of_vod.id }, { "kind": "span", "clip": edit, "from": 1.0, "to": 2.0 }]));
    let first = bar(&app, edit);
    assert_eq!(first.len(), 2, "not a clip each: {first:?}");
    assert_ne!(first[0].id, of_vod.id, "the VOD's clip itself moved, not copied");
    assert_eq!(app.clips(vod).iter().map(|c| c.id).collect::<Vec<_>>(), [of_vod.id], "the VOD's clip left its bar");
    put(json!([{ "kind": "clip", "id": first[0].id }, { "kind": "span", "clip": edit, "from": 1.0, "to": 2.0 }]));
    let ids = |clips: Vec<vodscribe_core::db::Clip>| clips.iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids(bar(&app, edit)), ids(first), "what the bar had was put again");
    for clip in bar(&app, edit) {
        app.made(clip.id);
    }
}

/// A clip added to an edit brings its chapters and highlights along, on the
/// same moments, but not its cuts (CHANGELOG.md, 0.5.0: "a clip added to an
/// edit bring the chapters and highlights in them along, on the same moments";
/// core edits.rs `Db::add_group`: "cuts are where its own stretches were, not
/// its").
#[test]
fn a_clip_added_to_an_edit_brings_its_chapters_and_highlights_but_not_its_cuts() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = app.make_clip(vod, "0:10", "0:20");
    for (time, kind, text) in [(2.0, "chapter", Some("Start")), (4.0, "cut", None), (6.0, "highlight", Some("Wow"))] {
        app.invoke("add_edit_mark", json!({ "editId": clip.id, "time": time, "kind": kind, "text": text })).unwrap();
    }
    app.made(clip.id);
    let edit = app.edit_of(&[(vod, 40.0, 45.0)]);
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    let marks = app.invoke("edit_marks", json!({ "id": edit })).unwrap();
    let seen: Vec<(&str, f64, &str)> = marks
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["kind"].as_str().unwrap(), ms(m["start"].as_f64().unwrap()), m["text"].as_str().unwrap_or("")))
        .collect();
    assert_eq!(seen, [("chapter", 7.0, "Start"), ("highlight", 11.0, "Wow")]);
}

// Its pieces.

/// A piece moved goes to its new place, the others keeping their order, and
/// undo puts it back (CHANGELOG.md, 0.4.0: "drag pieces to reorder them";
/// 0.4.1: "Ctrl+← and Ctrl+→ … move the piece at the playhead earlier or
/// later"; 0.4.0: "Undo and redo … for edits").
#[test]
fn a_piece_moved_goes_to_its_new_place_and_undo_puts_it_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 20.0, 30.0), (vod, 40.0, 50.0)]);
    let first = piece_ids(&app, edit)[0];
    app.invoke("move_piece", json!({ "id": first, "index": 2 })).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 20.0, 30.0), (vod, 40.0, 50.0), (vod, 0.0, 10.0)]);
    assert_eq!(undo(&app, edit).as_deref(), Some("Piece moved"));
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 10.0), (vod, 20.0, 30.0), (vod, 40.0, 50.0)]);
}

/// A piece of a clip in an edit moves among the clip's pieces, staying in its
/// group (CHANGELOG.md, 0.5.0: "Double-click it to open it, and its pieces
/// show one by one, to move among themselves").
#[test]
fn a_piece_of_a_group_moves_among_the_group_s_pieces() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = clip_in_two(&app, vod, "0:10", "0:30", 20.0);
    let edit = app.edit_of(&[(vod, 40.0, 50.0)]);
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    let second = piece_ids(&app, edit)[2];
    app.invoke("move_piece", json!({ "id": second, "index": 0 })).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 40.0, 50.0), (vod, 20.0, 30.0), (vod, 10.0, 20.0)]);
    let named = Some(clip.title.clone());
    assert_eq!(groups(&app, edit), [None, named.clone(), named]);
}

/// A piece trimmed plays what it was trimmed to, shorter or longer, and undo
/// puts it back (CHANGELOG.md, 0.4.0: "drag … their edges to make them shorter
/// or longer"; edits.rs `trim_piece`: "Trims piece `id` to `start`..`end` of
/// its VOD").
#[test]
fn a_piece_trimmed_plays_what_it_was_trimmed_to_and_undo_puts_it_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 10.0, 20.0)]);
    let piece = piece_ids(&app, edit)[0];
    app.invoke("trim_piece", json!({ "id": piece, "start": 12.0, "end": 18.0 })).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 12.0, 18.0)]);
    app.invoke("trim_piece", json!({ "id": piece, "start": 5.0, "end": 25.0 })).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 5.0, 25.0)]);
    assert_eq!(undo(&app, edit).as_deref(), Some("Piece resized"));
    assert_eq!(plays_now(&app, edit), [(vod, 12.0, 18.0)]);
}

/// A trim that ends before it starts is refused saying so, and the piece stays
/// as it was (core clips.rs `check_span`: "A clip has to end after it
/// starts."; core edits.rs `Db::set_piece_range`).
#[test]
fn a_trim_that_ends_before_it_starts_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 10.0, 20.0)]);
    let piece = piece_ids(&app, edit)[0];
    let refused = app.invoke("trim_piece", json!({ "id": piece, "start": 18.0, "end": 12.0 }));
    assert_eq!(refused, Err(json!(clips::ENDS_BEFORE_START)));
    assert_eq!(plays_now(&app, edit), [(vod, 10.0, 20.0)]);
}

/// A piece split in two plays as it did, the second part right after the
/// first, named as it was and in its group, and undo puts it back whole
/// (edits.rs `split_piece`: "Splits piece `id` in two at `at` seconds of its
/// VOD"; core edits.rs `Db::split_piece`: "the second right after the first";
/// "Beside it, in its group if it's in one").
#[test]
fn a_piece_split_in_two_plays_as_it_did_and_undo_puts_it_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = app.make_clip(vod, "0:40", "0:50");
    let edit = app.edit_of(&[(vod, 10.0, 30.0)]);
    app.invoke("add_to_edit", json!({ "editId": edit, "clipId": clip.id })).unwrap();
    let ids = piece_ids(&app, edit);
    app.invoke("split_piece", json!({ "id": ids[0], "at": 16.0 })).unwrap();
    app.invoke("split_piece", json!({ "id": ids[1], "at": 45.0 })).unwrap();
    let opened = view(&app, edit);
    assert_eq!(plays(&opened), [(vod, 10.0, 16.0), (vod, 16.0, 30.0), (vod, 40.0, 45.0), (vod, 45.0, 50.0)]);
    let titles: Vec<&str> = pieces(&opened).iter().map(|p| p["title"].as_str().unwrap()).collect();
    assert_eq!((titles[0], titles[1]), ("piece", "piece"));
    let named = Some(clip.title.clone());
    assert_eq!(groups(&app, edit), [None, None, named.clone(), named]);
    assert_eq!(undo(&app, edit).as_deref(), Some("Piece split"));
    assert_eq!(undo(&app, edit).as_deref(), Some("Piece split"));
    assert_eq!(plays_now(&app, edit), [(vod, 10.0, 30.0), (vod, 40.0, 50.0)]);
}

/// A split not inside the piece is refused: "That's not inside the piece.",
/// with nothing changed and no step to undo (edits.rs `split_piece`).
#[test]
fn a_split_not_inside_the_piece_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 10.0, 30.0)]);
    let piece = piece_ids(&app, edit)[0];
    let refused = app.invoke("split_piece", json!({ "id": piece, "at": 5.0 }));
    assert_eq!(refused, Err(json!("That's not inside the piece.")));
    assert_eq!(plays_now(&app, edit), [(vod, 10.0, 30.0)]);
    assert_eq!(undo(&app, edit), None);
}

/// A split that would leave a sliver at either end of the piece is refused as
/// not inside it (core edits.rs `SLIVER`: "Stretches shorter than this are left
/// out (a sliver)"; `Db::split_piece`: "False if `at` isn't inside it";
/// CHANGELOG.md, 0.7.1 Fixes: "Splitting at a join leaves no sliver.").
#[test]
fn a_split_that_would_leave_a_sliver_is_refused() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 10.0, 30.0)]);
    let piece = piece_ids(&app, edit)[0];
    for at in [10.02, 29.98] {
        let refused = app.invoke("split_piece", json!({ "id": piece, "at": at }));
        assert_eq!(refused, Err(json!("That's not inside the piece.")), "split at {at}");
    }
    assert_eq!(plays_now(&app, edit), [(vod, 10.0, 30.0)]);
}

/// A piece removed leaves the edit, and undo brings it back where it was
/// (edits.rs `remove_piece`; CHANGELOG.md, 0.4.0: "Undo and redo … for
/// edits").
#[test]
fn a_piece_removed_leaves_the_edit_and_undo_brings_it_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 20.0, 30.0)]);
    let first = piece_ids(&app, edit)[0];
    app.invoke("remove_piece", json!({ "id": first })).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 20.0, 30.0)]);
    assert_eq!(undo(&app, edit).as_deref(), Some("Piece removed"));
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 10.0), (vod, 20.0, 30.0)]);
}

/// A piece that's gone says so when it's moved, trimmed, split or removed:
/// "That piece is gone." (edits.rs `edit_of`: "The edit piece `id` is in").
#[test]
fn a_piece_that_is_gone_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0)]);
    let piece = piece_ids(&app, edit)[0];
    app.invoke("remove_piece", json!({ "id": piece })).unwrap();
    let gone = Err(json!("That piece is gone."));
    assert_eq!(app.invoke("move_piece", json!({ "id": piece, "index": 0 })), gone);
    assert_eq!(app.invoke("trim_piece", json!({ "id": piece, "start": 1.0, "end": 2.0 })), gone);
    assert_eq!(app.invoke("split_piece", json!({ "id": piece, "at": 5.0 })), gone);
    assert_eq!(app.invoke("remove_piece", json!({ "id": piece })), gone);
}

// Stretches of it taken out.

/// A stretch taken out of a piece splits it there, each part a piece of its
/// own, and one undo brings it back whole (CHANGELOG.md, 0.5.0: "Delete then
/// takes the selected lines' stretch out, as any stretch taken out: the piece
/// splits in two there, each part its own band (Ctrl+Z brings it back)";
/// edits.rs `remove_stretch`: "as one change").
#[test]
fn a_stretch_taken_out_splits_its_piece_and_one_undo_brings_it_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 10.0, 40.0)]);
    app.invoke("remove_stretch", json!({ "id": edit, "from": 5.0, "to": 10.0 })).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 10.0, 15.0), (vod, 20.0, 40.0)]);
    assert_eq!(undo(&app, edit).as_deref(), Some("Stretch removed"));
    assert_eq!(plays_now(&app, edit), [(vod, 10.0, 40.0)]);
}

/// Pieces, stretches and clips of its clip bar picked in an edit are taken out
/// at once, as one step a single undo brings back (CHANGELOG.md, 0.6.0: "Delete
/// takes the picked clips out of the clip bar, and in an edit the picked
/// pieces too, in one go that a single undo brings back"; edits.rs
/// `remove_from_edit`: "Takes several things out of edit `id` at once …, as
/// one change").
#[test]
fn what_is_picked_in_an_edit_is_taken_out_at_once_and_one_undo_brings_it_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0), (vod, 20.0, 30.0), (vod, 40.0, 50.0)]);
    app.invoke("put_in_edit", json!({ "editId": edit, "items": [range(vod, 1.0, 2.0, "one")], "bar": true })).unwrap();
    let clip = app.made(bar(&app, edit)[0].id);
    let second = piece_ids(&app, edit)[1];
    let args = json!({ "id": edit, "pieces": [second], "stretches": [[2.0, 4.0]], "clips": [clip.id] });
    app.invoke("remove_from_edit", args).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 2.0), (vod, 4.0, 10.0), (vod, 40.0, 50.0)]);
    assert!(bar(&app, edit).is_empty(), "the clip is still on the bar");
    assert_eq!(undo(&app, edit).as_deref(), Some("Pieces removed"));
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 10.0), (vod, 20.0, 30.0), (vod, 40.0, 50.0)]);
    let back: Vec<String> = bar(&app, edit).iter().map(|c| c.title.clone()).collect();
    assert_eq!(back, ["one"], "one undo didn't bring the clip back");
    for clip in bar(&app, edit) {
        app.made(clip.id);
    }
}

/// Stretches picked together are each taken out where they were when picked
/// (core edits.rs `Db::remove_from_edit`: "from the last back, so each one's
/// times are still where they were when it was picked").
#[test]
fn stretches_picked_together_are_each_taken_out_where_they_were_picked() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 30.0)]);
    let args = json!({ "id": edit, "pieces": [], "stretches": [[5.0, 10.0], [20.0, 25.0]], "clips": [] });
    app.invoke("remove_from_edit", args).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 5.0), (vod, 10.0, 20.0), (vod, 25.0, 30.0)]);
}

/// A clip picked that isn't on the edit's own clip bar is left where it is
/// (edits.rs `remove_from_edit`: "Only a clip of this edit's own clip bar").
#[test]
fn a_clip_not_on_the_edit_s_own_clip_bar_is_left_where_it_is() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let clip = app.make_clip(vod, "0:40", "0:50");
    let edit = app.edit_of(&[(vod, 0.0, 10.0)]);
    app.invoke("remove_from_edit", json!({ "id": edit, "pieces": [], "stretches": [], "clips": [clip.id] })).unwrap();
    assert_eq!(app.clips(vod).iter().map(|c| c.id).collect::<Vec<_>>(), [clip.id]);
    assert!(Path::new(clip.file_path.as_deref().unwrap()).is_file(), "its file went");
}

/// A stretch taken out between two cuts makes them a real split: those cuts
/// go, and a cut elsewhere stays (CHANGELOG.md, 0.5.0: "Delete with nothing
/// selected takes out the stretch between cuts at the playhead: the piece
/// splits there, and those cuts become a real split, drawn as a plain line";
/// edits.rs `remove_stretch`: "its cuts there made a real split").
#[test]
fn a_stretch_taken_out_between_cuts_makes_them_a_real_split() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 30.0)]);
    for time in [5.0, 10.0, 20.0] {
        app.invoke("add_edit_mark", json!({ "editId": edit, "time": time, "kind": "cut" })).unwrap();
    }
    app.invoke("remove_stretch", json!({ "id": edit, "from": 10.0, "to": 20.0 })).unwrap();
    assert_eq!(plays_now(&app, edit), [(vod, 0.0, 10.0), (vod, 20.0, 30.0)]);
    let marks = app.invoke("edit_marks", json!({ "id": edit })).unwrap();
    let cuts: Vec<f64> = marks.as_array().unwrap().iter().map(|m| ms(m["start"].as_f64().unwrap())).collect();
    assert_eq!(cuts, [5.0], "the cuts at the split stayed, or the one elsewhere went");
}

// Renamed and deleted.

/// A renamed edit has the name written, without the spaces around it, and
/// undo gives it back its old one (edits.rs `rename_edit`, `title`: "A title
/// as edits keep it (`clips::written_title`)").
#[test]
fn a_renamed_edit_has_the_name_written_and_undo_gives_back_its_old_one() {
    let app = TestApp::new();
    let edit = new_edit_id(&app);
    let answer = app.invoke("rename_edit", json!({ "id": edit, "title": "  Race night  " })).unwrap();
    assert_eq!(answer["edit"]["title"], "Race night");
    assert_eq!(undo(&app, edit).as_deref(), Some("Edit renamed"));
    assert_eq!(view(&app, edit)["edit"]["title"], "Edit 1");
}

/// A name with nothing in it is refused: "An edit needs a name." (edits.rs
/// `title`).
#[test]
fn an_edit_s_name_with_nothing_in_it_is_refused() {
    let app = TestApp::new();
    let edit = new_edit_id(&app);
    let refused = app.invoke("rename_edit", json!({ "id": edit, "title": "   " }));
    assert_eq!(refused, Err(json!("An edit needs a name.")));
    assert_eq!(view(&app, edit)["edit"]["title"], "Edit 1");
}

/// A deleted edit leaves the library's list and the list of edits, and the
/// page hears it's gone (edits.rs `delete_edit`: "Deletes edit `id`").
#[test]
fn a_deleted_edit_leaves_the_library_and_the_page_hears_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let kept = new_edit_id(&app);
    let edit = new_edit_id(&app);
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    let listed = app.invoke("list_edits", json!({})).unwrap();
    assert_eq!(listed.as_array().unwrap().iter().map(|e| e["id"].clone()).collect::<Vec<_>>(), [json!(kept)]);
    let order = app.invoke("library_order", json!({})).unwrap();
    assert_eq!(order, json!([{ "kind": "edit", "id": kept }, { "kind": "vod", "id": vod }]));
    eventually("the page hears the edit is gone", || app.heard("edit-removed").contains(&json!(edit)));
}

/// Deleting an edit deletes its rendered video (edits.rs `delete_edit`:
/// "Deletes edit `id`, and its video if it was rendered").
#[test]
fn deleting_an_edit_deletes_its_rendered_video() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0)]);
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let rendered = || {
        let now = crate::edits::made_now(app.handle());
        let edit = app.db().edit(edit, now).unwrap().filter(|e| e.status == ClipStatus::Ready)?;
        edit.file_path.map(PathBuf::from).filter(|f| f.is_file())
    };
    eventually("the edit rendered", || rendered().is_some());
    let video = rendered().unwrap();
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    eventually("its video deleted", || !video.exists());
}

/// Deleting an edit isn't undone: undo in its view finds nothing after it
/// (history.rs: "Renders, downloads and deleting a VOD or an edit aren't
/// undone").
#[test]
fn deleting_an_edit_is_not_undone() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0)]);
    let piece = piece_ids(&app, edit)[0];
    app.invoke("trim_piece", json!({ "id": piece, "start": 2.0, "end": 8.0 })).unwrap();
    app.invoke("delete_edit", json!({ "id": edit })).unwrap();
    assert_eq!(undo(&app, edit), None);
    assert_eq!(app.invoke("get_edit", json!({ "id": edit })), Err(json!("That edit is gone.")));
}
