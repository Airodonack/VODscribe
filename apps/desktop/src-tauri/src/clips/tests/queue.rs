use super::*;

use serde_json::{Value, json};
use vodscribe_core::testkit::Gate;

use crate::testkit::{TestApp, at};

/// ffmpeg held halfway through each clip of VOD `vod` it cuts, at a gate of
/// its own: the test sees which clip is being made, then lets it go, and the
/// next is held in turn.
struct Held<'a> {
    app: &'a TestApp,
    vod: i64,
    gate: Gate,
}

impl<'a> Held<'a> {
    fn new(app: &'a TestApp, vod: i64) -> Self {
        let gate = Gate::new();
        app.ffmpeg.holds_at(0.5, &gate);
        Held { app, vod, gate }
    }

    /// Waits till a clip is held, and gives its id: the one clip of the VOD
    /// being made, as none other is.
    fn now(&self) -> i64 {
        self.gate.reached();
        let cutting: Vec<i64> =
            self.app.clips(self.vod).iter().filter(|c| c.status == ClipStatus::Cutting).map(|c| c.id).collect();
        assert_eq!(cutting.len(), 1, "one clip made at a time, but these are: {cutting:?}");
        cutting[0]
    }

    /// Lets the clip held be made; the next is held at a gate of its own.
    fn let_go(&mut self) {
        let next = Gate::new();
        self.app.ffmpeg.holds_at(0.5, &next);
        std::mem::replace(&mut self.gate, next).open();
    }

    /// The next `n` clips made, in the order they're made, each held, then
    /// let go.
    fn order(&mut self, n: usize) -> Vec<i64> {
        (0..n)
            .map(|_| {
                let id = self.now();
                self.let_go();
                id
            })
            .collect()
    }

    /// Lets everything be made from now, held no more.
    fn done(self) {
        self.app.ffmpeg.works();
        self.gate.open();
    }
}

/// A clip of VOD `vod_id` made only for a drag out of the window (not in its
/// clip bar), as the page asks for one (dragout.ts, `make(false)`).
fn add_drag_clip(app: &TestApp, vod_id: i64, from: &str, to: &str) -> Clip {
    app.run(crate::clips::add_clip(app.handle().clone(), vod_id, at(from), at(to), Some(false))).unwrap()
}

/// Clips of VOD `vod_id`, one for each range, asked for one after another.
fn add_clips<const N: usize>(app: &TestApp, vod_id: i64, ranges: [(&str, &str); N]) -> [i64; N] {
    ranges.map(|(from, to)| app.add_clip(vod_id, from, to).unwrap().id)
}

/// The list of jobs as the page first reads it (`list_jobs`): each job's
/// id, name and whether it's the one being made.
fn jobs(app: &TestApp) -> Vec<(i64, String, bool)> {
    let listed = app.invoke("list_jobs", json!({})).unwrap();
    let job = |j: &Value| (j["id"].as_i64().unwrap(), j["label"].as_str().unwrap().to_string(), j["running"] == true);
    listed.as_array().unwrap().iter().map(job).collect()
}

/// The ids in the list of jobs, in its order.
fn job_ids(app: &TestApp) -> Vec<i64> {
    jobs(app).into_iter().map(|(id, _, _)| id).collect()
}

/// Cancel, on job `id` of `kind` ("clip", "edit") in the list of jobs.
fn cancel(app: &TestApp, kind: &str, id: i64) {
    app.invoke("cancel_job", json!({ "kind": kind, "id": id })).unwrap();
}

/// A new edit of `start`..`end` seconds of VOD `vod_id`, as the editor makes
/// one; its id.
fn add_edit(app: &TestApp, vod_id: i64, start: f64, end: f64) -> i64 {
    let id = app.invoke("create_edit", json!({})).unwrap()["id"].as_i64().unwrap();
    let piece = json!({ "editId": id, "vodId": vod_id, "start": start, "end": end });
    app.invoke("add_range_to_edit", piece).unwrap();
    id
}

/// Clips asked for one after another are made one at a time, each once the
/// one before is done, in the order they were asked for (app clips.rs:1:
/// "the queue that cuts their files, one at a time"; tests/e2e/scenarios/
/// jobs.mjs: "three clips of all of a long file asked for at once, one made
/// at a time").
#[test]
fn clips_are_made_one_at_a_time_in_the_order_asked() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let mut held = Held::new(&app, vod.id);
    let asked = add_clips(&app, vod.id, [("0:10", "0:20"), ("0:30", "0:40"), ("0:05", "0:08")]);
    assert_eq!(held.order(3), asked);
    held.done();
    for id in asked {
        assert_eq!(app.made(id).status, ClipStatus::Ready);
    }
}

/// A clip made only to be dragged out of the window is made next, ahead of
/// the clip bar's clips waiting (app clips.rs:171, `queue_made`: "made only
/// to be dragged out of the window, first, ahead of those waiting"; 0.7.1:
/// "Clips start as you pick them up.": "first in line").
#[test]
fn a_clip_made_for_a_drag_out_is_made_next_ahead_of_those_waiting() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let mut held = Held::new(&app, vod.id);
    let making = app.add_clip(vod.id, "0:01", "0:02").unwrap().id;
    held.now();
    let [a, b] = add_clips(&app, vod.id, [("0:03", "0:04"), ("0:05", "0:06")]);
    let dragged = add_drag_clip(&app, vod.id, "0:10", "0:20").id;
    assert_eq!(held.order(4), [making, dragged, a, b]);
    held.done();
}

/// Several clips made for one drag out of the window go first, in their
/// order (app clips.rs:171, `queue_made`: "first, ahead of those waiting,
/// in their order, since the drag waits for their files"). At the queue
/// itself: at the commands, a drag's clips are put in their order by
/// `drag_clip`, which waits for the kit's drag.
#[test]
fn several_clips_for_one_drag_are_made_in_their_order() {
    let cutter = Cutter::new();
    let (began, order) = mpsc::channel();
    let (go, wait) = mpsc::channel::<()>();
    let wait = Mutex::new(wait);
    cutter
        .jobs
        .run("cutter-test", move |id, _| {
            began.send(id).unwrap();
            if id == 1 {
                wait.lock().unwrap().recv().unwrap();
            }
        })
        .unwrap();
    cutter.queue_made(&[1], true);
    assert_eq!(order.recv_timeout(Duration::from_secs(5)), Ok(1));
    // While 1 is being made: two for the clip bar, then two for a drag.
    cutter.queue_made(&[2, 3], true);
    cutter.queue_made(&[8, 9], false);
    go.send(()).unwrap();
    let next: Vec<_> = (0..4).map(|_| order.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
    assert_eq!(next, [8, 9, 2, 3], "the drag's in the order picked, then the rest as they were");
}

/// A drag's clip put first doesn't stop the clip being made: that one is
/// made to its end, and the drag's after it (app clips.rs:171,
/// `queue_made`: "the one being made goes on: one encode at a time").
#[test]
fn the_clip_being_made_goes_on_while_a_drag_s_clips_are_put_first() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let mut held = Held::new(&app, vod.id);
    let making = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    held.now();
    let waiting = app.add_clip(vod.id, "0:30", "0:40").unwrap().id;
    let dragged = add_drag_clip(&app, vod.id, "0:01", "0:05").id;
    assert_eq!(held.order(3), [making, dragged, waiting]);
    held.done();
    let made = app.made(making);
    assert!((app.clip_media(&made).duration - 10.0).abs() < 0.001, "made whole");
}

/// Waiting clips dragged into another order in the list of jobs are made in
/// that order (app clips.rs:255, `reorder`; 0.7.1: "Waiting clips and edits
/// can be reordered.": "to have it made sooner or later"; jobs.mjs: "the
/// queue takes that order: it's the one made next").
#[test]
fn waiting_clips_take_the_order_dragged_in_the_list_of_jobs() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let mut held = Held::new(&app, vod.id);
    let making = app.add_clip(vod.id, "0:01", "0:02").unwrap().id;
    held.now();
    let [a, b, c] = add_clips(&app, vod.id, [("0:03", "0:04"), ("0:05", "0:06"), ("0:07", "0:08")]);
    app.invoke("reorder_jobs", json!({ "ids": [c, a, b] })).unwrap();
    assert_eq!(held.order(4), [making, c, a, b]);
    held.done();
}

/// A reorder names the clips waiting as the page last listed them: the one
/// being made stays the one being made, and one put in the queue since (a
/// drag's, put first) keeps its place (apps/desktop/src-tauri/src/jobs.rs:132,
/// `reorder`: "the others (added since the order was read) keep theirs. The
/// one running is begun, and stays").
#[test]
fn a_reorder_leaves_the_one_being_made_and_those_added_since_in_place() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let mut held = Held::new(&app, vod.id);
    let making = app.add_clip(vod.id, "0:01", "0:02").unwrap().id;
    held.now();
    let [a, b, c] = add_clips(&app, vod.id, [("0:03", "0:04"), ("0:05", "0:06"), ("0:07", "0:08")]);
    // The page reads the list; a drag's clip is put first before the order
    // it sends comes in.
    let read: Vec<i64> = jobs(&app).into_iter().filter(|(_, _, running)| !running).map(|(id, _, _)| id).collect();
    assert_eq!(read, [a, b, c]);
    let dragged = add_drag_clip(&app, vod.id, "0:10", "0:20").id;
    app.invoke("reorder_jobs", json!({ "ids": [c, b, a] })).unwrap();
    assert_eq!(held.order(5), [making, dragged, c, b, a]);
    held.done();
}

/// The list of jobs shows the clip being made and those waiting, each by its
/// name (0.7.1: "A list shows what's being made.": "the clips and edits
/// being made … with each one's name"; jobs.mjs: "show there with their
/// names, the first being made, the other two waiting").
#[test]
fn the_list_of_jobs_shows_the_clip_being_made_and_those_waiting_by_name() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.says(vod.id, 11.0, "The Eagle has landed");
    app.says(vod.id, 31.0, "One small step");
    let held = Held::new(&app, vod.id);
    let landed = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    held.now();
    let [step, quiet] = add_clips(&app, vod.id, [("0:30", "0:40"), ("0:50", "0:55")]);
    assert_eq!(
        jobs(&app),
        [
            (landed, "The Eagle has landed".into(), true),
            (step, "One small step".into(), false),
            (quiet, "Clip at 0:50".into(), false),
        ]
    );
    held.done();
}

/// A waiting clip canceled from the list of jobs says it was canceled at
/// once, while the one before it is still being made, and leaves the list
/// (app clips.rs:237, `cancel_job`: "A clip then shows it was canceled … at
/// once if it was waiting"; 0.7.1: "A canceled clip or edit says so.";
/// jobs.mjs: "the fourth's Cancel takes it out").
#[test]
fn a_waiting_clip_canceled_says_canceled_at_once() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let held = Held::new(&app, vod.id);
    let making = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    held.now();
    let waiting = app.add_clip(vod.id, "0:30", "0:40").unwrap().id;
    cancel(&app, "clip", waiting);
    let clip = app.clip(waiting).unwrap();
    assert_eq!((clip.status, clip.error.as_deref()), (ClipStatus::Error, Some(CANCELED)));
    let said = app.heard("clip").into_iter().rfind(|c| c["id"] == waiting);
    assert_eq!(said.map(|c| c["error"].clone()), Some(json!(CANCELED)), "the clip bar told");
    assert_eq!(job_ids(&app), [making]);
    assert_eq!(held.now(), making, "the one before still being made");
    held.done();
}

/// A clip canceled while it's being made says so on the clip bar and has no
/// file (app clips.rs:237, `cancel_job`: "once its job has ended if it was
/// running"; CHANGELOG.md, 0.7.1: "A canceled clip or edit says so.").
#[test]
fn a_clip_canceled_while_it_is_made_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    cancel(&app, "clip", clip.id);
    halfway.open();
    let clip = app.made(clip.id);
    assert_eq!((clip.status, clip.error.as_deref()), (ClipStatus::Error, Some(CANCELED)));
    assert_eq!(clip.file_path, None);
}

/// A canceled clip is made once it's asked for again at its own range, as
/// Clip on its stretch does (app clips.rs:293, `settle_canceled`: "so it
/// isn't made again at the next start and Clip again makes it"; 0.7.1: "A
/// canceled clip or edit says so.": "clip that stretch again to make it";
/// clips.svelte.ts:236, "One that failed is tried again": `setClipRange` at
/// its own range).
#[test]
fn a_canceled_clip_moved_to_its_own_range_is_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let held = Held::new(&app, vod.id);
    let making = app.add_clip(vod.id, "0:10", "0:20").unwrap().id;
    held.now();
    let canceled = app.add_clip(vod.id, "0:30", "0:40").unwrap();
    cancel(&app, "clip", canceled.id);
    held.done();
    app.made(making);
    assert_eq!(app.clip(canceled.id).unwrap().status, ClipStatus::Error, "not made meanwhile");
    let range = json!({ "id": canceled.id, "start": canceled.start, "end": canceled.end });
    app.invoke("set_clip_range", range).unwrap();
    let clip = app.made(canceled.id);
    assert_eq!((clip.status, clip.error.as_deref()), (ClipStatus::Ready, None));
    assert!((app.clip_media(&clip).duration - 10.0).abs() < 0.001);
}

/// An edit canceled from the list of jobs is left not rendered: one being
/// rendered once its render has stopped, one waiting at once (app
/// clips.rs:293, `settle_canceled`: "a listed edit as before it was asked
/// for, not rendered (as Stop render leaves it)"; 0.7.1: "A canceled clip or
/// edit says so.": "a canceled edit is left not rendered").
#[test]
fn a_canceled_edit_is_left_not_rendered() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (rendering, waiting) = (add_edit(&app, vod.id, 10.0, 15.0), add_edit(&app, vod.id, 20.0, 25.0));
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    app.invoke("render_edit_now", json!({ "id": rendering })).unwrap();
    halfway.reached();
    app.invoke("render_edit_now", json!({ "id": waiting })).unwrap();
    cancel(&app, "edit", waiting);
    cancel(&app, "edit", rendering);
    app.ffmpeg.works();
    halfway.open();
    // Asked for after both: once it's made, the queue is past them.
    app.make_clip(vod.id, "0:50", "0:55");
    let edits = app.invoke("list_edits", json!({})).unwrap();
    let rendered = |id: i64| {
        let edit = edits.as_array().unwrap().iter().find(|e| e["id"] == id).unwrap();
        (edit["status"].clone(), edit["file_path"].clone())
    };
    assert_eq!(rendered(rendering), (json!("waiting"), Value::Null), "the one being rendered");
    assert_eq!(rendered(waiting), (json!("waiting"), Value::Null), "the one waiting");
}

/// Removing a VOD stops the clip of it being made and makes none of its
/// others, which leave no file behind; another VOD's clip waiting is still
/// made (app clips.rs:212, `stop_vod`: "Stops the cut of a clip of VOD
/// `vod_id`, if one's being cut, and cuts none of its others"; 0.4.1 Fixes:
/// "Removing a VOD while its clips waited to be made could still download
/// them, leaving their files behind.").
#[test]
fn removing_a_vod_stops_its_clip_being_made_and_makes_none_of_its_others() {
    let app = TestApp::new();
    let removed = app.add_video_file("Day 3.mp4", 60.0);
    let kept = app.add_video_file("Day 4.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    app.add_clip(removed.id, "0:10", "0:20").unwrap();
    halfway.reached();
    add_clips(&app, removed.id, [("0:30", "0:40"), ("0:45", "0:50")]);
    let other = app.add_clip(kept.id, "0:01", "0:05").unwrap().id;
    app.invoke("delete_vod", json!({ "id": removed.id })).unwrap();
    app.ffmpeg.works();
    halfway.open();
    let other = app.made(other);
    assert_eq!(other.status, ClipStatus::Ready);
    assert!(app.clips(removed.id).is_empty(), "its clips gone");
    let files: Vec<PathBuf> = std::fs::read_dir(app.paths().clips()).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(files, [PathBuf::from(other.file_path.unwrap())], "only the other VOD's clip has a file");
}

/// A clip on an edit's clip bar, being made as the VOD it's cut from is
/// removed, is still made: it isn't the VOD's own bar's (app clips.rs:553,
/// `on_bar_of`: "edits keep their pieces of it, and their clip bars' clips
/// too"; 0.7.0 Fixes: "Clips finish when their VOD is removed.").
#[test]
fn a_clip_of_an_edit_s_bar_is_still_made_when_its_vod_is_removed() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = add_edit(&app, vod.id, 0.0, 30.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.run(crate::clips::add_edit_clip(app.handle().clone(), edit, 5.0, 15.0, None)).unwrap();
    halfway.reached();
    app.invoke("delete_vod", json!({ "id": vod.id })).unwrap();
    halfway.open();
    let clip = app.made(clip.id);
    assert_eq!((clip.status, clip.error.as_deref()), (ClipStatus::Ready, None));
    assert!((app.clip_media(&clip).duration - 10.0).abs() < 0.001);
}

/// A clip moved while it waits is made once, at its new range, and then
/// leaves the list of jobs, which shows what's being made and what waits
/// (0.7.1: "A list shows what's being made.": "the clips and edits being
/// made"; jobs.mjs: "show there with their names, the first being made,
/// the other two waiting"). The plan's disagreement 6: `set_clip_range`
/// (app clips.rs:716) queued it again with `queue`, not `queue_once`.
#[test]
fn a_clip_moved_while_it_waits_leaves_the_list_of_jobs_once_it_is_made() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let mut held = Held::new(&app, vod.id);
    let making = app.add_clip(vod.id, "0:01", "0:02").unwrap().id;
    held.now();
    let [moved, after] = add_clips(&app, vod.id, [("0:10", "0:20"), ("0:30", "0:40")]);
    app.invoke("set_clip_range", json!({ "id": moved, "start": 10, "end": 15 })).unwrap();
    assert_eq!(held.order(2), [making, moved]);
    assert_eq!(held.now(), after);
    assert_eq!(job_ids(&app), [after], "the clip moved is made, and listed no more");
    held.done();
    let made = app.made(moved);
    assert!((app.clip_media(&made).duration - 5.0).abs() < 0.001, "made at its new range");
}

/// A clip made only for a drag out of the window shows in the list of jobs
/// only once it's let go on another app, till that drag is over
/// (apps/desktop/src-tauri/src/job_list.rs:186, `describe`: "a clip made
/// only for a drag out of the window that's still in the window's drag …
/// is seen only once it's been let go there"; 0.7.1: "Dropped clips show in
/// the jobs list."). Below the commands until the kit has a drag.
#[test]
fn a_clip_only_dragged_out_is_listed_only_once_let_go_on_another_app() {
    let (db, vod) = library();
    let hidden = db.add_clip(vod, 1.0, 2.0, "dragged", false).unwrap().id;
    let pill = db.add_clip(vod, 3.0, 4.0, "pill", true).unwrap().id;
    let cutter = Cutter::new();
    let listed = |id| crate::job_list::describe(&db, crate::job_list::Queue::Render, id, |id| cutter.is_out(id));
    assert!(listed(hidden).is_none(), "listed while in the window's drag");
    assert!(listed(pill).is_some(), "a pill's making");
    cutter.dragged_out(&[hidden], true);
    assert_eq!(listed(hidden).map(|(_, label)| label).as_deref(), Some("dragged"));
    cutter.dragged_out(&[hidden], false);
    assert!(listed(hidden).is_none(), "listed once the drag was over");
}

/// The same clip dragged out again while the first drag still hands it over
/// (let go on a file manager that copies it as it's made) stays listed till
/// the last of them is over (app clips.rs:52, `Cutter::out`: "the same clip
/// can be dragged out again while the first still hands it over"). Below
/// the commands until the kit has a drag.
#[test]
fn a_drag_clip_stays_listed_till_its_last_drag_out_is_over() {
    let (db, vod) = library();
    let hidden = db.add_clip(vod, 1.0, 2.0, "dragged", false).unwrap().id;
    let cutter = Cutter::new();
    let listed = |id| crate::job_list::describe(&db, crate::job_list::Queue::Render, id, |id| cutter.is_out(id));
    cutter.dragged_out(&[hidden], true);
    cutter.dragged_out(&[hidden], true);
    cutter.dragged_out(&[hidden], false);
    assert!(listed(hidden).is_some(), "left out as the first drag ended, the second still handing it over");
    cutter.dragged_out(&[hidden], false);
    assert!(listed(hidden).is_none(), "listed once both were over");
}
