//! The storage commands, as Settings calls them: the size of the downloads
//! and Repair VODscribe, which deletes them (`cache_size`, `clear_cache`,
//! Settings › Help); the size of the kept videos (`media_size`, Settings ›
//! Storage); the data folder, named and opened (`data_dir`,
//! `open_data_dir`, Settings › Privacy); and what this computer plays
//! (`set_playable`, playable.ts at each start). Which folders are the
//! downloads, and what each size counts, is core lib.rs's `Paths`,
//! specified in core tests/layout.rs.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vodscribe_core::db::Status;
use vodscribe_core::playable::Playable;
use vodscribe_core::testkit::{Gate, eventually, said};

use crate::testkit::TestApp;

/// Bytes the command `cmd` answers (`cache_size`, `media_size`).
fn bytes(app: &TestApp, cmd: &str) -> u64 {
    app.invoke(cmd, json!({})).unwrap().as_u64().unwrap()
}

/// Repair VODscribe… answered Repair (`clear_cache`).
fn repair(app: &TestApp) -> Result<Value, Value> {
    app.invoke("clear_cache", json!({}))
}

/// A VOD from a site without its own player, its video kept (Download
/// video, `keep_video`): its id and the file kept.
fn kept_video(app: &TestApp) -> (i64, PathBuf) {
    let vod = app.add_site_vod("peertube", "talk-1", "The talk", 600.0);
    app.invoke("keep_video", json!({ "id": vod.id })).unwrap();
    eventually("its video kept", || app.db().vod(vod.id).unwrap().unwrap().media);
    let named = |f: &PathBuf| f.file_name().unwrap().to_string_lossy().starts_with(&format!("{}.", vod.id));
    let files: Vec<PathBuf> =
        std::fs::read_dir(app.paths().media()).unwrap().flatten().map(|e| e.path()).filter(named).collect();
    let [file] = files.try_into().unwrap();
    (vod.id, file)
}

fn size(file: &Path) -> u64 {
    std::fs::metadata(file).unwrap().len()
}

/// The size Settings gives for Repair is of what Repair deletes, the
/// speech model among it, and not the kept videos: after a repair it's
/// none, the kept video still there (setup.rs `cache_size`: "Bytes of
/// downloaded files that `clear_cache` would delete"; Settings.svelte:
/// "Repair deletes the speech model, tools and unfinished downloads (… in
/// all)").
#[test]
fn the_size_repair_gives_is_what_it_deletes() {
    let app = TestApp::new();
    app.worker();
    let model = bytes(&app, "cache_size");
    assert!(model > 0, "the speech model isn't counted");
    let (_, video) = kept_video(&app);
    repair(&app).unwrap();
    assert_eq!(bytes(&app, "cache_size"), 0);
    assert_eq!((bytes(&app, "media_size"), video.is_file()), (size(&video), true), "the kept video");
}

/// Repair keeps the library: its VODs, their transcripts and clips, the
/// kept videos and the settings (setup.rs `clear_cache`: "The library,
/// transcripts and settings are kept"; Settings.svelte, Repair's question:
/// "Your library, transcripts, chats and kept videos stay").
#[test]
fn repair_keeps_the_library_transcripts_settings_and_kept_videos() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let (site, video) = kept_video(&app);
    let mut settings = app.invoke("get_settings", json!({})).unwrap();
    settings["theme"] = json!("dark");
    app.invoke("set_settings", json!({ "settings": settings })).unwrap();
    let library = |app: &TestApp| {
        let vods = app.invoke("list_vods", json!({})).unwrap();
        let vods: Vec<_> = vods.as_array().unwrap().iter().map(|v| (v["id"].clone(), v["title"].clone())).collect();
        (vods, app.invoke("transcript", json!({ "vodId": vod.id })).unwrap(), app.clips(vod.id))
    };
    let before = library(&app);
    let video_bytes = std::fs::read(&video).unwrap();

    repair(&app).unwrap();
    assert_eq!(library(&app), before);
    assert!(Path::new(clip.file_path.as_deref().unwrap()).is_file(), "the clip's file");
    assert!(app.db().vod(site).unwrap().unwrap().media, "the kept video's");
    assert_eq!(std::fs::read(&video).unwrap(), video_bytes);
    assert_eq!(app.invoke("get_settings", json!({})).unwrap(), settings);
}

/// After a repair, setup has the tools and the speech model to download
/// again (setup.rs `clear_cache`: "setup runs again afterwards";
/// Settings.svelte: "then sets VODscribe up again").
#[test]
fn after_a_repair_setup_downloads_the_tools_and_model_again() {
    let app = TestApp::new();
    app.worker();
    let has = |app: &TestApp| {
        let status = app.invoke("setup_status", json!({})).unwrap();
        (status["yt_dlp"].clone(), status["ffmpeg"].clone(), status["model"].clone())
    };
    assert_eq!(has(&app), (json!(true), json!(true), json!(true)));
    repair(&app).unwrap();
    assert_eq!(has(&app), (json!(false), json!(false), json!(false)));
}

/// A transcription under way is paused by a repair, its lines so far kept
/// (setup.rs `clear_cache`: "pausing running jobs first"; Settings.svelte,
/// Repair's question: "Anything in progress is paused"). The worker is
/// held after its first minute.
#[test]
fn repair_pauses_a_transcription_under_way_keeping_its_lines() {
    let app = TestApp::new();
    app.worker().hears([said(10.0, "The Eagle has landed."), said(300.0, "Said later.")].concat());
    let vod = app.add_video_file("Ten minutes.mp4", 600.0);
    let held = Gate::new();
    app.worker().holds(&held);
    let asked = app.invoke("transcribe_stretch", json!({ "vodId": vod.id, "start": 0.0, "end": null })).unwrap();
    let request = asked["id"].as_i64().unwrap();
    held.reached();
    eventually("its first minute written", || app.db().requests(vod.id).unwrap().iter().any(|s| s.reached.is_some()));

    repair(&app).unwrap();
    assert_eq!(app.db().stretch(request).unwrap().unwrap().status, Status::Paused);
    let lines: Vec<_> = app.db().lines(vod.id).unwrap().into_iter().map(|l| (l.start, l.text)).collect();
    assert_eq!(lines, [(10.0, "The Eagle has landed.".to_string())]);
    held.open();
}

/// Where setup downloads the speech model from.
const MODELS: &str = "https://dl.vodscribe.com/models";

/// Repair waits for setup's download to finish, deleting nothing meanwhile
/// (setup.rs `clear_cache`: "Wait for the current download to finish
/// first."; "so setup, a yt-dlp update or the GPU add-on can't start
/// downloading into folders that are being deleted"). The download is held
/// at the speech model, which nothing answers.
#[test]
fn repair_waits_for_setups_download_to_finish() {
    let _turn = super::turn();
    let app = super::app();
    let held = Gate::new();
    app.net.holds(MODELS, &held);
    app.invoke("run_setup", json!({})).unwrap();
    held.reached();
    assert_eq!(repair(&app), Err(json!("Wait for the current download to finish first.")));
    assert!(app.ffmpeg.path().is_file() && app.yt_dlp.path().is_file(), "the tools deleted");
    assert!(app.state().setup.running(), "setup's download let go");
    held.open();
    eventually("setup ended", || !app.state().setup.running());
    repair(&app).unwrap();
}

/// Where the GPU add-on's pack downloads from (core gpu.rs), on the
/// systems it's offered on.
#[cfg(all(any(target_os = "linux", target_os = "windows"), target_arch = "x86_64"))]
const GPU_PACK: &str = "https://github.com/k2-fsa/";

/// Repair waits for the GPU add-on's download to finish too (setup.rs
/// `clear_cache`, as above). The download is held at its pack.
#[cfg(all(any(target_os = "linux", target_os = "windows"), target_arch = "x86_64"))]
#[test]
fn repair_waits_for_the_gpu_add_ons_download_to_finish() {
    let _turn = super::turn();
    let app = super::app();
    let held = Gate::new();
    app.net.holds(GPU_PACK, &held);
    app.invoke("install_gpu_pack", json!({})).unwrap();
    held.reached();
    assert_eq!(repair(&app), Err(json!("Wait for the current download to finish first.")));
    assert!(app.ffmpeg.path().is_file(), "the tools deleted");
    assert!(app.state().gpu.running() && !app.state().setup.running(), "the add-on's download let go");
    held.open();
    eventually("the add-on's download ended", || !app.state().gpu.running());
    repair(&app).unwrap();
}

/// Settings › Storage gives the space the kept videos take: none at first,
/// then what the kept one holds (setup.rs `media_size`: "Bytes of the
/// videos kept from sites without embeds"; CHANGELOG.md, 0.3.0: "Settings →
/// Storage chooses 480p to 1080p and shows the space they take"). What the
/// size counts is core tests/layout.rs's
/// (`the_kept_videos_are_counted_apart_from_the_cache`).
#[test]
fn the_kept_videos_size_is_the_space_they_take() {
    let app = TestApp::new();
    assert_eq!(bytes(&app, "media_size"), 0);
    let (_, video) = kept_video(&app);
    assert_eq!(bytes(&app, "media_size"), size(&video));
}

/// Settings › Privacy names the folder the library and settings are in
/// (setup.rs `data_dir`: "Where the library and settings live, for the
/// privacy section in Settings").
#[test]
fn the_folder_named_is_where_the_library_and_settings_are() {
    let app = TestApp::new();
    let named = app.invoke("data_dir", json!({})).unwrap();
    let folder = PathBuf::from(named.as_str().unwrap());
    let paths = app.paths();
    assert_eq!((paths.db().parent(), paths.settings().parent()), (Some(folder.as_path()), Some(folder.as_path())));
    assert!(paths.db().is_file(), "no library there");
}

/// Open folder opens the data folder, and nothing else whatever the page
/// sends (setup.rs `open_data_dir`: "Opens the data folder in the system
/// file manager. It takes no path, so the UI can't use it to open anything
/// else").
#[test]
fn open_folder_opens_the_data_folder_and_nothing_else() {
    let app = TestApp::new();
    let elsewhere = app.paths().root.with_file_name("elsewhere");
    app.invoke("open_data_dir", json!({})).unwrap();
    app.invoke("open_data_dir", json!({ "path": elsewhere })).unwrap();
    assert_eq!(app.desktop().opened(), [app.paths().root.clone(), app.paths().root.clone()]);
}

/// What this computer plays, as the page says at each start, is kept for
/// the downloads, which may run with no window open: the last said, also
/// after a restart (setup.rs `set_playable`: "Saves what this computer's
/// webview plays (playable.ts asks it at each start)"; core playable.rs:
/// "the answer is saved here, so downloads, which may run with no window
/// open, pick a format that plays"). How a download picks from it is core
/// playable.rs's (`Playable::selector`).
#[test]
fn what_this_computer_plays_is_kept_for_the_downloads() {
    let app = TestApp::new();
    let said = |app: &TestApp, playable: Value| {
        app.invoke("set_playable", json!({ "playable": playable })).unwrap();
        serde_json::from_value::<Playable>(playable).unwrap()
    };
    let kept = |app: &TestApp| Playable::load(&app.paths().playable());
    assert_eq!(kept(&app), Playable::default(), "before the page has said");
    let first = said(&app, json!({ "video": [["av1", "opus"], ["h264", "aac"]], "audio": ["opus"] }));
    assert_eq!(kept(&app), first);
    let app = TestApp::on(app.quit());
    assert_eq!(kept(&app), first, "after a restart");
    let next = said(&app, json!({ "video": [["vp9", "opus"]], "audio": ["vorbis"] }));
    assert_eq!(kept(&app), next);
}
