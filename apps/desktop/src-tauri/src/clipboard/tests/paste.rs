//! What a copy says of its pieces, and Ctrl+V's command: `paste_items` and
//! the state it leaves, after `copy_items` and `copied_text` as the page
//! calls them (copyPaste.ts). Ctrl+X's specs (a cut held till the clipboard
//! lets go) are in clips/tests/files.rs, beside Delete's.

use crate::testkit::TestApp;
use serde_json::{Value, json};
use vodscribe_core::edits::EditState;
use vodscribe_core::testkit::Media;
use vodscribe_core::time::Flicks;

use super::*;

fn library() -> (Db, i64) {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let vod = vodscribe_core::testkit::library::add_site_vod(&db, "youtube", "x", "t", 60.0).id;
    (db, vod)
}

fn piece(parent: Option<usize>, vod: i64, start: f64, end: Option<f64>) -> ItemState {
    ItemState {
        parent,
        vod_id: Some(vod),
        start: Some(Flicks::from_seconds(start)),
        end: end.map(Flicks::from_seconds),
        title: String::new(),
        origin: None,
    }
}

fn copy(kind: CopiedKind, items: Vec<ItemState>) -> Copied {
    Copied { kind, state: EditState { title: "c".into(), items, marks: Vec::new() } }
}

/// What the page is told of a copy: its pieces (its groups aren't), the
/// first one's stretch, and how long it plays; a piece with no end runs
/// to its VOD's end, one of a VOD that's gone plays nothing (clipboard.rs
/// `CopiedSaid`: "its title, the first of its pieces (a stretch of a VOD, in
/// its seconds) and how many it has"; "How long it plays, in seconds").
#[test]
fn a_copy_is_said_by_its_pieces() {
    let (db, vod) = library();
    let group = ItemState { parent: None, vod_id: None, start: None, end: None, title: "g".into(), origin: None };
    let items = vec![
        group.clone(),
        piece(Some(0), vod, 10.0, Some(18.0)),
        piece(None, vod, 30.0, None),
        piece(None, 9999, 5.0, None),
    ];
    let said_of = said(&db, &copy(CopiedKind::Clip, items)).unwrap();
    assert_eq!((said_of.kind, said_of.title.as_str(), said_of.pieces, said_of.length), ("clip", "c", 3, 38.0));
    let first = said_of.first.unwrap();
    assert_eq!((first.vod_id, first.vod_title.as_str(), first.start, first.end), (vod, "t", 10.0, 18.0));

    // Its first piece with no end: that stretch runs to the VOD's end.
    let stretch = copy(CopiedKind::Stretch, vec![piece(None, vod, 5.0, None)]);
    let said_of = said(&db, &stretch).unwrap();
    assert_eq!((said_of.kind, said_of.pieces, said_of.length), ("stretch", 1, 55.0));
    let first = said_of.first.unwrap();
    assert_eq!((first.start, first.end), (5.0, 60.0));

    // A group alone: no stretch, nothing played.
    let said_of = said(&db, &copy(CopiedKind::Clip, vec![group])).unwrap();
    assert!(said_of.first.is_none());
    assert_eq!((said_of.pieces, said_of.length), (0, 0.0));
}

// Ctrl+C then Ctrl+V, as the page calls them.

/// What the page writes to the system's clipboard for a copy, in these tests.
const COPIED: &str = "copied";

/// A stretch of VOD `vod` as the page names it for a copy.
fn range(vod: i64, start: f64, end: f64) -> Value {
    json!({ "kind": "range", "vodId": vod, "start": start, "end": end })
}

/// A clip as the page names it for a copy.
fn clip(id: i64) -> Value {
    json!({ "kind": "clip", "id": id })
}

/// Ctrl+C on `items`, in their order (copyPaste.ts `copyPicked`): copied,
/// then the copy's text written.
fn ctrl_c(app: &TestApp, items: &[Value]) {
    app.invoke("copy_items", json!({ "items": items })).unwrap();
    app.invoke("copied_text", json!({ "text": COPIED })).unwrap();
}

/// Ctrl+V on VOD `vod`'s clip bar: what the paste answered.
fn on_bar(app: &TestApp, vod: i64) -> Value {
    app.invoke("paste_items", json!({ "target": { "kind": "vod", "id": vod }, "bar": true, "text": COPIED })).unwrap()
}

/// Ctrl+V in edit `edit`'s timeline (or a clip's, open in the editor) at `at`
/// seconds of it: what the paste answered.
fn in_timeline(app: &TestApp, edit: i64, at: f64) -> Value {
    let target = json!({ "kind": "edit", "id": edit });
    app.invoke("paste_items", json!({ "target": target, "bar": false, "at": at, "text": COPIED })).unwrap()
}

/// The ids of the clips a paste's answer names under `key`, each made, so
/// the app ends with nothing in flight.
fn made(app: &TestApp, pasted: &Value, key: &str) -> Vec<i64> {
    let ids: Vec<i64> = pasted[key].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect();
    ids.iter().for_each(|&id| {
        app.made(id);
    });
    ids
}

/// The pills on VOD `vod`'s clip bar, in order, by their stretch.
fn pills(app: &TestApp, vod: i64) -> Vec<(f64, f64)> {
    app.clips(vod).into_iter().filter(|c| c.shown).map(|c| (c.start, c.end)).collect()
}

/// Ctrl+Z in `view` (a `Doc` as the page writes it).
fn undo(app: &TestApp, view: Value) {
    app.invoke("undo", json!({ "view": view })).unwrap();
}

/// Copies paste in the order they were copied: one clip each, after the
/// others on the bar (clipboard.rs `copy_items`: "Copies `items` (what's
/// selected or picked) as they are now, in their order"; CHANGELOG.md 0.7.1:
/// "Copy and paste clips." ("copies the selection or the clips picked")).
#[test]
fn copies_paste_in_the_order_they_were_copied() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    app.make_clip(vod, "0:01", "0:02");
    ctrl_c(&app, &[range(vod, 30.0, 40.0), range(vod, 10.0, 20.0)]);
    let pasted = on_bar(&app, vod);
    made(&app, &pasted, "clips");
    assert_eq!(pills(&app, vod), [(1.0, 2.0), (30.0, 40.0), (10.0, 20.0)]);
}

/// A paste is one change that one undo takes back whole, on a clip bar or in
/// a timeline (clipboard.rs `paste_items`: "as one change undo takes back
/// whole").
#[test]
fn a_paste_is_one_change_that_undo_takes_back_whole() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    ctrl_c(&app, &[range(vod, 30.0, 40.0), range(vod, 10.0, 20.0)]);
    let pasted = on_bar(&app, vod);
    made(&app, &pasted, "clips");
    assert_eq!(pills(&app, vod).len(), 2);
    undo(&app, json!({ "kind": "vod", "id": vod }));
    assert!(pills(&app, vod).is_empty(), "one undo, both pills");

    let edit = app.edit_of(&[(vod, 0.0, 10.0)]);
    let before = app.db().edit_state(edit).unwrap();
    in_timeline(&app, edit, 5.0);
    assert_eq!(app.db().edit_pieces(edit).unwrap().len(), 4, "split, and both copies in");
    undo(&app, json!({ "kind": "edit", "id": edit }));
    assert_eq!(app.db().edit_state(edit).unwrap(), before, "one undo, the split too");
}

/// A stretch pasted where it's already a clip makes none: the answer names
/// the clip it is, for the page to pick (clipboard.rs `Pasted`: "The clips
/// already on the bar that stretches pasted are: none made of those";
/// tests/e2e/scenarios/copy-paste.mjs: "pasted again, no second pill: the one
/// there is picked").
#[test]
fn a_stretch_pasted_where_it_is_already_a_clip_answers_that_clip() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    ctrl_c(&app, &[range(vod, 10.0, 20.0)]);
    let first = on_bar(&app, vod);
    let [id] = made(&app, &first, "clips")[..] else { panic!("pasted {first}") };
    let again = on_bar(&app, vod);
    assert_eq!(made(&app, &again, "clips"), Vec::<i64>::new(), "a second clip made");
    assert_eq!(made(&app, &again, "found"), [id]);
    assert_eq!(pills(&app, vod), [(10.0, 20.0)]);
}

/// A paste in a timeline answers the edit as it is now and the stretch the
/// copies play there (clipboard.rs `Pasted`: "On a timeline, the edit as it
/// is now, and the stretch the paste plays").
#[test]
fn a_paste_in_a_timeline_answers_the_edit_as_it_is_and_the_stretch_it_plays() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let edit = app.edit_of(&[(vod, 0.0, 10.0)]);
    ctrl_c(&app, &[range(vod, 20.0, 22.0)]);
    let pasted = in_timeline(&app, edit, 4.0);
    assert_eq!(pasted["span"], json!([4.0, 6.0]));
    assert_eq!(pasted["view"]["edit"]["id"], edit);
    let pieces: Vec<(f64, f64)> = pasted["view"]["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["start"].as_f64().unwrap(), p["end"].as_f64().unwrap()))
        .collect();
    assert_eq!(pieces, [(0.0, 4.0), (20.0, 22.0), (4.0, 10.0)]);
}

/// A paste says how many pieces it left out, their VOD gone, and how many
/// copies it refused, of another VOD for a VOD's clip bar (clipboard.rs
/// `Pasted`: "Pieces left out, their VOD gone; copies left out, of another
/// VOD").
#[test]
fn a_paste_says_what_it_left_out_and_why() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let gone = app.add_video_file("Day 4.mp4", 60.0).id;
    let other = app.add_video_file("Day 5.mp4", 60.0).id;
    ctrl_c(&app, &[range(vod, 10.0, 20.0), range(gone, 10.0, 20.0)]);
    // Removed with nothing using it: gone for a paste.
    app.invoke("delete_vod", json!({ "id": gone })).unwrap();
    let pasted = on_bar(&app, vod);
    assert_eq!(made(&app, &pasted, "clips").len(), 1);
    assert_eq!((&pasted["dropped"], &pasted["refused"]), (&json!(1), &json!(0)));

    ctrl_c(&app, &[range(vod, 30.0, 40.0), range(other, 10.0, 20.0)]);
    let pasted = on_bar(&app, vod);
    assert_eq!(made(&app, &pasted, "clips").len(), 1);
    assert_eq!((&pasted["dropped"], &pasted["refused"]), (&json!(0), &json!(1)));
    app.erasures_done();
}

/// A copied clip whose file was made, pasted, gets a copy of that file as its
/// own, not made again: ready with the same frames, and it stays when the
/// clip copied is deleted (clipboard.rs's module doc: "pasted, as it was, its
/// copy gets a copy of the file rather than being made again
/// (`Cutter::copy_file`), the same frames at once";
/// tests/e2e/scenarios/copy-paste.mjs: "ready at once, never cut, its file
/// another of the same bytes ... The first pill deleted, the copy stays, its
/// file with it").
#[test]
fn a_copied_clip_pasted_gets_a_copy_of_its_file_not_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let copied = app.make_clip(vod, "0:10", "0:20");
    let media = app.clip_media(&copied);
    ctrl_c(&app, &[clip(copied.id)]);
    // Made again, it would fail.
    app.ffmpeg.fails_at(0.5, "the source broke off");
    let pasted = on_bar(&app, vod);
    let [id] = made(&app, &pasted, "clips")[..] else { panic!("pasted {pasted}") };
    let pasted = app.made(id);
    assert_ne!(pasted.file_path, copied.file_path, "the copied clip's file shared");
    assert_eq!(app.clip_media(&pasted), media);
    app.invoke("delete_clip", json!({ "id": copied.id })).unwrap();
    assert_eq!(app.clip_media(&app.made(id)), media, "its file went with the clip copied");
}

/// A copied clip whose file changed since the copy is made again when
/// pasted, not given the changed file (clipboard.rs `CopiedFile`: "The file
/// as it was then: one changed or replaced since isn't copied").
#[test]
fn a_copied_clip_whose_file_changed_since_is_made_again_when_pasted() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let copied = app.make_clip(vod, "0:10", "0:20");
    let media = app.clip_media(&copied);
    ctrl_c(&app, &[clip(copied.id)]);
    // Replaced outside the app with something else, its time kept as a
    // copy that keeps times keeps it: the size alone tells it changed.
    let path = std::path::Path::new(copied.file_path.as_deref().unwrap());
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    let changed = Media::sound(1234.5);
    changed.write(path);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(modified).unwrap();
    let pasted = on_bar(&app, vod);
    let [id] = made(&app, &pasted, "clips")[..] else { panic!("pasted {pasted}") };
    let pasted = app.clip_media(&app.made(id));
    assert_ne!(pasted, changed, "given the changed file");
    assert_eq!(pasted, media, "not made again from its stretch");
}

/// A clip open in the editor takes a paste in its timeline, and is made
/// again with it (tests/e2e/scenarios/copy-paste.mjs: "The edit's clip open
/// in the editor takes a paste at its playhead, and is made again";
/// clipboard.rs `paste_items`: "Pastes what was copied last into `target` (a
/// VOD or an edit, or a clip open in the editor)").
#[test]
fn a_clip_open_in_the_editor_takes_a_paste_and_is_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0).id;
    let open = app.make_clip(vod, "0:10", "0:20");
    ctrl_c(&app, &[range(vod, 30.0, 32.0)]);
    let pasted = in_timeline(&app, open.id, 5.0);
    assert_eq!(pasted["span"], json!([5.0, 7.0]));
    let pieces: Vec<(f64, f64)> = app.db().edit_pieces(open.id).unwrap().iter().map(|p| (p.start, p.end)).collect();
    assert_eq!(pieces, [(10.0, 15.0), (30.0, 32.0), (15.0, 20.0)]);
    let again = app.made(open.id);
    assert_eq!(again.status, ClipStatus::Ready, "{:?}", again.error);
    assert_ne!(again.file_path, open.file_path, "left as it was made before the paste");
}
