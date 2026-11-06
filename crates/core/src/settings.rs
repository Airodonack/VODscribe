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
    /// H.264 or H.265, and AAC, in MP4: small, and read by nearly every editor.
    #[default]
    Mp4,
    /// DNxHR or ProRes, and PCM, in MOV: editing formats; DNxHR for DaVinci
    /// Resolve on Linux, which reads neither H.264 (the free edition) nor AAC.
    Mov,
}

/// The codec clips' (and rendered edits') pictures are encoded with; each
/// goes in one `ClipFormat` (`format`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClipCodec {
    /// H.264: light to decode, read by nearly every editor and player.
    #[default]
    H264,
    /// H.265: smaller files for the same picture, slower to make; Resolve's
    /// free edition on Linux and some Windows players can't read it.
    H265,
    /// DNxHR: every frame whole, for editing (Resolve, Premiere).
    Dnxhr,
    /// ProRes: every frame whole, for Apple's editors.
    Prores,
}

impl ClipCodec {
    /// The file a picture in this codec goes in.
    pub fn format(self) -> ClipFormat {
        match self {
            ClipCodec::H264 | ClipCodec::H265 => ClipFormat::Mp4,
            ClipCodec::Dnxhr | ClipCodec::Prores => ClipFormat::Mov,
        }
    }

    /// The codec a file of `format` was made with before codecs were a
    /// choice: a settings file from then reads as it did.
    fn of(format: ClipFormat) -> Self {
        match format {
            ClipFormat::Mp4 => ClipCodec::H264,
            ClipFormat::Mov => ClipCodec::Dnxhr,
        }
    }
}

/// How much picture clips keep, for the size it costs (`clips::output_args`
/// says what each is in each codec).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClipQuality {
    Proxy,
    #[default]
    Standard,
    High,
}

/// What clips and rendered edits are encoded as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClipEncoding {
    pub codec: ClipCodec,
    pub quality: ClipQuality,
}

impl ClipEncoding {
    /// The file it goes in, which names the extension.
    pub fn format(self) -> ClipFormat {
        self.codec.format()
    }
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
    /// The file clips and rendered edits are made as; always the one
    /// `clip_codec` goes in once loaded or saved (`normalized`).
    pub clip_format: ClipFormat,
    /// The codec clips are encoded with, and their quality; unset only in a
    /// file from before they were a choice, read as clips were made then
    /// (`clip_encoding`).
    pub clip_codec: Option<ClipCodec>,
    pub clip_quality: Option<ClipQuality>,
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
    /// Whether the page shows the pictures of the streamers annotations
    /// name, asking the image server for each (accounts.ts `AVATAR_BASE`):
    /// the request names the site and the streamer's login. On unless turned
    /// off; off, nothing is asked and their initials show. Only the page
    /// reads it.
    pub load_cast_pictures: bool,
    /// Whether the note on permission shows before a video or clip is
    /// downloaded from a site (the page's permission.ts): on until its
    /// "Don't show this again" is ticked or it's turned off in Settings.
    /// Only the page reads it.
    pub ask_before_downloading: bool,
    /// Whether an MP4 clip dragged out onto DaVinci Resolve on Linux is
    /// refused there and the page asks to make clips as MOV (the app's
    /// resolve.rs): on until its "Don't show again" is ticked or it's turned
    /// off in Settings.
    pub suggest_mov_for_resolve: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: Device::default(),
            theme: Theme::default(),
            emote_providers: EmoteProviders::default(),
            video_height: None,
            clip_format: ClipFormat::default(),
            clip_codec: None,
            clip_quality: None,
            key_bindings: BTreeMap::new(),
            show_keys: false,
            ask_for_annotations: true,
            load_cast_pictures: true,
            ask_before_downloading: true,
            suggest_mov_for_resolve: true,
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

    /// What clips and rendered edits are encoded as. Unset, as before they
    /// were a choice: the format's codec, H.264 at CRF 18 or DNxHR LB (the
    /// lowest step, as `ClipQuality::Proxy`).
    pub fn clip_encoding(&self) -> ClipEncoding {
        let codec = self.clip_codec.unwrap_or(ClipCodec::of(self.clip_format));
        let then = if codec == ClipCodec::Dnxhr { ClipQuality::Proxy } else { ClipQuality::Standard };
        ClipEncoding { codec, quality: self.clip_quality.unwrap_or(then) }
    }

    /// The codec and quality set, and the format the one the codec goes in:
    /// the codec decides, so a page that changes it needn't change the format.
    pub fn normalized(mut self) -> Self {
        let ClipEncoding { codec, quality } = self.clip_encoding();
        self.clip_codec = Some(codec);
        self.clip_quality = Some(quality);
        self.clip_format = codec.format();
        self
    }

    /// Missing or unreadable settings fall back to defaults.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else { return Self::default().normalized() };
        serde_json::from_str::<Self>(&text)
            .unwrap_or_else(|e| {
                eprintln!("{}: unreadable, using the defaults: {e}", path.display());
                Self::default()
            })
            .normalized()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        // One at a time: two saves at once (quick toggles in Settings) would
        // write the same temporary file, which could keep the older one or
        // leave unreadable JSON, and the next launch would fall back to defaults.
        static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _turn = SAVING.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&self.clone().normalized())?)?;
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
    fn clip_codecs_follow_old_formats_and_lead_new_ones() {
        // A settings.json as 0.7.1 wrote it: a format, no codec or quality.
        let read = |json: &str| serde_json::from_str::<Settings>(json).unwrap().normalized();
        let mov = read(r#"{"clip_format": "mov"}"#);
        assert_eq!(mov.clip_encoding(), ClipEncoding { codec: ClipCodec::Dnxhr, quality: ClipQuality::Proxy });
        assert_eq!(read(r#"{"clip_format": "mp4"}"#).clip_encoding(), ClipEncoding::default());
        assert_eq!(read("{}").clip_encoding(), ClipEncoding::default());
        assert_eq!(read("{}").clip_format, ClipFormat::Mp4);
        // The codec decides the format, whatever the page sent with it.
        let prores = read(r#"{"clip_format": "mp4", "clip_codec": "prores", "clip_quality": "high"}"#);
        assert_eq!((prores.clip_format, prores.clip_quality), (ClipFormat::Mov, Some(ClipQuality::High)));
        assert_eq!(read(r#"{"clip_format": "mov", "clip_codec": "h265"}"#).clip_format, ClipFormat::Mp4);
        let again = read(&serde_json::to_string(&prores).unwrap());
        assert_eq!(again.clip_encoding(), ClipEncoding { codec: ClipCodec::Prores, quality: ClipQuality::High });
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

    #[test]
    fn loading_cast_pictures_starts_on_and_stays_as_left() {
        // A settings.json as 0.6.0 wrote it: no word of pictures.
        let old: Settings = serde_json::from_str(r#"{"theme": "dark", "ask_for_annotations": false}"#).unwrap();
        assert!(old.load_cast_pictures);
        assert!(Settings::default().load_cast_pictures);
        let off = Settings { load_cast_pictures: false, ..old };
        let again: Settings = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
        assert!(!again.load_cast_pictures);
        assert!(!again.ask_for_annotations);
    }

    #[test]
    fn the_notes_start_shown_and_stay_as_left() {
        // A settings.json as 0.7.0 wrote it: no word of the notes.
        let old: Settings = serde_json::from_str(r#"{"theme": "dark", "load_cast_pictures": false}"#).unwrap();
        assert!(old.ask_before_downloading && old.suggest_mov_for_resolve);
        assert!(Settings::default().ask_before_downloading && Settings::default().suggest_mov_for_resolve);
        let off = Settings { ask_before_downloading: false, suggest_mov_for_resolve: false, ..old };
        let again: Settings = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
        assert!(!again.ask_before_downloading && !again.suggest_mov_for_resolve);
        assert!(!again.load_cast_pictures);
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
