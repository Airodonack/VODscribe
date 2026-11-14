//! Adding VODs, and refreshing their details: `add_vod` and `cancel_add`
//! (a link pasted), `add_files` (files dropped or picked), `media_extensions`
//! (the file picker's), and `refresh_details` (a site VOD's menu), through the
//! commands as the page calls them, and what they leave in the library.
//!
//! A link's host is resolved before anything else (`net::public_url`): on
//! the kit's network (`TestApp::net`), where every site's host is public
//! and one a test names is public or local, as it says.
//!
//! The site most specs add from is a fake one (`peertube.example`): its
//! details and channel come from yt-dlp's, as for any site without an API
//! of its own the app asks, and it has no chat and no VODscription, so
//! neither is fetched nor the file host asked. The specs of those add or
//! refresh a YouTube or Twitch VOD, answered by the kit's network.

use std::path::{Path, PathBuf};
use std::thread;

use serde_json::{Value, json};
use vodscribe_core::db::ChatStatus;
use vodscribe_core::testkit::{Gate, Media, Reply, TempDir, eventually, yt_dlp};

use super::*;
use crate::testkit::TestApp;

/// A folder of the user's own, outside the library, holding `media` as each
/// of `names`: the files a test drops on the window.
fn user_files(names: &[&str], media: &Media) -> (TempDir, Vec<PathBuf>) {
    let dir = TempDir::new("user-files");
    let paths = names.iter().map(|name| dir.join(name)).collect::<Vec<_>>();
    paths.iter().for_each(|path| media.write(path));
    (dir, paths)
}

/// `add_files`'s answer for `paths`, as the page gets it. The thread it
/// starts to ask for each VOD's picture is waited for by the kit.
fn add_files(app: &TestApp, paths: &[PathBuf]) -> AddedFiles {
    app.run(crate::vods::add_files(app.handle().clone(), as_strings(paths))).unwrap()
}

fn as_strings(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

/// Whether `a` and `b` are the same file.
fn same_file(a: &str, b: &Path) -> bool {
    std::fs::canonicalize(a).unwrap() == std::fs::canonicalize(b).unwrap()
}

/// `add_vod`'s answer for `url`, as the page gets it.
fn add_link(app: &TestApp, url: &str) -> Result<Value, Value> {
    app.invoke("add_vod", json!({ "url": url }))
}

/// The VODs the library has, gone ones too.
fn vods(app: &TestApp) -> Vec<Vod> {
    app.db().vods().unwrap()
}

/// A link of nothing (empty, or spaces) is answered with what to do,
/// and adds nothing (vods.rs `add_vod`: "Paste a VOD link first.").
#[test]
fn a_link_of_nothing_says_paste_a_vod_link_first() {
    let app = TestApp::new();
    for url in ["", "   ", "\n\t"] {
        assert_eq!(add_link(&app, url), Err(json!("Paste a VOD link first.")), "{url:?}");
    }
    assert!(vods(&app).is_empty());
}

/// A link to this computer or the local network is refused, and nothing is
/// added (CHANGELOG.md, 0.3.0: "VODscribe won't follow links to this
/// computer or your local network."). Its host is checked as the name it
/// resolves as (vods.rs `add_vod`: "Checked, and passed on as the name
/// it resolves as"), so one written in look-alike characters is refused
/// too, and so is a name that resolves there.
#[test]
fn a_link_to_this_computer_or_the_local_network_is_refused() {
    let app = TestApp::new();
    app.net.local("router.example");
    let refused = Err(json!("VODscribe doesn't follow links to this computer or the local network."));
    for url in [
        "http://127.0.0.1:1420/media/1",
        "https://localhost/videos/1",
        "https://[::1]/videos/1",
        "http://192.168.1.10/videos/1",
        "https://nas.local/videos/1",
        "https://ⓛocalhost/videos/1",
        "https://127。0。0。1/videos/1",
        // A name like any site's, that resolves to the local network.
        "https://router.example/videos/1",
    ] {
        assert_eq!(add_link(&app, url), refused, "{url}");
    }
    assert!(vods(&app).is_empty());
}

/// A ✕ that reaches the app before its add has begun is found by the add,
/// which then fails as canceled and adds nothing (vods.rs `cancel_add`:
/// "A cancel that gets here before its add has started is left for the add
/// to find"). The link is an address, so it needs no lookup, and the add is
/// canceled before the site is asked.
#[test]
fn a_cancel_that_comes_before_its_add_is_found_by_the_add() {
    let app = TestApp::new();
    app.invoke("cancel_add", json!({ "ticket": 7 })).unwrap();
    let added = app.invoke("add_vod", json!({ "url": "https://8.8.8.8/videos/1", "ticket": 7 }));
    assert_eq!(added, Err(json!("canceled")));
    assert!(vods(&app).is_empty());
}

/// A link to a talk on the fake site (`peertube.example`, public), which
/// serves its details: `title`, ten minutes long.
fn a_talk(app: &TestApp, id: &str, title: &str) -> String {
    app.net.public("peertube.example");
    let link = format!("https://peertube.example/w/{id}");
    app.yt_dlp.serves(yt_dlp::details("PeerTube", id, &link, title, 600.0));
    link
}

/// A VOD added from a site is in the library, and isn't transcribed till
/// asked (CHANGELOG.md, 0.6.0: "Adding a VOD doesn't transcribe it.";
/// `vods.rs` `add_vod`: "Not transcribed: that's asked for").
#[test]
fn a_site_vod_added_is_in_the_library_not_transcribed() {
    let app = TestApp::new();
    let link = a_talk(&app, "talk-1", "The talk");
    let added = add_link(&app, &link).unwrap();
    assert_eq!(added["created"], true);
    let library = vods(&app);
    assert_eq!(
        library.iter().map(|v| (v.info.url.as_str(), v.info.title.as_str())).collect::<Vec<_>>(),
        [(link.as_str(), "The talk")]
    );
    assert_eq!(added["vod"]["id"], library[0].id);
    assert!(app.db().requests(library[0].id).unwrap().is_empty(), "nothing asked of the transcriber");
    assert!(app.db().lines(library[0].id).unwrap().is_empty());
}

/// A VOD added goes to the top of the library (`db.rs` `add_vod`: "New VODs
/// go to the top of the library: their own clip does"), above what was
/// there, as the page lists it.
#[test]
fn a_vod_added_goes_to_the_top_of_the_library() {
    let app = TestApp::new();
    let before = app.add_video_file("Day 3.mp4", 60.0);
    let link = a_talk(&app, "talk-1", "The talk");
    let added = add_link(&app, &link).unwrap()["vod"]["id"].as_i64().unwrap();
    let listed = app.run(crate::vods::list_vods(app.handle().clone())).unwrap();
    assert_eq!(listed.iter().map(|v| v.id).collect::<Vec<_>>(), [added, before.id]);
}

/// A VOD added again is the one already there, left as it is
/// (`vods.rs` `add_to_library`: "one already there is left as it is").
#[test]
fn a_vod_added_twice_is_the_one_already_there() {
    let app = TestApp::new();
    let link = a_talk(&app, "talk-1", "The talk");
    let first = add_link(&app, &link).unwrap();
    app.says(first["vod"]["id"].as_i64().unwrap(), 12.0, "The Eagle has landed");
    let again = add_link(&app, &link).unwrap();
    assert_eq!((&again["created"], &again["vod"]["id"]), (&json!(false), &first["vod"]["id"]));
    let library = vods(&app);
    assert_eq!(library.len(), 1);
    assert_eq!(app.db().lines(library[0].id).unwrap().len(), 1, "its transcript as it was");
}

/// A link no site the video downloader knows serves is answered with why,
/// and nothing is added (e2e add-link-error.mjs: "A link VODscribe can't
/// use: an error under the box, and nothing added"), on the scenario's own
/// link.
#[test]
fn a_link_the_site_refuses_adds_nothing_and_says_why() {
    let app = TestApp::new();
    app.net.public("example.invalid");
    let said = add_link(&app, "https://example.invalid/not-a-vod");
    assert_eq!(
        said,
        Err(json!(
            "VODscribe can't use that link: it isn't a site the video downloader knows. Paste a link to a video page."
        ))
    );
    assert!(vods(&app).is_empty());
}

/// An add canceled while its link is looked up fails as canceled and adds
/// nothing (`vods.rs` `cancel_add`: "Stops looking up the link being
/// added under `ticket` (add_vod), which then fails as canceled";
/// CHANGELOG.md, 0.3.0: "Links and files being added show in the library
/// right away, with a spinner, and ✕ cancels one."). The site is held while
/// it looks the link up.
#[test]
fn an_add_canceled_while_it_is_looked_up_is_not_added() {
    let app = TestApp::new();
    let link = a_talk(&app, "talk-1", "The talk");
    let looking = Gate::new();
    app.yt_dlp.holds_details(&looking);
    // As the page awaits it, on a thread of its own.
    let handle = app.handle().clone();
    let adding = thread::spawn(move || tauri::async_runtime::block_on(crate::vods::add_vod(handle, link, Some(3))));
    looking.reached();
    app.invoke("cancel_add", json!({ "ticket": 3 })).unwrap();
    looking.open();
    assert_eq!(adding.join().unwrap().err().as_deref(), Some("canceled"));
    assert!(vods(&app).is_empty());
}

/// The file host is asked about one VOD at a time in the whole process
/// (annotations.rs `ASKING`, by the VOD's id), and tests run at once, each
/// library's first VOD numbered 1. A test that waits on its answer adds its
/// site VOD after `before` files, so it's numbered apart from the others'.
fn numbered_apart(app: &TestApp, before: usize) {
    for i in 0..before {
        app.add_video_file(&format!("Before {i}.mp4"), 60.0);
    }
}

/// The file host publishes a VODscription of YouTube video `id`, in version `etag`.
fn publishes(app: &TestApp, id: &str, etag: &'static str) {
    app.net.replies(&format!("https://dl.vodscribe.com/annotations/youtube/{id}.json"), move |asked| {
        (asked.method == "HEAD").then(|| Reply::status(200).with_header("ETag", etag))
    });
}

/// A VOD added from a site is offered its VODscription once the file host
/// is found to have one (`vods.rs` `add_vod`, `look_up_added`; CHANGELOG.md, 0.7.0:
/// "Adding a VOD offers its annotations."): the page hears it's found.
#[test]
fn a_vod_added_from_a_site_is_offered_its_vodscription_once_found() {
    let app = TestApp::new();
    numbered_apart(&app, 1);
    let link = "https://www.youtube.com/watch?v=added000001";
    app.yt_dlp.serves(yt_dlp::details("youtube", "added000001", link, "A stream", 600.0));
    publishes(&app, "added000001", "\"v1\"");
    let id = add_link(&app, link).unwrap()["vod"]["id"].as_i64().unwrap();
    // The look-up runs beside the add, which didn't wait for it.
    app.threads_done();
    assert_eq!(app.heard("annotations-found"), [json!(id)]);
}

/// A VOD added from a site has its chat fetched in the background (`vods.rs`
/// `add_vod`; CHANGELOG.md, 0.2.0: "Chat: for Kick, Twitch and YouTube
/// livestreams, the chat replay sits beside the transcript … It's fetched
/// in the background"), here a Twitch VOD's, from Twitch's API.
#[test]
fn a_vod_added_from_a_site_has_its_chat_fetched() {
    let app = TestApp::new();
    app.fetches_chats();
    let link = "https://www.twitch.tv/videos/2901646294";
    app.yt_dlp.serves(yt_dlp::details("twitch", "v2901646294", link, "A stream", 300.0));
    app.net.twitch_chat("2901646294", &[(10.0, "ada", "hello"), (20.0, "bo", "o7")]);
    let id = add_link(&app, link).unwrap()["vod"]["id"].as_i64().unwrap();
    eventually("the chat is fetched", || app.db().vod(id).unwrap().unwrap().chat == ChatStatus::Done);
    let shown = app.invoke("chat_windows", json!({ "vodId": id, "ranges": [[0.0, 300.0]], "cap": 10 })).unwrap();
    let said: Vec<(f64, &str)> = shown[0]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["time"].as_f64().unwrap(), m["author"].as_str().unwrap()))
        .collect();
    assert_eq!(said, [(10.0, "ada"), (20.0, "bo")]);
}

/// Files dropped on the window or picked are added as VODs that play from
/// where they are, and the files are left as they were (vods.rs
/// `add_files`: "They're played and transcribed in place and never
/// modified").
#[test]
fn files_dropped_or_picked_are_added_and_played_where_they_are_never_changed() {
    let app = TestApp::new();
    let (_dir, files) = user_files(&["Day 3.mp4", "Day 4.mkv"], &Media::video(60.0));
    let before = files.iter().map(|f| std::fs::read(f).unwrap()).collect::<Vec<_>>();
    let added = add_files(&app, &files);
    assert_eq!(added.errors, Vec::<String>::new());
    assert_eq!(added.vods.len(), 2);
    for (vod, file) in added.vods.iter().zip(&files) {
        let plays = app.db().playable_path(vod.id).unwrap().expect("it plays from a file");
        assert!(same_file(&plays, file), "VOD {} plays {plays}, not {file:?}", vod.id);
        assert!(vod.source_path.as_deref().is_some_and(|p| same_file(p, file)));
    }
    let after = files.iter().map(|f| std::fs::read(f).unwrap()).collect::<Vec<_>>();
    assert_eq!(after, before, "a file was changed");
    let told = app.heard("vod").iter().map(|v| v["id"].as_i64().unwrap()).collect::<Vec<_>>();
    assert!(added.vods.iter().all(|v| told.contains(&v.id)), "the page told of each: {told:?}");
}

/// Of files added together, each that can't be added says why in a message
/// of its own, naming it, and the others are added (vods.rs
/// `AddedFiles::errors`: "One message per file that couldn't be added").
#[test]
fn each_file_that_cannot_be_added_says_why_and_the_others_are_added() {
    let app = TestApp::new();
    let (dir, mut files) = user_files(&["Day 3.mp4", "Day 4.mp4"], &Media::video(60.0));
    std::fs::write(dir.join("notes.txt"), "not a video").unwrap();
    files.insert(1, dir.join("notes.txt"));
    files.push(dir.join("Missing.mp4"));
    let added = add_files(&app, &files);
    assert_eq!(added.vods.len(), 2, "{:?}", added.errors);
    assert!(same_file(added.vods[0].source_path.as_deref().unwrap(), &files[0]));
    assert!(same_file(added.vods[1].source_path.as_deref().unwrap(), &files[2]));
    assert_eq!(added.errors.len(), 2, "{:?}", added.errors);
    assert!(added.errors[0].contains("notes.txt"), "{}", added.errors[0]);
    assert!(added.errors[1].contains("Missing.mp4"), "{}", added.errors[1]);
    assert_eq!(vods(&app).len(), 2);
}

/// Several files added at once are all added: in one drop, and in drops
/// made at the same time, none fails for another's add (CHANGELOG.md, 0.4.0
/// Fixes: "Adding several files at once could fail with "database is
/// locked".").
#[test]
fn several_files_added_at_once_are_all_added() {
    let app = TestApp::new();
    let names = (1..=12).map(|n| format!("Day {n}.mp4")).collect::<Vec<_>>();
    let (_dir, files) = user_files(&names.iter().map(String::as_str).collect::<Vec<_>>(), &Media::video(60.0));
    let (one_drop, drops) = files.split_at(4);
    let answers = thread::scope(|s| {
        let at_once = drops
            .chunks(2)
            .map(|drop| {
                let (handle, paths) = (app.handle().clone(), as_strings(drop));
                s.spawn(move || tauri::async_runtime::block_on(crate::vods::add_files(handle, paths)).unwrap())
            })
            .collect::<Vec<_>>();
        let mut answers = vec![add_files(&app, one_drop)];
        answers.extend(at_once.into_iter().map(|t| t.join().unwrap()));
        answers
    });
    for answer in &answers {
        assert_eq!(answer.errors, Vec::<String>::new());
    }
    assert_eq!(answers.iter().map(|a| a.vods.len()).sum::<usize>(), 12);
    let titles = vods(&app).into_iter().map(|v| v.info.title).collect::<std::collections::BTreeSet<_>>();
    let wanted = names.iter().map(|n| n.trim_end_matches(".mp4").to_string()).collect();
    assert_eq!(titles, wanted);
}

/// A file ffmpeg can't read is refused, saying so, rather than added as a
/// silent VOD (CHANGELOG.md, 0.4.0 Fixes: "A file ffmpeg can't read passed
/// as silent instead of failing.").
#[test]
fn a_file_ffmpeg_cannot_read_is_refused_not_added_as_silence() {
    let app = TestApp::new();
    let dir = TempDir::new("user-files");
    let broken = dir.join("Broken.mp4");
    std::fs::write(&broken, "half a download, cut off").unwrap();
    let added = add_files(&app, std::slice::from_ref(&broken));
    assert!(added.vods.is_empty());
    assert_eq!(added.errors.len(), 1);
    assert!(added.errors[0].contains("Broken.mp4"), "{}", added.errors[0]);
    assert!(vods(&app).is_empty());
}

/// A video without sound can be added, to play and edit (CHANGELOG.md,
/// 0.4.0: "VODscribe's own player plays your files …": "Videos without
/// sound can be added.").
#[test]
fn a_video_without_sound_can_be_added() {
    let app = TestApp::new();
    let silent = Media { audio: None, ..Media::video(60.0) };
    let (_dir, files) = user_files(&["Screen.mp4"], &silent);
    let added = add_files(&app, &files);
    assert_eq!(added.errors, Vec::<String>::new());
    assert_eq!(added.vods.len(), 1);
    assert!(!added.vods[0].audio_only, "a video");
}

/// A sound file is added as sound alone, which plays in a slim bar
/// (CHANGELOG.md, 0.3.0: "Audio alone (a podcast, a song, an audio file)
/// plays in a slim bar instead of an empty video frame."; db.rs
/// `Vod::audio_only`: "the player needs no picture"). A video beside it isn't.
#[test]
fn a_sound_file_is_added_as_sound_alone() {
    let app = TestApp::new();
    let (_dir, sound) = user_files(&["Talk.m4a"], &Media::sound(60.0));
    let (_other, video) = user_files(&["Day 3.mp4"], &Media::video(60.0));
    let added = add_files(&app, &[sound[0].clone(), video[0].clone()]);
    assert_eq!(added.errors, Vec::<String>::new());
    let audio_only = added.vods.iter().map(|v| v.audio_only).collect::<Vec<_>>();
    assert_eq!(audio_only, [true, false]);
}

/// A file of the app's own data (a clip's file, a kept video, one received
/// from another app's drag) can't be added; one in the kept videos' folder
/// is only when it's received there, as another app's drag gives it
/// (vods.rs `add_local`: "one in the app's own data only if it's one
/// `received` (into the kept videos' folder)"). Received, it's the app's
/// own copy (`Vod::received`).
#[test]
fn a_file_of_the_apps_own_data_is_added_only_if_it_was_received() {
    let app = TestApp::new();
    let paths = app.paths();
    let kept = paths.media().join("4.mp4");
    let clip = paths.clips().join("Day 3 0.10.mp4");
    let arrived = paths.received_incoming().join("Day 3.mp4");
    Media::video(60.0).write(&kept);
    Media::video(10.0).write(&clip);
    Media::video(60.0).write(&arrived);
    let added = add_files(&app, &[kept.clone(), clip.clone(), arrived.clone()]);
    assert!(added.vods.is_empty(), "{:?}", added.vods.iter().map(|v| &v.source_path).collect::<Vec<_>>());
    assert_eq!(added.errors.len(), 3);
    for (error, name) in added.errors.iter().zip(["4.mp4", "Day 3 0.10.mp4", "Day 3.mp4"]) {
        assert!(error.contains(name) && error.ends_with("is part of VODscribe's own data."), "{error}");
    }
    assert!(crate::vods::add_received(app.handle(), &clip).is_err(), "a clip's file taken as received");
    let id = crate::vods::add_received(app.handle(), &arrived).unwrap();
    let vod = app.db().vod(id).unwrap().unwrap();
    assert!(vod.received && vod.listed);
    assert_eq!(vods(&app).len(), 1);
}

/// A second file received from a drop under the same name as the first is
/// a VOD of its own, each with its own file, not the first found again
/// (core db/tests.rs `a_second_file_received_under_the_same_name_is_a_new_vod`,
/// moved up to the drop: its doc, "Files received from drops arrive under
/// the name their app gave, which is free again once the first is renamed
/// for its VOD: a second file of that name is a VOD of its own").
#[test]
fn a_second_file_received_under_the_same_name_is_a_new_vod() {
    let app = TestApp::new();
    let received = |media: &Media| {
        let mut file = app.drop_files(&[("Day 3.mp4", None)]).remove(0);
        file.give(&media.to_bytes()).unwrap();
        file.whole();
        app.threads_done();
        let opened = app.heard("received").last().cloned().unwrap();
        assert_eq!(opened["kind"], "open", "{opened}");
        app.db().vod(opened["vod"].as_i64().unwrap()).unwrap().unwrap()
    };
    let first = received(&Media::video(60.0));
    let second = received(&Media::video(90.0));
    assert_ne!(first.id, second.id, "the first file's VOD found again");
    let length = |vod: &Vod| Media::read(Path::new(vod.source_path.as_deref().unwrap())).map(|m| m.duration);
    assert_eq!((length(&first), length(&second)), (Some(60.0), Some(90.0)));
    assert_eq!(vods(&app).len(), 2);
}

/// yt-dlp's details of site VOD `vod`, naming no channel.
fn details_of(vod: &Vod) -> Value {
    let info = &vod.info;
    yt_dlp::details(&info.platform, &info.platform_id, &info.url, &info.title, info.duration.unwrap())
}

/// `details_of` VOD `vod`, its channel `login` named, with its page.
fn with_channel(vod: &Vod, login: &str) -> Value {
    let mut details = details_of(vod);
    details["uploader"] = json!(login);
    details["uploader_id"] = json!(login);
    details["uploader_url"] = json!(format!("https://peertube.example/c/{login}"));
    details
}

/// A VOD of the fake site, ten minutes long, its channel unknown.
fn site_vod(app: &TestApp) -> Vod {
    app.add_site_vod("peertube", "talk-1", "The talk", 600.0)
}

/// `refresh_details`'s answer for VOD `id`, as the page gets it.
fn refresh(app: &TestApp, id: i64) -> CmdResult<Option<Vod>> {
    app.run(crate::vods::refresh_details(app.handle().clone(), id))
}

/// VOD `id` as the library has it, as the page reads it.
fn as_read(app: &TestApp, id: i64) -> Value {
    serde_json::to_value(app.db().vod(id).unwrap().unwrap()).unwrap()
}

/// Refresh details gets the channel's link the site names now, keeps it
/// on the VOD and tells the page; and it asks the file host again, which
/// offers a VODscription published since (CHANGELOG.md, 0.7.2: "Refresh a
/// VOD's details." ("gets its channel's link and any VODscription or
/// streamer card published since"); `vods.rs` `refresh_details`). The
/// streamer card is the page's to ask for (streamers.ts).
#[test]
fn refresh_details_gets_the_channels_link_and_asks_the_file_host_again() {
    let app = TestApp::new();
    let vod = site_vod(&app);
    app.yt_dlp.serves(with_channel(&vod, "quill"));
    let refreshed = refresh(&app, vod.id).unwrap().expect("refreshed");
    let channel = (refreshed.info.channel_login.as_deref(), refreshed.info.channel_url.as_deref());
    assert_eq!(channel, (Some("quill"), Some("https://peertube.example/c/quill")));
    let kept = app.db().vod(vod.id).unwrap().unwrap();
    assert_eq!((kept.info.channel_login, kept.info.channel_url), (Some("quill".into()), refreshed.info.channel_url));
    let told = app.heard("vod");
    assert_eq!(told.last().map(|v| (&v["id"], &v["channel_login"])), Some((&json!(vod.id), &json!("quill"))));

    // A YouTube VOD added before its VODscription was published.
    numbered_apart(&app, 1);
    let video = app.add_site_vod("youtube", "refresh0001", "A stream", 600.0);
    publishes(&app, "refresh0001", "\"v1\"");
    refresh(&app, video.id).unwrap().expect("refreshed");
    // The file host's answer comes after the refresh's, which didn't wait for it.
    app.threads_done();
    assert_eq!(app.heard("annotations-found"), [json!(video.id)]);
}

/// A refresh loses nothing the VOD has: a site that names no channel now
/// leaves the one it had, and a refresh that fails changes nothing
/// (CHANGELOG.md, 0.7.2: "Refresh a VOD's details." ("Nothing the VOD has
/// is lost."); vods.rs `refresh_details`: "a failure changes nothing").
#[test]
fn refresh_details_loses_nothing_the_vod_has() {
    let app = TestApp::new();
    let vod = site_vod(&app);
    app.says(vod.id, 12.0, "The Eagle has landed");
    app.yt_dlp.serves(with_channel(&vod, "quill"));
    refresh(&app, vod.id).unwrap().unwrap();
    let had = as_read(&app, vod.id);

    app.yt_dlp.serves(details_of(&vod));
    let answer = refresh(&app, vod.id).unwrap().expect("refreshed");
    assert_eq!(answer.info.channel_login.as_deref(), Some("quill"), "the channel the site no longer names");
    assert_eq!(as_read(&app, vod.id), had);

    let mut another = with_channel(&vod, "someone-else");
    another["id"] = json!("talk-2");
    app.yt_dlp.serves(another);
    assert!(refresh(&app, vod.id).is_err(), "the site answered for another video");
    assert_eq!(as_read(&app, vod.id), had);
    let said = app.db().segments(vod.id).unwrap();
    assert_eq!(said.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["The Eagle has landed"]);
}

/// A refresh runs beside the VOD's video downloading, and leaves it to
/// finish: the refresh is answered while the download is under way, and the
/// download then keeps its video (vods.rs `refresh_details`: "It runs
/// beside a download or transcription of the VOD without touching either").
#[test]
fn refresh_details_runs_beside_a_download_or_transcription_without_touching_either() {
    let app = TestApp::new();
    let vod = site_vod(&app);
    let downloading = Gate::new();
    app.yt_dlp.holds(&downloading);
    app.invoke("keep_video", json!({ "id": vod.id })).unwrap();
    downloading.reached();
    app.yt_dlp.serves(with_channel(&vod, "quill"));
    let refreshed = refresh(&app, vod.id).unwrap().expect("refreshed beside the download");
    assert_eq!(refreshed.info.channel_login.as_deref(), Some("quill"));
    assert!(!app.db().vod(vod.id).unwrap().unwrap().media, "kept before its download was let go");
    downloading.open();
    eventually("its video kept", || app.db().vod(vod.id).unwrap().unwrap().media);
    let kept = app.db().vod(vod.id).unwrap().unwrap();
    assert_eq!(kept.info.channel_login.as_deref(), Some("quill"));
    let ended = app.heard("video-progress").into_iter().rfind(|p| p["id"] == vod.id).unwrap();
    assert_eq!(ended["error"], Value::Null, "{ended}");
}

/// A second Refresh details of a VOD while its first is under way gets
/// nothing to do; another VOD's goes ahead, and once the first ends, its
/// next does (vods.rs `refresh_details`: "None when one for the VOD is
/// under way already: a second press does nothing"). At `Refreshing::claim`:
/// at the command it needs yt-dlp's details held at a gate, which the fake
/// doesn't do yet.
#[test]
fn a_second_refresh_of_a_vod_under_way_does_nothing_and_another_vods_goes_ahead() {
    let under_way = Mutex::default();
    let first = Refreshing::claim(&under_way, 7).unwrap();
    assert!(Refreshing::claim(&under_way, 7).is_none());
    assert!(Refreshing::claim(&under_way, 8).is_some());
    drop(first);
    assert!(Refreshing::claim(&under_way, 7).is_some());
}

/// The file picker offers the extensions VODscribe plays, and a file of each
/// is added (vods.rs `media_extensions`: "The file extensions VODscribe
/// accepts, for the file picker"). Decided for 0.7.4: the test pins
/// today's list.
#[test]
fn the_file_picker_takes_the_extensions_vodscribe_plays() {
    let app = TestApp::new();
    let offered = app.invoke("media_extensions", json!({})).unwrap();
    let today =
        ["mp4", "m4v", "mkv", "webm", "mov", "avi", "ts", "flv", "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus"];
    assert_eq!(offered, json!(today));
    let names = today.iter().map(|ext| format!("File.{ext}")).collect::<Vec<_>>();
    let (_dir, files) = user_files(&names.iter().map(String::as_str).collect::<Vec<_>>(), &Media::video(60.0));
    let added = add_files(&app, &files);
    assert_eq!(added.errors, Vec::<String>::new());
    assert_eq!(added.vods.len(), today.len());
}
