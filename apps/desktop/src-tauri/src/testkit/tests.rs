//! The app kit's own specs: the app starts on its fakes, and again on the
//! library of one that quit or was killed; its commands are reached as the
//! page reaches them; what it says and asks is heard; and a drag out of the
//! window ends where the test lets it go.

use serde_json::json;

use super::*;

#[test]
fn the_app_starts_on_an_empty_library_of_its_own() {
    let app = TestApp::new();
    assert!(app.db().vods().unwrap().is_empty());
    assert!(app.paths().root.starts_with(std::env::temp_dir()));
    assert_eq!(app.invoke("list_vods", json!({})), Ok(json!([])));
}

#[test]
fn commands_are_reached_by_name_with_the_pages_arguments() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    assert_eq!(app.invoke("list_clips", json!({ "vodId": vod.id })), Ok(json!([])));
    let refused = app.invoke("add_clip", json!({ "vodId": vod.id, "start": 20.0, "end": 10.0 }));
    assert!(refused.is_err(), "{refused:?}");
    assert!(app.invoke("list_clips", json!({ "vod": vod.id })).is_err(), "an argument by another name");
}

#[test]
fn a_clip_made_through_the_kit_is_cut_by_the_fake_ffmpeg() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    assert_eq!(media.cut_from.map(|(file, _)| file), vod.source_path);
    assert!(app.heard("clip").iter().any(|c| c["id"] == clip.id && c["status"] == "ready"), "{:?}", app.heard("clip"));
}

#[test]
fn the_desktop_hears_what_its_shown() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.invoke("show_clip", json!({ "id": clip.id })).unwrap();
    let shown = app.desktop().shown();
    assert_eq!(shown.len(), 1);
    assert_eq!(Media::read(&shown[0]), Some(app.clip_media(&clip)), "the clip's file, under its name");
}

#[test]
fn times_are_read_as_a_clock_shows_them() {
    assert_eq!(at("0:10"), 10.0);
    assert_eq!(at("1:02:03.5"), 3723.5);
    assert_eq!(at("75"), 75.0);
}

#[test]
fn a_clip_of_a_sites_vod_is_cut_from_footage_the_fake_site_serves() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "The test video", 600.0);
    let clip = app.make_clip(vod.id, "1:00", "1:10");
    let (from, start) = app.clip_media(&clip).cut_from.unwrap();
    assert_eq!(from, vod.info.url);
    assert!((start - 60.0).abs() < 0.01, "{start}");
    assert_eq!(app.clips(vod.id).iter().map(|c| c.id).collect::<Vec<_>>(), [clip.id]);
}

#[test]
fn the_desktop_hears_what_its_asked_to_open() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.invoke("open_file", json!({ "id": vod.id })).unwrap();
    assert_eq!(app.desktop().opened(), [std::path::PathBuf::from(vod.source_path.unwrap())]);
}

/// A test that fails while a fake is held at a gate it never opens ends,
/// failed, rather than waiting on the job its app is closing.
#[test]
fn a_test_failing_with_a_fake_held_ends() {
    struct Ended(std::sync::mpsc::Sender<()>);
    impl Drop for Ended {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let (ended, done) = std::sync::mpsc::channel();
    let failing = std::thread::spawn(move || {
        // Dropped last, once the app is.
        let _ended = Ended(ended);
        let app = TestApp::new();
        let vod = app.add_video_file("Day 3.mp4", 60.0);
        let halfway = vodscribe_core::testkit::Gate::new();
        app.ffmpeg.holds_at(0.5, &halfway);
        app.add_clip(vod.id, "0:10", "0:20").unwrap();
        halfway.reached();
        panic!("a test failing while ffmpeg is held");
    });
    assert!(done.recv_timeout(std::time::Duration::from_secs(10)).is_ok(), "the app's end hung");
    assert!(failing.join().is_err(), "the test failed");
}

/// An app started again on the folder of one that quit has its library as
/// it was: its VODs, its clips and their files.
#[test]
fn a_restarted_app_has_its_library() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    let root = app.paths().root;
    let app = TestApp::on(app.quit());
    assert_eq!(app.paths().root, root);
    assert_eq!(app.db().vods().unwrap().iter().map(|v| v.id).collect::<Vec<_>>(), [vod.id]);
    let again = app.clip(clip.id).unwrap();
    assert_eq!((again.start, again.end, again.status), (clip.start, clip.end, ClipStatus::Ready));
    assert_eq!(app.clip_media(&again), media);
}

/// Clips left unmade when the app stopped, one being made and one waiting,
/// are made when it starts again, whether it quit or was killed
/// (clips.rs `Cutter::run`: "beginning with the clips left unfinished when
/// the app last stopped").
#[test]
fn clips_left_unmade_are_made_when_the_app_starts_again() {
    for end in [TestApp::quit, TestApp::kill] {
        let app = TestApp::new();
        let vod = app.add_video_file("Day 3.mp4", 60.0);
        let halfway = vodscribe_core::testkit::Gate::new();
        app.ffmpeg.holds_at(0.5, &halfway);
        let cutting = app.add_clip(vod.id, "0:10", "0:20").unwrap();
        let waiting = app.add_clip(vod.id, "0:30", "0:40").unwrap();
        halfway.reached();
        let app = TestApp::on(end(app));
        for clip in [cutting, waiting] {
            let made = app.made(clip.id);
            let (_, from) = app.clip_media(&made).cut_from.unwrap();
            assert!((from - clip.start).abs() < 0.01, "clip {}: cut from {from}", clip.id);
        }
    }
}

/// A made clip dragged out and let go on another app is given to it: its
/// file, under the name the drag offered.
#[test]
fn a_drag_let_go_on_an_app_hands_it_the_clips_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    assert_eq!(app.drag_out(&[clip.id]), Ok(json!(true)));
    let drag = app.drags().last();
    drag.let_go(LetGo::On(Receiver::taking_files("Kdenlive")));
    let ended = app.drag_ended();
    assert_eq!((ended["drag"].clone(), ended["delivered"].clone()), (json!(1), json!(true)), "{ended}");
    let given = drag.given();
    assert_eq!(given.len(), 1);
    assert_eq!(given[0].file_name().unwrap().to_string_lossy(), drag.names()[0]);
    assert_eq!(Media::read(&given[0]), Some(app.clip_media(&clip)));
    assert_eq!(drag.file_types(), ["public.mpeg-4"]);
    // With no picture from the page, the clip's own.
    assert_eq!(drag.image(), std::fs::read(crate::clips::thumb_path(app.handle(), clip.id)).unwrap());
}

/// A drag called off (Escape), or brought back to the window, gives
/// nothing, and the page hears only that it's over.
#[test]
fn a_drag_called_off_or_brought_back_gives_nothing() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    for to in [LetGo::CalledOff, LetGo::Back] {
        app.drag_out(&[clip.id]).unwrap();
        let drag = app.drags().last();
        drag.let_go(to);
        let ended = app.drag_ended();
        assert_eq!((ended["delivered"].clone(), ended["failed"].clone()), (json!(false), Value::Null), "{ended}");
        assert!(drag.given().is_empty());
    }
}

/// A drag let go on nothing gives nothing.
#[test]
fn a_drag_let_go_on_nothing_gives_nothing() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.drag_out(&[clip.id]).unwrap();
    let drag = app.drags().last();
    drag.let_go(LetGo::Nowhere);
    let ended = app.drag_ended();
    assert_eq!((ended["delivered"].clone(), ended["finished_only"].clone()), (json!(false), json!(false)), "{ended}");
    assert!(drag.given().is_empty());
}

/// A drag whose button came up before it could begin isn't begun, and
/// that's no error (clips.rs `began`).
#[test]
fn a_drag_whose_button_came_up_first_is_not_begun() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.drags().releases_early();
    assert_eq!(app.drag_out(&[clip.id]), Ok(json!(false)));
    assert_eq!(app.drags().begun(), 0);
}

/// A clip still being made, let go on an app that takes finished files
/// only, gives it none, and the page hears which app it was.
#[test]
fn an_app_taking_finished_files_only_gets_none_of_a_clip_being_made() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = vodscribe_core::testkit::Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    assert_eq!(app.drag_out(&[clip.id]), Ok(json!(true)));
    let drag = app.drags().last();
    drag.let_go(LetGo::On(Receiver::finished_only("Kdenlive")));
    let ended = app.drag_ended();
    assert_eq!(
        (ended["finished_only"].clone(), ended["receiver"].clone()),
        (json!(true), json!("Kdenlive")),
        "{ended}"
    );
    assert!(drag.given().is_empty());
    halfway.open();
}

/// An app that waits for a clip still being made gets it once it's made,
/// and the page hears meanwhile that it waits.
#[test]
fn an_app_waiting_for_a_clip_being_made_gets_it_once_made() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = vodscribe_core::testkit::Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    app.drag_out(&[clip.id]).unwrap();
    let drag = app.drags().last();
    drag.let_go(LetGo::On(Receiver::taking_copies("Finder")));
    eventually("the page hears the app waits", || !app.heard("drag-making").is_empty());
    assert_eq!(app.heard("drag-making")[0]["making"], 1);
    halfway.open();
    assert_eq!(app.drag_ended()["delivered"], true);
    assert_eq!(Media::read(&drag.given()[0]), Some(app.clip_media(&app.made(clip.id))));
}

/// On Linux an MP4 clip let go on DaVinci Resolve is refused it, and the
/// page hears only that (resolve.rs).
#[cfg(target_os = "linux")]
#[test]
fn a_drag_let_go_on_resolve_is_refused_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.drag_out(&[clip.id]).unwrap();
    let drag = app.drags().last();
    drag.let_go(LetGo::On(Receiver::taking_files("resolve")));
    let ended = app.drag_ended();
    assert_eq!((ended["resolve"].clone(), ended["delivered"].clone()), (json!("refused"), json!(false)), "{ended}");
    assert!(drag.given().is_empty());
}

/// A file the app couldn't delete as it ran (a folder it can't write to
/// holds it here, as Windows holds one open) is deleted at the next start,
/// read from the list its library kept (files.rs `Leftovers`).
#[cfg(unix)]
#[test]
fn a_file_left_undeleted_goes_at_the_next_start() {
    use std::os::unix::fs::PermissionsExt;
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let file = PathBuf::from(clip.file_path.clone().unwrap());
    let (clips, store) = (app.paths().clips(), app.paths().leftovers());
    let mode = |m| std::fs::set_permissions(&clips, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(file.is_file(), "deleted after all");
    let folder = app.quit();
    assert!(
        std::fs::read_to_string(&store).unwrap().contains(&*clip.file_path.unwrap()),
        "not kept for the next start"
    );
    mode(0o755);
    let _app = TestApp::on(folder);
    assert!(!file.exists(), "left after the next start");
    assert!(!store.exists(), "still listed");
}

/// The app's clock stands still till the test moves it, and a cut's
/// progress goes by it: a cut over in no time says nothing of its progress,
/// and one the clock moved on for says how far it was then.
#[test]
fn the_apps_clock_stands_still_till_the_test_moves_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let before = app.clock().now();
    let still = app.make_clip(vod.id, "0:10", "0:20");
    assert_eq!(app.clock().now(), before, "moved by itself");
    let progress = |id: i64| -> Vec<Value> {
        app.heard("clip-progress").into_iter().filter(|p| p["id"] == id).map(|p| p["progress"].clone()).collect()
    };
    assert_eq!(progress(still.id), Vec::<Value>::new());
    let first = vodscribe_core::testkit::Gate::new();
    app.ffmpeg.holds_at(0.25, &first);
    let moved = app.add_clip(vod.id, "0:30", "0:40").unwrap();
    first.reached();
    app.clock().advance(std::time::Duration::from_secs(1));
    assert_eq!(app.clock().now() - before, std::time::Duration::from_secs(1));
    first.open();
    app.made(moved.id);
    assert_eq!(progress(moved.id), [json!(0.25)]);
}

/// A cut held at its end has its file made and the library not yet told:
/// the clip is still being made till the gate opens.
#[test]
fn a_cut_held_at_its_end_is_made_but_not_yet_recorded() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let ended = vodscribe_core::testkit::Gate::new();
    app.holds_cut_ends(&ended);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    ended.reached();
    let files: Vec<PathBuf> = std::fs::read_dir(app.paths().clips()).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(Media::read(&files[0]).is_some(), "not whole");
    assert_eq!(app.clip(clip.id).unwrap().status, ClipStatus::Cutting);
    ended.open();
    let made = app.made(clip.id);
    assert_eq!(made.file_path.map(PathBuf::from), Some(files[0].clone()));
}

/// The variables an app was started with are its test's own: another app
/// running at once, and the process's, aren't seen.
#[test]
fn the_variables_an_app_was_started_with_are_its_tests_own() {
    let (waits, other) = (TestApp::new(), TestApp::new());
    waits.started_with("VODSCRIBE_DRAG_WAITS", "1");
    assert_eq!(crate::system::var(waits.handle(), "VODSCRIBE_DRAG_WAITS").as_deref(), Some("1"));
    assert_eq!(crate::system::var(other.handle(), "VODSCRIBE_DRAG_WAITS"), None);
    assert_eq!(crate::system::var(waits.handle(), "PATH"), None, "the process's");
}
