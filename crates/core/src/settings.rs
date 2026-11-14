use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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
    /// Every codec, in the order Settings › Encoding offers them.
    pub const ALL: [ClipCodec; 4] = [ClipCodec::H264, ClipCodec::H265, ClipCodec::Dnxhr, ClipCodec::Prores];

    /// Its name as Settings › Encoding shows it (the page's `CODEC_NAMES`).
    pub fn name(self) -> &'static str {
        match self {
            ClipCodec::H264 => "MP4 H.264",
            ClipCodec::H265 => "MP4 H.265",
            ClipCodec::Dnxhr => "MOV DNxHR",
            ClipCodec::Prores => "MOV ProRes",
        }
    }

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
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
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

/// What's done with a VODscription found for a VOD just added or refreshed
/// (the page's offers.svelte.ts). The offer asked for (its gift button, its
/// row's menu) shows whatever this is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UseVodscription {
    /// Offered, with a "Don't ask again" box that keeps the answer given.
    #[default]
    Ask,
    /// Used with no offer, and a toast says so.
    Always,
    /// Not offered; its gift button still offers it.
    Never,
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
    /// "Don't ask again" is ticked or it's turned off in Settings. Only the
    /// page reads it.
    pub ask_before_downloading: bool,
    /// Whether an MP4 clip dragged out onto DaVinci Resolve on Linux is
    /// refused there and the page asks to make clips as MOV (the app's
    /// resolve.rs): on until its "Don't ask again" is ticked or it's turned
    /// off in Settings.
    pub suggest_mov_for_resolve: bool,
    /// What's done with a VODscription found (Settings › Privacy, or the
    /// offer's "Don't ask again"); unset only in a file from before it was
    /// a choice, read from `ask_to_use_vodscription` (`use_vodscription`).
    /// Looking one up is `ask_for_annotations`, apart from this. Only the
    /// page reads it.
    pub use_vodscription: Option<UseVodscription>,
    /// The offer's switch before `use_vodscription`, on or off: written as
    /// on only while it's `Ask` (`normalized`), for a version that reads
    /// only this.
    pub ask_to_use_vodscription: bool,
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
            use_vodscription: None,
            ask_to_use_vodscription: true,
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
/// hand) is left out, so that action has its defaults, rather than every
/// binding going with it (`read_each`).
fn bindings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Vec<String>>, D::Error> {
    let Value::Object(all) = Value::deserialize(d)? else { return Ok(BTreeMap::new()) };
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

    /// What's done with a VODscription found. Unset, as the switch before
    /// it had it: on, asked; off, never.
    pub fn use_vodscription(&self) -> UseVodscription {
        self.use_vodscription.unwrap_or(if self.ask_to_use_vodscription {
            UseVodscription::Ask
        } else {
            UseVodscription::Never
        })
    }

    /// The codec and quality set, and the format the one the codec goes in:
    /// the codec decides, so a page that changes it needn't change the format.
    /// So with a VODscription found: what's done decides, and the switch
    /// before it is written to match.
    pub fn normalized(mut self) -> Self {
        let ClipEncoding { codec, quality } = self.clip_encoding();
        self.clip_codec = Some(codec);
        self.clip_quality = Some(quality);
        self.clip_format = codec.format();
        let found = self.use_vodscription();
        self.use_vodscription = Some(found);
        self.ask_to_use_vodscription = found == UseVodscription::Ask;
        self
    }

    /// Missing settings are the defaults. A file that isn't a JSON object (a
    /// damaged file, or one edited by hand into what isn't settings) is read
    /// as the defaults too, and set aside (`set_aside`), so the next change
    /// doesn't save the defaults over it; `taken_aside` says where it went.
    /// Otherwise each setting is read on its own (`read_each`): a value this
    /// version doesn't know (a codec or theme a later version adds) or of the
    /// wrong type loses only itself, and is kept in the file (`save`).
    pub fn load(path: &Path) -> Self {
        let read = match fs::read(path) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default().normalized(),
            Err(e) => Err(e.to_string()),
        };
        let unreadable = match &read {
            Ok(bytes) => match serde_json::from_slice::<Value>(bytes) {
                Ok(Value::Object(file)) => return read_each(file).0.normalized(),
                Ok(_) => "not a JSON object".to_string(),
                Err(e) => e.to_string(),
            },
            Err(e) => e.clone(),
        };
        #[cfg(any(test, feature = "testkit"))]
        crate::testkit::tools::held_at(crate::testkit::tools::At::SettingsRead, path);
        match set_aside(path, read.as_deref().ok()) {
            Ok(Some(aside)) => {
                eprintln!("{}: unreadable ({unreadable}), set aside as {}", path.display(), aside.display())
            }
            Ok(None) => {}
            Err(e) => eprintln!("{}: unreadable ({unreadable}), using the defaults: {e}", path.display()),
        }
        Self::default().normalized()
    }

    /// Where the settings file at `path` was set aside as unreadable (`load`)
    /// since this was last asked, for the user to be told once.
    pub fn taken_aside(path: &Path) -> Option<PathBuf> {
        lock(&ASIDE).remove(path)
    }

    /// Writes the settings over the file at `path`, keeping what `load`
    /// left out of it (`kept_from`) for the version that wrote it.
    pub fn save(&self, path: &Path) -> Result<()> {
        #[cfg(any(test, feature = "testkit"))]
        crate::testkit::tools::held_at(crate::testkit::tools::At::SettingsSave, path);
        let _turn = lock(&SAVING);
        let settings = self.clone().normalized();
        let text = match kept_from(path, &settings)? {
            None => serde_json::to_string_pretty(&settings)?,
            Some(written) => serde_json::to_string_pretty(&written)?,
        };
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}

/// The settings in `file`, each read on its own, as a missing one would be
/// when its value can't be read (a value of the wrong type, or one this
/// version doesn't know); and those left out, with every key this version
/// doesn't have, as they were written.
fn read_each(file: Map<String, Value>) -> (Settings, Map<String, Value>) {
    let known = serde_json::to_value(Settings::default()).expect("settings are written as JSON");
    let (read, left_out): (Map<_, _>, Map<_, _>) = file.into_iter().partition(|(key, value)| {
        known.get(key).is_some() && serde_json::from_value::<Settings>(serde_json::json!({ key: value })).is_ok()
    });
    // Each was read alone, and no setting is read from another, so together
    // they read too.
    (serde_json::from_value(Value::Object(read)).unwrap_or_default(), left_out)
}

/// What to write for `settings` over the file at `path`, if that file holds
/// what `load` left out (`read_each`): a later version's value, or a setting
/// it added. Each is written back as it was unless the user changed that
/// setting since, so going back a version and changing another setting
/// doesn't lose it for the later one. `None` when there's nothing to keep.
fn kept_from(path: &Path, settings: &Settings) -> Result<Option<Value>> {
    let Some(Value::Object(file)) = fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()) else {
        return Ok(None);
    };
    let (read_as, left_out) = read_each(file);
    if left_out.is_empty() {
        return Ok(None);
    }
    let read_as = serde_json::to_value(read_as.normalized())?;
    let mut written = serde_json::to_value(settings)?;
    if let Some(out) = written.as_object_mut() {
        for (key, value) in left_out {
            if out.get(&key) == read_as.get(&key) {
                out.insert(key, value);
            }
        }
    }
    Ok(Some(written))
}

/// Taken by each save, and by setting a file aside: two saves at once (quick
/// toggles in Settings) would write the same temporary file, which could keep
/// the older one or leave unreadable JSON, and the next launch would fall
/// back to defaults; and a file is set aside only while no save can replace it.
static SAVING: Mutex<()> = Mutex::new(());

/// The settings files set aside (`set_aside`) and not yet told of
/// (`Settings::taken_aside`), by the path they were read from.
static ASIDE: Mutex<BTreeMap<PathBuf, PathBuf>> = Mutex::new(BTreeMap::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Moves the unreadable settings file at `path` out of the way, as an
/// unreadable library is (`db::set_aside`): to `settings-broken-<date>.json`
/// beside it (`-2`… if taken), never deleted, for recovering or sending with
/// a bug report. Only if it still holds what was read (`read`, none if it
/// couldn't be): a save made since, or another load that set it aside
/// first, is left alone (`None`). Returns where it went.
fn set_aside(path: &Path, read: Option<&[u8]>) -> Result<Option<PathBuf>> {
    let _turn = lock(&SAVING);
    if read.is_some_and(|read| fs::read(path).ok().as_deref() != Some(read)) || !path.exists() {
        return Ok(None);
    }
    let date = &crate::db::utc_time(crate::db::now() as f64)[..10];
    let aside = (1..)
        .map(|n| {
            let name = if n == 1 {
                format!("settings-broken-{date}.json")
            } else {
                format!("settings-broken-{date}-{n}.json")
            };
            path.with_file_name(name)
        })
        .find(|aside| !aside.exists())
        .context("no free name")?;
    fs::rename(path, &aside)?;
    lock(&ASIDE).insert(path.to_path_buf(), aside.clone());
    Ok(Some(aside))
}

#[cfg(test)]
mod tests;
