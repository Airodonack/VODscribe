use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    #[default]
    Cpu,
    Gpu,
}

/// The app's colors: the system's light or dark, or always one of them.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// What clips (and rendered edits) are made as, for the editors they go to.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClipFormat {
    /// H.264 and AAC in MP4: small, and read by nearly every editor.
    #[default]
    Mp4,
    /// DNxHR LB and PCM in MOV: an editing format, for DaVinci Resolve on
    /// Linux, which reads neither H.264 (the free edition) nor AAC.
    Mov,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub device: Device,
    pub theme: Theme,
    /// The emote providers chat's word emotes come from (`word_emotes`), in
    /// the order that wins when two have an emote of the same name. All off
    /// until the user turns them on, since each is a service contacted.
    #[serde(deserialize_with = "providers")]
    pub emote_providers: EmoteProviders,
    /// The tallest video downloaded: kept from other sites
    /// (`pipeline::keeps_video`) and for clips (`clips::selector`), in pixels;
    /// `video_height()` gives the default when unset.
    pub video_height: Option<u32>,
    /// What clips and rendered edits are made as.
    pub clip_format: ClipFormat,
    /// The keyboard controls the user changed (Settings › Controls), by the
    /// page's action id: each the whole list of that action's keys, an empty
    /// one for none. Only the page reads them (keymap.ts); ids it no longer
    /// has are kept as they are.
    #[serde(deserialize_with = "bindings")]
    pub key_bindings: BTreeMap<String, Vec<String>>,
    /// Whether the players' keyboard legend is open: left open, it stays open
    /// across VODs, edits and launches. Only the page reads it.
    pub show_keys: bool,
    /// Whether a VOD added from a site is looked up on the file host, to
    /// offer its annotations if they're published (`annotations::look_up`):
    /// the request names the site and the VOD's id, nothing else. On unless
    /// turned off; off, nothing is asked.
    pub ask_for_annotations: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: Device::default(),
            theme: Theme::default(),
            emote_providers: EmoteProviders::default(),
            video_height: None,
            clip_format: ClipFormat::default(),
            key_bindings: BTreeMap::new(),
            show_keys: false,
            ask_for_annotations: true,
        }
    }
}

/// Every known emote provider, each once, in priority order.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EmoteProviders(pub Vec<EmoteProvider>);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EmoteProvider {
    /// A `word_emotes::PROVIDER_IDS` id.
    pub id: String,
    pub on: bool,
}

impl Default for EmoteProviders {
    fn default() -> Self {
        Self::from(Vec::new())
    }
}

impl From<Vec<EmoteProvider>> for EmoteProviders {
    /// Keeps the known ids, first of each, in the order given; any missing
    /// are added after, off.
    fn from(list: Vec<EmoteProvider>) -> Self {
        let mut out: Vec<EmoteProvider> = Vec::new();
        for p in list {
            if crate::word_emotes::PROVIDER_IDS.contains(&p.id.as_str()) && !out.iter().any(|o| o.id == p.id) {
                out.push(p);
            }
        }
        for id in crate::word_emotes::PROVIDER_IDS {
            if !out.iter().any(|o| o.id == *id) {
                out.push(EmoteProvider { id: id.to_string(), on: false });
            }
        }
        EmoteProviders(out)
    }
}

impl EmoteProviders {
    /// The ids of those turned on, in priority order.
    pub fn enabled(&self) -> Vec<&str> {
        self.0.iter().filter(|p| p.on).map(|p| p.id.as_str()).collect()
    }
}

fn providers<'de, D: serde::Deserializer<'de>>(d: D) -> Result<EmoteProviders, D::Error> {
    Ok(EmoteProviders::from(Vec::<EmoteProvider>::deserialize(d)?))
}

/// The key bindings that are lists of keys; any other (the file edited by
/// hand) is left out, so that action has its defaults, rather than making
/// the whole file unreadable and every setting its default.
fn bindings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Vec<String>>, D::Error> {
    let serde_json::Value::Object(all) = serde_json::Value::deserialize(d)? else { return Ok(BTreeMap::new()) };
    Ok(all.into_iter().filter_map(|(id, keys)| Some((id, serde_json::from_value(keys).ok()?))).collect())
}

/// The tallest video downloaded unless chosen otherwise: 1080p, the most
/// offered; clips are posted at 1080p. The page's DEFAULT_HEIGHT
/// (platform.ts) is checked to agree.
pub const DEFAULT_VIDEO_HEIGHT: u32 = 1080;

impl Settings {
    /// The tallest video to download (`DEFAULT_VIDEO_HEIGHT` unless chosen).
    pub fn video_height(&self) -> u32 {
        self.video_height.unwrap_or(DEFAULT_VIDEO_HEIGHT)
    }

    /// Missing or unreadable settings fall back to defaults.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else { return Self::default() };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("{}: unreadable, using the defaults: {e}", path.display());
            Self::default()
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        // One at a time: two saves at once (quick toggles in Settings) would
        // write the same temporary file, which could keep the older one or
        // leave unreadable JSON, and the next launch would fall back to defaults.
        static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _turn = SAVING.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emote_providers_start_off_and_stay_known() {
        let fresh: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(fresh.emote_providers.enabled(), Vec::<&str>::new());
        assert_eq!(fresh.emote_providers.0.len(), crate::word_emotes::PROVIDER_IDS.len());
        let saved: Settings = serde_json::from_str(
            r#"{"emote_providers": [{"id": "ffz", "on": true}, {"id": "nope", "on": true}, {"id": "ffz", "on": false}, {"id": "7tv", "on": true}]}"#,
        )
        .unwrap();
        assert_eq!(saved.emote_providers.enabled(), ["ffz", "7tv"]);
        let order: Vec<_> = saved.emote_providers.0.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order, ["ffz", "7tv", "bttv"]);
    }

    #[test]
    fn key_bindings_start_empty_and_keep_what_they_are_given() {
        // A settings.json as 0.5.x wrote it: no key bindings.
        let old: Settings = serde_json::from_str(
            r#"{"device": "cpu", "theme": "dark", "emote_providers": [], "video_height": 720, "clip_format": "mov"}"#,
        )
        .unwrap();
        assert!(old.key_bindings.is_empty());
        assert_eq!(old.video_height, Some(720));
        let saved: Settings =
            serde_json::from_str(r#"{"key_bindings": {"play.toggle": ["p"], "mute.toggle": [], "gone.away": ["x"]}}"#)
                .unwrap();
        let again: Settings = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(again.key_bindings["play.toggle"], ["p"]);
        assert!(again.key_bindings["mute.toggle"].is_empty());
        assert_eq!(again.key_bindings["gone.away"], ["x"]);
    }

    #[test]
    fn the_keyboard_legend_starts_closed_and_stays_as_left() {
        // A settings.json as 0.5.x wrote it: no word of the legend.
        let old: Settings = serde_json::from_str(r#"{"theme": "dark", "video_height": 720}"#).unwrap();
        assert!(!old.show_keys);
        let open = Settings { show_keys: true, ..old };
        let again: Settings = serde_json::from_str(&serde_json::to_string(&open).unwrap()).unwrap();
        assert!(again.show_keys);
        assert_eq!(again.video_height, Some(720));
    }

    #[test]
    fn asking_for_annotations_starts_on_and_stays_as_left() {
        // A settings.json as 0.6.0 wrote it: no word of annotations.
        let old: Settings = serde_json::from_str(r#"{"theme": "dark", "show_keys": true}"#).unwrap();
        assert!(old.ask_for_annotations);
        assert!(Settings::default().ask_for_annotations);
        let off = Settings { ask_for_annotations: false, ..old };
        let again: Settings = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
        assert!(!again.ask_for_annotations);
        assert!(again.show_keys);
    }

    /// A key binding written by hand in the wrong shape (a string for a
    /// list, a number in one) made the whole file unreadable, so every
    /// setting fell back to its default, and the next change saved those
    /// over the file. Only the bindings that aren't lists of keys go.
    #[test]
    fn a_misshapen_key_binding_loses_only_itself() {
        let loaded: Settings = serde_json::from_str(
            r#"{"theme": "dark", "video_height": 720, "key_bindings":
                {"undo": "Ctrl+q", "redo": ["Ctrl+y", 5], "mute.toggle": [], "play.toggle": ["p", "k"], "x": null}}"#,
        )
        .unwrap();
        assert_eq!((loaded.theme, loaded.video_height), (Theme::Dark, Some(720)));
        assert_eq!(loaded.key_bindings.keys().collect::<Vec<_>>(), ["mute.toggle", "play.toggle"]);
        assert_eq!(loaded.key_bindings["play.toggle"], ["p", "k"]);
        let loaded: Settings = serde_json::from_str(r#"{"theme": "light", "key_bindings": ["undo"]}"#).unwrap();
        assert_eq!(loaded.theme, Theme::Light);
        assert!(loaded.key_bindings.is_empty());
    }
}
