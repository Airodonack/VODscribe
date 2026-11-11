//! The specs of the settings file, at its seam: `Settings::load` and
//! `Settings::save` on a file in a folder of its own, and the JSON as written.
//! A spec each, named as it is said (plans/sections-074.md, 1B).

use super::*;
use serde_json::{Value, json};

/// A settings folder of its own, and the file as the app reads it.
struct File {
    _dir: crate::TempDir,
    path: PathBuf,
}

impl File {
    /// A folder with no settings file in it.
    fn none(name: &str) -> Self {
        let dir = crate::TempDir::new(name);
        let path = dir.join("settings.json");
        File { _dir: dir, path }
    }

    /// A folder with a settings file written as `text`, by hand or by an
    /// older version.
    fn with(name: &str, text: &str) -> Self {
        let file = Self::none(name);
        fs::write(&file.path, text).unwrap();
        file
    }

    fn load(&self) -> Settings {
        Settings::load(&self.path)
    }

    /// The file as the page reads it.
    fn written(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(&self.path).unwrap()).unwrap()
    }

    /// Saved as it is, then read again as the next launch does.
    fn saved_and_loaded(&self, settings: &Settings) -> Settings {
        settings.save(&self.path).unwrap();
        self.load()
    }
}

/// The settings a file written as `json` is read as.
fn read(json: &str) -> Settings {
    File::with("settings-read", json).load()
}

/// Read, saved and read again: what the user gets at the launch after the one
/// that changed `change`.
fn left_as(json: &str, change: impl FnOnce(&mut Settings)) -> Settings {
    let file = File::with("settings-left", json);
    let mut settings = file.load();
    change(&mut settings);
    file.saved_and_loaded(&settings)
}

/// Missing settings are the defaults (settings.rs `load`: "Missing or
/// unreadable settings fall back to defaults").
#[test]
fn no_settings_file_is_every_default() {
    let file = File::none("settings-missing");
    let loaded = file.load();
    assert_eq!(loaded.theme, Theme::System);
    assert_eq!(loaded.video_height, None);
    assert_eq!(loaded.clip_encoding(), ClipEncoding::default());
    assert!(loaded.ask_for_annotations && loaded.load_cast_pictures && !loaded.show_keys);
    assert!(loaded.key_bindings.is_empty());
    assert_eq!(loaded.emote_providers.enabled(), Vec::<&str>::new());
    assert!(!file.path.exists(), "reading writes nothing");
}

/// Unreadable settings are the defaults (settings.rs `load`: "Missing or
/// unreadable settings fall back to defaults"). What becomes of the file is
/// `an_unreadable_settings_file_is_set_aside_not_saved_over`.
#[test]
fn an_unreadable_settings_file_is_every_default() {
    for text in ["", "{", "not settings", r#"["theme"]"#, r#"{"theme": "purple"}"#, r#"{"video_height": "tall"}"#] {
        let loaded = read(text);
        assert_eq!(loaded.theme, Theme::System, "{text:?}");
        assert_eq!(loaded.video_height, None, "{text:?}");
        assert!(loaded.ask_for_annotations, "{text:?}");
    }
}

/// A key binding written by hand in the wrong shape (a string for a
/// list, a number in one) made the whole file unreadable, so every
/// setting fell back to its default, and the next change saved those
/// over the file. Only the bindings that aren't lists of keys go
/// (settings.rs `bindings`: "any other (the file edited by hand) is left
/// out, so that action has its defaults, rather than making the whole file
/// unreadable and every setting its default").
#[test]
fn a_misshapen_key_binding_loses_only_itself() {
    let loaded = read(
        r#"{"theme": "dark", "video_height": 720, "key_bindings":
            {"undo": "Ctrl+q", "redo": ["Ctrl+y", 5], "mute.toggle": [], "play.toggle": ["p", "k"], "x": null}}"#,
    );
    assert_eq!((loaded.theme, loaded.video_height), (Theme::Dark, Some(720)));
    assert_eq!(loaded.key_bindings.keys().collect::<Vec<_>>(), ["mute.toggle", "play.toggle"]);
    assert_eq!(loaded.key_bindings["play.toggle"], ["p", "k"]);
    let loaded = read(r#"{"theme": "light", "key_bindings": ["undo"]}"#);
    assert_eq!(loaded.theme, Theme::Light);
    assert!(loaded.key_bindings.is_empty());
}

/// "Every keyboard control can be changed" (CHANGELOG.md, 0.6.0); a file
/// from before has none (settings.rs `key_bindings`: "The keyboard controls
/// the user changed"), as does a new one.
#[test]
fn key_bindings_start_empty() {
    // A settings.json as 0.5.x wrote it: no key bindings.
    let old = read(r#"{"device": "cpu", "theme": "dark", "emote_providers": [], "video_height": 720}"#);
    assert!(old.key_bindings.is_empty());
    assert!(File::none("settings-new").load().key_bindings.is_empty());
    assert_eq!(File::none("settings-new-saved").saved_and_loaded(&Settings::default()).key_bindings.len(), 0);
}

/// A binding of no keys is kept as none, not put back to its default
/// (settings.rs `key_bindings`: "an empty one for none").
#[test]
fn a_key_binding_of_no_keys_is_kept_as_none() {
    let again = left_as(r#"{"key_bindings": {"play.toggle": ["p"], "mute.toggle": []}}"#, |_| {});
    assert!(again.key_bindings["mute.toggle"].is_empty());
    assert_eq!(again.key_bindings["play.toggle"], ["p"]);
}

/// Bindings of actions the page no longer has stay in the file as they are
/// (settings.rs `key_bindings`: "ids it no longer has are kept as they are").
#[test]
fn bindings_of_actions_the_page_no_longer_has_are_kept() {
    let file = File::with("settings-gone", r#"{"key_bindings": {"gone.away": ["x", "Ctrl+x"]}}"#);
    file.saved_and_loaded(&file.load());
    assert_eq!(file.written()["key_bindings"], json!({"gone.away": ["x", "Ctrl+x"]}));
}

/// All off until the user turns them on (settings.rs `emote_providers`: "All
/// off until the user turns them on, since each is a service contacted";
/// CHANGELOG.md, 0.2.1, "Chat can show 7TV, BetterTTV and FrankerFaceZ
/// emotes": "Each is off until you turn it on").
#[test]
fn emote_providers_start_off() {
    for loaded in [File::none("settings-emotes").load(), read("{}"), read(r#"{"emote_providers": []}"#)] {
        assert_eq!(loaded.emote_providers.enabled(), Vec::<&str>::new());
        assert_eq!(loaded.emote_providers.0.len(), crate::word_emotes::PROVIDER_IDS.len());
        assert!(loaded.emote_providers.0.iter().all(|p| !p.on));
    }
}

/// The known providers, the first of each, in the order the user gave (the
/// order that wins), any missing added after, off (settings.rs
/// `EmoteProviders::from`: "Keeps the known ids, first of each, in the order
/// given; any missing are added after, off"; CHANGELOG.md, 0.2.1, same
/// bullet: "drag them to choose which wins").
#[test]
fn emote_providers_keep_their_order_and_only_known_ones_once_with_new_ones_after_off() {
    let saved = left_as(
        r#"{"emote_providers": [{"id": "ffz", "on": true}, {"id": "nope", "on": true}, {"id": "ffz", "on": false}, {"id": "7tv", "on": true}]}"#,
        |_| {},
    );
    assert_eq!(saved.emote_providers.enabled(), ["ffz", "7tv"]);
    let order: Vec<_> = saved.emote_providers.0.iter().map(|p| (p.id.as_str(), p.on)).collect();
    assert_eq!(order, [("ffz", true), ("7tv", true), ("bttv", false)]);
}

/// Unset, the tallest video downloaded is 1080p (settings.rs
/// `DEFAULT_VIDEO_HEIGHT`: "1080p, the most offered"; plans/sections-074.md
/// "Decided" 2026-11-11, D3: the default since f238005, "Downloaded video is
/// 1080p unless chosen otherwise, clips included", v0.4.0). The page's
/// DEFAULT_HEIGHT agrees: core pipeline.rs checks it.
#[test]
fn an_unset_video_height_is_1080() {
    assert_eq!(File::none("settings-height").load().video_height(), 1080);
    assert_eq!(read(r#"{"video_height": null}"#).video_height(), 1080);
    assert_eq!(read(r#"{"video_height": 720}"#).video_height(), 720);
}

/// A file from before codecs were a choice makes clips as they were made
/// then: the format's codec, H.264 at CRF 18 (standard) or DNxHR LB (proxy)
/// (settings.rs `clip_encoding`: "Unset, as before they were a choice";
/// plans/clip-encodings.md "Cautions"; clips pilot C12).
#[test]
fn a_file_from_before_codecs_makes_clips_as_they_were_made_then() {
    // A settings.json as 0.7.1 wrote it: a format, no codec or quality.
    let mov = read(r#"{"clip_format": "mov"}"#);
    assert_eq!(mov.clip_encoding(), ClipEncoding { codec: ClipCodec::Dnxhr, quality: ClipQuality::Proxy });
    assert_eq!(read(r#"{"clip_format": "mp4"}"#).clip_encoding(), ClipEncoding::default());
    assert_eq!(read("{}").clip_encoding(), ClipEncoding { codec: ClipCodec::H264, quality: ClipQuality::Standard });
    assert_eq!(read("{}").clip_format, ClipFormat::Mp4);
}

/// The codec decides the format, whatever the page sent with it (settings.rs
/// `normalized`: "the codec decides, so a page that changes it needn't change
/// the format").
#[test]
fn the_codec_decides_the_format_whatever_the_page_sent() {
    let prores = read(r#"{"clip_format": "mp4", "clip_codec": "prores", "clip_quality": "high"}"#);
    assert_eq!((prores.clip_format, prores.clip_quality), (ClipFormat::Mov, Some(ClipQuality::High)));
    assert_eq!(read(r#"{"clip_format": "mov", "clip_codec": "h265"}"#).clip_format, ClipFormat::Mp4);
    assert_eq!(read(r#"{"clip_format": "mp4", "clip_codec": "dnxhr"}"#).clip_format, ClipFormat::Mov);
    assert_eq!(read(r#"{"clip_format": "mov", "clip_codec": "h264"}"#).clip_format, ClipFormat::Mp4);
}

/// A saved file says the codec, quality and format together, so a version that
/// reads only the format agrees with one that reads the codec (settings.rs
/// `clip_format`: "always the one `clip_codec` goes in once loaded or saved";
/// `save` applies `normalized`).
#[test]
fn a_saved_file_says_the_codec_quality_and_format_together() {
    let file = File::none("settings-encoding");
    let page_sent =
        Settings { clip_codec: Some(ClipCodec::Prores), clip_quality: Some(ClipQuality::High), ..Default::default() };
    file.saved_and_loaded(&page_sent);
    let written = file.written();
    assert_eq!(
        (&written["clip_codec"], &written["clip_quality"], &written["clip_format"]),
        (&json!("prores"), &json!("high"), &json!("mov"))
    );
    // A file from before codecs gets all three at its first save.
    let old = File::with("settings-encoding-old", r#"{"clip_format": "mov"}"#);
    old.saved_and_loaded(&old.load());
    let written = old.written();
    assert_eq!(
        (&written["clip_codec"], &written["clip_quality"], &written["clip_format"]),
        (&json!("dnxhr"), &json!("proxy"), &json!("mov"))
    );
}

/// The keyboard legend starts closed, and left open it stays open after a
/// restart (settings.rs `show_keys`: "left open, it stays open across VODs,
/// edits and launches"; CHANGELOG.md, 0.6.0, "The keyboard button opens a
/// cheat sheet.").
#[test]
fn the_keyboard_legend_starts_closed_and_stays_as_left() {
    // A settings.json as 0.5.x wrote it: no word of the legend.
    let json = r#"{"theme": "dark", "video_height": 720}"#;
    assert!(!read(json).show_keys);
    let again = left_as(json, |s| s.show_keys = true);
    assert!(again.show_keys);
    assert_eq!(again.video_height, Some(720));
    assert!(!left_as(r#"{"show_keys": true}"#, |s| s.show_keys = false).show_keys);
}

/// Asking for annotations starts on and stays as left (settings.rs
/// `ask_for_annotations`: "On unless turned off"; CHANGELOG.md, 0.7.0,
/// "Asking for annotations can be off.").
#[test]
fn asking_for_annotations_starts_on_and_stays_as_left() {
    // A settings.json as 0.6.0 wrote it: no word of annotations.
    let json = r#"{"theme": "dark", "show_keys": true}"#;
    assert!(read(json).ask_for_annotations);
    assert!(File::none("settings-annotations").load().ask_for_annotations);
    let again = left_as(json, |s| s.ask_for_annotations = false);
    assert!(!again.ask_for_annotations);
    assert!(again.show_keys);
}

/// Loading the streamers' pictures starts on and stays as left (settings.rs
/// `load_cast_pictures`: "On unless turned off"; CHANGELOG.md, 0.7.0,
/// "Streamers' pictures can be turned off.").
#[test]
fn loading_streamer_pictures_starts_on_and_stays_as_left() {
    // A settings.json as 0.6.0 wrote it: no word of pictures.
    let json = r#"{"theme": "dark", "ask_for_annotations": false}"#;
    assert!(read(json).load_cast_pictures);
    assert!(File::none("settings-pictures").load().load_cast_pictures);
    let again = left_as(json, |s| s.load_cast_pictures = false);
    assert!(!again.load_cast_pictures);
    assert!(!again.ask_for_annotations);
}

/// The notes start shown and stay as left (settings.rs `ask_before_downloading`
/// and `suggest_mov_for_resolve`: "on until its 'Don't ask again' is ticked or
/// it's turned off in Settings"; CHANGELOG.md, 0.7.1, "Settings decide which
/// notes are shown.").
#[test]
fn the_notes_start_shown_and_stay_as_left() {
    // A settings.json as 0.7.0 wrote it: no word of the notes.
    let json = r#"{"theme": "dark", "load_cast_pictures": false}"#;
    let old = read(json);
    assert!(old.ask_before_downloading && old.suggest_mov_for_resolve && old.ask_to_use_vodscription);
    let fresh = File::none("settings-notes").load();
    assert!(fresh.ask_before_downloading && fresh.suggest_mov_for_resolve && fresh.ask_to_use_vodscription);
    let again = left_as(json, |s| {
        s.ask_before_downloading = false;
        s.suggest_mov_for_resolve = false;
    });
    assert!(!again.ask_before_downloading && !again.suggest_mov_for_resolve);
    assert!(again.ask_to_use_vodscription, "each note its own");
    assert!(again.ask_for_annotations, "the offer's own switch, apart from looking up");
    assert!(!again.load_cast_pictures);
    let again = left_as(json, |s| s.use_vodscription = Some(UseVodscription::Never));
    assert!(again.ask_before_downloading && again.suggest_mov_for_resolve);
}

/// A VODscription found is used as last answered (settings.rs
/// `use_vodscription`; CHANGELOG.md, 0.7.2, "VODscription offers remember
/// your answer."): ask, always or never, kept across a restart.
#[test]
fn a_vodscription_found_is_used_as_last_answered() {
    for answer in [UseVodscription::Ask, UseVodscription::Always, UseVodscription::Never] {
        let again = left_as("{}", |s| s.use_vodscription = Some(answer));
        assert_eq!(again.use_vodscription(), answer);
    }
    assert_eq!(read("{}").use_vodscription(), UseVodscription::Ask);
}

/// A file from before the three answers reads its switch (settings.rs
/// `use_vodscription()`: "Unset, as the switch before it had it: on, asked;
/// off, never").
#[test]
fn a_file_from_before_the_three_answers_reads_its_switch() {
    assert_eq!(read(r#"{"ask_to_use_vodscription": false}"#).use_vodscription(), UseVodscription::Never);
    assert_eq!(read(r#"{"ask_to_use_vodscription": true}"#).use_vodscription(), UseVodscription::Ask);
    assert_eq!(read("{}").use_vodscription(), UseVodscription::Ask);
}

/// The old switch is written to match the answer, for a version that reads
/// only it: on only while the answer is Ask (settings.rs
/// `ask_to_use_vodscription`: "written as on only while it's `Ask`").
#[test]
fn the_old_switch_is_written_to_match_for_a_version_that_reads_only_it() {
    for (answer, asks) in [("ask", true), ("always", false), ("never", false)] {
        // Whatever the switch said, the answer decides.
        let file = File::with(
            "settings-switch",
            &format!(r#"{{"use_vodscription": "{answer}", "ask_to_use_vodscription": {}}}"#, !asks),
        );
        file.saved_and_loaded(&file.load());
        let written = file.written();
        assert_eq!(written["use_vodscription"], answer);
        assert_eq!(written["ask_to_use_vodscription"], asks, "{answer}");
    }
}

/// The theme starts as the system's (settings.rs `Theme`: default System;
/// CHANGELOG.md, 0.2.1, "Settings has a new Appearance section.": "System (as
/// before), Light or Dark"), and a choice stays.
#[test]
fn the_theme_starts_as_the_systems() {
    assert_eq!(File::none("settings-theme").load().theme, Theme::System);
    assert_eq!(read("{}").theme, Theme::System);
    for theme in [Theme::Light, Theme::Dark, Theme::System] {
        assert_eq!(left_as("{}", |s| s.theme = theme).theme, theme);
    }
}

/// What's in the settings folder besides `settings.json`, by name.
fn beside(dir: &Path) -> Vec<(String, String)> {
    let mut found: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), fs::read_to_string(e.path()).unwrap_or_default()))
        .filter(|(name, _)| name != "settings.json")
        .collect();
    found.sort();
    found
}

/// A settings file that can't be read is set aside, as a library that won't
/// open is, so the next change doesn't save the defaults over it (plans/
/// sections-074.md "Decided", unstated behaviour 3, owner 2026-11-11: "an
/// unreadable settings file is set aside like an unreadable library and the
/// user told").
#[test]
fn an_unreadable_settings_file_is_set_aside_not_saved_over() {
    let dir = crate::TempDir::new("settings-unreadable");
    let path = dir.join("settings.json");
    let unreadable = r#"{"theme": "dark", "video_height": 720,"#;
    fs::write(&path, unreadable).unwrap();
    let loaded = Settings::load(&path);
    assert_eq!(loaded.theme, Theme::System, "read as every default");
    Settings { show_keys: true, ..loaded }.save(&path).unwrap();
    let kept = beside(&dir);
    assert_eq!(kept.len(), 1, "one file set aside: {kept:?}");
    let (name, text) = &kept[0];
    assert!(name.starts_with("settings-broken-") && name.ends_with(".json"), "{name}");
    assert_eq!(text, unreadable, "kept as it was");
    assert!(Settings::load(&path).show_keys, "the change is saved in its place");
    assert_eq!(Settings::taken_aside(&path), Some(dir.join(name)), "where it went, to tell the user");
    assert_eq!(Settings::taken_aside(&path), None, "told once");
}

/// Another unreadable file the same day goes beside the first, never over
/// it, as a library set aside twice in a day does (db.rs `set_aside`:
/// "`-2`… if taken").
#[test]
fn a_second_unreadable_settings_file_the_same_day_keeps_the_first() {
    let dir = crate::TempDir::new("settings-unreadable-twice");
    let path = dir.join("settings.json");
    fs::write(&path, "first").unwrap();
    Settings::load(&path);
    fs::write(&path, "second").unwrap();
    Settings::load(&path);
    let mut kept: Vec<_> = beside(&dir).into_iter().map(|(name, text)| (text, name.ends_with("-2.json"))).collect();
    kept.sort();
    assert_eq!(kept, [("first".to_string(), false), ("second".to_string(), true)]);
    assert!(Settings::taken_aside(&path).is_some_and(|aside| aside.to_string_lossy().ends_with("-2.json")));
}

/// A missing file, or one only a key binding of is misshapen, is read as
/// it is, and nothing is set aside (`bindings`: "rather than making the
/// whole file unreadable and every setting its default").
#[test]
fn a_missing_or_readable_settings_file_is_not_set_aside() {
    let dir = crate::TempDir::new("settings-readable");
    let path = dir.join("settings.json");
    assert_eq!(Settings::load(&path).theme, Theme::System);
    fs::write(&path, r#"{"theme": "dark", "key_bindings": {"undo": "Ctrl+q"}}"#).unwrap();
    assert_eq!(Settings::load(&path).theme, Theme::Dark);
    assert_eq!(beside(&dir), []);
    assert_eq!(Settings::taken_aside(&path), None);
}

/// The settings files earlier versions wrote (tests/e2e/fixtures, as the
/// upgrade scenarios start from them) are read, never set aside.
#[test]
fn the_settings_files_earlier_versions_wrote_are_not_set_aside() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/fixtures");
    let mut seen = 0;
    for entry in fs::read_dir(&fixtures).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("settings-") && name.ends_with(".json")) {
            continue;
        }
        let dir = crate::TempDir::new("settings-earlier");
        let path = dir.join("settings.json");
        fs::copy(entry.path(), &path).unwrap();
        Settings::load(&path);
        assert_eq!(Settings::taken_aside(&path), None, "{name}");
        assert!(path.exists(), "{name}");
        seen += 1;
    }
    assert!(seen >= 5, "the fixtures were found: {seen}");
}

/// A settings file that can't be read for lack of permission is read as the
/// defaults and set aside too, as one that isn't settings is (settings.rs
/// `load`: "Unreadable ones ... are the defaults too, and the file is set
/// aside").
#[cfg(unix)]
#[test]
fn a_settings_file_the_user_may_not_read_is_set_aside() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::TempDir::new("settings-denied");
    let path = dir.join("settings.json");
    fs::write(&path, r#"{"theme": "dark"}"#).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&path).is_ok() {
        return; // running as root: the file can't be made unreadable
    }
    assert_eq!(Settings::load(&path).theme, Theme::System);
    assert!(!path.exists());
    assert!(Settings::taken_aside(&path).is_some());
}

/// A clip's encoding goes in the file its codec does (settings.rs
/// `ClipEncoding::format`: "The file it goes in"; `ClipCodec::format`).
#[test]
fn a_clip_encoding_goes_in_the_file_its_codec_does() {
    let of = |codec| ClipEncoding { codec, quality: ClipQuality::Standard }.format();
    assert_eq!(
        [ClipCodec::H264, ClipCodec::H265, ClipCodec::Dnxhr, ClipCodec::Prores].map(of),
        [ClipFormat::Mp4, ClipFormat::Mp4, ClipFormat::Mov, ClipFormat::Mov]
    );
}
