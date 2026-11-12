//! The app kit's own specs: the app starts on its fakes, and again on the
//! library of one that quit or was killed; its commands are reached as the
//! page reaches them; what it says and asks is heard; a drag out of the
//! window ends where the test lets it go; and its screen is the test's.

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

/// The window has a screen only once the test gives it one; then it keeps
/// the frame the player shows (a fake video's, decoded by the fake ffmpeg,
/// saying its number) and black once it's cleared, and once it's closed for
/// good it's shown nothing more.
#[test]
fn the_screen_is_the_tests_and_keeps_what_the_player_shows_till_closed() {
    let app = TestApp::new();
    assert!(!crate::screen::attached(app.handle()), "the mock runtime's window has none");
    app.with_screen();
    assert!(crate::screen::attached(app.handle()));
    let mut source = Media::video(10.0);
    let video = source.video.as_mut().unwrap();
    (video.width, video.height) = (32, 18);
    let vod = app.add_file("Day 3.mp4", &source);
    app.invoke("player_open", json!({ "id": vod.id, "at": 2.0, "play": false })).unwrap();
    assert_eq!(app.screen().frame_shown(), 60, "the frame at 0:02, of 30 a second");
    let shown = app.screen().showing().unwrap();
    assert_eq!((shown.width, shown.height, shown.pts), (32, 18, 2.0));
    crate::screen::show(app.handle(), None);
    assert!(app.screen().showing().is_none(), "black");
    crate::screen::close(app.handle());
    assert!(app.screen().closed());
    let shows = app.screen().shows();
    crate::screen::show(app.handle(), Some(shown));
    assert!(app.screen().showing().is_none() && app.screen().shows() == shows, "nothing once it's closed");
}

/// The lines VOD `id` has once its transcription has settled, as (start, text).
fn transcribed(app: &TestApp, id: i64) -> Vec<(f64, String)> {
    eventually("transcribed", || !crate::vods::busy(app.db().vod(id).unwrap().unwrap().status));
    app.db().lines(id).unwrap().into_iter().map(|l| (l.start, l.text)).collect()
}

/// Each app's worker transcribes what its queue asks, saying the words it
/// heard where the VOD says them: a section downloaded for a stretch of a
/// site's VOD starts where it was cut, so a word at 1:10 is at 1:10, not ten
/// seconds into the section. Two apps at once hear their own words.
#[test]
fn each_apps_worker_says_its_words_where_the_vod_says_them() {
    let (app, other) = (TestApp::new(), TestApp::new());
    app.worker().hears(vodscribe_core::testkit::said(70.0, "Inside the stretch"));
    other.worker().hears(vodscribe_core::testkit::said(70.0, "Another app's words"));
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "A talk", 600.0);
    let file = other.add_video_file("Day 3.mp4", 600.0);
    for (app, id) in [(&app, vod.id), (&other, file.id)] {
        app.invoke("transcribe_stretch", json!({ "vodId": id, "start": 60.0, "end": 120.0 })).unwrap();
    }
    assert_eq!(transcribed(&app, vod.id), [(70.0, "Inside the stretch".to_string())]);
    assert_eq!(transcribed(&other, file.id), [(70.0, "Another app's words".to_string())]);
    assert_eq!(app.db().audio_path(vod.id).unwrap(), None, "the whole VOD downloaded, not a section");
}

/// A VOD saying `lines` lines, one a second, added: its id. An erasure of a
/// thousand takes four parts (a part erases a third of a thousand lines).
fn a_long_transcript(app: &TestApp, lines: usize) -> i64 {
    let vod = app.add_video_file("A long talk.mp4", lines as f64);
    let said: Vec<vodscribe_core::transcript::Segment> = (0..lines)
        .map(|i| {
            let words = vodscribe_core::testkit::said(i as f64, "word");
            vodscribe_core::transcript::Segment { start: i as f64, end: i as f64 + 0.5, text: "word".into(), words }
        })
        .collect();
    app.db().insert_segments(vod.id, &said).unwrap();
    vod.id
}

/// An erasure held between its parts has erased one and let go of the
/// library; held in a part, it keeps the library, while the views' reads,
/// on a connection of their own, answer. Let go, it erases the rest.
#[test]
fn an_erasure_is_held_between_its_parts_or_in_one() {
    let app = TestApp::new();
    let vod = a_long_transcript(&app, 1000);
    let between = Gate::new();
    app.holds_erasures(&between);
    app.invoke("delete_vod", json!({ "id": vod })).unwrap();
    between.reached();
    let left = app.db().erase_left(vod).unwrap();
    assert!(0 < left && left < 1000, "{left} lines left after a part");
    assert!(app.state().db.try_lock().is_ok(), "the library held between parts");
    let in_a_part = Gate::new();
    app.holds_erasure_parts(&in_a_part);
    app.holds_erasures(&Gate::opened());
    between.open();
    in_a_part.reached();
    assert!(app.state().db.try_lock().is_err(), "the library let go of in a part");
    assert_eq!(app.invoke("list_vods", json!({})), Ok(json!([])), "a view's read waited");
    in_a_part.open();
    app.erasures_done();
    assert_eq!(app.db().erase_left(vod).unwrap(), 0);
}

/// An edit's plan held once it's worked out isn't answered till let go;
/// one asked meanwhile, not held, is.
#[test]
fn a_plan_held_is_answered_once_let_go() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = app.invoke("create_edit", json!({})).unwrap()["id"].as_i64().unwrap();
    app.invoke("add_range_to_edit", json!({ "editId": edit, "vodId": vod.id, "start": 10.0, "end": 20.0 })).unwrap();
    let planned = Gate::new();
    app.holds_plans(&planned);
    let handle = app.handle().clone();
    let first = std::thread::spawn(move || tauri::async_runtime::block_on(crate::player::player_plan(handle, edit)));
    planned.reached();
    app.holds_plans(&Gate::opened());
    let second = app.invoke("player_plan", json!({ "id": edit })).unwrap();
    assert_eq!(second.as_array().map(Vec::len), Some(1), "{second}");
    assert!(!first.is_finished(), "answered while held");
    planned.open();
    assert_eq!(first.join().unwrap().map(|stretches| stretches.len()), Ok(1));
}

/// A search held once it's asked hasn't run: one typed after it runs
/// first, and the one held, typed, gives way to it.
#[test]
fn a_search_held_once_asked_lets_one_typed_after_it_go_first() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.says(vod.id, 10.0, "The Eagle has landed");
    let asked = Gate::new();
    app.holds_searches(&asked);
    let handle = app.handle().clone();
    let search = move |handle| {
        let filter = vodscribe_core::db::SearchFilter::default();
        tauri::async_runtime::block_on(crate::vods::search(handle, "eagle".into(), filter, Some(true)))
    };
    let first = std::thread::spawn({
        let handle = handle.clone();
        move || search(handle).map(|_| ())
    });
    asked.reached();
    app.holds_searches(&Gate::opened());
    let second = app.invoke("search", json!({ "query": "eagle", "filter": {}, "typed": true })).unwrap();
    assert_eq!(second["hits"].as_array().map(Vec::len), Some(1), "{second}");
    asked.open();
    assert_eq!(first.join().unwrap(), Err("overtaken by a newer search".to_string()));
}

/// A settings file read and found unreadable, held before it's set aside,
/// is left as a save meanwhile wrote it; a save held before its turn lets
/// one asked after it write first, and then writes over it.
#[test]
fn the_settings_are_held_after_a_read_and_before_a_save() {
    use vodscribe_core::settings::{Settings, Theme};
    let app = TestApp::new();
    let path = app.paths().settings();
    std::fs::write(&path, "not settings").unwrap();
    let read = Gate::new();
    app.holds_settings_reads(&read);
    let loading = std::thread::spawn({
        let path = path.clone();
        move || Settings::load(&path)
    });
    read.reached();
    let dark = Settings { theme: Theme::Dark, ..Settings::default() };
    dark.save(&path).unwrap();
    read.open();
    loading.join().unwrap();
    assert_eq!(Settings::load(&path).theme, Theme::Dark, "the file saved meanwhile set aside");
    assert_eq!(Settings::taken_aside(&path), None);
    let asked = Gate::new();
    app.holds_settings_saves(&asked);
    let saving = std::thread::spawn({
        let path = path.clone();
        move || Settings { theme: Theme::Light, ..Settings::default() }.save(&path).unwrap()
    });
    asked.reached();
    app.holds_settings_saves(&Gate::opened());
    Settings::default().save(&path).unwrap();
    assert_eq!(Settings::load(&path).theme, Theme::System, "the save asked after it went first");
    asked.open();
    saving.join().unwrap();
    assert_eq!(Settings::load(&path).theme, Theme::Light);
}

/// A line said by someone is read with their name, and one said by no one
/// without; a site's VOD added on a channel is that channel's, and the fake
/// site says so again after a restart, when the details are asked anew.
#[test]
fn a_line_has_its_speaker_and_a_vod_its_channel() {
    let app = TestApp::new();
    let vod = app.add_site_vod_of("twitch", "2901646294", "Day 3", 600.0, "Ada Plays");
    assert_eq!(app.db().vod(vod.id).unwrap().unwrap().info.channel.as_deref(), Some("Ada Plays"));
    app.says_as(vod.id, 10.0, "Hello chat", "Ada");
    app.says(vod.id, 20.0, "Said by no one named");
    let lines = app.invoke("transcript", json!({ "vodId": vod.id })).unwrap();
    assert_eq!(lines[0]["speaker"], "Ada");
    assert_eq!(lines[0]["text"], "Hello chat");
    assert!(lines[1].get("speaker").is_none(), "{lines}");
    let app = TestApp::on(app.quit());
    let yt_dlp = vodscribe_core::tools::YtDlp { path: app.yt_dlp.path().into(), js_runtime: None };
    let asked = app.paths().root.with_file_name("details.json");
    let no = std::sync::atomic::AtomicBool::new(false);
    let info = vodscribe_core::fetch::probe(&yt_dlp, &vod.info.url, &asked, &no).unwrap();
    assert_eq!(info.channel.as_deref(), Some("Ada Plays"));
}

/// A clip dragged out to an editor through the kit hands it the clip's
/// file, and the erasures are done once none is listed with the jobs.
#[test]
fn a_clip_dragged_out_gives_its_file_and_erasures_are_waited_out() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let given = app.dragged_out(&clip);
    assert_eq!(Media::read(&given), Some(app.clip_media(&clip)));
    let other = a_long_transcript(&app, 1000);
    let between = Gate::new();
    app.holds_erasures(&between);
    app.invoke("delete_vod", json!({ "id": other })).unwrap();
    between.reached();
    let jobs = app.invoke("list_jobs", json!({})).unwrap();
    assert!(jobs.as_array().unwrap().iter().any(|job| job["kind"] == "erase"), "{jobs}");
    between.open();
    app.erasures_done();
    assert_eq!(app.db().erase_left(other).unwrap(), 0);
}

/// A library whose worker was faked has it again from its next start, so
/// a transcription its queue takes up again reaches a worker (which hears
/// nothing till told) and is done, rather than failing for want of one.
#[test]
fn a_restarted_app_has_its_worker_from_the_start() {
    let app = TestApp::new();
    let held = Gate::new();
    app.worker().hears(vodscribe_core::testkit::said(10.0, "Before the quit"));
    app.worker().holds(&held);
    let vod = app.add_video_file("Day 3.mp4", 300.0);
    app.invoke("transcribe_stretch", json!({ "vodId": vod.id, "start": 0.0, "end": null })).unwrap();
    held.reached();
    eventually("its first minute written", || app.db().requests(vod.id).unwrap().iter().any(|s| s.reached.is_some()));
    let app = TestApp::on(app.quit());
    transcribed(&app, vod.id);
    let done = app.db().vod(vod.id).unwrap().unwrap();
    assert_eq!((done.status, done.error), (vodscribe_core::db::Status::Done, None), "no worker took it up");
}

/// A thread a command started and didn't wait for (`started`: an added
/// file's picture asked for, vods.rs `announce_file`) still running as the
/// app ends is waited for before the app's state goes: let go only once the
/// end has begun, it still finds the state. (Ended under it, a real one read
/// freed memory: sections-074 2A1's report.)
#[test]
fn the_apps_end_waits_for_the_threads_its_commands_started() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let app = TestApp::new();
    let late = Gate::new();
    let found = Arc::new(AtomicBool::new(false));
    let handle = app.handle().clone();
    let thread = std::thread::spawn({
        let (late, found, handle) = (late.clone(), found.clone(), handle.clone());
        move || {
            late.wait();
            found.store(handle.try_state::<AppState>().is_some(), Ordering::SeqCst);
        }
    });
    started(app.handle(), thread);
    late.reached();
    assert_eq!(app.threads_running(), 1);
    // Let go once the app's end waits for it, or (were it not to) its state is gone.
    let opener = std::thread::spawn(move || {
        let waited_for = || handle.try_state::<Started>().is_none_or(|s| s.lock().is_empty());
        while handle.try_state::<AppState>().is_some() && !waited_for() {
            std::thread::yield_now();
        }
        late.open();
    });
    drop(app);
    opener.join().unwrap();
    assert!(found.load(Ordering::SeqCst), "the app's state went before the thread it started ended");
}

/// The thread an added file's command starts to ask for its picture
/// (vods.rs `announce_file`) is one the app waits for (`threads_done`).
#[test]
fn an_added_files_picture_is_asked_for_on_a_thread_the_app_waits_for() {
    let app = TestApp::new();
    let path = app.paths().root.with_file_name("files").join("Day 4.mp4");
    Media::video(60.0).write(&path);
    app.run(crate::vods::add_files(app.handle().clone(), vec![path.to_string_lossy().into_owned()])).unwrap();
    let started = app.app.state::<Started>().lock().len();
    assert_eq!(started, 1, "its thread isn't waited for");
    app.threads_done();
    assert_eq!(app.threads_running(), 0);
}

/// An app starts on a library that won't open, as the app's own start
/// does (lib.rs `open_at_start`): on the recovery screen, with the error
/// the page shows, and none of the rest; and it ends.
#[test]
fn an_app_starts_on_a_library_that_wont_open_on_the_recovery_screen() {
    let folder = Folder::new();
    let paths = folder.paths();
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a library").unwrap();
    let app = TestApp::on(folder);
    let problem = app.invoke("library_problem", json!({})).unwrap();
    assert!(problem.as_str().is_some_and(|p| p.contains("open the library")), "{problem}");
    assert!(app.handle().try_state::<AppState>().is_none(), "the rest started");
}

/// The fakes told what to do as an app starts are told before its queues
/// take up what was left: a cut left unmade, held as it begins again, is
/// held from its first run.
#[test]
fn fakes_told_as_an_app_starts_are_told_before_its_queues_run() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = Gate::new();
    app.ffmpeg.holds_at(0.25, &first);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    first.reached();
    let again = Gate::new();
    let app = TestApp::on_with(app.kill(), |fakes| fakes.ffmpeg.holds_at(0.25, &again));
    again.reached();
    assert_ne!(app.clip(clip.id).unwrap().status, ClipStatus::Ready, "made before it was let go");
    again.open();
    assert_eq!(app.made(clip.id).status, ClipStatus::Ready);
}

/// A quit stops what its app started (lib.rs `quit`, `tools::stop_tracked`),
/// and only that: another test's app, its cut held as this one's is, makes
/// its clip once let go.
#[test]
fn a_quit_stops_only_what_its_own_app_started() {
    let (ours, theirs) = (TestApp::new(), TestApp::new());
    let (held, theirs_held) = (Gate::new(), Gate::new());
    ours.ffmpeg.holds_at(0.5, &held);
    theirs.ffmpeg.holds_at(0.5, &theirs_held);
    let vod = ours.add_video_file("Day 3.mp4", 60.0);
    ours.add_clip(vod.id, "0:10", "0:20").unwrap();
    let their_vod = theirs.add_video_file("Day 3.mp4", 60.0);
    let their_clip = theirs.add_clip(their_vod.id, "0:10", "0:20").unwrap();
    held.reached();
    theirs_held.reached();
    let _folder = ours.quit();
    theirs_held.open();
    assert_eq!(theirs.made(their_clip.id).status, ClipStatus::Ready, "the other app's cut was stopped");
}

/// An app ends itself as its event loop's end would end it (lib.rs `exit`,
/// which Tauri's mock runtime can't): quit's work done, the code it ended
/// with kept, and its folder the test's to start it on again.
#[test]
fn an_app_ends_itself_as_its_event_loop_would() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.invoke("copy_items", json!({ "items": [{ "kind": "clip", "id": clip.id }] })).unwrap();
    app.invoke("copied_text", json!({ "text": "copied" })).unwrap();
    app.invoke("cut_clips", json!({ "ids": [clip.id] })).unwrap();
    crate::exit(app.handle(), 3);
    let (code, folder) = app.exited();
    assert_eq!(code, 3);
    let app = TestApp::on(folder);
    assert!(app.clip(clip.id).is_none(), "quit's work wasn't done: the cut clip is still there");
}
