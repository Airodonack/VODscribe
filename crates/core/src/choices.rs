//! The small choices the page remembers (apps/desktop/src/lib/stored.ts): the
//! players' volume and speed, which columns show, and the like, each a short
//! string under a short key. Only the page reads them. They're kept here
//! rather than in the web view's storage, which is per origin: an installed
//! app's page comes from a new port each launch (the desktop's app_server.rs),
//! so the web view's was a new, empty one every time.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

/// Every choice, by the page's key.
pub type Choices = BTreeMap<String, String>;

/// The longest key, in bytes.
pub const MAX_KEY: usize = 100;
/// The longest value, in bytes: the longest kept now (search's sources, as
/// JSON) is under a hundred.
pub const MAX_VALUE: usize = 4096;
/// The most choices kept: the page has about fifteen.
pub const MAX_CHOICES: usize = 200;

/// Whether `choices` may keep `value` under `key`: both within their sizes,
/// and room for it if the key is new.
fn fits(choices: &Choices, key: &str, value: &str) -> bool {
    key.len() <= MAX_KEY && value.len() <= MAX_VALUE && (choices.contains_key(key) || choices.len() < MAX_CHOICES)
}

/// The choices in `path`: none if it's missing or unreadable, and of a file
/// edited by hand, only the strings under keys within the sizes (whatever
/// the key; one the page no longer has stays as it is).
pub fn load(path: &Path) -> Choices {
    let Ok(text) = fs::read_to_string(path) else { return Choices::new() };
    let all = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(all)) => all,
        Ok(_) => {
            eprintln!("{}: not an object of choices, starting without", path.display());
            return Choices::new();
        }
        Err(e) => {
            eprintln!("{}: unreadable, starting without: {e}", path.display());
            return Choices::new();
        }
    };
    let mut choices = Choices::new();
    for (key, value) in all {
        if let serde_json::Value::String(value) = value
            && fits(&choices, &key, &value)
        {
            choices.insert(key, value);
        }
    }
    choices
}

/// Keeps `value` under `key` in `choices` and writes them all to `path`;
/// refused (and nothing changed) if it doesn't fit (`MAX_KEY`, `MAX_VALUE`,
/// `MAX_CHOICES`).
pub fn keep(path: &Path, choices: &mut Choices, key: &str, value: &str) -> Result<()> {
    if !fits(choices, key, value) {
        bail!("a choice too long, or one too many, to keep ({} bytes under a {}-byte key)", value.len(), key.len());
    }
    if choices.get(key).map(String::as_str) == Some(value) {
        return Ok(());
    }
    choices.insert(key.to_string(), value.to_string());
    // Whole or not at all: a quit halfway through a write leaves the old file.
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_string_pretty(choices)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_choices_come_back() {
        let dir = crate::TempDir::new("choices");
        let path = dir.join("choices.json");
        assert!(load(&path).is_empty(), "nothing kept yet");
        let mut choices = load(&path);
        keep(&path, &mut choices, "vodscribe.volume", "0.4").unwrap();
        keep(&path, &mut choices, "vodscribe.library", "hidden").unwrap();
        keep(&path, &mut choices, "vodscribe.volume", "0.6").unwrap();
        assert_eq!(load(&path), choices);
        assert_eq!(load(&path)["vodscribe.volume"], "0.6");
        assert_eq!(load(&path).len(), 2);
    }

    #[test]
    fn an_unreadable_file_starts_without_and_is_written_over() {
        let dir = crate::TempDir::new("choices-bad");
        let path = dir.join("choices.json");
        for bad in ["{", "", "[1, 2]", "\"text\"", "null"] {
            fs::write(&path, bad).unwrap();
            assert!(load(&path).is_empty(), "{bad:?}");
        }
        let mut choices = load(&path);
        keep(&path, &mut choices, "vodscribe.speed", "1.5").unwrap();
        assert_eq!(load(&path)["vodscribe.speed"], "1.5");
    }

    /// A key the page no longer has (or doesn't have yet: a newer version's)
    /// stays, and is written back with the rest; values that aren't strings,
    /// and anything too long, go.
    #[test]
    fn unknown_keys_stay_and_misshapen_values_go() {
        let dir = crate::TempDir::new("choices-unknown");
        let path = dir.join("choices.json");
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

    #[test]
    fn too_long_or_too_many_is_refused_and_changes_nothing() {
        let dir = crate::TempDir::new("choices-caps");
        let path = dir.join("choices.json");
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
}
