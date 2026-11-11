//! Adding VODs, and refreshing their details: `add_vod` and `cancel_add`
//! (a link pasted), `add_files` (files dropped or picked), `media_extensions`
//! (the file picker's), and `refresh_details` (a site VOD's menu), through the
//! commands as the page calls them, and what they leave in the library.
//!
//! A link's host is resolved before anything else (`net::public_url`), so a
//! link with a site's name would be looked up for real: those specs wait for
//! a faked resolver. The links here need no lookup: an address, or a name
//! that is this computer's or the local network's by its form alone.
//!
//! The site in the refresh specs is a fake one (`peertube.example`): its
//! channel comes from yt-dlp's details, as for any site without an API of
//! its own the app asks, and no VODscription is published for it, so no
//! file host is asked.

use std::path::{Path, PathBuf};
use std::thread;

use serde_json::{Value, json};
use vodscribe_core::testkit::{Gate, Media, TempDir, eventually, yt_dlp};

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

/// `add_files`'s answer for `paths`, as the page gets it, once each VOD's
/// picture is asked for (`pictures_asked_for`).
fn add_files(app: &TestApp, paths: &[PathBuf]) -> AddedFiles {
    let added = app.run(crate::vods::add_files(app.handle().clone(), as_strings(paths))).unwrap();
    pictures_asked_for(app, &added.vods.iter().map(|v| v.id).collect::<Vec<_>>());
    added
}

/// Waits till the picture of each file's VOD in `ids`, just added, has
/// been asked for: its storyboard begun, or the VOD known to have no
/// picture. The add asks on a thread of its own (vods.rs `announce_file`,
/// `thumbnails::cache_thumbnail`) that reads the app's state, so the app
/// mustn't end before it has (the kit ends the app's queues, not that thread).
fn pictures_asked_for(app: &TestApp, ids: &[i64]) {
    let boards = app.paths().storyboards();
    for &id in ids {
        let begun = || {
            let named = format!("{id}-");
            std::fs::read_dir(&boards)
                .into_iter()
                .flatten()
                .flatten()
                .any(|f| f.file_name().to_string_lossy().starts_with(&named))
        };
        let none = || app.db().vod(id).unwrap().is_some_and(|v| v.thumb.is_some());
        eventually(&format!("VOD {id}'s picture asked for"), || begun() || none());
    }
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
/// and adds nothing (vods.rs:55, `add_vod`: "Paste a VOD link first.").
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
/// resolves as (vods.rs:68, `add_vod`: "Checked, and passed on as the name
/// it resolves as"), so one written in look-alike characters is refused too.
#[test]
fn a_link_to_this_computer_or_the_local_network_is_refused() {
    let app = TestApp::new();
    let refused = Err(json!("VODscribe doesn't follow links to this computer or the local network."));
    for url in [
        "http://127.0.0.1:1420/media/1",
        "https://localhost/videos/1",
        "https://[::1]/videos/1",
        "http://192.168.1.10/videos/1",
        "https://nas.local/videos/1",
        "https://ⓛocalhost/videos/1",
        "https://127。0。0。1/videos/1",
    ] {
        assert_eq!(add_link(&app, url), refused, "{url}");
    }
    assert!(vods(&app).is_empty());
}

/// A ✕ that reaches the app before its add has begun is found by the add,
/// which then fails as canceled and adds nothing (vods.rs:42, `cancel_add`:
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

/// Files dropped on the window or picked are added as VODs that play from
/// where they are, and the files are left as they were (vods.rs:167,
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
/// of its own, naming it, and the others are added (vods.rs:163,
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
        pictures_asked_for(&app, &answer.vods.iter().map(|v| v.id).collect::<Vec<_>>());
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
/// (vods.rs:217, `add_local`: "one in the app's own data only if it's one
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
    pictures_asked_for(&app, &[id]);
    let vod = app.db().vod(id).unwrap().unwrap();
    assert!(vod.received && vod.listed);
    assert_eq!(vods(&app).len(), 1);
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
/// on the VOD and tells the page (CHANGELOG.md, 0.7.2: "Refresh a VOD's
/// details." ("gets its channel's link"); vods.rs:133, `refresh_details`).
/// The file host's half (asked again about its VODscription) isn't here:
/// that host is reached outside the faked network.
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
}

/// A refresh loses nothing the VOD has: a site that names no channel now
/// leaves the one it had, and a refresh that fails changes nothing
/// (CHANGELOG.md, 0.7.2: "Refresh a VOD's details." ("Nothing the VOD has
/// is lost."); vods.rs:140, `refresh_details`: "a failure changes nothing").
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
/// download then keeps its video (vods.rs:138, `refresh_details`: "It runs
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
/// next does (vods.rs:140, `refresh_details`: "None when one for the VOD is
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
/// is added (vods.rs:310, `media_extensions`: "The file extensions
/// VODscribe accepts, for the file picker"). The list is today's (handbook
/// plans/sections-074.md, "Decided", unstated item 4: "the test pins
/// today's list").
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
