//! The text a copy goes by: what the app tells the page of a copy, and the
//! paste that goes ahead only while the system's clipboard holds that text.

use crate::testkit::TestApp;
use serde_json::{Value, json};

/// Copies the stretch `start`..`end` of VOD `vod` as Ctrl+C does and writes
/// `text` to the system's clipboard for it, as the page does: what the app
/// told of the copy.
fn copy(app: &TestApp, vod: i64, start: f64, end: f64, text: &str) -> Value {
    let said = app
        .invoke("copy_items", json!({ "items": [{ "kind": "range", "vodId": vod, "start": start, "end": end }] }))
        .unwrap();
    app.invoke("copied_text", json!({ "text": text })).unwrap();
    said
}

/// A paste on VOD `vod`'s clip bar while the system's clipboard holds
/// `text`: the clips it made, or None if it didn't go ahead.
fn paste(app: &TestApp, vod: i64, text: &str) -> Option<Vec<i64>> {
    let pasted = app
        .invoke("paste_items", json!({ "target": { "kind": "vod", "id": vod }, "bar": true, "text": text }))
        .unwrap();
    if pasted.is_null() {
        return None;
    }
    let ids: Vec<i64> = pasted["clips"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect();
    // Let their making end, so the app can end with nothing in flight.
    ids.iter().for_each(|&id| {
        app.made(id);
    });
    Some(ids)
}

/// app clipboard.rs:4: "A paste goes ahead only while the system's
/// clipboard still holds that text: something copied since, in any app, is
/// what the user means to paste, not this."; plans/copy-paste.md ("The text
/// doubles as the marker").
#[test]
fn a_paste_goes_ahead_only_while_the_clipboard_holds_the_copys_text() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "x", "Stream", 60.0).id;
    assert_eq!(paste(&app, vod, "anything"), None, "nothing was copied");
    copy(&app, vod, 10.0, 20.0, "Hello (0:10–0:20)");
    assert_eq!(paste(&app, vod, "something copied in another app"), None);
    assert!(app.clips(vod).is_empty());
    let made = paste(&app, vod, "Hello (0:10–0:20)").expect("the clipboard holds the copy's text");
    assert_eq!(made.len(), 1);
    assert_eq!(app.clips(vod).len(), 1);
}

/// app clipboard.rs:1: "kept here, in the app, until the next copy or the
/// app quits (a reload of the page keeps it)": pastes don't use it up.
#[test]
fn a_copy_lasts_till_the_next_copy_or_the_app_quits() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "x", "Stream", 60.0).id;
    copy(&app, vod, 10.0, 20.0, "first");
    assert!(paste(&app, vod, "first").is_some());
    assert!(paste(&app, vod, "first").is_some(), "a paste leaves the copy");
    copy(&app, vod, 30.0, 40.0, "second");
    assert_eq!(paste(&app, vod, "first"), None, "the next copy took its place");
    assert!(paste(&app, vod, "second").is_some());
    let app = TestApp::on(app.quit());
    assert_eq!(paste(&app, vod, "second"), None, "the app's quit ended the copy");
}

/// clipboardText.ts:12 ("as the app says it (src-tauri clipboard.rs
/// `CopiedSaid`)"); app clipboard.rs:74: the page reads `kind`, `title`,
/// `first` (`vodId`, `vodTitle`, `start`, `end`), `pieces` and `length`.
#[test]
fn the_copy_the_app_says_is_the_shape_the_page_reads() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "x", "Stream", 60.0).id;
    let said = copy(&app, vod, 10.0, 20.0, "text");
    assert_eq!(
        said,
        json!([{
            "kind": "stretch",
            "title": "Clip at 0:10",
            "first": { "vodId": vod, "vodTitle": "Stream", "start": 10.0, "end": 20.0 },
            "pieces": 1,
            "length": 10.0,
        }])
    );

    // A clip of its bar is said as a clip, by its own title.
    let clip = app.make_clip(vod, "0:30", "0:45");
    let said = app.invoke("copy_items", json!({ "items": [{ "kind": "clip", "id": clip.id }] })).unwrap();
    assert_eq!(
        said,
        json!([{
            "kind": "clip",
            "title": clip.title,
            "first": { "vodId": vod, "vodTitle": "Stream", "start": 30.0, "end": 45.0 },
            "pieces": 1,
            "length": 15.0,
        }])
    );
}
