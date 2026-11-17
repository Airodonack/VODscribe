//! The specs of the small choices the page remembers (choices.rs). Each test
//! is named for what it says the app does and cites where that's said.

use super::*;

/// A folder of its own and the choices file in it.
fn file(name: &str) -> (crate::TempDir, std::path::PathBuf) {
    let dir = crate::TempDir::new(name);
    let path = dir.join("choices.json");
    (dir, path)
}

/// "The volume, the speed, captions on or off, whether chat shows and the
/// other choices VODscribe remembers were forgotten at every restart"
/// (CHANGELOG.md, 0.7.0 Fixes: "Remembered choices last after a restart.");
/// choices.rs, `keep` and `load`.
#[test]
fn kept_choices_come_back() {
    let (_dir, path) = file("choices");
    assert!(load(&path).is_empty(), "nothing kept yet");
    let mut choices = load(&path);
    keep(&path, &mut choices, "vodscribe.volume", "0.4").unwrap();
    keep(&path, &mut choices, "vodscribe.library", "hidden").unwrap();
    keep(&path, &mut choices, "vodscribe.volume", "0.6").unwrap();
    assert_eq!(load(&path), choices);
    assert_eq!(load(&path)["vodscribe.volume"], "0.6");
    assert_eq!(load(&path).len(), 2);
}

/// "none if it's missing or unreadable" (choices.rs, `load`): the page
/// starts without them and the next choice writes the file anew.
#[test]
fn an_unreadable_file_starts_without_and_is_written_over() {
    let (_dir, path) = file("choices-bad");
    for bad in ["{", "", "[1, 2]", "\"text\"", "null"] {
        fs::write(&path, bad).unwrap();
        assert!(load(&path).is_empty(), "{bad:?}");
    }
    let mut choices = load(&path);
    keep(&path, &mut choices, "vodscribe.speed", "1.5").unwrap();
    assert_eq!(load(&path)["vodscribe.speed"], "1.5");
}

/// "a key the page no longer has stays as it is" and "of a file edited by
/// hand, only the strings under keys within the sizes" (choices.rs, `load`):
/// a key the page no longer has (or doesn't have yet: a newer version's)
/// stays, and is written back with the rest; values that aren't strings, and
/// anything too long, go.
#[test]
fn unknown_keys_stay_and_misshapen_values_go() {
    let (_dir, path) = file("choices-unknown");
    let long = "x".repeat(MAX_VALUE + 1);
    let long_key = "k".repeat(MAX_KEY + 1);
    fs::write(
        &path,
        format!(
            r#"{{"vodscribe.someday": "yes", "vodscribe.volume": 0.4, "vodscribe.pane": null,
                "vodscribe.speed": "2", "too.long": "{long}", "{long_key}": "1"}}"#
        ),
    )
    .unwrap();
    let mut choices = load(&path);
    assert_eq!(choices.keys().collect::<Vec<_>>(), ["vodscribe.someday", "vodscribe.speed"]);
    keep(&path, &mut choices, "vodscribe.library", "shown").unwrap();
    assert_eq!(load(&path)["vodscribe.someday"], "yes");
    assert_eq!(load(&path).len(), 3);
}

/// "refused (and nothing changed) if it doesn't fit (`MAX_KEY`, `MAX_VALUE`,
/// `MAX_CHOICES`)" (choices.rs, `keep`).
#[test]
fn too_long_or_too_many_is_refused_and_changes_nothing() {
    let (_dir, path) = file("choices-caps");
    let mut choices = Choices::new();
    assert!(keep(&path, &mut choices, "a", &"x".repeat(MAX_VALUE + 1)).is_err());
    assert!(keep(&path, &mut choices, &"k".repeat(MAX_KEY + 1), "1").is_err());
    assert!(choices.is_empty() && !path.exists());
    keep(&path, &mut choices, "a", &"x".repeat(MAX_VALUE)).unwrap();
    for i in 1..MAX_CHOICES {
        choices.insert(format!("key{i}"), "1".into());
    }
    assert!(keep(&path, &mut choices, "one.more", "1").is_err());
    assert!(!choices.contains_key("one.more"));
    // A choice already kept can still change.
    keep(&path, &mut choices, "key1", "2").unwrap();
    assert_eq!(load(&path).len(), MAX_CHOICES);
    assert_eq!(load(&path)["key1"], "2");
}

/// "a key ... within the sizes" (choices.rs, `MAX_KEY`, `MAX_VALUE`): a key
/// or a value exactly as long as the limit is kept, and comes back.
#[test]
fn a_key_or_value_as_long_as_the_limit_is_kept() {
    let (_dir, path) = file("choices-limit");
    let mut choices = Choices::new();
    let key = "k".repeat(MAX_KEY);
    keep(&path, &mut choices, &key, &"v".repeat(MAX_VALUE)).unwrap();
    assert_eq!(load(&path)[&key].len(), MAX_VALUE);
}

/// "The most choices kept: the page has about fifteen" (choices.rs,
/// `MAX_CHOICES`): a file edited by hand to hold more keeps no more than
/// that many.
#[test]
fn a_file_with_more_than_the_most_keeps_only_that_many() {
    let (_dir, path) = file("choices-many");
    let all: serde_json::Map<String, serde_json::Value> =
        (0..MAX_CHOICES + 20).map(|i| (format!("key{i:03}"), "1".into())).collect();
    fs::write(&path, serde_json::to_string(&all).unwrap()).unwrap();
    assert_eq!(load(&path).len(), MAX_CHOICES);
}

/// "Whole or not at all: a quit halfway through a write leaves the old file"
/// (choices.rs, `keep`): the file is written beside itself and moved over, so
/// no half-written copy stays in the folder.
#[test]
fn a_kept_choice_leaves_only_the_file_in_the_folder() {
    let (dir, path) = file("choices-whole");
    let mut choices = Choices::new();
    keep(&path, &mut choices, "vodscribe.volume", "0.4").unwrap();
    keep(&path, &mut choices, "vodscribe.volume", "0.5").unwrap();
    let names: Vec<_> = fs::read_dir(&*dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["choices.json"]);
}

/// "Keeps `value` under `key` ... and writes them all" (choices.rs, `keep`):
/// a choice kept again as it is changes nothing, the file untouched.
#[test]
fn a_choice_kept_as_it_is_writes_nothing() {
    let (_dir, path) = file("choices-same");
    let mut choices = Choices::new();
    keep(&path, &mut choices, "vodscribe.speed", "2").unwrap();
    fs::write(&path, "marker").unwrap();
    keep(&path, &mut choices, "vodscribe.speed", "2").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "marker");
}
