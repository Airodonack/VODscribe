use super::*;

use crate::testkit::TestApp;
use serde_json::json;

/// On Linux, macOS and Windows alike a drag out of the window begins before
/// the clips' files are made: the page asks the app, and the app says so
/// (0.7.1: "On Linux, clips drag out at once. The drag doesn't wait at the
/// window's edge for the file, which lands where you drop it once it's made",
/// and the Windows and Mac bullets of that release; `drags_before_made`'s doc
/// in app clips.rs: "Linux, macOS and Windows"). The switch that goes back to
/// waiting (`VODSCRIBE_DRAG_WAITS=1`) is the developer's and is tested with
/// the fake drag.
#[test]
fn clips_drag_out_before_their_files_are_made_on_every_system() {
    let app = TestApp::new();
    assert_eq!(app.invoke("drag_before_made", json!({})), Ok(json!(true)));
}

/// What a drag out of the window comes to, by what was seen of it (app
/// clips.rs `drag_outcome`; 0.7.1: "Some apps take finished files only. If one
/// takes nothing or refuses the drop before the clip is made, VODscribe says
/// which app it was", and "A drag called off makes nothing. Escape, or a drop
/// on nothing, stops the clip's making ... If the clip can't be made,
/// VODscribe says why only when you let it go on another app."): let go
/// on an app that took nothing while the files were still being made, it
/// takes finished files only (named, where the system says); one that
/// failed is said only once it's let go on another app; called off or
/// dropped on the app's own window, nothing.
#[test]
fn a_drag_ends_as_its_receiver_took_it() {
    use Ending::*;
    let named = || Some("Kdenlive".to_string());
    let blocked = || Some("blocked".to_string());
    let ended = |delivered, failed: Option<&str>, finished_only, receiver: Option<&str>| DragEnded {
        drag: None,
        delivered,
        failed: failed.map(String::from),
        finished_only,
        receiver: receiver.map(String::from),
        resolve: None,
    };
    assert_eq!(drag_outcome(Elsewhere, true, None, true, named()), ended(true, None, false, None), "given");
    assert_eq!(
        drag_outcome(Elsewhere, false, None, false, named()),
        ended(false, None, true, Some("Kdenlive")),
        "let go before it was made, nothing taken"
    );
    assert_eq!(drag_outcome(Elsewhere, false, None, false, None), ended(false, None, true, None), "name unknown");
    assert_eq!(drag_outcome(Elsewhere, false, None, true, named()), DragEnded::default(), "made, not taken");
    assert_eq!(drag_outcome(Elsewhere, false, blocked(), false, named()), ended(false, Some("blocked"), false, None));
    assert_eq!(drag_outcome(Nowhere, false, blocked(), false, None), ended(false, Some("blocked"), false, None));
    assert_eq!(drag_outcome(Nowhere, false, None, false, None), DragEnded::default(), "dropped on nothing");
    assert_eq!(drag_outcome(Elsewhere, false, Some(CANCELED.into()), false, named()), DragEnded::default());
    for ending in [CalledOff, Back] {
        for (given, failure) in [(true, None), (false, None), (false, blocked())] {
            assert_eq!(drag_outcome(ending, given, failure, false, named()), DragEnded::default(), "{ending:?}");
        }
    }
}

/// The page told an early release from a refused drag by the word
/// "released" in its error: a reworded sentence would have shown an
/// ordinary release, too soon, as an error (app clips.rs `began`: "refused
/// because the button came up first, it didn't, which is no error to show;
/// refused otherwise, why").
#[test]
fn a_drag_refused_for_an_early_release_is_none_begun_not_an_error() {
    assert_eq!(began(Ok(())), Ok(true));
    assert_eq!(began(Err(ReleasedEarly.into())), Ok(false));
    assert_eq!(began(Err(anyhow::Error::new(ReleasedEarly).context("Not now."))), Ok(false), "said otherwise");
    assert_eq!(began(Err(anyhow::anyhow!("no view"))), Err("no view".into()));
    assert_eq!(
        began(Err(anyhow::anyhow!("The button was released before the drag could begin."))),
        Err("The button was released before the drag could begin.".into()),
        "only the type tells",
    );
}
