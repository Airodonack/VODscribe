//! Importing a VODscription, and asking the file host for one.

use super::*;

#[test]
fn a_files_recorded_at_is_when_the_vod_started_only_if_nothing_better_says() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let started = |id: i64| db.vod(id).unwrap().unwrap().info.started_at;
    // Nothing says: the file does.
    let (vod, _) = db.import_annotations(&file("twitch", "v123"), None).unwrap();
    assert_eq!(vod.info.started_at.as_deref(), Some("2026-10-01T18:00:00Z"));
    // The chat says better, and a file imported again doesn't undo it.
    db.set_chat_start(vod.id, 1790631004.6).unwrap();
    db.import_annotations(&file("twitch", "v123"), None).unwrap();
    assert_eq!(started(vod.id).as_deref(), Some("2026-09-28T21:30:05Z"));
    // The site said when it was added: that stays too, till a chat says.
    let mut info = crate::edits::tests::info("abc");
    info.started_at = Some("2026-10-01T17:59:30Z".into());
    let (site, _) = db.add_vod(&info).unwrap();
    db.import_annotations(&file("youtube", "abc"), None).unwrap();
    assert_eq!(started(site.id).as_deref(), Some("2026-10-01T17:59:30Z"));
    db.set_chat_start(site.id, 1790631005.0).unwrap();
    assert_eq!(started(site.id).as_deref(), Some("2026-09-28T21:30:05Z"));
    // A time that doesn't read as one is left out.
    let mut odd = file("youtube", "xyzxyzxyz12");
    odd.source.recorded_at = Some("yesterday".into());
    let (odd, _) = db.import_annotations(&odd, None).unwrap();
    assert_eq!(odd.info.started_at, None);
}

#[test]
fn an_import_adds_the_vod_transcribed_with_its_marks_cast_and_chat() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let chat = [message(2.0, "hi"), message(4.0, "KEKW")];
    let (vod, created) = db.import_annotations(&file("twitch", "v123"), Some(&chat)).unwrap();
    assert!(created);
    assert_eq!(vod.info.url, "https://www.twitch.tv/videos/123");
    assert_eq!(vod.info.duration, Some(60.0));
    assert_eq!(vod.info.published.as_deref(), Some("2026-10-01"));
    assert_eq!(vod.transcribed, Transcribed::All);
    assert!(!vod.outdated, "an imported transcript isn't from an older model");
    assert_eq!(vod.chat, ChatStatus::Done);
    let lines = db.lines(vod.id).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].speaker.as_deref(), Some("Marlowe"));
    assert_eq!(lines[1].speaker, None);
    assert_eq!(db.words_between(vod.id, 0.0, 60.0).unwrap().len(), 2);
    let marks = db.marks(vod.id).unwrap();
    let kinds: Vec<_> = marks.iter().map(|m| (m.kind.as_str(), m.start, m.text.as_deref())).collect();
    assert_eq!(
        kinds,
        [("chapter", 0.0, Some("Start")), ("highlight", 5.0, Some("Big laugh")), ("chapter", 20.0, Some("Middle"))]
    );
    let h = &marks[1].detail;
    assert_eq!(
        (h.end, h.category.as_deref(), h.note.as_deref()),
        (Some(9.0), Some("funny"), Some("everyone loses it"))
    );
    assert_eq!(h.cast, ["Marlowe", "Ada"], "names trimmed, blanks left out");
    assert!(marks[0].detail.cast.is_empty(), "a chapter has no cast");
    assert_eq!(marks[0].detail.icon.as_deref(), Some("🎮"), "an icon, trimmed");
    assert_eq!(marks[2].detail.icon, None, "an icon that isn't one emoji is left out");
    assert!(marks.iter().all(|m| m.detail.source == SOURCE));
    let extra = db.vod_annotations(vod.id).unwrap().unwrap();
    assert_eq!(extra.cast[0].aliases, ["Mar"]);
    assert_eq!(extra.activity[1].name.as_deref(), Some("Chess"));
    assert_eq!(db.chat(vod.id).unwrap().len(), 2);
    let found = db.search("general", &Default::default(), 10).unwrap();
    assert_eq!(found.hits.len(), 1, "imported lines are searched");
}

#[test]
fn a_cast_members_card_and_the_vods_contexts_are_read_and_kept_through_an_import() {
    let mut json = serde_json::to_value(file("youtube", "abcdefghijk")).unwrap();
    json["cast"] = serde_json::json!([
        { "name": "Marlowe", "role": "streamer", "streamer": " marlowe-01 " },
        { "name": "Ada", "role": "friend", "streamer": "../ada" },
        { "name": "Bo", "role": "friend", "streamer": "Bo" },
        { "name": "Cy", "role": "friend", "streamer": 7 }
    ]);
    json["contexts"] = serde_json::json!(["nopixel", " nopixel", "No Pixel", 3, "gta_rp"]);
    let annotations = from_value(json).expect("a card or context that can't be read isn't fatal");
    let cards: Vec<_> = annotations.cast.iter().map(|m| m.streamer.as_deref()).collect();
    assert_eq!(cards, [Some("marlowe-01"), None, None, None], "trimmed; a path, a capital or a number left out");
    assert_eq!(annotations.contexts, ["nopixel", "gta_rp"], "each once, what isn't an id left out");

    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.import_annotations(&annotations, None).unwrap();
    let kept = db.vod_annotations(vod.id).unwrap().unwrap();
    assert_eq!((kept.cast, kept.contexts), (annotations.cast.clone(), annotations.contexts.clone()));
    // A file with none: none kept, and none said.
    let (plain, _) = db.import_annotations(&file("youtube", "bcdefghijkl"), None).unwrap();
    let kept = db.vod_annotations(plain.id).unwrap().unwrap();
    assert!(kept.contexts.is_empty() && kept.cast.iter().all(|m| m.streamer.is_none()));
    assert!(serde_json::to_value(&kept.cast[0]).unwrap().get("streamer").is_none());
}

#[test]
fn a_streamer_card_is_there_not_there_or_fails() {
    let (base, asked) = host(|line| match line {
        l if l.contains("/streamers/marlowe.json") => ("HTTP/1.1 200 OK".into(), r#"{"id":"marlowe"}"#.into()),
        l if l.contains("/streamers/broken.json") => ("HTTP/1.1 500 Internal Server Error".into(), String::new()),
        l if l.contains("/streamers/garbled.json") => ("HTTP/1.1 200 OK".into(), "{".into()),
        _ => ("HTTP/1.1 404 Not Found".into(), String::new()),
    });
    assert_eq!(streamer_card_at(&base, "marlowe").unwrap(), Some(serde_json::json!({ "id": "marlowe" })));
    assert_eq!(streamer_card_at(&base, "ada").unwrap(), None);
    assert!(streamer_card_at(&base, "broken").is_err());
    assert!(streamer_card_at(&base, "garbled").is_err());
    // An id that can't be one is never asked for.
    assert_eq!(streamer_card_at(&base, "../annotations/filter").unwrap(), None);
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 4, "{asked:?}");
    assert!(asked.iter().all(|l| l.starts_with("GET /streamers/")), "{asked:?}");
}

#[test]
fn a_cast_members_player_and_account_are_read_and_kept_through_an_import() {
    let mut json = serde_json::to_value(file("youtube", "abcdefghijk")).unwrap();
    json["cast"] = serde_json::json!([
        { "name": "Marlowe", "role": "streamer", "player": " Marlowe Plays ",
          "account": { "platform": "Twitch", "login": " marlowe_plays " } },
        { "name": "Ada", "role": "friend", "account": { "platform": "youtube", "login": "@ada.makes" } },
        { "name": "Bo", "role": "friend", "player": 3, "account": { "platform": "vimeo", "login": "bo" } },
        { "name": "Cy", "role": "friend", "player": "", "account": { "platform": "kick", "login": "../cy" } },
        { "name": "Di", "role": "friend", "account": { "platform": "kick", "login": "@di" } },
        { "name": "Ed", "role": "friend", "account": "ed" },
        { "name": "Fi", "role": "friend", "account": { "platform": "twitch", "login": "." } },
        { "name": "Gu", "role": "friend", "account": { "platform": "kick", "login": ".." } },
        { "name": "Hal", "role": "friend", "account": { "platform": "youtube", "login": "@..." } },
        { "name": "Io", "role": "friend", "account": { "platform": "twitch", "login": "i.o" } }
    ]);
    let annotations = from_value(json).expect("a player or account that can't be read isn't fatal");
    let cast = &annotations.cast;
    assert_eq!(cast[0].player.as_deref(), Some("Marlowe Plays"), "trimmed");
    let twitch = Account { platform: "twitch".into(), login: "marlowe_plays".into() };
    assert_eq!(cast[0].account.as_ref(), Some(&twitch), "the platform in lower case, the login trimmed");
    assert_eq!(cast[1].account.as_ref().map(|a| a.login.as_str()), Some("@ada.makes"), "a YouTube handle");
    assert_eq!((cast[2].player.as_deref(), cast[2].account.as_ref()), (None, None), "an unknown platform");
    assert_eq!((cast[3].player.as_deref(), cast[3].account.as_ref()), (None, None), "a login that's a path");
    assert_eq!(cast[4].account, None, "an @ is YouTube's only");
    assert_eq!(cast[5].account, None);
    for (i, login) in [(6, "."), (7, ".."), (8, "@...")] {
        assert_eq!(cast[i].account, None, "a login of only dots ({login}) is a path's");
    }
    assert_eq!(cast[9].account.as_ref().map(|a| a.login.as_str()), Some("i.o"), "a dot in a name is one");

    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.import_annotations(&annotations, None).unwrap();
    let kept = db.vod_annotations(vod.id).unwrap().unwrap().cast;
    assert_eq!(kept, annotations.cast, "the import keeps them as read");
    let stored = serde_json::to_value(&kept).unwrap();
    assert_eq!(
        stored[1],
        serde_json::json!({ "name": "Ada", "aliases": [], "role": "friend",
        "account": { "platform": "youtube", "login": "@ada.makes" } }),
        "and leaves out what's not there"
    );
}

#[test]
fn other_points_of_view_are_read_and_kept_through_an_import() {
    let pov = |player: &str, platform: &str, id: &str, start: &str, duration: f64| {
        serde_json::json!({ "player": player, "account": { "platform": "twitch", "login": "burn" },
                            "video": { "platform": platform, "id": id }, "start": start, "duration": duration })
    };
    let mut json = serde_json::to_value(file("twitch", "v123")).unwrap();
    assert!(from_value(json.clone()).unwrap().povs.is_empty(), "a file may leave them out");
    json["povs"] = serde_json::json!([
        pov(" Burn ", "Twitch", " v2906773722 ", "2026-10-01T17:30:00Z", 7200.0),
        pov("", "twitch", "v1", "2026-10-01T17:30:00Z", 60.0),
        pov("Burn", "vimeo", "1", "2026-10-01T17:30:00Z", 60.0),
        pov("Burn", "twitch", "../v1", "2026-10-01T17:30:00Z", 60.0),
        pov("Burn", "twitch", "v1", "yesterday", 60.0),
        pov("Burn", "twitch", "v1", "2026-10-01T17:30:00Z", -1.0),
        { "player": "Burn", "account": { "platform": "vimeo", "login": "burn" },
          "video": { "platform": "twitch", "id": "v1" }, "start": "2026-10-01T17:30:00Z", "duration": 60.0 },
        "Burn"
    ]);
    let annotations = from_value(json.clone()).expect("a point of view that can't be read isn't fatal");
    let burn = Pov {
        player: "Burn".into(),
        account: Account { platform: "twitch".into(), login: "burn".into() },
        video: Video { platform: "twitch".into(), id: "v2906773722".into() },
        start: "2026-10-01T17:30:00Z".into(),
        duration: 7200.0,
    };
    assert_eq!(annotations.povs, std::slice::from_ref(&burn), "trimmed, the platform in lower case, the rest left out");
    json["povs"] = serde_json::json!("none");
    assert!(from_value(json).unwrap().povs.is_empty(), "a list that isn't one is none");

    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.import_annotations(&annotations, None).unwrap();
    assert_eq!(db.vod_annotations(vod.id).unwrap().unwrap().povs, [burn]);
    // Imported again without them: none any more.
    db.import_annotations(&file("twitch", "v123"), None).unwrap();
    assert!(db.vod_annotations(vod.id).unwrap().unwrap().povs.is_empty());
}

#[test]
fn importing_again_replaces_the_import_and_keeps_the_users_marks() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.import_annotations(&file("youtube", "abcdefghijk"), None).unwrap();
    assert_eq!(vod.chat, ChatStatus::Pending, "with no chat file, the site's chat is fetched as for any VOD");
    let mine = db.add_mark(vod.id, "highlight", 30.0, Some("mine")).unwrap();
    // An imported chapter the user renames is theirs from then on.
    let middle = db.marks(vod.id).unwrap().into_iter().find(|m| m.text.as_deref() == Some("Middle")).unwrap();
    db.set_mark_text(middle.id, Some("My middle")).unwrap();

    let mut next = file("youtube", "abcdefghijk");
    next.chapters = vec![Chapter { start: 0.0, title: "Opening".into(), icon: None }];
    next.highlights.clear();
    next.cast.push(CastMember {
        name: "Ada".into(),
        aliases: vec![],
        role: "chatter".into(),
        player: None,
        account: None,
        streamer: None,
    });
    next.transcript.segments.truncate(1);
    let (again, created) = db.import_annotations(&next, Some(&[message(1.0, "first")])).unwrap();
    assert_eq!((again.id, created), (vod.id, false));
    let texts: Vec<_> = db.marks(vod.id).unwrap().into_iter().map(|m| (m.text.unwrap(), m.detail.source)).collect();
    assert_eq!(
        texts,
        [("Opening".into(), SOURCE.into()), ("My middle".into(), "user".into()), ("mine".into(), "user".into())]
    );
    assert!(db.marks(vod.id).unwrap().iter().any(|m| m.id == mine.id));
    assert_eq!(db.lines(vod.id).unwrap().len(), 1);
    assert_eq!(db.stretches(vod.id).unwrap().len(), 1, "one done stretch, the whole VOD");
    assert_eq!(db.vod_annotations(vod.id).unwrap().unwrap().cast.len(), 2);
    assert_eq!(db.chat(vod.id).unwrap().len(), 1, "the new chat in place of none");
}

#[test]
fn an_imported_mark_the_user_changed_is_not_imported_again() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.import_annotations(&file("youtube", "abcdefghijk"), None).unwrap();
    let find = |text: &str| db.marks(vod.id).unwrap().into_iter().find(|m| m.text.as_deref() == Some(text)).unwrap();
    // "Middle" renamed, "Big laugh" moved, then undone to the state after both (as undo does).
    db.set_mark_text(find("Middle").id, Some("My middle")).unwrap();
    db.move_mark(find("Big laugh").id, 6.0).unwrap();
    let states = db.mark_states(vod.id).unwrap();
    db.restore_marks(vod.id, &states).unwrap();

    db.import_annotations(&file("youtube", "abcdefghijk"), None).unwrap();
    let marks: Vec<_> =
        db.marks(vod.id).unwrap().into_iter().map(|m| (m.start, m.text.unwrap(), m.detail.source)).collect();
    assert_eq!(
        marks,
        [
            (0.0, "Start".into(), SOURCE.into()),
            (6.0, "Big laugh".into(), "user".into()),
            (20.0, "My middle".into(), "user".into()),
        ],
        "the user's changed copies stand in for the import's"
    );
    // Deleted, the import brings the original back.
    db.delete_mark(find("My middle").id).unwrap();
    db.import_annotations(&file("youtube", "abcdefghijk"), None).unwrap();
    assert_eq!(find("Middle").detail.source, SOURCE);
}

#[test]
fn undo_puts_imported_marks_back_as_imported() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.import_annotations(&file("youtube", "abcdefghijk"), None).unwrap();
    let before = db.mark_states(vod.id).unwrap();
    db.add_mark(vod.id, "cut", 3.0, None).unwrap();
    db.restore_marks(vod.id, &before).unwrap();
    assert_eq!(db.mark_states(vod.id).unwrap(), before);
    assert!(before.iter().any(|m| m.detail.cast == ["Marlowe", "Ada"]), "with the highlight's cast");
    // So the next import still replaces them.
    db.import_annotations(&file("youtube", "abcdefghijk"), None).unwrap();
    assert_eq!(db.marks(vod.id).unwrap().len(), 3);
}

#[test]
fn an_import_onto_a_vod_already_there_or_one_that_cannot_be_added() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let err = db.import_annotations(&file("local", "/videos/a.mp4"), None).unwrap_err();
    assert_eq!(err.to_string(), "Add the video file first, then import its annotations.");
    let err = db.import_annotations(&file("vimeo", "1"), None).unwrap_err();
    assert!(err.to_string().contains("not one from vimeo"));
    assert!(db.vods().unwrap().is_empty(), "nothing added");

    let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
    db.request(vod.id, 0.0, None).unwrap();
    let err = db.import_annotations(&file("youtube", "abc"), None).unwrap_err();
    assert!(err.to_string().starts_with("That VOD is being transcribed"));
    assert!(db.marks(vod.id).unwrap().is_empty(), "nothing imported");
    db.clear_stretches(vod.id).unwrap();
    let (imported, created) = db.import_annotations(&file("youtube", "abc"), None).unwrap();
    assert_eq!((imported.id, created), (vod.id, false));
    assert_eq!(imported.info.title, "VOD abc", "the library's title stays");
    assert_eq!(imported.transcribed, Transcribed::All);
}

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// A file host on this computer answering each request with `answer`
/// (status line and headers, and a body) for its request line, as many
/// as come; its address, and the request lines it was sent.
fn host(answer: fn(&str) -> (String, String)) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { return };
            let mut req = [0u8; 4096];
            let n = conn.read(&mut req).unwrap_or(0);
            let line = String::from_utf8_lossy(&req[..n]).lines().next().unwrap_or("").to_string();
            let (head, body) = answer(&line);
            seen.lock().unwrap().push(line.clone());
            let body = if line.starts_with("HEAD") { String::new() } else { body };
            let _ = write!(conn, "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    (format!("http://{addr}"), asked)
}

/// A VOD added by a link to a plain video file is never asked about: its
/// id is the link's file name, which would tell the file host what the
/// user watches, and from where.
#[test]
fn a_plain_video_links_vod_is_never_asked_about() {
    let info = crate::fetch::parse_info(&serde_json::json!({
        "id": "q3-all-hands-recording", "extractor_key": "Generic", "title": "q3-all-hands-recording",
        "webpage_url": "https://files.example.internal/q3-all-hands-recording.mp4"
    }))
    .unwrap();
    assert!(!can_be_published(&info.platform), "{}", info.platform);
    for site in ["twitch", "youtube", "kick"] {
        assert!(can_be_published(site), "{site}");
    }
}

/// The file host holds only Twitch's, YouTube's and Kick's VODs: a VOD
/// from another site is never asked about, as nothing can be there for it.
#[test]
fn only_twitch_youtube_and_kick_vods_are_asked_about() {
    for site in ["dailymotion", "vimeo", "rumble", "generic", crate::local::PLATFORM, ""] {
        assert!(!can_be_published(site), "{site}");
    }
}

/// A file of a million highlights was read, then imported one mark at a
/// time with the library locked: minutes in which the app answered
/// nothing, then a pane and a timeline of a million rows.
#[test]
fn a_file_that_would_hold_the_app_up_is_refused_before_anything_is_imported() {
    let highlight = serde_json::json!({ "start": 1.0, "end": 2.0, "title": "Big laugh", "kind": "funny" });
    let json = file_with(&["highlights"], many(1_000_000, highlight));
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let err = from_value(json).and_then(|a| db.import_annotations(&a, None)).unwrap_err();
    assert_eq!(err.to_string(), "This annotations file is too big: it has more than 5,000 highlights.");
    assert!(db.vods().unwrap().is_empty(), "nothing added");
}

/// A VOD an import adds has an id of its site's shape, and a Kick VOD a
/// channel that is one: the id goes into the path of the requests made
/// for it (Kick's `/api/v1/stream/<id>/playback` on web.kick.com).
#[test]
fn a_vod_an_import_adds_has_its_sites_shape_of_id_and_channel() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let kick = "01a169a1-9aa8-7653-8a9e-390e694cc11a";
    let refused = [
        ("kick", "../../x?", "Marlowe"),
        ("kick", "01a169a1-9aa8-7653-8a9e-390e694cc11a/../x", "Marlowe"),
        ("kick", kick, "../marlowe"),
        ("kick", kick, "marlowe/videos"),
        ("youtube", "abc", "Marlowe"),
        ("youtube", "noa29qsTc4/", "Marlowe"),
        ("twitch", "v12x", "Marlowe"),
        ("twitch", "../123", "Marlowe"),
    ];
    for (platform, id, channel) in refused {
        let mut odd = file(platform, id);
        odd.source.channel = Some(channel.into());
        let err = db.import_annotations(&odd, None).unwrap_err().to_string();
        assert!(err.starts_with("The annotations file's VOD isn't one"), "{platform} {id} {channel}: {err}");
    }
    assert!(db.vods().unwrap().is_empty(), "nothing added");
    for (platform, id) in [("kick", kick), ("youtube", "noa29qsTc4g"), ("twitch", "v2901646294"), ("twitch", "123")] {
        let (vod, created) = db.import_annotations(&file(platform, id), None).unwrap();
        assert!(created, "{platform} {id}");
        assert_eq!(vod.info.platform_id, id);
    }
    // A VOD the library has already is its own, whatever its id.
    let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
    let (imported, created) = db.import_annotations(&file("youtube", "abc"), None).unwrap();
    assert_eq!((imported.id, created), (vod.id, false));
}

#[test]
fn the_file_host_is_followed_to_no_other_host() {
    // A redirect could lead anywhere, the local network included: not followed,
    // the look-up fails, and no second request goes out.
    let (base, asked) = host(|_| {
        ("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/annotations/youtube/moved.json".into(), String::new())
    });
    assert!(look_up_at(&base, "youtube", "moved").is_err());
    assert_eq!(asked.lock().unwrap().len(), 1);
}

#[test]
fn looking_up_says_there_not_there_or_fails() {
    let (base, asked) = host(|line| match line {
        l if l.contains("/annotations/youtube/there.json") => ("HTTP/1.1 200 OK\r\nETag: \"v1\"".into(), String::new()),
        l if l.contains("/dated.json") => {
            ("HTTP/1.1 200 OK\r\nLast-Modified: Sat, 24 Oct 2026 10:00:00 GMT".into(), String::new())
        }
        l if l.contains("/broken.json") => ("HTTP/1.1 500 Internal Server Error".into(), String::new()),
        _ => ("HTTP/1.1 404 Not Found".into(), String::new()),
    });
    assert_eq!(look_up_at(&base, "youtube", "there").unwrap(), Found::Available { version: Some("\"v1\"".into()) });
    let dated = look_up_at(&base, "youtube", "dated").unwrap();
    assert_eq!(dated, Found::Available { version: Some("Sat, 24 Oct 2026 10:00:00 GMT".into()) });
    assert_eq!(look_up_at(&base, "twitch", "v1").unwrap(), Found::None);
    assert!(look_up_at(&base, "youtube", "broken").is_err());
    let asked = asked.lock().unwrap().clone();
    assert!(asked.iter().all(|l| l.starts_with("HEAD ")), "only asked, never fetched: {asked:?}");
    assert_eq!(asked[2], "HEAD /annotations/twitch/v1.json HTTP/1.1");
    // Nothing listening: a failure, which tells nothing.
    let gone = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    assert!(look_up_at(&format!("http://{gone}"), "youtube", "there").is_err());
}

#[test]
fn a_download_is_the_vods_own_file_with_its_version() {
    let (base, _) = host(|line| {
        let platform = if line.contains("/youtube/") { "youtube" } else { "twitch" };
        let json = serde_json::to_string(&file(platform, "abc")).unwrap();
        ("HTTP/1.1 200 OK\r\nETag: \"v2\"".into(), json)
    });
    let (annotations, version) = download_from(&base, "youtube", "abc").unwrap();
    assert_eq!((annotations.source.id.as_str(), version.as_deref()), ("abc", Some("\"v2\"")));
    // A file whose source is another VOD isn't this one's.
    let err = download_from(&base, "youtube", "xyz").unwrap_err();
    assert_eq!(err.to_string(), "The annotations found are of another VOD.");
    let (_, _) = download_from(&base, "twitch", "abc").unwrap();
    let (gone, _) = host(|_| ("HTTP/1.1 404 Not Found".into(), String::new()));
    let err = download_from(&gone, "youtube", "abc").unwrap_err();
    assert_eq!(err.to_string(), "The annotations for this VOD aren't there any more.");
}

#[test]
fn an_answer_for_a_deleted_vod_never_lands_on_the_next_one_given_its_id() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (old, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
    // Deleted with its video while the file host is asked, then another VOD added: a
    // deleted VOD's id is never given again (`vod_ids`), so the answer finds no row.
    db.delete_vod(old.id).unwrap();
    let (new, _) = db.add_vod(&crate::edits::tests::info("xyz")).unwrap();
    assert_ne!(new.id, old.id);
    db.set_annotations_found(old.id, &Found::Available { version: Some("v1".into()) }).unwrap();
    assert_eq!(db.vod(new.id).unwrap().unwrap().annotations, Availability::Unknown);
}

#[test]
fn what_is_found_is_kept_and_an_import_of_it_is_up_to_date_till_it_changes() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
    assert_eq!(vod.annotations, Availability::Unknown);
    let offer = db.annotations_offer(vod.id).unwrap();
    assert_eq!(
        offer,
        Offer { available: Availability::Unknown, up_to_date: false, local_transcript: false, newer: false }
    );

    db.set_annotations_found(vod.id, &Found::None).unwrap();
    assert_eq!(db.vod(vod.id).unwrap().unwrap().annotations, Availability::None);
    let v1 = Found::Available { version: Some("v1".into()) };
    db.set_annotations_found(vod.id, &v1).unwrap();
    assert_eq!(db.vod(vod.id).unwrap().unwrap().annotations, Availability::Available);
    assert!(!db.annotations_offer(vod.id).unwrap().up_to_date, "found, not brought in");

    db.import_annotations(&file("youtube", "abc"), None).unwrap();
    db.set_annotations_imported(vod.id, Some("v1")).unwrap();
    let offer = db.annotations_offer(vod.id).unwrap();
    assert_eq!(
        offer,
        Offer { available: Availability::Available, up_to_date: true, local_transcript: false, newer: false }
    );
    // Asked again: the same file is still up to date; a new one isn't.
    db.set_annotations_found(vod.id, &v1).unwrap();
    assert!(db.annotations_offer(vod.id).unwrap().up_to_date);
    db.set_annotations_found(vod.id, &Found::Available { version: Some("v2".into()) }).unwrap();
    assert!(!db.annotations_offer(vod.id).unwrap().up_to_date);
    // A server that gives no version can't be told up to date.
    db.set_annotations_imported(vod.id, None).unwrap();
    assert!(!db.annotations_offer(vod.id).unwrap().up_to_date);
}

/// The file host gives a file's ETag as `"<md5>"`, or `W/"<md5>"` when it
/// compresses the answer: the same file, whichever way it's given.
#[test]
fn a_version_given_weak_or_in_capitals_is_the_same_file() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.import_annotations(&file("youtube", "abcdefghijk"), None).unwrap();
    db.set_annotations_imported(vod.id, Some("\"0123456789abcdef0123456789abcdef\"")).unwrap();
    let seen = Found::Available { version: Some("W/\"0123456789ABCDEF0123456789ABCDEF\"".into()) };
    db.set_annotations_found(vod.id, &seen).unwrap();
    let offer = db.annotations_offer(vod.id).unwrap();
    assert_eq!(
        offer,
        Offer { available: Availability::Available, up_to_date: true, local_transcript: false, newer: false }
    );
    assert!(!db.vod(vod.id).unwrap().unwrap().annotations_newer);
    // Another file is still newer.
    let other = Found::Available { version: Some("W/\"fedcba9876543210fedcba9876543210\"".into()) };
    db.set_annotations_found(vod.id, &other).unwrap();
    assert!(db.annotations_offer(vod.id).unwrap().newer);
    assert!(db.vod(vod.id).unwrap().unwrap().annotations_newer);
}

#[test]
fn a_transcript_made_here_is_told_from_an_imported_one() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
    db.conn
        .execute("INSERT INTO segments (vod_id, start, end, text, words) VALUES (?1, 0, 1, 'hi', '[]')", [vod.id])
        .unwrap();
    db.insert_done(vod.id, 0.0, None, Some(crate::model::CPU_MODEL.id)).unwrap();
    assert!(db.annotations_offer(vod.id).unwrap().local_transcript);
    db.import_annotations(&file("youtube", "abc"), None).unwrap();
    assert!(!db.annotations_offer(vod.id).unwrap().local_transcript, "replaced by the import's");
}

#[test]
fn transcribed_here_after_the_import_it_no_longer_has_the_latest() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let (vod, _) = db.import_annotations(&file("youtube", "abcdefghijk"), None).unwrap();
    db.set_annotations_imported(vod.id, Some("v1")).unwrap();
    assert!(db.annotations_offer(vod.id).unwrap().up_to_date);
    // Transcribe again (a part of it, or all): the lines there are this computer's now.
    db.insert_done(vod.id, 0.0, Some(30.0), Some(crate::model::CPU_MODEL.id)).unwrap();
    let offer = db.annotations_offer(vod.id).unwrap();
    // Not "the latest already" with Replace anyway and no word of the transcript it replaces.
    assert_eq!(
        offer,
        Offer { available: Availability::Available, up_to_date: false, local_transcript: true, newer: false }
    );
}
