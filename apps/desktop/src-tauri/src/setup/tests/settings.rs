//! The settings commands, as the page calls them: `get_settings` and
//! `set_settings` (appSettings.svelte.ts loads them once and saves them
//! whole), `reset_settings` (Settings › Help), `settings_set_aside` (the
//! toast at the start) and `missing_clip_codecs` (Settings › Encoding).
//! What the settings file holds, and how each version's reads, is core
//! settings.rs's, specified in its tests.

use serde_json::{Value, json};
use tauri::Manager;
use vodscribe_core::settings::Settings;
use vodscribe_core::testkit::tools as fakes;
use vodscribe_core::testkit::{Ffmpeg, Gate};

use crate::testkit::{Folder, TestApp};

/// The settings as the page gets them (`get_settings`).
fn settings(app: &TestApp) -> Value {
    app.invoke("get_settings", json!({})).unwrap()
}

/// Every setting as it starts, as the page gets them on a new library.
fn every_default() -> Value {
    serde_json::to_value(Settings::default().normalized()).unwrap()
}

/// `change` made in Settings, as the page saves it: the settings it has,
/// with the change over them, sent whole (`set_settings`). Returns what
/// was sent.
fn change(app: &TestApp, change: Value) -> Value {
    let mut whole = settings(app);
    for (key, value) in change.as_object().unwrap() {
        whole[key] = value.clone();
    }
    app.invoke("set_settings", json!({ "settings": whole })).unwrap();
    whole
}

/// The settings file as it is written.
fn written(app: &TestApp) -> Value {
    serde_json::from_slice(&std::fs::read(app.paths().settings()).unwrap()).unwrap()
}

/// Choices made in every section of Settings, as settings.mjs makes them:
/// what's done with a VODscription found, a theme, a video quality, a clip
/// format and quality, a note stopped and a key changed.
fn chosen() -> Value {
    json!({
        "use_vodscription": "always",
        "theme": "dark",
        "video_height": 480,
        "clip_codec": "prores",
        "clip_quality": "high",
        "ask_before_downloading": false,
        "key_bindings": { "play": ["k", "p"] },
    })
}

/// Settings chosen are what Settings reads next, and are still chosen after
/// a restart (e2e settings.mjs: "a video quality and a clip format and
/// quality chosen in Settings are still chosen after a restart";
/// appSettings.svelte.ts: "loaded once, and saved whole on each change").
#[test]
fn settings_chosen_are_read_back_and_kept_after_a_restart() {
    let app = TestApp::new();
    let mut sent = change(&app, chosen());
    // As settings.mjs reads them: the codec's file, and the switch before
    // the three answers written to match.
    sent["clip_format"] = json!("mov");
    sent["ask_to_use_vodscription"] = json!(false);
    assert_eq!(settings(&app), sent);
    let app = TestApp::on(app.quit());
    assert_eq!(settings(&app), sent, "after a restart");
}

/// A choice this version doesn't have stays in the file through a change
/// made in Settings, though the page never had it, and so does a setting
/// only a newer version has (CHANGELOG.md, 0.7.4: "Settings from a newer
/// version load. A choice this version doesn't have, such as a newer
/// codec, falls back alone, and the newer version still finds it."). The
/// file's own rule is core settings.rs's
/// (`what_a_newer_version_wrote_is_kept_until_its_setting_changes`); this
/// is the page's way round it: read, changed and sent back whole.
#[test]
fn a_change_made_in_settings_keeps_what_a_newer_version_wrote() {
    let app = TestApp::new();
    let newer = json!({ "theme": "light", "clip_codec": "av1", "later_setting": { "on": true } });
    std::fs::write(app.paths().settings(), newer.to_string()).unwrap();
    let read = settings(&app);
    assert_eq!((&read["theme"], &read["clip_codec"]), (&json!("light"), &json!("h264")), "falls back alone");
    change(&app, json!({ "theme": "dark" }));
    let file = written(&app);
    assert_eq!(
        (&file["theme"], &file["clip_codec"], &file["later_setting"]),
        (&json!("dark"), &json!("av1"), &json!({ "on": true }))
    );
    assert_eq!(settings(&app)["theme"], "dark");
}

/// Two saves at once leave a file that reads, whole, as the one that went
/// last: none is lost to unreadable JSON and the defaults (settings.rs
/// `SAVING`: "two saves at once (quick toggles in Settings) would write the
/// same temporary file, which could keep the older one or leave unreadable
/// JSON, and the next launch would fall back to defaults"). The first is
/// held before its turn, so the second goes first. Their order is the
/// page's, which sends one at a time (appSettings.test.mjs).
#[test]
fn two_saves_at_once_leave_a_whole_file_of_the_last() {
    let app = TestApp::new();
    let mut light = settings(&app);
    light["theme"] = json!("light");
    let held = Gate::new();
    app.holds_settings_saves(&held);
    let handle = app.handle().clone();
    let first = std::thread::spawn(move || {
        let settings = serde_json::from_value(light).unwrap();
        tauri::async_runtime::block_on(crate::setup::set_settings(handle.state(), settings))
    });
    held.reached();
    app.holds_settings_saves(&Gate::opened());
    change(&app, json!({ "theme": "dark", "video_height": 720 }));
    held.open();
    first.join().unwrap().unwrap();
    let read = settings(&app);
    assert_eq!((&read["theme"], &read["video_height"]), (&json!("light"), &Value::Null));
    assert_eq!(app.invoke("settings_set_aside", json!({})).unwrap(), Value::Null, "read whole");
    assert!(!app.paths().settings().with_extension("tmp").exists(), "a save left half done");
}

/// Reset settings puts every setting back as it started, answering them,
/// and so they stay after a restart; a second Reset, with nothing changed,
/// does the same (CHANGELOG.md, 0.4.1: "Settings → Help has Reset settings:
/// every setting in Settings back as it started"; e2e settings.mjs: "Reset
/// settings puts them back"; setup.rs `reset_settings`: "the settings file
/// goes").
#[test]
fn reset_puts_every_setting_back_as_it_started() {
    let app = TestApp::new();
    let started = settings(&app);
    assert_eq!(started, every_default());
    change(&app, chosen());
    assert_eq!(app.invoke("reset_settings", json!({})).unwrap(), started);
    assert_eq!(settings(&app), started);
    assert!(!app.paths().settings().exists(), "the settings file is there");
    assert_eq!(app.invoke("reset_settings", json!({})).unwrap(), started, "a second reset");
    let app = TestApp::on(app.quit());
    assert_eq!(settings(&app), started, "after a restart");
}

/// Reset settings keeps the library and everything in it: its VODs, their
/// transcripts, clips, edits and videos, and what VODscribe keeps beside
/// the settings (CHANGELOG.md, 0.4.1: "with your library, transcripts,
/// clips, edits and videos kept"; setup.rs `reset_settings`: "keeping
/// everything else").
#[test]
fn reset_keeps_the_library_and_everything_beside_the_settings() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let edit = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let site = app.add_site_vod("peertube", "talk-1", "The talk", 600.0);
    app.invoke("keep_video", json!({ "id": site.id })).unwrap();
    vodscribe_core::testkit::eventually("its video kept", || app.db().vod(site.id).unwrap().unwrap().media);
    let playable = json!({ "video": [["av1", "opus"]], "audio": ["opus"] });
    app.invoke("set_playable", json!({ "playable": playable })).unwrap();
    app.invoke("keep_choice", json!({ "key": "vodscribe.volume", "value": "0.5" })).unwrap();
    change(&app, chosen());
    let library = |app: &TestApp| {
        let transcript = app.invoke("transcript", json!({ "vodId": vod.id })).unwrap();
        let edit = app.db().edit(edit, Default::default()).unwrap().map(|e| e.id);
        (app.invoke("list_vods", json!({})).unwrap(), transcript, app.clips(vod.id), edit)
    };
    let before = library(&app);

    app.invoke("reset_settings", json!({})).unwrap();
    assert_eq!(library(&app), before);
    assert!(std::path::Path::new(clip.file_path.as_deref().unwrap()).is_file(), "the clip's file");
    assert!(std::path::Path::new(vod.source_path.as_deref().unwrap()).is_file(), "the VOD's file");
    assert!(app.db().vod(site.id).unwrap().unwrap().media, "the kept video");
    let playable = serde_json::from_value(playable).unwrap();
    assert_eq!(vodscribe_core::playable::Playable::load(&app.paths().playable()), playable);
    assert_eq!(app.invoke("kept_choices", json!({})).unwrap()["vodscribe.volume"], "0.5");
}

/// A settings file that can't be read is said once, by the name it was set
/// aside as, beside the library; the settings are every default meanwhile
/// (CHANGELOG.md, 0.7.4: "Unreadable settings are kept, not overwritten. A
/// settings file that can't be read is set aside beside your library, and
/// VODscribe says so."; setup.rs `settings_set_aside`: "for the page to say
/// so once"). With nothing set aside, nothing is said. That it isn't saved
/// over is core settings.rs's
/// (`an_unreadable_settings_file_is_set_aside_not_saved_over`).
#[test]
fn an_unreadable_settings_file_is_said_once_by_the_name_it_was_set_aside_as() {
    let folder = Folder::new();
    let path = folder.paths().settings();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ not settings").unwrap();
    let app = TestApp::on(folder);
    // As the page asks at its start (App.svelte): the settings, then what was set aside.
    assert_eq!(settings(&app), every_default());
    let named = app.invoke("settings_set_aside", json!({})).unwrap();
    let name = named.as_str().unwrap_or_else(|| panic!("nothing said: {named}"));
    assert!(name.starts_with("settings-broken-") && name.ends_with(".json"), "{name}");
    assert_eq!(std::fs::read_to_string(path.with_file_name(name)).unwrap(), "{ not settings");
    assert_eq!(app.invoke("settings_set_aside", json!({})).unwrap(), Value::Null, "said again");

    let readable = TestApp::new();
    change(&readable, json!({ "theme": "dark" }));
    assert_eq!(settings(&readable)["theme"], "dark");
    assert_eq!(readable.invoke("settings_set_aside", json!({})).unwrap(), Value::Null);
}

/// What `missing_clip_codecs` answers the page.
fn greyed(app: &TestApp) -> Value {
    app.invoke("missing_clip_codecs", json!({})).unwrap()
}

/// With no ffmpeg yet, Settings › Encoding greys nothing out, and the
/// ffmpeg it will have is VODscribe's own (setup.rs `FfmpegCodecs`: "With
/// none found, its own, which setup downloads"). The ffmpeg's own list is
/// asked of it (clips/tests/codecs.rs,
/// `the_ffmpeg_s_encoders_are_asked_once_and_again_when_it_changes`).
#[test]
fn with_no_ffmpeg_yet_nothing_is_greyed_out() {
    let app = TestApp::new();
    std::fs::remove_file(app.ffmpeg.path()).unwrap();
    assert_eq!(greyed(&app), json!({ "missing": [], "own": true }));
}

/// With the system's ffmpeg in use, what it can't make is greyed out, and
/// it's said to be the system's (setup.rs `FfmpegCodecs`: "VODscribe's own
/// (`tools::own_ffmpeg`), else the system's: the page says which can't make
/// a format"; CHANGELOG.md, 0.7.3: "Formats ffmpeg lacks are grayed out.").
#[test]
fn the_systems_ffmpeg_in_use_is_said_to_be_the_systems() {
    let app = TestApp::new();
    std::fs::remove_file(app.ffmpeg.path()).unwrap();
    let system = app.paths().root.with_file_name("system");
    std::fs::create_dir_all(&system).unwrap();
    let ffmpeg = Ffmpeg::at(vodscribe_core::deps::managed_path(&system, "ffmpeg"));
    ffmpeg.lacks("libx265");
    // Found on a PATH only as a program it may run.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(ffmpeg.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    fakes::path_for(&app.paths().bin(), &[system]);
    assert_eq!(greyed(&app), json!({ "missing": ["h265"], "own": false }));
}
