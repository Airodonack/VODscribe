//! Reading a VODscription file: its format, limits and ids.

use super::*;

#[test]
fn the_chat_is_read_from_beside_the_file() {
    let dir = std::env::temp_dir().join(format!("vodscribe-annotations-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("v123.json");
    std::fs::write(&path, serde_json::to_string(&file("twitch", "v123")).unwrap()).unwrap();
    assert_eq!(chat_path(&path), Some(dir.join("v123.chat.json")));
    assert_eq!(read(&path, None).unwrap().1, None, "no chat beside it");
    let chat = r##"[{"id": 1, "time": 2.5, "author": "Ada", "color": "#ff0000",
                    "parts": [{"type": "text", "text": "hi "}, {"type": "emote", "name": "KEKW", "url": "https://x/1"}],
                    "kind": "message", "amount": null}]"##;
    std::fs::write(dir.join("v123.chat.json"), chat).unwrap();
    let (annotations, chat) = read(&path, None).unwrap();
    assert_eq!(annotations.source.id, "v123");
    let chat = chat.unwrap();
    assert_eq!((chat[0].time, chat[0].author.as_str(), chat[0].parts.len()), (2.5, "Ada", 2));
    let err = read(&dir.join("v123.chat.json"), None).unwrap_err().to_string();
    assert!(err.contains("pick the annotations file beside it"), "{err}");
    let long = format!(r#"[{{"time": 1, "author": "{}", "parts": []}}]"#, "x".repeat(MAX_TEXT + 1));
    std::fs::write(dir.join("v123.chat.json"), long).unwrap();
    let err = read(&path, None).unwrap_err().to_string();
    assert_eq!(err, "This chat file is too big: a text in it is longer than 10,000 characters.");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_wrong_file_says_so() {
    let err = parse("{\"format\": \"other\"}").unwrap_err().to_string();
    assert_eq!(err, "This isn't a VODscribe annotations file: its format is \"other\".");
    let err = parse(&format!("{{\"format\": \"{FORMAT}\", \"version\": 2}}")).unwrap_err().to_string();
    assert_eq!(err, "This isn't a VODscribe annotations file: version 2 isn't 1.");
    assert!(parse("not json").unwrap_err().to_string().contains("it isn't JSON"));
    // The page's own reader takes the same file.
    let json = serde_json::to_string(&file("twitch", "v1")).unwrap();
    assert_eq!(parse(&json).unwrap(), file("twitch", "v1"));
}

/// plans/sections-074.md, "Decided 2026-11-11": a VODscription file with no format or version
/// "is said plainly by app and page alike (\"it has no format\", \"it has no version\")"; the
/// page's `parseAnnotations` (annotations.ts) says the same.
#[test]
fn a_file_with_no_format_or_version_says_it_has_none() {
    let err = parse("{\"version\": 1}").unwrap_err().to_string();
    assert_eq!(err, "This isn't a VODscribe annotations file: it has no format.");
    let err = parse(&format!("{{\"format\": \"{FORMAT}\"}}")).unwrap_err().to_string();
    assert_eq!(err, "This isn't a VODscribe annotations file: it has no version.");
}

#[test]
fn a_chapter_icon_is_one_emoji_or_none() {
    for icon in ["🎮", " 🍳 ", "👩‍🍳", "🏳️‍🌈", "❤️", "🇫🇷"] {
        assert_eq!(one_emoji(icon), Some(icon.trim()), "{icon}");
    }
    for icon in ["", " ", "🎮🎮", "a", "🎮 game", "Game", "7", "?", ":)", "日"] {
        assert_eq!(one_emoji(icon), None, "{icon}");
    }
}

#[test]
fn a_card_id_is_lowercase_letters_digits_and_dashes() {
    for good in ["marlowe", "s-0001", "a_b", "7"] {
        assert_eq!(card_id(good), Some(good));
    }
    assert_eq!(card_id("  marlowe "), Some("marlowe"));
    for bad in ["", "-a", "_a", "Marlowe", "a/b", "..", "a.b", "a b", "é", &"a".repeat(65)] {
        assert_eq!(card_id(bad), None, "{bad:?}");
    }
    assert_eq!(card_url("http://h", "marlowe").as_deref(), Some("http://h/streamers/marlowe.json"));
    assert_eq!(card_url("http://h", "../x"), None);
    assert_eq!(index_url(BASE_URL), "https://dl.vodscribe.com/streamers.json");
}

#[test]
fn a_vods_file_is_at_its_site_and_id_escaped() {
    assert_eq!(
        file_url(BASE_URL, "twitch", "v2901646294"),
        "https://dl.vodscribe.com/annotations/twitch/v2901646294.json"
    );
    assert_eq!(file_url("http://h", "youtube", "a/../b?c"), "http://h/annotations/youtube/a%2F..%2Fb%3Fc.json");
    assert!(can_be_published("kick") && !can_be_published(crate::local::PLATFORM));
}

#[test]
fn a_file_with_more_than_the_app_takes_is_refused_with_a_message() {
    let segment = serde_json::json!({ "start": 1.0, "end": 2.0, "text": "hi" });
    let word = serde_json::json!({ "text": "hi", "start": 1.0, "end": 1.5 });
    let chapter = serde_json::json!({ "start": 0.0, "title": "A" });
    let highlight = serde_json::json!({ "start": 0.0, "end": 1.0, "title": "A", "kind": "funny" });
    let member = serde_json::json!({ "name": "A", "role": "friend" });
    let activity = serde_json::json!({ "start": 0.0, "end": 1.0, "kind": "game" });
    let cases = [
        (&["transcript", "segments"][..], many(MAX_SEGMENTS + 1, segment), "it has more than 200,000 lines"),
        (&["transcript", "segments", "0", "words"], many(MAX_WORDS + 1, word), "a line has more than 2,000 words"),
        (&["chapters"], many(MAX_CHAPTERS + 1, chapter), "it has more than 2,000 chapters"),
        (&["highlights"], many(MAX_HIGHLIGHTS + 1, highlight), "it has more than 5,000 highlights"),
        (&["highlights", "0", "cast"], many(MAX_NAMES + 1, "A".into()), "a highlight names more than 100 people"),
        (&["cast"], many(MAX_CAST + 1, member), "it has more than 1,000 people in its cast"),
        (&["cast", "0", "aliases"], many(MAX_NAMES + 1, "A".into()), "a cast member has more than 100 other names"),
        (&["activity"], many(MAX_ACTIVITY + 1, activity), "it has more than 10,000 stretches of activity"),
        (&["povs"], many(MAX_POVS + 1, serde_json::Value::Null), "it has more than 1,000 other points of view"),
        (&["chapters", "0", "title"], "x".repeat(MAX_TEXT + 1).into(), "a text in it is longer than 10,000 characters"),
        (&["produced", "build"], "é".repeat(MAX_TEXT + 1).into(), "a text in it is longer than 10,000 characters"),
    ];
    for (path, value, said) in cases {
        let err = from_value(file_with(path, value)).unwrap_err().to_string();
        assert_eq!(err, format!("This annotations file is too big: {said}."), "{path:?}");
    }
    // At the limits, it's read.
    let at = [
        (&["chapters", "0", "title"][..], "é".repeat(MAX_TEXT).into()),
        (&["cast", "0", "aliases"], many(MAX_NAMES, "A".into())),
        (&["chapters"], many(MAX_CHAPTERS, serde_json::json!({ "start": 0.0, "title": "A" }))),
    ];
    for (path, value) in at {
        from_value(file_with(path, value)).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    }
}

#[test]
fn a_chat_file_with_more_than_the_app_takes_is_refused_with_a_message() {
    let message = serde_json::json!({ "time": 1.0, "author": "Ada", "parts": [{ "type": "text", "text": "hi" }] });
    let part = serde_json::json!({ "type": "text", "text": "hi" });
    let mut long = message.clone();
    long["parts"] = many(MAX_PARTS + 1, part);
    let mut wordy = message.clone();
    wordy["author"] = "x".repeat(MAX_TEXT + 1).into();
    let cases = [
        (many(MAX_MESSAGES + 1, serde_json::Value::Null), "it has more than 2,000,000 messages"),
        (serde_json::json!([long]), "a message has more than 1,000 parts"),
        (serde_json::json!([wordy]), "a text in it is longer than 10,000 characters"),
    ];
    for (json, said) in cases {
        let err = chat_within_limits(&json).unwrap_err().to_string();
        assert_eq!(err, format!("This chat file is too big: {said}."));
    }
    let mut most = message.clone();
    most["parts"] = many(MAX_PARTS, serde_json::json!({ "type": "text", "text": "é".repeat(MAX_TEXT) }));
    chat_within_limits(&serde_json::json!([most])).unwrap();
}

#[test]
fn a_twitch_id_loses_its_v_only_before_a_number() {
    // The cases platform.test.mjs gives the page's `twitchNumber`.
    assert_eq!(twitch_number("v2901646294"), "2901646294");
    assert_eq!(twitch_number("2901646294"), "2901646294");
    assert_eq!(twitch_number("vanityClip-AbCd"), "vanityClip-AbCd");
    assert_eq!(watch_url("twitch", "v2901646294", None).as_deref(), Some("https://www.twitch.tv/videos/2901646294"));
    // A Kick link names no time, as the page's don't.
    assert_eq!(
        watch_url("kick", "01a169a1-9aa8-7653-8a9e-390e694cc11a", Some("Airodonack")).as_deref(),
        Some("https://kick.com/airodonack/videos/01a169a1-9aa8-7653-8a9e-390e694cc11a")
    );
}
