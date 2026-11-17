//! The specs of the list of jobs the library shows, at its commands and the
//! news it sends. The clips' part (the order they're made in, what Cancel
//! leaves of them, a drag's clips) is in clips/tests/queue.rs and is cited
//! here, not repeated:
//! - `the_list_of_jobs_shows_the_clip_being_made_and_those_waiting_by_name`
//! - `waiting_clips_take_the_order_dragged_in_the_list_of_jobs`
//! - `a_reorder_leaves_the_one_being_made_and_those_added_since_in_place`
//! - `a_waiting_clip_canceled_says_canceled_at_once`
//! - `a_clip_canceled_while_it_is_made_says_so`
//! - `a_canceled_edit_is_left_not_rendered`
//! - `a_clip_moved_while_it_waits_leaves_the_list_of_jobs_once_it_is_made`
//! - `a_clip_only_dragged_out_is_listed_only_once_let_go_on_another_app`
//! - `a_drag_clip_stays_listed_till_its_last_drag_out_is_over`
//!
//! The words the page puts to each job are `src/lib/jobs.test.mjs`'s.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use vodscribe_core::testkit::{Gate, Media, eventually};

use crate::testkit::{LetGo, Receiver, TestApp};

/// The list as the page first reads it (`list_jobs`).
fn listed(app: &TestApp) -> Vec<Value> {
    app.invoke("list_jobs", json!({})).unwrap().as_array().unwrap().clone()
}

/// A job's kind, id and label, and whether it's begun.
fn seen(jobs: &[Value]) -> Vec<(String, i64, String, bool)> {
    let job = |j: &Value| {
        (
            j["kind"].as_str().unwrap().to_string(),
            j["id"].as_i64().unwrap(),
            j["label"].as_str().unwrap().into(),
            j["running"] == true,
        )
    };
    jobs.iter().map(job).collect()
}

/// The kinds and ids in the list, in its order.
fn order(app: &TestApp) -> Vec<(String, i64)> {
    seen(&listed(app)).into_iter().map(|(kind, id, _, _)| (kind, id)).collect()
}

/// The last list the page was told (`jobs`).
fn told(app: &TestApp) -> Option<Value> {
    app.heard("jobs").pop()
}

/// A minute of YouTube VOD, "A talk", with one line said; the fake site serves it.
fn a_talk(app: &TestApp) -> vodscribe_core::db::Vod {
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 60.0);
    app.says(vod.id, 12.0, "The Eagle has landed");
    vod
}

fn remove(app: &TestApp, id: i64) {
    app.invoke("delete_vod", json!({ "id": id })).unwrap();
}

fn cancel(app: &TestApp, kind: &str, id: i64) {
    app.invoke("cancel_job", json!({ "kind": kind, "id": id })).unwrap();
}

/// Edit `id` asked to be rendered, as the editor's Render does.
fn render(app: &TestApp, id: i64) {
    app.invoke("render_edit_now", json!({ "id": id })).unwrap();
}

/// The list shows the edits being rendered and those waiting by their
/// names, as it does clips (CHANGELOG.md, 0.7.1: "A list shows what's being
/// made.": "the clips and edits being made … with each one's name").
#[test]
fn the_list_of_jobs_shows_the_edit_being_rendered_and_those_waiting_by_name() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (rendering, waiting) = (app.edit_of(&[(vod.id, 10.0, 20.0)]), app.edit_of(&[(vod.id, 30.0, 40.0)]));
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    render(&app, rendering);
    halfway.reached();
    render(&app, waiting);
    assert_eq!(
        seen(&listed(&app)),
        [("edit".into(), rendering, "Best of".into(), true), ("edit".into(), waiting, "Best of".into(), false)]
    );
    halfway.open();
}

/// The list says how far the job being made is, and nothing of those
/// waiting nor of one that has not said yet (CHANGELOG.md, 0.7.1: "A list
/// shows what's being made.": "how far the current one is"; app `job_list.rs`
/// `Job`: "How far it is, 0..1; None while that isn't known (waiting, or just
/// begun)"). ffmpeg says a quarter of the clip once the clock has moved a
/// quarter second; the cut is held at its end.
#[test]
fn the_list_of_jobs_says_how_far_the_one_being_made_is_and_nothing_of_those_waiting() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (quarter, ended) = (Gate::new(), Gate::new());
    app.ffmpeg.holds_at(0.25, &quarter);
    app.holds_cut_ends(&ended);
    let making = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    quarter.reached();
    let waiting = app.add_clip(vod.id, "0:30", "0:40").unwrap().id;
    let progress = || listed(&app).iter().map(|j| j["progress"].clone()).collect::<Vec<_>>();
    assert_eq!(progress(), [Value::Null, Value::Null], "begun, not said how far");
    app.clock().advance(Duration::from_millis(250));
    quarter.open();
    ended.reached();
    let jobs = listed(&app);
    assert_eq!(jobs.iter().map(|j| j["id"].as_i64().unwrap()).collect::<Vec<_>>(), [making, waiting]);
    assert_eq!(jobs[0]["progress"], json!(0.25));
    assert_eq!(jobs[1]["progress"], Value::Null);
    eventually("the page told how far it is", || told(&app) == Some(Value::Array(jobs.clone())));
    ended.open();
}

/// A job just begun is shown with no progress, not the one it came to the
/// time before when it's the same clip made again (app `job_list.rs` `Job`:
/// "How far it is, 0..1; None while that isn't known (waiting, or just
/// begun)"; `begun`: "no progress known yet").
#[test]
fn a_clip_made_again_is_shown_with_no_progress_till_it_says_how_far_it_is() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = Gate::new();
    app.ffmpeg.holds_at(0.25, &first);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    first.reached();
    // It says a quarter, once the clock has moved, and is made.
    app.clock().advance(Duration::from_millis(250));
    first.open();
    app.made(clip);
    // Made again at another range, and held before it says how far it is.
    let again = Gate::new();
    app.ffmpeg.holds_at(0.25, &again);
    app.invoke("set_clip_range", json!({ "id": clip, "start": 10, "end": 15 })).unwrap();
    again.reached();
    let jobs = listed(&app);
    assert_eq!(seen(&jobs), [("clip".into(), clip, "Clip at 0:10".into(), true)]);
    assert_eq!(jobs[0]["progress"], Value::Null);
    app.ffmpeg.works();
    again.open();
    app.made(clip);
}

/// How far a clip being made is along is what a drag out of the window shows
/// where it's dropped, for that clip only and only once it's said (app
/// `job_list.rs` `progress_of`: "How far (0..1) job `id` on the clips' queue
/// is along, if it's the one running and that's known"; app `clips.rs`
/// `DragFile`: "The job making it, whose progress macOS shows where it's
/// dropped"). At the function: no kit helper yet reads what a drag shows.
#[test]
fn how_far_the_clip_being_made_is_along_is_known_only_for_it_and_once_it_has_said() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (quarter, ended) = (Gate::new(), Gate::new());
    app.ffmpeg.holds_at(0.25, &quarter);
    app.holds_cut_ends(&ended);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    quarter.reached();
    let along = |id: i64| crate::job_list::progress_of(app.handle(), id);
    assert_eq!(along(clip), None, "begun, not said how far");
    app.clock().advance(Duration::from_millis(250));
    quarter.open();
    ended.reached();
    assert_eq!(along(clip), Some(0.25));
    assert_eq!(along(clip + 1), None, "another job's");
    ended.open();
}

/// A VOD being erased after it was removed is in the list by its title,
/// with how far its erasure is (CHANGELOG.md, 0.7.1: "A list shows what's
/// being made.": "the VODs being erased after you removed them"; app
/// `job_list.rs` `Kind::Erase`: "A removed VOD's transcript, chat and files,
/// erased").
#[test]
fn a_vod_being_erased_after_it_was_removed_is_listed_by_its_title_with_how_far_it_is() {
    let app = TestApp::new();
    let vod = a_talk(&app);
    let between = Gate::new();
    app.holds_erasures(&between);
    remove(&app, vod.id);
    between.reached();
    let jobs = listed(&app);
    assert_eq!(seen(&jobs), [("erase".into(), vod.id, "A talk".into(), true)]);
    let along = jobs[0]["progress"].as_f64().expect("it has said how far it is");
    assert!(0.0 < along && along < 1.0, "a part of it erased, its files not yet: {along}");
    between.open();
    app.erasures_done();
    assert_eq!(listed(&app), Vec::<Value>::new());
}

/// The list holds the clips and edits first, the one being made then those
/// waiting, then the erasures the same way, then the files being received
/// (app `job_list.rs`, module doc: "the clips' queue …, then the erasures of
/// removed VODs …, which run beside it, then the files dropped on the window
/// being received").
#[test]
fn the_list_of_jobs_holds_the_clips_first_then_the_erasures_then_the_files_being_received() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (first, second) = (a_talk(&app), app.add_site_vod("twitch", "2901646294", "Another talk", 60.0));
    app.says(second.id, 12.0, "One small step");
    let (between, halfway) = (Gate::new(), Gate::new());
    app.holds_erasures(&between);
    app.ffmpeg.holds_at(0.5, &halfway);
    // The erasures and a file received come first in time; the clips after.
    remove(&app, first.id);
    between.reached();
    remove(&app, second.id);
    let mut given = app.drop_files(&[("Day 4.mp4", Some(10))]);
    given[0].give(b"FAKE").unwrap();
    let making = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    halfway.reached();
    let waiting = app.add_clip(vod.id, "0:30", "0:40").unwrap().id;
    let received = listed(&app).last().unwrap()["id"].as_i64().unwrap();
    assert_eq!(
        seen(&listed(&app)),
        [
            ("clip".into(), making, "Clip at 0:10".into(), true),
            ("clip".into(), waiting, "Clip at 0:30".into(), false),
            ("erase".into(), first.id, "A talk".into(), true),
            ("erase".into(), second.id, "Another talk".into(), false),
            ("receive".into(), received, "Day 4.mp4".into(), true),
        ]
    );
    between.open();
    halfway.open();
    drop(given);
}

/// A file being received is in the list by its name, with how far it is
/// once its size is known (app `job_list.rs`, module doc: "the files dropped
/// on the window being received, each with how far it is"; app `receive.rs`
/// `Receives::listed`: "how far it is (0..1; None while that isn't known)").
#[test]
fn a_file_being_received_is_listed_by_its_name_with_how_far_it_is_once_its_size_is_known() {
    let app = TestApp::new();
    let media = Media::video(60.0).to_bytes();
    let mut known = app.drop_files(&[("Day 3.mp4", Some(media.len() as u64))]).remove(0);
    let mut unknown = app.drop_files(&[("Day 4.mp4", None)]).remove(0);
    let half = media.len() / 2;
    known.give(&media[..half]).unwrap();
    unknown.give(&media[..half]).unwrap();
    let jobs = listed(&app);
    let names: Vec<_> = seen(&jobs).into_iter().map(|(kind, _, label, running)| (kind, label, running)).collect();
    assert_eq!(names, [("receive".into(), "Day 3.mp4".into(), true), ("receive".into(), "Day 4.mp4".into(), true)]);
    let along = jobs[0]["progress"].as_f64().unwrap();
    assert!((along - half as f64 / media.len() as f64).abs() < 1e-9, "{along}");
    assert_eq!(jobs[1]["progress"], Value::Null);
    known.whole();
    unknown.whole();
    app.threads_done();
}

/// A clip that's gone while its cut winds down is out of the list at once
/// (app `job_list.rs` `arrange`: "those `describe` doesn't find left out
/// (gone meanwhile)").
#[test]
fn a_clip_deleted_while_it_is_made_is_out_of_the_list_at_once() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    halfway.reached();
    assert_eq!(order(&app), [("clip".to_string(), clip)]);
    app.invoke("delete_clip", json!({ "id": clip })).unwrap();
    assert_eq!(order(&app), Vec::<(String, i64)>::new(), "listed while its cut winds down");
    halfway.open();
}

/// A clip asked for again while it is made is made again after, and is one
/// job to see (app `job_list.rs` `arrange`: "each job once (one asked for
/// again while it runs is made again after: it's the same job to see)").
#[test]
fn a_clip_asked_for_again_while_it_is_made_is_listed_once() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    halfway.reached();
    app.invoke("set_clip_range", json!({ "id": clip, "start": 10, "end": 15 })).unwrap();
    assert_eq!(seen(&listed(&app)), [("clip".into(), clip, "Clip at 0:10".into(), true)]);
    app.ffmpeg.works();
    halfway.open();
    app.made(clip);
}

/// A job is sent as the page reads it: its kind in lower case, its id, its
/// name, whether it's begun and how far it is (src/lib/api.ts `Job`); the
/// page's Cancel names a kind in the same words (app `job_list.rs` `Kind`).
#[test]
fn a_job_is_sent_as_the_page_reads_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let (making, waiting) =
        (app.add_clip(vod.id, "0:10", "0:20").unwrap().id, app.add_clip(vod.id, "0:30", "0:40").unwrap().id);
    halfway.reached();
    assert_eq!(
        listed(&app),
        [
            json!({ "kind": "clip", "id": making, "label": "Clip at 0:10", "running": true, "progress": null }),
            json!({ "kind": "clip", "id": waiting, "label": "Clip at 0:30", "running": false, "progress": null }),
        ]
    );
    for kind in ["clip", "edit", "erase", "receive"] {
        assert_eq!(app.invoke("cancel_job", json!({ "kind": kind, "id": 4040 })), Ok(Value::Null), "{kind}");
    }
    assert!(app.invoke("cancel_job", json!({ "kind": "Clip", "id": making })).is_err(), "kinds are in lower case");
    halfway.open();
}

/// The page is sent the whole list each time it changes, and the list it
/// reads first is the one it's told (app `job_list.rs`, module doc: "Sent
/// whole as `jobs` whenever it changes"; `send_changes`: "Sends the list
/// each time word comes that it may have changed, if it has"). Once the last
/// job ends it's told the list is empty, which closes it.
#[test]
fn the_page_is_told_the_whole_list_each_time_it_changes_and_never_the_same_twice() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let making = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    halfway.reached();
    let waiting = app.add_clip(vod.id, "0:30", "0:40").unwrap().id;
    eventually("told the list as it is", || told(&app) == Some(Value::Array(listed(&app))));
    assert_eq!(seen(&listed(&app)).len(), 2);
    cancel(&app, "clip", waiting);
    eventually("told the waiting clip is out", || told(&app).is_some_and(|l| l.as_array().unwrap().len() == 1));
    // The clip being made, dragged out and let go on another app, changes
    // nothing the list shows: the page isn't told the list again.
    app.drag_out(&[making]).unwrap();
    app.drags().last().let_go(LetGo::On(Receiver::taking_copies("Finder")));
    app.ffmpeg.works();
    halfway.open();
    app.made(making);
    app.drag_ended();
    eventually("told the list is empty", || told(&app) == Some(json!([])));
    let all = app.heard("jobs");
    assert!(all.windows(2).all(|pair| pair[0] != pair[1]), "told the same list twice: {all:?}");
}

/// Waiting edits dragged into another order in the list of jobs are rendered
/// in that order (CHANGELOG.md, 0.7.1: "Waiting clips and edits can be
/// reordered.": "Drag a clip or edit that's waiting up or down the list to
/// have it made sooner or later"), and the list shows that order at once.
#[test]
fn waiting_edits_take_the_order_dragged_in_the_list_of_jobs() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edits = [10.0, 20.0, 30.0].map(|from| app.edit_of(&[(vod.id, from, from + 5.0)]));
    let [a, b, c] = edits;
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let making = app.add_clip(vod.id, "0:01", "0:02").unwrap().id;
    halfway.reached();
    for id in edits {
        render(&app, id);
    }
    app.invoke("reorder_jobs", json!({ "ids": [c, a, b] })).unwrap();
    let kind = "edit".to_string();
    assert_eq!(
        order(&app),
        [("clip".to_string(), making), (kind.clone(), c), (kind.clone(), a), (kind, b)],
        "the list shows the order dragged"
    );
    app.ffmpeg.works();
    halfway.open();
    let ready = || {
        let heard = app.heard("edit");
        let done = heard.iter().filter(|e| e["status"] == "ready").map(|e| e["id"].as_i64().unwrap());
        done.collect::<Vec<_>>()
    };
    eventually("all three rendered", || ready().len() == 3);
    assert_eq!(ready(), [c, a, b]);
}

/// The jobs waiting are the clips' and edits' only: a reorder naming an id
/// that is an erasure's or a file's leaves the clips' order as it was (app
/// `job_list.rs` `reorder_jobs`: "Puts the clips and edits waiting in the
/// order of `ids`").
#[test]
fn a_reorder_moves_only_the_clips_and_edits_waiting() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let removed = a_talk(&app);
    let (between, halfway) = (Gate::new(), Gate::new());
    app.holds_erasures(&between);
    app.ffmpeg.holds_at(0.5, &halfway);
    remove(&app, removed.id);
    between.reached();
    let making = app.add_clip(vod.id, "0:01", "0:02").unwrap().id;
    halfway.reached();
    let (a, b) = (app.add_clip(vod.id, "0:03", "0:04").unwrap().id, app.add_clip(vod.id, "0:05", "0:06").unwrap().id);
    // The erasure's id between two clips' ids, as a drag over the list could name it.
    app.invoke("reorder_jobs", json!({ "ids": [b, removed.id, a] })).unwrap();
    let clip = |id: i64| ("clip".to_string(), id);
    assert_eq!(order(&app), [clip(making), clip(b), clip(a), ("erase".to_string(), removed.id)]);
    between.open();
    halfway.open();
}

/// A canceled erasure stops, and what's left of the VOD is erased at the next
/// start (app `job_list.rs` `cancel_job`: "an erasure is stopped, what's left
/// of the VOD erased at the next start (`Sweeper::cancel`)"; tests/e2e/
/// scenarios/jobs.mjs: "at the next start what was left of it is erased").
#[test]
fn a_canceled_erasure_stops_and_what_is_left_is_erased_at_the_next_start() {
    let app = TestApp::new();
    let vod = a_talk(&app);
    let kept = app.paths().source_audio(vod.id, "m4a");
    std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
    std::fs::write(&kept, b"made").unwrap();
    let between = Gate::new();
    app.holds_erasures(&between);
    remove(&app, vod.id);
    between.reached();
    cancel(&app, "erase", vod.id);
    between.open();
    app.erasures_done();
    assert_eq!(listed(&app), Vec::<Value>::new(), "out of the list");
    assert!(kept.exists(), "its files were erased though it was stopped");
    let app = TestApp::on(app.quit());
    app.erasures_done();
    assert!(!kept.exists(), "what was left of it stayed at the next start");
}

/// An erasure waiting its turn that's canceled leaves the list and is erased
/// at the next start (app `job_list.rs` `cancel_job`: "an erasure is
/// stopped, what's left of the VOD erased at the next start").
#[test]
fn a_canceled_erasure_waiting_is_out_of_the_list_and_erased_at_the_next_start() {
    let app = TestApp::new();
    let (first, second) = (a_talk(&app), app.add_site_vod("twitch", "2901646294", "Another talk", 60.0));
    app.says(second.id, 12.0, "One small step");
    let kept = app.paths().source_audio(second.id, "m4a");
    std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
    std::fs::write(&kept, b"made").unwrap();
    let between = Gate::new();
    app.holds_erasures(&between);
    remove(&app, first.id);
    between.reached();
    remove(&app, second.id);
    assert_eq!(order(&app), [("erase".to_string(), first.id), ("erase".to_string(), second.id)]);
    cancel(&app, "erase", second.id);
    assert_eq!(order(&app), [("erase".to_string(), first.id)]);
    between.open();
    app.erasures_done();
    assert!(kept.exists(), "erased though it was canceled");
    let app = TestApp::on(app.quit());
    app.erasures_done();
    assert!(!kept.exists(), "what was left of it stayed at the next start");
}

/// A file being received that's canceled is off the list at once, stops at
/// its next bytes, and what came of it goes (app `job_list.rs` `cancel_job`:
/// "a file being received stops, and what came of it goes
/// (`Receives::cancel`)").
#[test]
fn a_canceled_file_being_received_leaves_the_list_at_once_and_what_came_of_it_goes() {
    let app = TestApp::new();
    let media = Media::video(60.0).to_bytes();
    let mut giving = app.drop_files(&[("Day 3.mp4", Some(media.len() as u64))]).remove(0);
    giving.give(&media[..10]).unwrap();
    let id = listed(&app)[0]["id"].as_i64().unwrap();
    let incoming = || std::fs::read_dir(app.paths().received_incoming()).unwrap().count();
    assert_ne!(incoming(), 0, "something written as it comes");
    cancel(&app, "receive", id);
    assert_eq!(listed(&app), Vec::<Value>::new(), "listed after its cancel");
    eventually("the page told it's gone", || told(&app) == Some(json!([])));
    assert!(giving.give(&media[10..]).is_err(), "it took the rest");
    eventually("what came of it gone", || incoming() == 0);
    app.threads_done();
    let kept: Vec<PathBuf> =
        app.db().vods().unwrap().into_iter().filter_map(|v| v.source_path.map(PathBuf::from)).collect();
    assert_eq!(kept, Vec::<PathBuf>::new(), "added to the library");
}
