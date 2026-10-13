//! Timeline layers: saved chat searches drawn over the chat activity beside a
//! transcript, each in its own color, with the messages they match highlighted
//! in chat. A layer is a list of terms (words, phrases or emote names), for
//! every channel or just one (emotes are often a channel's own).

use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Layer {
    /// Stable within the file; the UI makes them.
    pub id: String,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    pub terms: Vec<Term>,
    /// Drawn on the timeline (the UI shows a few at once).
    pub visible: bool,
    /// Only for this channel's VODs; `None` for every channel.
    pub channel: Option<Channel>,
}

/// A term, shown as a pill: a word, a phrase, or an emote's name. Matched as
/// search matches words, so only letters and digits count (not emoji).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Term {
    pub text: String,
    /// The emote's image, when the term was added as one (see `emotes`).
    pub emote: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Channel {
    pub platform: String,
    pub channel: String,
}

fn layer(id: &str, name: &str, color: &str, terms: &[&str], visible: bool) -> Layer {
    Layer {
        id: id.into(),
        name: name.into(),
        color: color.into(),
        terms: terms.iter().map(|t| Term { text: (*t).into(), emote: None }).collect(),
        visible,
        channel: None,
    }
}

/// The layers a new library starts with, to edit or delete.
pub fn defaults() -> Vec<Layer> {
    vec![
        layer("laughter", "Laughter", "#f59e0b", &["KEKW", "LUL", "OMEGALUL", "LMAO", "LOL", "ICANT"], true),
        layer("hype", "Hype", "#ec4899", &["Pog", "PogChamp", "POGGERS", "PogU", "W"], false),
        layer("clip", "Clip it", "#14b8a6", &["clip", "clipped", "clip it"], false),
    ]
}

/// The saved layers, or the defaults if there are none yet (or they can't be read).
pub fn load(path: &Path) -> Vec<Layer> {
    let Ok(text) = fs::read_to_string(path) else { return defaults() };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("{}: unreadable, using the default layers: {e}", path.display());
        defaults()
    })
}

pub fn save(path: &Path, layers: &[Layer]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_string_pretty(layers)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_falls_back_to_the_defaults() {
        let dir = std::env::temp_dir().join(format!("vodscribe-layers-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("layers.json");
        assert_eq!(load(&path), defaults(), "nothing saved yet");
        let mut mine = defaults();
        mine[0].channel = Some(Channel { platform: "kick".into(), channel: "Barky".into() });
        mine[0]
            .terms
            .push(Term { text: "barkylol".into(), emote: Some("https://files.kick.com/emotes/1/fullsize".into()) });
        save(&path, &mine).unwrap();
        assert_eq!(load(&path), mine);
        save(&path, &[]).unwrap();
        assert!(load(&path).is_empty(), "deleting every layer sticks");
        fs::write(&path, "{").unwrap();
        assert_eq!(load(&path), defaults());
        let _ = fs::remove_dir_all(&dir);
    }
}
