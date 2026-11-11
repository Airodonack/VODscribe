//! Listing and ordering VODs.

use super::*;

use serde_json::{Value, json};
use vodscribe_core::db::Status;
use vodscribe_core::edits::LibraryItem;
use vodscribe_core::stretches::Transcribed;
use vodscribe_core::testkit::{Gate, eventually};

use crate::testkit::TestApp;

/// The library's list as the page reads it: `library_order`.
fn order(app: &TestApp) -> Vec<LibraryItem> {
    app.run(library_order(app.handle().clone())).unwrap()
}

/// The VODs of the library's list as the page reads them: `list_vods`.
fn listed(app: &TestApp) -> Vec<Vod> {
    app.run(list_vods(app.handle().clone())).unwrap()
}

/// The ids of the VODs `list_vods` gives, in its order.
fn listed_ids(app: &TestApp) -> Vec<i64> {
    listed(app).iter().map(|v| v.id).collect()
}

/// A new edit, as New edit makes it.
fn new_edit(app: &TestApp) -> i64 {
    app.invoke("create_edit", json!({})).unwrap()["id"].as_i64().unwrap()
}

/// Puts the library's list in the order of `items`, as dropping a row does.
fn reorder(app: &TestApp, items: Vec<LibraryItem>) {
    app.run(reorder_library(app.handle().state(), items)).unwrap();
}

/// VOD `id`'s row as the page reads it.
fn row(app: &TestApp, id: i64) -> Vod {
    listed(app).into_iter().find(|v| v.id == id).unwrap_or_else(|| panic!("VOD {id} isn't listed"))
}

/// Asks for `start`..`end` seconds of VOD `id` to be transcribed, as Transcribe does.
fn ask(app: &TestApp, id: i64, start: f64, end: Option<f64>) -> Value {
    app.invoke("transcribe_stretch", json!({ "vodId": id, "start": start, "end": end })).unwrap()
}

/// The library's list, VODs and edits, in its order
/// (vods.rs:338, `library_order`'s doc comment).
#[test]
fn the_library_lists_vods_and_edits_in_its_order() {
    let app = TestApp::new();
    let a = app.add_video_file("a.mp4", 60.0).id;
    let edit = new_edit(&app);
    let b = app.add_video_file("b.mp4", 60.0).id;
    // Each new one goes on top, VODs and edits alike (db.rs add_vod, edits.rs add_edit).
    assert_eq!(order(&app), [LibraryItem::Vod(b), LibraryItem::Edit(edit), LibraryItem::Vod(a)]);
    assert_eq!(listed_ids(&app), [b, a], "the VODs of the list, in the same order");
}

/// The order the library is put in is the order it is kept in
/// (vods.rs:344, "Puts the library's list, VODs and edits, in the order of
/// `items`"; db.rs:349, V5: "the library's order, which the user can rearrange").
#[test]
fn the_order_put_is_the_order_kept() {
    let app = TestApp::new();
    let a = app.add_video_file("a.mp4", 60.0).id;
    let b = app.add_video_file("b.mp4", 60.0).id;
    let edit = new_edit(&app);
    let put = vec![LibraryItem::Vod(a), LibraryItem::Edit(edit), LibraryItem::Vod(b)];
    reorder(&app, put.clone());
    assert_eq!(order(&app), put);
    assert_eq!(listed_ids(&app), [a, b]);
    // A VOD added after goes on top of what was put.
    let c = app.add_video_file("c.mp4", 60.0).id;
    assert_eq!(order(&app)[0], LibraryItem::Vod(c));
    assert_eq!(listed_ids(&app), [c, a, b]);
}

/// The order the library is put in lasts after a restart
/// (vods.rs:344; db.rs:349, V5: "the library's order, which the user can rearrange").
#[test]
fn the_order_lasts_after_a_restart() {
    let app = TestApp::new();
    let a = app.add_video_file("a.mp4", 60.0).id;
    let b = app.add_video_file("b.mp4", 60.0).id;
    let edit = new_edit(&app);
    let put = vec![LibraryItem::Vod(b), LibraryItem::Vod(a), LibraryItem::Edit(edit)];
    reorder(&app, put.clone());
    let app = TestApp::on(app.quit());
    assert_eq!(order(&app), put);
    assert_eq!(listed_ids(&app), [b, a]);
}

/// A VOD taken out of the list but used by an edit isn't listed, and the edit
/// keeps its piece (vods.rs:375: "Takes VOD `id` out of the library's list …
/// What other clips use of it stays, a source only"; CHANGELOG.md 0.5.0:
/// "Removing a VOD that an edit uses leaves the edit's pieces of it where they are").
#[test]
fn a_vod_taken_out_of_the_list_but_used_by_an_edit_is_not_listed() {
    let app = TestApp::new();
    let used = app.add_video_file("used.mp4", 60.0).id;
    let other = app.add_video_file("other.mp4", 60.0).id;
    let edit = new_edit(&app);
    app.invoke("add_range_to_edit", json!({ "editId": edit, "vodId": used, "start": 10.0, "end": 20.0 })).unwrap();
    assert_eq!(app.invoke("delete_vod", json!({ "id": used })).unwrap(), Value::Null);
    assert_eq!(listed_ids(&app), [other]);
    assert_eq!(order(&app), [LibraryItem::Edit(edit), LibraryItem::Vod(other)]);
    assert!(app.db().vod(used).unwrap().is_some_and(|v| !v.listed), "still there, a source only");
    let view = app.invoke("get_edit", json!({ "id": edit })).unwrap();
    assert_eq!(view["pieces"].as_array().map(Vec::len), Some(1), "the edit's piece stays: {view}");
}

/// A VOD's row says how much of it is transcribed: none, some or all, and the
/// stretches done say which part (CHANGELOG.md 0.6.0, "Adding a VOD doesn't
/// transcribe it": "One partly transcribed says how much (\"Transcribed 24:00
/// of 3:04:00\")"). The words are the page's (stretches.ts `requestDetail`);
/// the numbers are the app's.
#[test]
fn a_vod_s_row_says_how_much_is_transcribed() {
    const LENGTH: f64 = 11040.0; // 3:04:00
    let app = TestApp::new();
    let made = std::cell::Cell::new(0);
    let vod = || {
        made.set(made.get() + 1);
        app.add_video_file(&format!("{}.mp4", made.get()), LENGTH).id
    };
    // Stretches done on each VOD, as (start, end); no end is the VOD's.
    let done = |stretches: &[(f64, Option<f64>)]| {
        let id = vod();
        for &(start, end) in stretches {
            let stretch = app.db().request(id, start, end).unwrap().id;
            app.db().finish_stretch(stretch).unwrap();
        }
        id
    };
    let none = vod();
    let part = done(&[(0.0, Some(1440.0))]); // 24:00 of 3:04:00
    let all = done(&[(0.0, None)]);
    // Stretches next to each other, or half a second short of the end, are all of it.
    let joined = done(&[(0.0, Some(600.0)), (600.3, None)]);
    let near_end = done(&[(0.0, Some(LENGTH - 0.4))]);
    let gap = done(&[(0.0, Some(600.0)), (601.0, None)]);
    let short = done(&[(0.0, Some(LENGTH - 1.0))]);
    let transcribed = |id| row(&app, id).transcribed;
    assert_eq!(
        [none, part, all, joined, near_end, gap, short].map(transcribed),
        [
            Transcribed::None,
            Transcribed::Some,
            Transcribed::All,
            Transcribed::All,
            Transcribed::All,
            Transcribed::Some,
            Transcribed::Some
        ]
    );
    let stretches = app.invoke("transcript_stretches", json!({ "vodId": part })).unwrap();
    let stretches: Vec<(f64, Value)> =
        stretches.as_array().unwrap().iter().map(|s| (s["start"].as_f64().unwrap(), s["end"].clone())).collect();
    assert_eq!(stretches, [(0.0, json!(1440.0))], "24:00 of the row's {:?}", row(&app, part).info.duration);
    assert_eq!(row(&app, part).info.duration, Some(LENGTH));
}

/// Requests the app was in the middle of when it quit are waiting again after
/// a restart, and taken up: a download that was under way is made again, and a
/// transcription that was under way waits for the worker as one asked for
/// (db.rs `requeue_interrupted`; its doc: "A quit mid-stage: each VOD goes back
/// to wait for the stage it was in").
#[test]
fn interrupted_requests_are_waiting_again_after_a_restart() {
    let app = TestApp::new();
    let downloading = app.add_site_vod("youtube", "abc", "Downloading", 600.0).id;
    let transcribing = app.add_video_file("transcribing.mp4", 60.0).id;
    // The worker isn't running in a test, so this request stays where the test puts it.
    let stretch = app.db().request(transcribing, 0.0, None).unwrap().id;
    app.db().set_stretch_status(stretch, Status::Transcribing, 0.6, None).unwrap();
    let gate = Gate::new();
    app.yt_dlp.holds(&gate);
    ask(&app, downloading, 0.0, None);
    gate.reached();
    assert_eq!(row(&app, downloading).status, Status::Downloading);
    assert_eq!(row(&app, transcribing).status, Status::Transcribing);

    let app = TestApp::on(app.quit());
    // Ready: its download is done again, or its audio is there, and it waits
    // for the transcription's turn, which doesn't come in a test.
    eventually("both wait for their transcription", || {
        [downloading, transcribing].iter().all(|&id| row(&app, id).status == Status::Ready)
    });
}
