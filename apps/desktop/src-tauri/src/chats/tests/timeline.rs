//! The timeline's layers as the page reaches them: `get_timeline` and
//! `set_timeline` (chats.rs), through the app's commands, on its library's
//! folder.

use serde_json::{Value, json};

use crate::testkit::{Folder, TestApp};

/// A timeline as the page sends it: one layer for a channel, with an emote,
/// one chat event drawn, the user's marks hidden.
fn mine() -> Value {
    json!({
        "layers": [{
            "id": "a1",
            "name": "Barky's laughs",
            "color": "#14b8a6",
            "terms": [{ "text": "barkylol", "emote": "https://files.kick.com/emotes/1/fullsize" }, { "text": "lol", "emote": null }],
            "visible": true,
            "channel": { "platform": "kick", "channel": "Barky" },
        }],
        "events": ["sub", "tip"],
        "marks": { "user": false, "imported": true },
    })
}

/// 0.2.0: "Timeline layers mark the moments you care about: laughter, hype
/// and "clip it" to start"; `get_timeline`: "The timeline's layers and event
/// toggles (see `layers`), or the starting ones."
#[test]
fn a_new_library_gives_the_starting_layers_and_no_events() {
    let app = TestApp::new();
    let timeline = app.invoke("get_timeline", json!({})).unwrap();
    let names: Vec<_> = timeline["layers"].as_array().unwrap().iter().map(|l| l["name"].clone()).collect();
    assert_eq!(names, [json!("Laughter"), json!("Hype"), json!("Clip it")]);
    assert_eq!(timeline["events"], json!([]));
    assert_eq!(timeline["marks"], json!({ "user": true, "imported": true }));
}

/// 0.2.0: "Timeline layers mark the moments you care about: ... any words
/// and emotes, for every channel or one." What `set_timeline` is given is
/// what `get_timeline` answers, and it is still there when the app starts
/// again on the same library.
#[test]
fn what_the_page_saved_is_found_again_after_a_restart() {
    let app = TestApp::new();
    app.invoke("set_timeline", json!({ "timeline": mine() })).unwrap();
    assert_eq!(app.invoke("get_timeline", json!({})).unwrap(), mine());
    let app = TestApp::on(app.quit());
    assert_eq!(app.invoke("get_timeline", json!({})).unwrap(), mine());
}

/// 0.2.0: layers can be deleted; none left is kept and doesn't bring the
/// starting ones back.
#[test]
fn deleting_every_layer_is_kept() {
    let app = TestApp::new();
    app.invoke(
        "set_timeline",
        json!({ "timeline": { "layers": [], "events": [], "marks": { "user": true, "imported": true } } }),
    )
    .unwrap();
    assert_eq!(app.invoke("get_timeline", json!({})).unwrap()["layers"], json!([]));
}

/// 0.3.0 Fixes: "Timeline layers that failed to load could be overwritten."
/// A file the app can't read gives the starting layers, and asking for them
/// doesn't write over it.
#[test]
fn a_file_that_cant_be_read_is_not_written_over_by_asking_for_it() {
    let folder = Folder::new();
    let file = folder.paths().layers();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "{").unwrap();
    let app = TestApp::on(folder);
    let timeline = app.invoke("get_timeline", json!({})).unwrap();
    assert_eq!(timeline["layers"].as_array().unwrap().len(), 3, "not the starting layers");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{");
}

/// What the page sends that isn't a timeline is refused, and the saved
/// timeline is as it was.
#[test]
fn a_timeline_that_cant_be_read_is_refused_and_the_saved_one_stays() {
    let app = TestApp::new();
    app.invoke("set_timeline", json!({ "timeline": mine() })).unwrap();
    assert!(app.invoke("set_timeline", json!({ "timeline": { "events": ["sub"] } })).is_err());
    assert!(app.invoke("set_timeline", json!({ "timeline": { "layers": [{ "id": "x" }] } })).is_err());
    assert_eq!(app.invoke("get_timeline", json!({})).unwrap(), mine());
}
