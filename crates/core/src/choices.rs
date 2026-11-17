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
mod tests;
