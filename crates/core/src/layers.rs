//! Timeline layers: saved chat searches drawn over the chat activity beside a
//! transcript, each in its own color, with the messages they match highlighted
//! in chat. A layer is a list of terms (words, phrases or emote names), for
//! every channel or just one (emotes are often a channel's own). Alongside
//! them, built-in toggles for chat events (subs, gifts, tips; see `chat::Kind`).

use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::chat::Kind;

/// What `layers.json` holds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Timeline {
    pub layers: Vec<Layer>,
    /// The chat events drawn on the timeline (never `Kind::Message`).
    #[serde(default)]
    pub events: Vec<Kind>,
}

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
        layer("clip", "Clip it", "#ef4444", &["clip", "clipped", "clip it"], false),
    ]
}

/// The saved timeline, or the default layers (and no events) if there's none
/// yet or it can't be read. Reads the plain list of layers the file held before
/// event toggles too.
pub fn load(path: &Path) -> Timeline {
    let fresh = || Timeline { layers: defaults(), events: Vec::new() };
    let Ok(text) = fs::read_to_string(path) else { return fresh() };
    serde_json::from_str(&text)
        .or_else(|_| serde_json::from_str(&text).map(|layers| Timeline { layers, events: Vec::new() }))
        .unwrap_or_else(|e| {
            eprintln!("{}: unreadable, using the default layers: {e}", path.display());
            fresh()
        })
}

pub fn save(path: &Path, timeline: &Timeline) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_string_pretty(timeline)?)?;
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
        let fresh = Timeline { layers: defaults(), events: Vec::new() };
        assert_eq!(load(&path), fresh, "nothing saved yet");
        let mut mine = fresh.clone();
        mine.layers[0].channel = Some(Channel { platform: "kick".into(), channel: "Barky".into() });
        let emote = Some("https://files.kick.com/emotes/1/fullsize".into());
        mine.layers[0].terms.push(Term { text: "barkylol".into(), emote });
        mine.events = vec![Kind::Sub, Kind::Tip];
        save(&path, &mine).unwrap();
        assert_eq!(load(&path), mine);
        save(&path, &Timeline { layers: Vec::new(), events: Vec::new() }).unwrap();
        assert!(load(&path).layers.is_empty(), "deleting every layer sticks");
        fs::write(&path, "{").unwrap();
        assert_eq!(load(&path), fresh);
        // Before event toggles, the file was just the layers.
        fs::write(&path, serde_json::to_string(&defaults()[..1]).unwrap()).unwrap();
        assert_eq!(load(&path), Timeline { layers: defaults()[..1].to_vec(), events: Vec::new() });
        let _ = fs::remove_dir_all(&dir);
    }
}
