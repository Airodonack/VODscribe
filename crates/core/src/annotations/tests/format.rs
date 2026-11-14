//! Reading a VODscription file: its format and version, its limits, the chat beside it, and the ids and
//! names in it. The same rules are the page's (apps/desktop/src/lib/annotations.test.mjs).

use super::*;
use crate::TempDir;
use crate::chat::{ChatPart, Kind};

/// A file at `path` holding `text`.
fn written(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

/// A VODscription file of `file("twitch", "v123")` in a folder of its own, as `v123.json`.
fn v123() -> (TempDir, PathBuf) {
    let dir = TempDir::new("annotations-format");
    let path = dir.join("v123.json");
    written(&path, &serde_json::to_string(&file("twitch", "v123")).unwrap());
    (dir, path)
}

/// A file of `file("twitch", "v123")` with `path` set to `value`, read as the app reads it.
fn read_with(path: &[&str], value: serde_json::Value) -> Annotations {
    from_value(file_with(path, value)).unwrap()
}

/// The user's message for a file the app won't read.
fn refused(json: serde_json::Value) -> String {
    from_value(json).unwrap_err().to_string()
}

/// A file's chat is a list of messages; one of each kind, the shapes the app writes.
fn chat_of_every_kind() -> Vec<ChatMessage> {
    let mut hi = message(2.5, "hi ");
    hi.color = Some("#ff0000".into());
    hi.parts.push(ChatPart::Emote { name: "KEKW".into(), url: "https://x/1".into() });
    let mut sub = message(30.0, "thanks for the sub");
    (sub.kind, sub.amount) = (Kind::Sub, Some(1.0));
    let mut gift = message(31.0, "");
    (gift.kind, gift.amount) = (Kind::Gift, Some(5.0));
    let mut tip = message(90.0, "take my bits");
    (tip.kind, tip.amount, tip.event_time) = (Kind::Tip, Some(100.0), Some(12.5));
    vec![hi, sub, gift, tip]
}

/// `annotations.rs` `parse` ("checking its format and version first; the error's message is for the user");
/// the page's `ANNOTATIONS_FORMAT` (annotations.ts).
#[test]
fn a_file_of_another_format_is_refused_saying_so() {
    let err = parse("{\"format\": \"other\"}").unwrap_err().to_string();
    assert_eq!(err, "This isn't a VODscribe annotations file: its format is \"other\".");
    let err = refused(serde_json::json!({ "format": 7, "version": 1 }));
    assert_eq!(err, "This isn't a VODscribe annotations file: its format is 7.");
    // The format is checked before the version: a file of another format says that first.
    let err = refused(serde_json::json!({ "format": "other", "version": 2 }));
    assert!(err.ends_with("its format is \"other\"."), "{err}");
}

/// annotations.ts `ANNOTATIONS_FORMAT` ("`version` counts breaking changes to the shape"); `annotations.rs`
/// `FORMAT` and `VERSION`.
#[test]
fn a_file_of_another_version_is_refused_saying_which() {
    let err = parse(&format!("{{\"format\": \"{FORMAT}\", \"version\": 2}}")).unwrap_err().to_string();
    assert_eq!(err, "This isn't a VODscribe annotations file: version 2 isn't 1.");
    let err = refused(serde_json::json!({ "format": FORMAT, "version": "1" }));
    assert_eq!(err, "This isn't a VODscribe annotations file: version \"1\" isn't 1.");
    let err = refused(serde_json::json!({ "format": FORMAT, "version": 0 }));
    assert_eq!(err, "This isn't a VODscribe annotations file: version 0 isn't 1.");
}

/// `annotations.rs` `parse` ("it isn't JSON").
#[test]
fn a_file_that_is_not_json_is_refused_saying_so() {
    for text in ["not json", "", "{\"format\": ", "<html></html>"] {
        let err = parse(text).unwrap_err().to_string();
        assert_eq!(err, "This isn't a VODscribe annotations file: it isn't JSON.", "{text:?}");
    }
}

/// `annotations.rs`, the comment on the `MAX_` limits ("well past what the longest VOD's annotations have,
/// short of what would hold the app up") and `within_limits` ("saying what for the user"). A list or a text
/// one past its limit is refused; at its limit it is read.
#[test]
fn a_file_with_more_than_the_app_takes_is_refused_with_a_message() {
    use serde_json::json;
    let lists = [
        (
            &["transcript", "segments"][..],
            MAX_SEGMENTS,
            json!({ "start": 1.0, "end": 2.0, "text": "hi" }),
            "it has more than 200,000 lines",
        ),
        (
            &["transcript", "segments", "0", "words"],
            MAX_WORDS,
            json!({ "text": "hi", "start": 1.0, "end": 1.5 }),
            "a line has more than 2,000 words",
        ),
        (&["chapters"], MAX_CHAPTERS, json!({ "start": 0.0, "title": "A" }), "it has more than 2,000 chapters"),
        (
            &["highlights"],
            MAX_HIGHLIGHTS,
            json!({ "start": 0.0, "end": 1.0, "title": "A", "kind": "funny" }),
            "it has more than 5,000 highlights",
        ),
        (&["highlights", "0", "cast"], MAX_NAMES, json!("A"), "a highlight names more than 100 people"),
        (&["cast"], MAX_CAST, json!({ "name": "A", "role": "friend" }), "it has more than 1,000 people in its cast"),
        (&["cast", "0", "aliases"], MAX_NAMES, json!("A"), "a cast member has more than 100 other names"),
        (
            &["activity"],
            MAX_ACTIVITY,
            json!({ "start": 0.0, "end": 1.0, "kind": "game" }),
            "it has more than 10,000 stretches of activity",
        ),
        (&["povs"], MAX_POVS, json!(null), "it has more than 1,000 other points of view"),
        (&["contexts"], MAX_CONTEXTS, json!("a"), "it has more than 100 contexts"),
    ];
    for (path, max, item, said) in lists {
        let err = refused(file_with(path, many(max + 1, item.clone())));
        assert_eq!(err, format!("This annotations file is too big: {said}."), "{path:?}");
        from_value(file_with(path, many(max, item))).unwrap_or_else(|e| panic!("{path:?} at its limit: {e}"));
    }
    let said = "This annotations file is too big: a text in it is longer than 10,000 characters.";
    // A text counts characters, not bytes; a key is a text too.
    for path in [&["chapters", "0", "title"][..], &["produced", "build"]] {
        assert_eq!(refused(file_with(path, "é".repeat(MAX_TEXT + 1).into())), said, "{path:?}");
        from_value(file_with(path, "é".repeat(MAX_TEXT).into()))
            .unwrap_or_else(|e| panic!("{path:?} at its limit: {e}"));
    }
    let key = |n: usize| json!({ "é".repeat(n): 1 });
    assert_eq!(refused(file_with(&["produced"], key(MAX_TEXT + 1))), said, "a key");
    from_value(file_with(&["produced"], key(MAX_TEXT))).expect("a key at its limit");
}

/// `annotations.rs` `chat_within_limits` ("saying what for the user"). A chat file's messages, a message's
/// parts and any text are limited, as a VODscription's are; at the limit it is read.
#[test]
fn a_chat_file_with_more_than_the_app_takes_is_refused_with_a_message() {
    let message = serde_json::json!({ "time": 1.0, "author": "Ada", "parts": [{ "type": "text", "text": "hi" }] });
    let part = serde_json::json!({ "type": "text", "text": "hi" });
    let with_parts = |n: usize| {
        let mut m = message.clone();
        m["parts"] = many(n, part.clone());
        serde_json::json!([m])
    };
    let with_author = |n: usize| {
        let mut m = message.clone();
        m["author"] = "é".repeat(n).into();
        serde_json::json!([m])
    };
    let cases = [
        (many(MAX_MESSAGES + 1, serde_json::Value::Null), "it has more than 2,000,000 messages"),
        (with_parts(MAX_PARTS + 1), "a message has more than 1,000 parts"),
        (with_author(MAX_TEXT + 1), "a text in it is longer than 10,000 characters"),
    ];
    for (json, said) in cases {
        let err = chat_within_limits(&json).unwrap_err().to_string();
        assert_eq!(err, format!("This chat file is too big: {said}."));
    }
    for json in [many(MAX_MESSAGES, serde_json::Value::Null), with_parts(MAX_PARTS), with_author(MAX_TEXT)] {
        chat_within_limits(&json).unwrap();
    }
    // Read from its file, the same message reaches the user.
    let (dir, path) = v123();
    written(&dir.join("v123.chat.json"), &with_author(MAX_TEXT + 1).to_string());
    let err = read(&path, None).unwrap_err().to_string();
    assert_eq!(err, "This chat file is too big: a text in it is longer than 10,000 characters.");
}

/// `annotations.rs` `chat_path` ("`x.chat.json` beside `x.json`"); annotations.ts's header comment ("The chat
/// comes as a second file beside it").
#[test]
fn the_chat_is_read_from_beside_the_file_as_name_dot_chat_dot_json() {
    let (dir, path) = v123();
    assert_eq!(chat_path(&path), Some(dir.join("v123.chat.json")));
    assert_eq!(chat_path(Path::new("v123.json")), Some(PathBuf::from("v123.chat.json")));
    assert_eq!(chat_path(Path::new("v123.chat.json")), None, "a chat file has no chat of its own");
    assert_eq!(chat_path(Path::new("v123.txt")), None);
    assert_eq!(read(&path, None).unwrap().1, None, "no chat beside it");
    let chat = r##"[{"id": 1, "time": 2.5, "author": "Ada", "color": "#ff0000",
                    "parts": [{"type": "text", "text": "hi "}, {"type": "emote", "name": "KEKW", "url": "https://x/1"}],
                    "kind": "message", "amount": null}]"##;
    written(&dir.join("v123.chat.json"), chat);
    let (annotations, chat) = read(&path, None).unwrap();
    assert_eq!(annotations.source.id, "v123");
    let chat = chat.unwrap();
    assert_eq!((chat[0].time, chat[0].author.as_str(), chat[0].parts.len()), (2.5, "Ada", 2));
    // The chat can be named; then the one beside the file isn't read.
    let other = dir.join("other.json");
    written(&other, "[]");
    assert_eq!(read(&path, Some(&other)).unwrap().1, Some(Vec::new()));
    // The chat file is not an annotations file, and says what to pick.
    let err = read(&dir.join("v123.chat.json"), None).unwrap_err().to_string();
    assert_eq!(err, "That's the chat that goes with an annotations file: pick the annotations file beside it.");
    // A chat file that isn't one is refused saying so.
    written(&other, "{\"not\": \"a chat\"}");
    let err = read(&path, Some(&other)).unwrap_err().to_string();
    assert!(err.ends_with("isn't a chat VODscribe can read"), "{err}");
}

/// `annotations.rs`'s module doc ("its chat in a second file in the app's own chat shape"); annotations.ts's
/// header comment. A message of each kind, written as the app writes a chat, is read back whole.
#[test]
fn a_chat_file_holds_messages_as_the_app_writes_them() {
    let (dir, path) = v123();
    let chat = chat_of_every_kind();
    written(&dir.join("v123.chat.json"), &serde_json::to_string(&chat).unwrap());
    assert_eq!(read(&path, None).unwrap().1, Some(chat));
}

/// Decided for 0.7.4: a VODscription file with no format or version says so plainly, in the app
/// and the page alike ("it has no format", "it has no version"). `annotations.rs` `from_value`;
/// the page's `parseAnnotations` (annotations.ts) says the same.
#[test]
fn a_file_with_no_format_or_version_says_it_has_none() {
    let err = parse("{\"version\": 1}").unwrap_err().to_string();
    assert_eq!(err, "This isn't a VODscribe annotations file: it has no format.");
    let err = parse(&format!("{{\"format\": \"{FORMAT}\"}}")).unwrap_err().to_string();
    assert_eq!(err, "This isn't a VODscribe annotations file: it has no version.");
}

/// `annotations.rs` `one_emoji` ("one emoji (a single character as the reader sees it, of no letter or digit
/// and not plain ASCII), trimmed") and its use in `import_marks`; a chapter's icon that is not one is left
/// out by the import, not the file refused (the page's `oneEmoji` is the same rule).
#[test]
fn a_chapter_icon_is_one_emoji_or_none() {
    for icon in ["🎮", " 🍳 ", "👩‍🍳", "🏳️‍🌈", "❤️", "🇫🇷"] {
        assert_eq!(one_emoji(icon), Some(icon.trim()), "{icon}");
    }
    for icon in ["", " ", "🎮🎮", "a", "🎮 game", "Game", "7", "?", ":)", "日"] {
        assert_eq!(one_emoji(icon), None, "{icon}");
    }
}

/// `annotations.rs` `card_id` ("lowercase letters, digits, `_` and `-`, a letter or digit first, at most 64;
/// trimmed"; it names a file on the file host, so nothing else gets into the path).
#[test]
fn a_card_id_is_lowercase_letters_digits_dashes_and_underscores_a_letter_or_digit_first_at_most_64() {
    let longest = "a".repeat(64);
    for good in ["marlowe", "s-0001", "a_b", "7", "0-", "z_", &longest] {
        assert_eq!(card_id(good), Some(good), "{good}");
    }
    assert_eq!(card_id("  marlowe "), Some("marlowe"));
    assert_eq!(card_id("\tmarlowe\n"), Some("marlowe"));
    for bad in
        ["", " ", "-a", "_a", "Marlowe", "mARlowe", "a/b", "a\\b", "..", "a.b", "a b", "é", "a%2fb", &"a".repeat(65)]
    {
        assert_eq!(card_id(bad), None, "{bad:?}");
    }
    // The same rule names a cast member's card and a VOD's contexts in a file.
    let cast = serde_json::json!([{ "name": "A", "role": "friend", "streamer": " marlowe " },
                                  { "name": "B", "role": "friend", "streamer": "../marlowe" }]);
    let cast = read_with(&["cast"], cast).cast;
    assert_eq!((cast[0].streamer.as_deref(), cast[1].streamer.as_deref()), (Some("marlowe"), None));
}

/// `annotations.rs` `Account::read` ("a listed platform (any case) and a login of letters, digits and `_.-`,
/// not only dots …, a YouTube handle with its "@" first; trimmed; None for anything else").
#[test]
fn an_account_is_a_listed_site_and_a_login_else_it_is_left_out() {
    let read =
        |platform: &str, login: &str| Account::read(&serde_json::json!({ "platform": platform, "login": login }));
    let account = |platform: &str, login: &str| Some(Account { platform: platform.into(), login: login.into() });
    assert_eq!(read("twitch", "marlowe_live"), account("twitch", "marlowe_live"));
    assert_eq!(read("kick", "a.b-c_d"), account("kick", "a.b-c_d"), "a dot in a name is one");
    assert_eq!(read(" Twitch ", " Marlowe "), account("twitch", "Marlowe"), "the site in lower case, both trimmed");
    assert_eq!(read("YOUTUBE", "@marlowe.makes"), account("youtube", "@marlowe.makes"), "a handle keeps its @");
    assert_eq!(read("youtube", "UC_x5XG1OV2P6uZZ5FSM9Ttw"), account("youtube", "UC_x5XG1OV2P6uZZ5FSM9Ttw"));
    for (platform, login, why) in [
        ("vimeo", "marlowe", "a site not listed"),
        ("", "marlowe", "no site"),
        ("twitch", "", "no login"),
        ("twitch", "  ", "a blank login"),
        ("twitch", ".", "a login of only dots is a path's"),
        ("kick", "..", "a login of only dots is a path's"),
        ("youtube", "@...", "a handle of only dots is a path's"),
        ("youtube", "@", "an @ and nothing"),
        ("twitch", "@marlowe", "an @ is YouTube's only"),
        ("twitch", "../marlowe", "a path"),
        ("twitch", "mar/lowe", "a slash"),
        ("twitch", "mar lowe", "a space inside"),
        ("twitch", "marlowé", "a letter outside ASCII"),
        ("youtube", "@@marlowe", "two @"),
    ] {
        assert_eq!(read(platform, login), None, "{why}");
    }
    for value in [
        serde_json::json!("marlowe"),
        serde_json::json!(null),
        serde_json::json!({ "platform": "twitch" }),
        serde_json::json!({ "login": "marlowe" }),
        serde_json::json!({ "platform": 1, "login": "x" }),
        serde_json::json!({ "platform": "twitch", "login": 1 }),
    ] {
        assert_eq!(Account::read(&value), None, "{value}");
    }
    // In a file, an account that can't be read is left out and the cast member stays.
    let cast = serde_json::json!([{ "name": "A", "role": "friend", "account": { "platform": "vimeo", "login": "a" } },
                                  { "name": "B", "role": "friend", "account": { "platform": "kick", "login": "b" } }]);
    let cast = read_with(&["cast"], cast).cast;
    assert_eq!((cast[0].account.as_ref(), cast[1].account.as_ref().map(|a| a.login.as_str())), (None, Some("b")));
}

/// `annotations.rs` `Pov::read` ("a player's name, an account `Account::read` takes, a video on a listed
/// platform with an id of letters, digits and `_-`, a start that reads as a time and a length that is one;
/// trimmed; None for anything else").
#[test]
fn a_point_of_view_is_read_only_whole_else_it_is_left_out() {
    let pov = |player: &str, login: &str, platform: &str, id: &str, start: &str, duration: serde_json::Value| {
        serde_json::json!({ "player": player, "account": { "platform": "twitch", "login": login },
                            "video": { "platform": platform, "id": id }, "start": start, "duration": duration })
    };
    let start = "2026-10-01T17:30:00Z";
    let whole = |value: serde_json::Value| Pov::read(&value);
    assert_eq!(
        whole(pov(" Burn ", " burn ", "Twitch", " v2906773722 ", " 2026-10-01T17:30:00.5+02:00 ", 7200.5.into())),
        Some(Pov {
            player: "Burn".into(),
            account: Account { platform: "twitch".into(), login: "burn".into() },
            video: Video { platform: "twitch".into(), id: "v2906773722".into() },
            start: "2026-10-01T17:30:00.5+02:00".into(),
            duration: 7200.5,
        }),
        "trimmed, the video's site in lower case"
    );
    let bad = [
        ("a blank player", pov(" ", "burn", "twitch", "v1", start, 60.into())),
        ("an account that can't be read", pov("Burn", "../burn", "twitch", "v1", start, 60.into())),
        ("a video's site not listed", pov("Burn", "burn", "vimeo", "1", start, 60.into())),
        ("an id that is a path", pov("Burn", "burn", "twitch", "../v1", start, 60.into())),
        ("an id with a dot", pov("Burn", "burn", "twitch", "v.1", start, 60.into())),
        ("no id", pov("Burn", "burn", "twitch", "", start, 60.into())),
        ("a start that is no time", pov("Burn", "burn", "twitch", "v1", "yesterday", 60.into())),
        ("a start with no zone", pov("Burn", "burn", "twitch", "v1", "2026-10-01T17:30:00", 60.into())),
        ("a length of none", pov("Burn", "burn", "twitch", "v1", start, 0.into())),
        ("a negative length", pov("Burn", "burn", "twitch", "v1", start, (-1).into())),
        ("a length that is no number", pov("Burn", "burn", "twitch", "v1", start, "60".into())),
        ("a text", serde_json::json!("Burn")),
        ("nothing", serde_json::Value::Null),
    ];
    for (why, value) in bad {
        assert_eq!(whole(value), None, "{why}");
    }
    for key in ["player", "account", "video", "start", "duration"] {
        let mut value = pov("Burn", "burn", "twitch", "v1", start, 60.into());
        value.as_object_mut().unwrap().remove(key);
        assert_eq!(whole(value), None, "without {key}");
    }
    // In a file, the ones that can't be read are left out and the file is read.
    let povs = serde_json::json!([
        pov("Burn", "burn", "twitch", "v1", start, 60.into()),
        pov("", "burn", "twitch", "v1", start, 60.into()),
        "Burn"
    ]);
    assert_eq!(read_with(&["povs"], povs).povs.len(), 1);
}

/// `annotations.rs` `CastMember::player` ("one that isn't a name is left out rather than the file refused").
#[test]
fn a_player_that_is_not_a_name_is_left_out_rather_than_the_file_refused() {
    let member = |player: serde_json::Value| serde_json::json!({ "name": "A", "role": "friend", "player": player });
    let cast = serde_json::json!([member(" Marlowe Plays ".into()), member("".into()), member("  ".into()),
                                  member(3.into()), member(serde_json::Value::Null),
                                  { "name": "B", "role": "friend" }]);
    let cast = read_with(&["cast"], cast).cast;
    let players: Vec<_> = cast.iter().map(|m| m.player.as_deref()).collect();
    assert_eq!(players, [Some("Marlowe Plays"), None, None, None, None, None]);
    assert_eq!(cast.len(), 6, "every cast member is kept");
}

/// `annotations.rs` `Annotations::contexts` ("one `context_id` can't take is left out").
#[test]
fn a_context_that_is_not_an_id_is_left_out() {
    let contexts = serde_json::json!(["nopixel", " gta-rp ", "NoPixel", "../x", "", 7, null, "a b"]);
    assert_eq!(read_with(&["contexts"], contexts).contexts, ["nopixel", "gta-rp"]);
    assert!(read_with(&["contexts"], "nopixel".into()).contexts.is_empty(), "a list that isn't one is none");
}

/// `annotations.rs` `Annotations::povs` and `contexts` ("Optional"): a file that leaves out `povs` and
/// `contexts` is read, with none.
#[test]
fn a_file_without_povs_or_contexts_reads_with_none() {
    let mut json = serde_json::to_value(file("twitch", "v123")).unwrap();
    let object = json.as_object_mut().unwrap();
    object.remove("povs");
    object.remove("contexts");
    let annotations = from_value(json).unwrap();
    assert!(annotations.povs.is_empty() && annotations.contexts.is_empty());
    // As written back, they aren't there either.
    let written = serde_json::to_value(&annotations).unwrap();
    assert!(written.get("povs").is_none() && written.get("contexts").is_none(), "{written}");
}

/// `annotations.rs`'s module doc ("`<BASE_URL>/annotations/<platform>/<platform id>.json`") and `file_url`
/// ("each part with anything but letters, digits and `-._~` escaped, so an id can't reach another path").
#[test]
fn a_vods_published_file_is_at_its_site_and_id_escaped() {
    assert_eq!(
        file_url(BASE_URL, "twitch", "v2901646294"),
        "https://dl.vodscribe.com/annotations/twitch/v2901646294.json"
    );
    assert_eq!(file_url("http://h", "youtube", "a/../b?c"), "http://h/annotations/youtube/a%2F..%2Fb%3Fc.json");
    assert_eq!(file_url("http://h", "youtube", "A-z_0.9~"), "http://h/annotations/youtube/A-z_0.9~.json");
    assert_eq!(file_url("http://h", "kick", "é %"), "http://h/annotations/kick/%C3%A9%20%25.json");
    assert_eq!(file_url("http://h", "a/b", "x"), "http://h/annotations/a%2Fb/x.json", "the site is escaped too");
}

/// `annotations.rs`'s module doc ("`<BASE_URL>/streamers/<id>.json`, with an index of their accounts at
/// `<BASE_URL>/streamers.json`") and `card_url`, `index_url`.
#[test]
fn a_streamer_card_is_at_its_id_and_the_index_beside_them() {
    assert_eq!(card_url("http://h", "marlowe").as_deref(), Some("http://h/streamers/marlowe.json"));
    assert_eq!(card_url(BASE_URL, " marlowe ").as_deref(), Some("https://dl.vodscribe.com/streamers/marlowe.json"));
    for bad in ["../x", "Marlowe", "", "a/b"] {
        assert_eq!(card_url("http://h", bad), None, "{bad:?}");
    }
    assert_eq!(index_url(BASE_URL), "https://dl.vodscribe.com/streamers.json");
    assert_eq!(index_url("http://h"), "http://h/streamers.json");
}

/// `annotations.rs` `twitch_number` ("The page's `twitchNumber` (platform.ts) is the same rule, tested with
/// the same cases"); a Kick link names no time, as the page's don't.
#[test]
fn a_twitch_id_loses_its_v_only_before_a_number() {
    // The cases platform.test.mjs gives the page's `twitchNumber`.
    assert_eq!(twitch_number("v2901646294"), "2901646294");
    assert_eq!(twitch_number("2901646294"), "2901646294");
    assert_eq!(twitch_number("vanityClip-AbCd"), "vanityClip-AbCd");
    assert_eq!(watch_url("twitch", "v2901646294", None).as_deref(), Some("https://www.twitch.tv/videos/2901646294"));
    assert_eq!(
        watch_url("kick", "01a169a1-9aa8-7653-8a9e-390e694cc11a", Some("Airodonack")).as_deref(),
        Some("https://kick.com/airodonack/videos/01a169a1-9aa8-7653-8a9e-390e694cc11a")
    );
}

/// tests/e2e/fixtures/annotations-minute.json and .chat.json: the file the end-to-end scenarios import is a
/// file the format takes, with its chat beside it.
#[test]
fn the_e2e_fixture_reads_as_a_vodscription() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/fixtures");
    let (annotations, chat) = read(&fixtures.join("annotations-minute.json"), None).unwrap();
    assert!(!annotations.transcript.segments.is_empty() && !annotations.cast.is_empty());
    assert!(!chat.expect("its chat is beside it").is_empty());
}
