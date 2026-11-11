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
    assert!(old.ask_before_downloading && old.suggest_mov_for_resolve && old.ask_to_use_vodscription);
    let fresh = Settings::default();
    assert!(fresh.ask_before_downloading && fresh.suggest_mov_for_resolve && fresh.ask_to_use_vodscription);
    let off = Settings {
        ask_before_downloading: false,
        suggest_mov_for_resolve: false,
        ask_to_use_vodscription: false,
        ..old
    };
    let again: Settings = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
    assert!(!again.ask_before_downloading && !again.suggest_mov_for_resolve && !again.ask_to_use_vodscription);
    assert!(again.ask_for_annotations, "the offer's own switch, apart from looking up");
    assert!(!again.load_cast_pictures);
}

#[test]
fn a_vodscription_found_is_used_as_last_answered() {
    let read = |json: &str| serde_json::to_value(serde_json::from_str::<Settings>(json).unwrap().normalized()).unwrap();
    // A settings.json from before the three answers: the offer's switch alone.
    assert_eq!(read(r#"{"ask_to_use_vodscription": false}"#)["use_vodscription"], "never");
    assert_eq!(read(r#"{"ask_to_use_vodscription": true}"#)["use_vodscription"], "ask");
    assert_eq!(read("{}")["use_vodscription"], "ask");
    // The answer decides, and the switch is written to match, for an app
    // that reads only that: on only while the offer is asked.
    for (answer, asks) in [("ask", true), ("always", false), ("never", false)] {
        let saved = read(&format!(r#"{{"use_vodscription": "{answer}", "ask_to_use_vodscription": {}}}"#, !asks));
        assert_eq!(saved["use_vodscription"], answer);
        assert_eq!(saved["ask_to_use_vodscription"], asks, "{answer}");
        assert_eq!(read(&saved.to_string())["use_vodscription"], answer, "{answer}, read again");
    }
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
