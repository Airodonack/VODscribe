//! The app kit's own specs: the app starts on its fakes, its commands are
//! reached as the page reaches them, and what it says and asks is heard.

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
