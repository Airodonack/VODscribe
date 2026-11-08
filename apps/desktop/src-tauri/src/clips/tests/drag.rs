use super::*;

/// What a drag out of the window comes to, by what was seen of it: let go
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
/// ordinary release, too soon, as an error.
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
