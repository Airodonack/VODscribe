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

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
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

impl Settings {
    /// The tallest video to download: 1080p, the most offered, unless chosen
    /// otherwise; clips are posted at 1080p.
    pub fn video_height(&self) -> u32 {
        self.video_height.unwrap_or(1080)
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
}
