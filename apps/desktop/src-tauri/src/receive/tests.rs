//! The specs of `receive.rs`: files dropped on the window with no path on
//! this computer, received into the library. Each drop is the system's
//! drop target's as the kit plays it (`TestApp::drop_files`), its bytes
//! given by the test; what's seen is what the page is told ("received",
//! the drag's events), the jobs list (`list_jobs`, `cancel_job`), the
//! library and the folders files are received into. Each test is named for
//! what it says the app does and cites where that's said.
//!
//! Every read of something the page must have been told waits for it
//! (`told`): the page's events are sent from the receiving threads and the
//! jobs list's at once, and one sent while another is being heard reaches
//! the listeners a moment later.
//!
//! Elsewhere: the start's tidying of a quit's part files, the folders an
//! archive manager saved into and a file it can't add (tests/start.rs
//! `at_start_files_a_quit_left_being_received_go`), a file received whole
//! added at the next start (`a_file_received_whole_and_not_yet_added_is_added_at_the_next_start`),
//! links where files are received (`a_link_where_files_are_received_leaves_the_users_file_alone`,
//! `a_linked_folder_where_files_are_received_or_kept_leaves_the_users_files_alone`);
//! the add itself (vods/tests/adding.rs `a_file_of_the_apps_own_data_is_added_only_if_it_was_received`,
//! `a_second_file_received_under_the_same_name_is_a_new_vod`); and a drop
//! given nothing on Linux (testkit/tests.rs `a_drop_given_nothing_is_told_to_the_page`).

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tauri::{Listener, Manager};
use vodscribe_core::db::Vod;
use vodscribe_core::testkit::{Media, eventually};

use crate::AppState;
use crate::testkit::{Folder, Giving, TestApp};

/// What the page has heard of `event`, once it has heard it `n` times:
/// each is waited for, never read before it's sent.
fn heard(app: &TestApp, event: &str, n: usize) -> Vec<Value> {
    eventually(&format!("the page heard {event} {n} times"), || app.heard(event).len() >= n);
    let heard = app.heard(event);
    assert_eq!(heard.len(), n, "heard {event} more than expected: {heard:?}");
    heard
}

/// What the page has been told of files received ("received"), once it has
/// been told `n` things.
fn told(app: &TestApp, n: usize) -> Vec<Value> {
    heard(app, "received", n)
}

/// The page is told file `name` failed, saying `why`.
fn failed(name: &str, why: &str) -> Value {
    json!({ "kind": "failed", "name": name, "why": why })
}

/// The names in the folder files are received into, sorted; none if it isn't there.
fn incoming(app: &TestApp) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(app.paths().received_incoming())
        .map(|dir| dir.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// The jobs list, as the page asks for it.
fn jobs(app: &TestApp) -> Value {
    app.invoke("list_jobs", json!({})).unwrap()
}

/// What the jobs list calls each job, in order.
fn labels(app: &TestApp) -> Vec<Value> {
    jobs(app).as_array().unwrap().iter().map(|job| job["label"].clone()).collect()
}

/// The VOD the page was told to open (`told` the `n`th thing).
fn opened(app: &TestApp, n: usize) -> Vod {
    let told = told(app, n).remove(n - 1);
    assert_eq!(told["kind"], "open", "{told}");
    app.db().vod(told["vod"].as_i64().unwrap()).unwrap().expect("the VOD it opens")
}

/// The length of the video VOD `vod` plays, read from its file.
fn length(vod: &Vod) -> Option<f64> {
    Media::read(Path::new(vod.source_path.as_deref().expect("its file"))).map(|m| m.duration)
}

/// `file` given whole: all of `bytes`, then its end.
fn give_whole(mut file: Giving, bytes: &[u8]) {
    file.give(bytes).unwrap();
    file.whole();
}

/// A drag of files with no path on this computer is told to the page as it
/// comes over, by their names, and its drop with none: "a drag of virtual
/// files comes with their names for paths (the page shows it's welcome),
/// and its drop without them (they come in as jobs)" (receive.rs
/// `Window::heard`). One offering none the window takes is refused as it
/// comes over, and the page hears nothing of it (drop_target `Target::takes`).
#[test]
fn a_drag_of_files_with_no_path_comes_over_by_name_and_drops_with_none() {
    let app = TestApp::new();
    let given = app.drop_files(&[("Day 3.mp4", None), ("notes.txt", None)]);
    assert_eq!(given.len(), 1, "the text file was taken");
    assert_eq!(heard(&app, "tauri://drag-enter", 1)[0]["paths"], json!(["Day 3.mp4"]));
    assert_eq!(heard(&app, "tauri://drag-drop", 1)[0]["paths"], json!([]));
    assert!(app.drop_files(&[("notes.txt", None)]).is_empty(), "a drag of none it takes was taken");
    assert_eq!(app.heard("tauri://drag-enter").len(), 1, "the page heard a drag of none it takes");
}

/// Only video and audio files are received, each under its name made safe
/// for a file, its extension in lower case: "The name a file received as
/// `name` is written under (as a file it would be added from: one of the
/// media extensions), else None" (receive.rs `file_name`). The jobs list
/// shows each by that name.
#[test]
fn only_video_and_audio_files_are_received_their_names_made_safe() {
    let app = TestApp::new();
    let names = ["Clip [1].MP4", "a:b.mkv", "Talk.MP3", "notes.txt", "noext", ".mp4"];
    let given = app.drop_files(&names.map(|name| (name, None)));
    assert_eq!(given.len(), 4, "{:?}", given.iter().map(Giving::name).collect::<Vec<_>>());
    assert_eq!(labels(&app), [json!("Clip (1).mp4"), json!("a b.mkv"), json!("Talk.mp3"), json!("Clip.mp4")]);
}

/// A file being received is written into the folder files are received
/// into, as `<name>.part`, till it's whole: "Each is written into the kept
/// videos' folder, under `received/incoming` (`incoming`), as
/// `<name>.part`" (receive.rs's module doc).
#[test]
fn a_file_being_received_is_written_where_files_are_received_as_a_part_file() {
    let app = TestApp::new();
    let mut file = app.drop_files(&[("Day 3.mp4", None)]).remove(0);
    file.give(&Media::video(60.0).to_bytes()).unwrap();
    assert_eq!(incoming(&app), ["Day 3.mp4.part"]);
    let received: Vec<_> = std::fs::read_dir(app.paths().received()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(received, ["incoming"], "written elsewhere in the kept videos' folder");
}

/// A file received whole is added as a VOD of its own, the app's own copy
/// kept in the library under its VOD's id, and the page opens it: "renamed
/// once whole and added as a file (`vods::add_received`), which moves it to
/// `received/<its VOD's id>`" (receive.rs's module doc); 0.7.2: "Drop in
/// videos from zips and apps. A video dragged from a zip in File Explorer
/// or an archive manager on Linux, or one a Mac app hands over only once
/// it's dropped, is copied into VODscribe, listed with the jobs as it
/// comes, and opens once it's in."
#[test]
fn a_file_received_whole_is_added_as_a_vod_kept_in_the_library_and_opened() {
    let app = TestApp::new();
    let file = app.drop_files(&[("Day 3.mp4", None)]).remove(0);
    give_whole(file, &Media::video(60.0).to_bytes());
    let vod = opened(&app, 1);
    assert!(vod.received && vod.listed, "{vod:?}");
    assert_eq!(vod.info.title, "Day 3");
    assert_eq!(Path::new(vod.source_path.as_deref().unwrap()), app.paths().source_received(vod.id, "mp4"));
    assert_eq!(length(&vod), Some(60.0));
    assert_eq!(incoming(&app), Vec::<String>::new(), "left where it arrived");
}

/// A file being received is listed with the jobs as it comes, by its name,
/// and is off the list by the time the page opens it: "meanwhile it's
/// listed with the jobs ("Receiving", how far, and Cancel)" (receive.rs's
/// module doc); "Off the jobs first, so the page opens it with its job
/// gone" (receive.rs `Sink::finish`).
#[test]
fn a_file_being_received_is_listed_with_the_jobs_till_it_is_opened() {
    let app = TestApp::new();
    // What's being received as the page hears it's to open it.
    let listed_when_opened: Arc<Mutex<Option<usize>>> = Arc::default();
    {
        let (handle, listed) = (app.handle().clone(), listed_when_opened.clone());
        app.handle().listen_any("received", move |_| {
            *listed.lock().unwrap() = Some(handle.state::<AppState>().receives.listed().len());
        });
    }
    let media = Media::video(60.0).to_bytes();
    let mut file = app.drop_files(&[("Day 3.mp4", None)]).remove(0);
    file.give(&media[..4]).unwrap();
    let listed = jobs(&app);
    let id = listed[0]["id"].clone();
    assert_eq!(
        listed,
        json!([{ "kind": "receive", "id": id, "label": "Day 3.mp4", "running": true, "progress": null }])
    );
    // The page hears the list's news too, as it changes.
    eventually("the page told it's listed", || app.heard("jobs").last() == Some(&listed));
    give_whole(file, &media[4..]);
    opened(&app, 1);
    assert_eq!(*listed_when_opened.lock().unwrap(), Some(0), "still listed as the page opened it");
    assert_eq!(jobs(&app), json!([]));
    eventually("the page told it's off the list", || app.heard("jobs").last() == Some(&json!([])));
}

/// How far a file is shows in the jobs list only when its app says how long
/// it is: "Each file being received: its id, name and how far it is (0..1;
/// None while that isn't known)" (receive.rs `Receives::listed`); never past
/// all of it.
#[test]
fn how_far_a_file_is_is_known_only_when_its_app_says_its_length() {
    let app = TestApp::new();
    let mut given = app.drop_files(&[("a.mp4", Some(10)), ("b.mp4", None), ("c.mp4", Some(0))]);
    for file in &mut given {
        file.give(b"12345").unwrap();
    }
    let along = || jobs(&app).as_array().unwrap().iter().map(|job| job["progress"].clone()).collect::<Vec<_>>();
    assert_eq!(along(), [json!(0.5), json!(null), json!(null)]);
    given[0].give(b"1234567").unwrap();
    assert_eq!(along()[0], json!(1.0), "more than its app said is all of it");
}

/// Of a drop's files, the page opens only the first to be added; each
/// drop's own first: "The page hears ("received") the first file of each
/// drop to be added, which it opens" (receive.rs's module doc).
#[test]
fn only_the_first_file_of_a_drop_to_be_added_is_opened() {
    let app = TestApp::new();
    let mut given = app.drop_files(&[("Day 3.mp4", None), ("Day 4.mp4", None)]);
    let (day4, day3) = (given.pop().unwrap(), given.pop().unwrap());
    give_whole(day4, &Media::video(40.0).to_bytes());
    assert_eq!(length(&opened(&app, 1)), Some(40.0), "not the first added");
    give_whole(day3, &Media::video(30.0).to_bytes());
    app.threads_done();
    // The next drop's first is opened: the second added of the first drop wasn't.
    let next = app.drop_files(&[("Day 5.mp4", None)]).remove(0);
    give_whole(next, &Media::video(50.0).to_bytes());
    assert_eq!(length(&opened(&app, 2)), Some(50.0), "the second added of a drop was opened");
    let titles: Vec<String> = app.db().vods().unwrap().into_iter().map(|v| v.info.title).collect();
    assert_eq!(titles.len(), 3, "{titles:?}");
}

/// Cancel in the jobs list takes a file being received off the list at
/// once, the others left; the rest of its bytes are refused, nothing of it
/// is left, and no failure is said: "Cancels receiving file `id` (the jobs'
/// Cancel): it's off the list at once, and stops at its next bytes; what
/// was written goes" (receive.rs `Receives::cancel`); "Canceled or failed,
/// nothing is left" (receive.rs's module doc).
#[test]
fn cancel_in_the_jobs_list_stops_only_its_file_and_leaves_nothing_of_it() {
    let app = TestApp::new();
    let media = Media::video(60.0).to_bytes();
    let mut given = app.drop_files(&[("Day 3.mp4", None), ("Day 4.mp4", None)]);
    for file in &mut given {
        file.give(&media[..4]).unwrap();
    }
    let (mut day4, day3) = (given.pop().unwrap(), given.pop().unwrap());
    let id = jobs(&app)[1]["id"].clone();
    app.invoke("cancel_job", json!({ "kind": "receive", "id": id })).unwrap();
    assert_eq!(labels(&app), [json!("Day 3.mp4")], "off the list at once, and only it");
    assert!(day4.give(&media[4..]).is_err(), "its next bytes were taken");
    assert_eq!(incoming(&app), ["Day 3.mp4.part"], "something of it is left");
    give_whole(day3, &media[4..]);
    opened(&app, 1);
    assert_eq!(app.db().vods().unwrap().len(), 1);
}

/// A file its app fails to give leaves nothing, and the page is told which,
/// in one plain sentence: "Why, for one its app didn't give" (receive.rs
/// `NOT_GIVEN`); "each file that failed, which it says" (receive.rs's module
/// doc).
#[test]
fn a_file_its_app_fails_to_give_leaves_nothing_and_is_said() {
    let app = TestApp::new();
    let mut file = app.drop_files(&[("Day 4.mp4", Some(100))]).remove(0);
    file.give(b"FAKE").unwrap();
    file.fails("the archive is broken");
    assert_eq!(told(&app, 1), [failed("Day 4.mp4", "The app it came from didn't hand it over.")]);
    assert_eq!(incoming(&app), Vec::<String>::new());
    assert_eq!(jobs(&app), json!([]));
    assert_eq!(app.db().vods().unwrap().len(), 0);
}

/// A file let go of unfinished, as the window closing lets its drop go,
/// leaves nothing and is no failure to say: "Canceled from the jobs, or the
/// fetch let go of (the window closing): not a failure to tell" (receive.rs
/// `Sink::finish`).
#[test]
fn a_file_let_go_of_unfinished_leaves_nothing_and_is_not_said() {
    let app = TestApp::new();
    let mut file = app.drop_files(&[("Day 5.mp4", None)]).remove(0);
    file.give(b"FAKE").unwrap();
    drop(file);
    assert_eq!(incoming(&app), Vec::<String>::new());
    assert_eq!(jobs(&app), json!([]));
    assert_eq!(app.heard("received"), Vec::<Value>::new());
}

/// A file received whole that isn't a video VODscribe can read goes, and the
/// page is told: "Why, for one received that couldn't be added" (receive.rs
/// `UNREADABLE`); "Canceled or failed, nothing is left" (receive.rs's module
/// doc).
#[test]
fn a_file_received_that_cannot_be_read_goes_and_is_said() {
    let app = TestApp::new();
    let file = app.drop_files(&[("Day 6.mp4", None)]).remove(0);
    give_whole(file, b"not a video");
    assert_eq!(told(&app, 1), [failed("Day 6.mp4", "It isn't a video VODscribe can read.")]);
    assert_eq!(incoming(&app), Vec::<String>::new());
    assert_eq!(jobs(&app), json!([]));
    assert_eq!(app.db().vods().unwrap().len(), 0);
}

/// A file that can't be saved where files are received is said, and isn't
/// listed with the jobs: "Why, for one that couldn't be written" (receive.rs
/// `unsaved`, said as the drop comes in: `Window::incoming`).
#[test]
fn a_file_that_cannot_be_saved_is_said_and_not_listed() {
    let app = TestApp::new();
    let at = app.paths().received_incoming();
    let _ = std::fs::remove_dir_all(&at);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, b"in the way").unwrap();
    assert!(app.drop_files(&[("Day 7.mp4", None)]).is_empty(), "it's being received");
    assert_eq!(told(&app, 1), [failed("Day 7.mp4", "It couldn't be saved.")]);
    assert_eq!(jobs(&app), json!([]));
}

/// A file is received under the name its app gives, unless a file of that
/// name is there already, finished or still being written: then it's
/// numbered, "(2)", "(3)"... (receive.rs `free_name`: "`name` in `dir`, or
/// the first of "<stem> (2).<ext>", "(3)"... that isn't there, finished or
/// being written"). The jobs list still shows the name it came with.
#[test]
fn a_name_taken_finished_or_being_written_gets_a_number() {
    let app = TestApp::new();
    let waiting = app.paths().received_incoming().join("a.mp4");
    std::fs::create_dir_all(waiting.parent().unwrap()).unwrap();
    std::fs::write(&waiting, b"whole").unwrap();
    let _given = app.drop_files(&[("a.mp4", None), ("a.mp4", None)]);
    assert_eq!(incoming(&app), ["a (2).mp4.part", "a (3).mp4.part", "a.mp4"]);
    assert_eq!(labels(&app), [json!("a.mp4"), json!("a.mp4")]);
}

/// A file made whole once a file of its name has come meanwhile takes the
/// next free name, writing over nothing: "Its file, whole at `part`, under
/// the name it was written for; or, something there now, the next free
/// one" (receive.rs `Sink::keep`); "Renames `from` to `to`, unless
/// something is at `to` already" (receive.rs `rename_new`).
#[test]
fn a_file_made_whole_under_a_name_taken_meanwhile_writes_over_nothing() {
    let app = TestApp::new();
    let mut file = app.drop_files(&[("a.mp4", None)]).remove(0);
    file.give(&Media::video(60.0).to_bytes()).unwrap();
    let theirs = app.paths().received_incoming().join("a.mp4");
    std::fs::write(&theirs, b"come meanwhile").unwrap();
    file.whole();
    let vod = opened(&app, 1);
    assert_eq!(length(&vod), Some(60.0));
    assert_eq!(std::fs::read(&theirs).unwrap(), b"come meanwhile", "written over");
}

/// Two files received at once, named as VODs' files are kept ("1.mp4",
/// "2.mp4"), each keep their own bytes whichever is added first: the
/// folder files are received into is "a folder of their own, so a name one
/// comes with ("1.mp4") is never the one a VOD's file is kept under
/// (`source_received`)" (core lib.rs `Paths::received_incoming`).
#[test]
fn files_received_named_as_vods_files_take_no_other_files_place() {
    let app = TestApp::new();
    let mut given = app.drop_files(&[("1.mp4", None), ("2.mp4", None)]);
    let (two, one) = (given.pop().unwrap(), given.pop().unwrap());
    give_whole(two, &Media::video(20.0).to_bytes());
    let first = opened(&app, 1);
    give_whole(one, &Media::video(10.0).to_bytes());
    app.threads_done();
    let vods = app.db().vods().unwrap();
    let mut lengths: Vec<(String, Option<f64>)> = vods.iter().map(|v| (v.info.title.clone(), length(v))).collect();
    lengths.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(lengths, [("1".to_string(), Some(10.0)), ("2".to_string(), Some(20.0))], "a file lost");
    assert_eq!(first.info.title, "2");
}

/// While the library can't be opened (the recovery screen), a drop of files
/// with no path receives nothing: "(Not while the library can't be opened:
/// the recovery screen.)" (receive.rs `Window::incoming`).
#[test]
fn while_the_library_cannot_be_opened_a_drop_receives_nothing() {
    let folder = Folder::new();
    let paths = folder.paths();
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a library").unwrap();
    let app = TestApp::on(folder);
    assert!(app.drop_files(&[("Day 3.mp4", None)]).is_empty(), "it's being received");
    assert_eq!(incoming(&app), Vec::<String>::new());
    #[cfg(target_os = "linux")]
    {
        use crate::drop_target::Target;
        assert_eq!(crate::receive::target(app.handle(), "main").stage(), None, "an archive manager's drop taken");
    }
}

/// On Linux, the files an archive manager saves for the window are saved
/// first in the kept videos' folder of received files: "on Linux the files
/// an archive manager saves for the app (File Roller, Engrampa, Ark), first
/// into a hidden folder of `received`" (receive.rs's module doc).
#[cfg(target_os = "linux")]
#[test]
fn on_linux_an_archive_managers_files_are_saved_first_where_received_files_are_kept() {
    use crate::drop_target::Target;
    let app = TestApp::new();
    assert_eq!(crate::receive::target(app.handle(), "main").stage(), Some(app.paths().received()));
}

/// At the start, a file in the kept videos' folder of received files that
/// no VOD has goes (a crash as its VOD was erased); a VOD's own file, a
/// hidden file and a folder stay: "when the library's files are known
/// (`kept`, every VOD's file), a file in `received` that no VOD has (a
/// crash as its VOD was erased)" (receive.rs `tidy`, which removes "only
/// files and folders, never a link": `tidy_in`).
#[test]
fn at_start_a_received_file_no_vod_has_goes_and_nothing_else() {
    let app = TestApp::new();
    let file = app.drop_files(&[("Day 3.mp4", None)]).remove(0);
    give_whole(file, &Media::video(60.0).to_bytes());
    let vod = opened(&app, 1);
    let folder = app.quit();
    let received = folder.paths().received();
    let (stray, hidden, part_folder) =
        (received.join(format!("{}.mp4", vod.id + 1)), received.join(".hidden.mp4"), received.join("c.part"));
    std::fs::write(&stray, b"left").unwrap();
    std::fs::write(&hidden, b"left").unwrap();
    std::fs::create_dir_all(&part_folder).unwrap();
    let app = TestApp::on(folder);
    assert!(!stray.exists(), "a file no VOD has was kept");
    assert!(hidden.is_file() && part_folder.is_dir(), "a hidden file or a folder went");
    assert_eq!(length(&app.db().vod(vod.id).unwrap().unwrap()), Some(60.0), "a VOD's own file went");
}

/// A library this version can't read the VODs of keeps its received files
/// at the start, as their VODs can't be told apart; part files still go:
/// "Removes ... what a crash or a quit left being received: part files, ...
/// and, when the library's files are known (`kept`, every VOD's file), a
/// file in `received` that no VOD has" (receive.rs `tidy`).
#[test]
fn at_start_a_library_whose_vods_cannot_be_read_keeps_its_received_files() {
    let app = TestApp::new();
    let folder = app.quit();
    let paths = folder.paths();
    let (kept, part) = (paths.received().join("4.mp4"), paths.received_incoming().join("Day 3.mp4.part"));
    std::fs::create_dir_all(paths.received_incoming()).unwrap();
    std::fs::write(&kept, b"a VOD's?").unwrap();
    std::fs::write(&part, b"left").unwrap();
    vodscribe_core::testkit::newer(&paths.db(), "ALTER TABLE vods DROP COLUMN channel_url;");
    let app = TestApp::on(folder);
    assert!(app.db().vods().is_err(), "its VODs can be read");
    assert!(kept.is_file(), "a file a VOD may have went");
    assert!(!part.exists(), "a file being received was kept");
}

/// A file received whole and not yet added waits, at a start without
/// ffmpeg, for a start with it, which adds it: "Not while ffmpeg is
/// missing, which reads it: they wait for a start with it" (receive.rs
/// `take_up`).
#[test]
fn a_file_received_whole_waits_for_a_start_with_ffmpeg_to_be_added() {
    let app = TestApp::new();
    let folder = app.quit();
    let arrived = folder.paths().received_incoming().join("Day 6.mp4");
    Media::video(60.0).write(&arrived);
    let app = TestApp::on_with(folder, |fakes| std::fs::remove_file(fakes.ffmpeg.path()).unwrap());
    assert!(arrived.is_file(), "it went at a start without ffmpeg");
    assert_eq!(app.db().vods().unwrap().len(), 0);
    let app = TestApp::on(app.quit());
    let vods = app.db().vods().unwrap();
    assert_eq!(vods.iter().map(|v| v.info.title.as_str()).collect::<Vec<_>>(), ["Day 6"]);
    assert!(!arrived.exists(), "left where it arrived");
}

/// A link where files are received is no file received there: the start
/// neither adds it nor removes it as one that can't be added. "Only files:
/// a link there isn't one received" (receive.rs `take_up`); "only files and
/// folders, never a link" (receive.rs `tidy_in`).
#[cfg(unix)]
#[test]
fn a_link_where_files_are_received_stays_where_it_is_at_the_start() {
    let outside = vodscribe_core::testkit::TempDir::new("outside");
    let theirs = outside.join("wedding.mp4");
    Media::video(60.0).write(&theirs);
    let folder = TestApp::new().quit();
    let link = folder.paths().received_incoming().join("Day 6.mp4");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&theirs, &link).unwrap();
    let app = TestApp::on(folder);
    assert!(std::fs::symlink_metadata(&link).is_ok(), "the link went");
    assert_eq!(app.db().vods().unwrap().len(), 0, "the link was added");
}
