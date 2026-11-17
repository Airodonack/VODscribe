//! The specs of the page's remembered choices (choices.rs): kept through
//! `keep_choice`, read through `kept_choices`, as the page does.

use serde_json::{Value, json};
use vodscribe_core::choices::MAX_VALUE;

use crate::testkit::{Folder, TestApp};

/// Every choice kept, as the page reads them before it shows.
fn kept(app: &TestApp) -> Value {
    app.invoke("kept_choices", json!({})).unwrap()
}

/// "Remembered choices last after a restart. The volume, the speed, captions
/// on or off, whether chat shows and the other choices VODscribe remembers
/// were forgotten at every restart" (CHANGELOG.md, 0.7.0 Fixes).
#[test]
fn a_choice_kept_is_there_after_a_restart() {
    let app = TestApp::new();
    assert_eq!(kept(&app), json!({}));
    app.invoke("keep_choice", json!({ "key": "vodscribe.volume", "value": "0.4" })).unwrap();
    app.invoke("keep_choice", json!({ "key": "vodscribe.speed", "value": "1.5" })).unwrap();
    app.invoke("keep_choice", json!({ "key": "vodscribe.volume", "value": "0.6" })).unwrap();
    assert_eq!(kept(&app), json!({ "vodscribe.volume": "0.6", "vodscribe.speed": "1.5" }));

    let app = TestApp::on(app.quit());
    assert_eq!(kept(&app), json!({ "vodscribe.volume": "0.6", "vodscribe.speed": "1.5" }));
}

/// "refused (and nothing changed) if it doesn't fit" (choices.rs, `keep`
/// in core): the page is told it wasn't kept, and the choice stays as it was.
#[test]
fn a_choice_too_long_to_keep_is_refused_and_the_old_one_stays() {
    let app = TestApp::new();
    app.invoke("keep_choice", json!({ "key": "vodscribe.search", "value": "[\"chat\"]" })).unwrap();
    let refused = app.invoke("keep_choice", json!({ "key": "vodscribe.search", "value": "x".repeat(MAX_VALUE + 1) }));
    assert!(refused.is_err());
    assert_eq!(kept(&app), json!({ "vodscribe.search": "[\"chat\"]" }));
    let app = TestApp::on(app.quit());
    assert_eq!(kept(&app), json!({ "vodscribe.search": "[\"chat\"]" }));
}

/// "Apart from the library's state, so they're there on the recovery screen
/// too" (choices.rs, module doc): a library that won't open starts the app
/// on the recovery screen, which has the choices as they were.
#[test]
fn the_choices_are_there_on_the_recovery_screen_too() {
    let app = TestApp::new();
    app.invoke("keep_choice", json!({ "key": "vodscribe.library", "value": "hidden" })).unwrap();
    let folder: Folder = app.quit();
    std::fs::write(folder.paths().db(), b"not a database").unwrap();
    let app = TestApp::on(folder);
    assert_eq!(kept(&app), json!({ "vodscribe.library": "hidden" }));
}
