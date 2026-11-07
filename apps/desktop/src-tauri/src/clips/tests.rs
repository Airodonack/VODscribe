//! The specs of clips and their commands. Each test is named for what it
//! says the app does and cites where that's said.

use super::*;

/// A clip whose file is read from 0.7.1's clip folder (it couldn't be
/// linked in) leaves that file there when it goes; its own goes.
#[test]
fn a_clip_going_leaves_files_outside_the_library() {
    let root = std::env::temp_dir().join(format!("vodscribe-let-go-{}", std::process::id()));
    let paths = vodscribe_core::Paths::new(root.join("lib"));
    let old = root.join("Videos/VODscribe/A talk.mp4");
    let own = paths.clip_file(4, "mp4");
    for file in [&old, &own] {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"clip").unwrap();
    }
    let_go_files(&paths, 3, Some(&old.to_string_lossy()), false);
    assert!(old.is_file(), "0.7.1's file deleted");
    let_go_files(&paths, 4, Some(&own.to_string_lossy()), false);
    assert!(!own.exists());
    std::fs::remove_dir_all(&root).unwrap();
}

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

/// What a drag out of the window comes to, by what was seen of it: let go
/// on an app that took nothing while the files were still being made, it
/// takes finished files only (named, where the system says); one that
/// failed is said only once it's let go on another app; called off or
/// dropped on the app's own window, nothing.
#[test]
fn a_drag_ends_as_its_receiver_took_it() {
    use Ending::*;
    let named = || Some("Kdenlive".to_string());
    let blocked = || Some("blocked".to_string());
    let ended = |delivered, failed: Option<&str>, finished_only, receiver: Option<&str>| DragEnded {
        drag: None,
        delivered,
        failed: failed.map(String::from),
        finished_only,
        receiver: receiver.map(String::from),
        resolve: None,
    };
    assert_eq!(drag_outcome(Elsewhere, true, None, true, named()), ended(true, None, false, None), "given");
    assert_eq!(
        drag_outcome(Elsewhere, false, None, false, named()),
        ended(false, None, true, Some("Kdenlive")),
        "let go before it was made, nothing taken"
    );
    assert_eq!(drag_outcome(Elsewhere, false, None, false, None), ended(false, None, true, None), "name unknown");
    assert_eq!(drag_outcome(Elsewhere, false, None, true, named()), DragEnded::default(), "made, not taken");
    assert_eq!(drag_outcome(Elsewhere, false, blocked(), false, named()), ended(false, Some("blocked"), false, None));
    assert_eq!(drag_outcome(Nowhere, false, blocked(), false, None), ended(false, Some("blocked"), false, None));
    assert_eq!(drag_outcome(Nowhere, false, None, false, None), DragEnded::default(), "dropped on nothing");
    assert_eq!(drag_outcome(Elsewhere, false, Some(CANCELED.into()), false, named()), DragEnded::default());
    for ending in [CalledOff, Back] {
        for (given, failure) in [(true, None), (false, None), (false, blocked())] {
            assert_eq!(drag_outcome(ending, given, failure, false, named()), DragEnded::default(), "{ending:?}");
        }
    }
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

/// Several clips made at once went in one at a time: one that failed
/// part-way left those before it in the library, waiting to be made, with
/// no step to undo them.
#[test]
fn making_several_clips_is_all_or_none() {
    let (db, vod) = library();
    let history = history::History::default();
    assert!(make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (-1.0, 3.0)], true).is_err());
    assert!(db.clips(vod).unwrap().is_empty(), "none made");
    assert_eq!(history.steps(Doc::Vod(vod)), 0, "no step recorded");
    let made = make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (3.0, 90.0)], true).unwrap();
    assert_eq!(made.iter().map(|c| (c.start, c.end)).collect::<Vec<_>>(), [(1.0, 2.0), (3.0, 60.0)]);
    assert_eq!(history.steps(Doc::Vod(vod)), 1, "one step for both");

    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
    let doc = Doc::Edit(edit);
    let before = history.steps(doc);
    assert!(make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (40.0, 50.0)], true).is_err());
    assert!(db.bar_clips(edit).unwrap().is_empty(), "none made on the edit's bar");
    assert_eq!(history.steps(doc), before, "no step recorded");
    let (made, queued) = make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (3.0, 4.0)], true).unwrap();
    assert_eq!(made.iter().map(|c| c.id).collect::<Vec<_>>(), queued);
    assert_eq!(history.steps(doc), before + 1);
}

/// A drag's receiver asked for a clip's file (its name made beside it,
/// `handed_out`) as the clip was deleted: the name was linked after the
/// delete had looked for names to remove, and a full-size copy stayed in
/// the clips folder that nothing named or removed.
#[test]
fn a_clip_deleted_as_a_drag_hands_it_out_leaves_no_name_behind() {
    use std::sync::atomic::AtomicU64;
    static N: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "vodscribe-hand-out-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let paths = vodscribe_core::Paths::new(&root);
    std::fs::create_dir_all(&root).unwrap();
    let db = Db::open(&root.join("library.sqlite")).unwrap();
    let vod = db.add_vod(&info()).unwrap().0.id;
    let clip = db.add_clip(vod, 1.0, 2.0, "c", true).unwrap();
    let file = db.files().clip_file(clip.id, "mp4");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"clip").unwrap();
    db.set_clip_file(clip.id, 1.0, 2.0, ClipStatus::Ready, Some(&file.to_string_lossy()), None).unwrap();
    let db = Arc::new(Mutex::new(db));
    let asking = {
        let (db, paths) = (db.clone(), paths.clone());
        std::thread::spawn(move || hand_out_made(&db, &paths, clip.id, "c 0:01.mp4"))
    };
    // Deleted while the receiver's ask is under way, as `remove` does.
    std::thread::sleep(Duration::from_millis(100));
    {
        let db = db.lock().unwrap();
        let path = db.clip(clip.id).unwrap().and_then(|c| c.file_path);
        db.delete_clip(clip.id).unwrap();
        let_go_files(&paths, clip.id, path.as_deref(), false);
    }
    let given = asking.join().unwrap();
    let left: Vec<_> = std::fs::read_dir(paths.clip_names(clip.id)).into_iter().flatten().flatten().collect();
    assert!(left.is_empty(), "left in the clips folder: {left:?}");
    assert!(!paths.clip_names(clip.id).exists(), "its names' folder is left");
    // Given only if the ask came first; then it was deleted with the clip.
    if let Made::Ready(path) = given {
        assert!(!path.exists(), "handed out {path:?}, still there");
    }
    let _ = std::fs::remove_dir_all(&root);
}

fn info() -> vodscribe_core::fetch::VodInfo {
    vodscribe_core::fetch::VodInfo {
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
    }
}

fn library() -> (Db, i64) {
    let db = Db::open(Path::new(":memory:")).unwrap();
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

/// Deleting an edit took the clips on its clip bar out of the library
/// (the database's cascade) but left their files in the clips folder,
/// their cuts running and the page not told: they go as a VOD's do.
#[test]
fn deleting_an_edit_releases_its_bar_clips() {
    let (db, vod) = library();
    let of_vod = db.add_clip(vod, 1.0, 2.0, "v", true).unwrap().id;
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
    let a = db.add_edit_clip(edit, 1.0, 2.0, "a", true).unwrap().id;
    let b = db.add_edit_clip(edit, 3.0, 4.0, "b", true).unwrap();
    let file = db.files().clip_file(b.id, "mp4").to_string_lossy().into_owned();
    assert!(db.set_clip_file(b.id, b.start, b.end, ClipStatus::Ready, Some(&file), None).unwrap());
    let b = b.id;
    db.hand_out_clip(b).unwrap();
    let stopped = std::cell::RefCell::new(Vec::new());
    let stop = |id| stopped.borrow_mut().push(id);

    // Not an edit (a VOD's own clip): nothing goes, none stopped.
    assert!(crate::edits::delete_edit_in(&db, of_vod, stop).is_err());
    assert_eq!(db.bar_clips(edit).unwrap().len(), 2);
    assert!(stopped.borrow().is_empty());

    let gone = crate::edits::delete_edit_in(&db, edit, stop).unwrap();
    let clips: Vec<_> = gone.clips.iter().map(|(c, out)| (c.id, c.file_path.as_deref(), *out)).collect();
    assert_eq!(clips, [(a, None, false), (b, Some(&*file), true)], "its clips, b dragged out");
    assert_eq!(*stopped.borrow(), [a, b], "their cuts stopped");
    assert!(db.revision(edit).unwrap().is_none() && db.clip(a).unwrap().is_none() && db.clip(b).unwrap().is_none());
    assert_eq!(db.clips(vod).unwrap().len(), 1, "the VOD's own clip stays");
}

/// Several clips deleted at once went one by one, a clip that was gone
/// skipped: one that couldn't go, in the middle, left those before it
/// deleted with no step to undo them.
#[test]
fn deleting_several_clips_is_all_or_none() {
    let (db, vod) = library();
    let a = db.add_clip(vod, 1.0, 2.0, "a", true).unwrap().id;
    let b = db.add_clip(vod, 3.0, 4.0, "b", true).unwrap().id;
    let history = history::History::default();
    let stopped = std::cell::RefCell::new(Vec::new());
    let stop = |id| stopped.borrow_mut().push(id);
    assert!(delete_clips_in(&db, &history, &[a, 9999, b], stop).is_err());
    assert_eq!(db.clips(vod).unwrap().len(), 2, "every clip in place");
    assert_eq!(history.steps(Doc::Vod(vod)), 0, "no step recorded");
    assert!(stopped.borrow().is_empty(), "none stopped");

    let gone = delete_clips_in(&db, &history, &[b, a], stop).unwrap();
    assert_eq!(gone.iter().map(|(c, _)| c.id).collect::<Vec<_>>(), [b, a]);
    assert!(db.clips(vod).unwrap().is_empty());
    assert_eq!(history.steps(Doc::Vod(vod)), 1, "one step for both");
}

/// The page told an early release from a refused drag by the word
/// "released" in its error: a reworded sentence would have shown an
/// ordinary release, too soon, as an error.
#[test]
fn a_drag_refused_for_an_early_release_is_none_begun_not_an_error() {
    assert_eq!(began(Ok(())), Ok(true));
    assert_eq!(began(Err(ReleasedEarly.into())), Ok(false));
    assert_eq!(began(Err(anyhow::Error::new(ReleasedEarly).context("Not now."))), Ok(false), "said otherwise");
    assert_eq!(began(Err(anyhow::anyhow!("no view"))), Err("no view".into()));
    assert_eq!(
        began(Err(anyhow::anyhow!("The button was released before the drag could begin."))),
        Err("The button was released before the drag could begin.".into()),
        "only the type tells",
    );
}

// Specs at the commands, on the app as it runs (`testkit::TestApp`).

use serde_json::json;
use vodscribe_core::testkit::Gate;

use crate::testkit::TestApp;

/// A clip is its stretch of the VOD, made from the VOD's own file when it's
/// one on this computer (crates/core/src/clips.rs: clips "start and end on
/// the exact frame asked for"; this file's doc: "the VOD's file where it
/// plays from one here").
#[test]
fn a_clip_of_a_file_is_its_stretch_of_that_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    let (file, start) = media.cut_from.unwrap();
    assert_eq!(Some(file), vod.source_path);
    assert!((start - 10.0).abs() < 0.001, "starts at {start}");
    assert!((media.duration - 10.0).abs() < 0.001, "lasts {}", media.duration);
}

/// A clip is titled with the first words said in it, or failing that where
/// it starts (crates/core/src/clips.rs, `title`).
#[test]
fn a_clip_is_titled_with_what_is_said_in_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 120.0);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let said = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    let silent = app.add_clip(vod.id, "1:05", "1:10").unwrap();
    assert_eq!((said.title.as_str(), silent.title.as_str()), ("The Eagle has landed", "Clip at 1:05"));
    // Their files made, so nothing of this test runs on after it.
    app.made(said.id);
    app.made(silent.id);
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
