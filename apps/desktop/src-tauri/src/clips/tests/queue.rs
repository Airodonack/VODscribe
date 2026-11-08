use super::*;

use serde_json::json;
use vodscribe_core::testkit::Gate;

use crate::testkit::TestApp;

/// A clip made only to be dragged out was queued last, behind every clip
/// and edit render waiting, and the drag waited for all of them.
#[test]
fn a_clip_made_for_a_drag_out_is_made_next() {
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

/// A clip made only for a drag out of the window showed in the list of
/// jobs while the drag was still in the window, where nothing had been let
/// go yet: it's listed once it's let go on another app, until it's over.
#[test]
fn a_drag_clip_is_listed_with_the_jobs_only_once_let_go_elsewhere() {
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

/// The same clip dragged out again while the first drag still hands it
/// over (let go on a file manager that copies it as it's made): the
/// first's end left the second's making out of the list of jobs.
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

/// A clip on an edit's clip bar of a VOD taken out of the library stays,
/// as the edit's pieces of it do: its cut went on stopped, and was never
/// queued again (only the VOD's own bar is), so it showed being cut until
/// the app next started.
#[test]
fn taking_a_vod_out_stops_only_the_clips_of_its_own_bar() {
    let (db, vod) = library();
    let of_vod = db.add_clip(vod, 1.0, 2.0, "v", true).unwrap().id;
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
    let of_edit = db.add_edit_clip(edit, 1.0, 2.0, "e", true).unwrap().id;
    assert!(on_bar_of(&db, of_vod, vod), "a clip of the VOD's bar");
    assert!(!on_bar_of(&db, of_edit, vod), "a clip of the edit's bar");
    assert!(!on_bar_of(&db, edit, vod), "the edit");
}

/// A clip canceled while it's being made says so on the clip bar and has no
/// file (CHANGELOG.md, 0.7.1: "A canceled clip or edit says so.").
#[test]
fn a_clip_canceled_while_it_is_made_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    app.invoke("cancel_job", json!({ "kind": "clip", "id": clip.id })).unwrap();
    halfway.open();
    let clip = app.made(clip.id);
    assert_eq!((clip.status, clip.error.as_deref()), (ClipStatus::Error, Some(CANCELED)));
    assert_eq!(clip.file_path, None);
}
