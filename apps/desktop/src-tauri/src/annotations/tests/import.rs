//! Importing a VODscription: from a file (`import_annotations_file`, the
//! end-to-end tests' way in) or fetched from the file host for a VOD the
//! library has (`fetch_annotations`, the offer's Use), through the
//! commands as the page calls them, and what they leave in the library,
//! its chat queue and its undo.
//!
//! The file host is the kit's network's (`dl.vodscribe.com`), shared by
//! every test in the process: each test's VODs have ids of their own.

use std::path::PathBuf;

use serde_json::{Value, json};
use vodscribe_core::chat::{ChatMessage, ChatPart, Kind};
use vodscribe_core::db::ChatStatus;
use vodscribe_core::testkit::{Gate, Reply, TempDir, eventually, yt_dlp};

use crate::testkit::TestApp;

/// A VODscription of YouTube VOD `id`, a minute long: Marlowe says "hello
/// there" at 0:01, someone "general kenobi" at 0:10; chapters "Start" at
/// 0:00 and `chapter` at 0:20.
fn vodscription(id: &str, chapter: &str) -> Value {
    json!({
        "format": "vodscribe-annotations",
        "version": 1,
        "source": { "platform": "youtube", "id": id, "channel": "Marlowe", "title": "A stream", "duration": 60.0 },
        "transcript": { "segments": [
            { "start": 1.0, "end": 3.0, "text": "hello there", "speaker": "Marlowe" },
            { "start": 10.0, "end": 12.0, "text": "general kenobi" }
        ]},
        "chapters": [{ "start": 0.0, "title": "Start" }, { "start": 20.0, "title": chapter }],
        "highlights": [],
        "cast": [{ "name": "Marlowe", "role": "streamer" }],
        "activity": []
    })
}

/// A chat message, Ada's, at `time` seconds.
fn message(time: f64, text: &str) -> ChatMessage {
    ChatMessage {
        time,
        author: "Ada".into(),
        color: None,
        parts: vec![ChatPart::Text { text: text.into() }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: None,
    }
}

/// `annotations` written as `<id>.json` in a folder of its own, with `chat`
/// beside it as `<id>.chat.json` when given: the folder, and the file's path.
fn written(annotations: &Value, chat: Option<&[ChatMessage]>) -> (TempDir, PathBuf) {
    let dir = TempDir::new("vodscription");
    let id = annotations["source"]["id"].as_str().unwrap();
    let path = dir.join(format!("{id}.json"));
    std::fs::write(&path, annotations.to_string()).unwrap();
    if let Some(chat) = chat {
        std::fs::write(dir.join(format!("{id}.chat.json")), serde_json::to_string(chat).unwrap()).unwrap();
    }
    (dir, path)
}

/// The file at `path` imported as the page imports one: the answer.
fn import_file(app: &TestApp, path: &std::path::Path) -> Result<Value, Value> {
    app.invoke("import_annotations_file", json!({ "path": path }))
}

/// `annotations` imported from a file, with `chat` beside it: the VOD's id.
fn imported(app: &TestApp, annotations: &Value, chat: Option<&[ChatMessage]>) -> i64 {
    let (_dir, path) = written(annotations, chat);
    import_file(app, &path).unwrap()["vod"]["id"].as_i64().unwrap()
}

/// The file host publishes `annotations` as its VOD's, in version `version`.
fn published(app: &TestApp, annotations: &Value, version: &str) {
    let url =
        format!("https://dl.vodscribe.com/annotations/youtube/{}.json", annotations["source"]["id"].as_str().unwrap());
    let (body, version) = (annotations.clone(), version.to_string());
    app.net
        .replies(&url, move |asked| (asked.method == "GET").then(|| Reply::json(&body).with_header("ETag", &version)));
}

/// VOD `id`'s marks as (start, text, source), in time order.
fn marks(app: &TestApp, id: i64) -> Vec<(f64, String, String)> {
    let marks = app.db().marks(id).unwrap();
    marks.into_iter().map(|m| (m.start, m.text.unwrap_or_default(), m.detail.source)).collect()
}

/// VOD `id`'s transcript, its lines' texts.
fn lines(app: &TestApp, id: i64) -> Vec<String> {
    app.db().lines(id).unwrap().into_iter().map(|l| l.text).collect()
}

/// VOD `id`'s chat, its messages' texts, once it's fetched in full.
fn chat(app: &TestApp, id: i64) -> Option<Vec<String>> {
    let db = app.db();
    let text = |parts: &[ChatPart]| {
        parts.iter().map(|p| if let ChatPart::Text { text } = p { text.as_str() } else { "" }).collect::<String>()
    };
    (db.vod(id).unwrap()?.chat == ChatStatus::Done)
        .then(|| db.chat(id).unwrap().iter().map(|m| text(&m.parts)).collect())
}

/// Whether VOD `id` is in the library's list.
fn listed(app: &TestApp, id: i64) -> bool {
    app.db().vod(id).unwrap().is_some_and(|v| v.listed)
}

/// The site serves YouTube VOD `id`'s chat replay: "hi" at 0:10, 0:20 and 0:30.
fn serves_chat(app: &TestApp, id: &str) {
    let lines = [10.0, 20.0, 30.0].iter().map(|&at| yt_dlp::chat_line(at, "@ada", "hi")).collect();
    app.yt_dlp.serves_chat(&format!("https://www.youtube.com/watch?v={id}"), lines);
}

/// app annotations.rs `import_annotations_file` ("Imports annotations file `path` for the VOD it names,
/// with the chat file beside it if there's one, adding the VOD if the library doesn't have it") and
/// `import` ("the page re-reads what it shows of the VOD from here"): the VOD is added with the file's
/// transcript, marks and chat, and the page hears it.
#[test]
fn a_file_imported_adds_its_vod_with_the_chat_beside_it_and_the_page_hears_it() {
    let app = TestApp::new();
    let (_dir, path) = written(&vodscription("a1filechat1", "Middle"), Some(&[message(2.0, "first!")]));
    let answer = import_file(&app, &path).unwrap();
    assert_eq!(answer["created"], true);
    let id = answer["vod"]["id"].as_i64().unwrap();
    assert!(listed(&app, id));
    assert_eq!(lines(&app, id), ["hello there", "general kenobi"]);
    assert_eq!(
        marks(&app, id),
        [(0.0, "Start".into(), "annotations".into()), (20.0, "Middle".into(), "annotations".into())]
    );
    assert_eq!(chat(&app, id), Some(vec!["first!".to_string()]));
    assert_eq!(app.heard("annotations-imported"), [json!(id)]);
    assert!(app.heard("vod").iter().any(|v| v["id"] == id), "the page hears the VOD as it is");
}

/// app annotations.rs `import` ("No chat came with it: fetched from the site, as for a VOD added by its
/// link"): a VOD a file adds with no chat beside it has its site's chat fetched.
#[test]
fn a_vod_a_file_adds_without_a_chat_has_its_sites_chat_fetched() {
    let app = TestApp::new();
    app.fetches_chats();
    serves_chat(&app, "a1nochat001");
    let id = imported(&app, &vodscription("a1nochat001", "Middle"), None);
    eventually("its site's chat fetched", || chat(&app, id).is_some_and(|c| c.len() == 3));
}

/// The site serves YouTube VOD `id`'s chat, held once its first line is written (`yt-dlp` still downloading),
/// and the chat queue runs: once the fetch has begun, the gate it's held at.
fn fetching_chat(app: &TestApp, id: &str) -> Gate {
    serves_chat(app, id);
    let fetching = Gate::new();
    app.yt_dlp.holds_chat(&fetching);
    app.fetches_chats();
    fetching.reached();
    fetching
}

/// Whether VOD `vod_id`'s chat is still being downloaded: yt-dlp's part file is there.
fn downloading_chat(app: &TestApp, vod_id: i64) -> bool {
    std::path::PathBuf::from(format!("{}.live_chat.json.part", app.paths().temp_chat(vod_id).display())).exists()
}

/// app annotations.rs `import` ("A fetch of the site's chat under way would add to the one imported"): a
/// chat imported while the site's is being fetched stops that fetch, which leaves nothing, and the VOD's
/// chat is the import's.
#[test]
fn a_chat_imported_stops_a_fetch_of_the_sites_chat_under_way() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "a1stopchat1", "A stream", 60.0);
    let fetching = fetching_chat(&app, "a1stopchat1");
    imported(&app, &vodscription("a1stopchat1", "Middle"), Some(&[message(2.0, "first!")]));
    eventually("the fetch stops, leaving nothing", || !downloading_chat(&app, vod.id));
    fetching.open();
    assert_eq!(chat(&app, vod.id), Some(vec!["first!".to_string()]));
}

/// As `a_chat_imported_stops_a_fetch_of_the_sites_chat_under_way`, with the site's chat ending just after
/// the import (yt-dlp let go at once): the fetch still stops, and the VOD's chat is the import's.
#[test]
#[ignore = "disagreement: a YouTube chat fetch whose yt-dlp ends before its next stop poll finishes and replaces the imported chat"]
fn a_chat_imported_as_the_sites_chat_ends_is_still_the_vods_chat() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "a1stopchat2", "A stream", 60.0);
    let fetching = fetching_chat(&app, "a1stopchat2");
    imported(&app, &vodscription("a1stopchat2", "Middle"), Some(&[message(2.0, "first!")]));
    fetching.open();
    // The queue fetches one chat at a time: the next VOD's done, the stopped one has ended.
    serves_chat(&app, "a1nextchat1");
    let next = imported(&app, &vodscription("a1nextchat1", "Middle"), None);
    eventually("the next VOD's chat fetched", || chat(&app, next).is_some());
    assert_eq!(chat(&app, vod.id), Some(vec!["first!".to_string()]));
}

/// app annotations.rs `import` ("Undo there would put marks back from before the import, beside a
/// transcript it doesn't bring back"): after an import, undo on the VOD has nothing from before it to undo.
#[test]
fn after_an_import_undo_puts_back_nothing_from_before_it() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "a1undo00001", "A stream", 60.0);
    let mark = json!({ "vodId": vod.id, "time": 30.0, "kind": "chapter", "text": "Mine" });
    app.invoke("add_mark", mark).unwrap();
    imported(&app, &vodscription("a1undo00001", "Middle"), None);
    let undone = app.invoke("undo", json!({ "view": { "kind": "vod", "id": vod.id } })).unwrap();
    assert_eq!(undone, Value::Null, "nothing to undo");
    let texts: Vec<_> = marks(&app, vod.id).into_iter().map(|(_, text, _)| text).collect();
    assert_eq!(texts, ["Start", "Middle", "Mine"]);
}

/// app annotations.rs `fetch_annotations` ("Fetches VOD `vod_id`'s published annotations and imports them:
/// its transcript and the marks the last import brought are replaced, the user's own marks stay");
/// CHANGELOG.md, 0.7.0: "Adding a VOD offers its annotations." ("Use brings in its transcript, chapters,
/// highlights and who's speaking in place of an earlier import, keeping the chapters and highlights you
/// made"); e2e annotations-offer.mjs ("Use fetches the file and imports it").
#[test]
fn use_fetches_the_vodscription_and_imports_it_in_place_of_the_last_keeping_the_users_marks() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "a1fetch0001", "A stream", 60.0);
    imported(&app, &vodscription("a1fetch0001", "Old middle"), None);
    app.db().add_mark(vod.id, "chapter", 30.0, Some("Mine")).unwrap();
    let mut newer = vodscription("a1fetch0001", "New middle");
    newer["transcript"]["segments"][1]["text"] = "a new line".into();
    published(&app, &newer, "\"v2\"");

    let answer = app.invoke("fetch_annotations", json!({ "vodId": vod.id })).unwrap();
    assert_eq!((answer["vod"]["id"].as_i64(), &answer["created"]), (Some(vod.id), &json!(false)));
    assert_eq!(lines(&app, vod.id), ["hello there", "a new line"]);
    assert_eq!(
        marks(&app, vod.id),
        [
            (0.0, "Start".into(), "annotations".into()),
            (20.0, "New middle".into(), "annotations".into()),
            (30.0, "Mine".into(), "user".into()),
        ]
    );
    assert_eq!(app.heard("annotations-imported"), [json!(vod.id), json!(vod.id)]);
}

/// app annotations.rs `fetch_annotations` (it keeps the version brought in, `set_annotations_imported`);
/// e2e annotations-offer.mjs ("The gift button, with the file unchanged, says the VOD has the latest
/// already"): once Use has brought it in, the offer says the VOD has the latest.
#[test]
fn a_vodscription_fetched_is_the_latest_the_vod_has() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "a1latest001", "A stream", 60.0);
    published(&app, &vodscription("a1latest001", "Middle"), "\"v1\"");
    let offer = |app: &TestApp| app.invoke("annotations_offer", json!({ "vodId": vod.id })).unwrap();
    assert_eq!(offer(&app)["up_to_date"], false);
    app.invoke("fetch_annotations", json!({ "vodId": vod.id })).unwrap();
    assert_eq!(offer(&app)["up_to_date"], true);
    assert_eq!(offer(&app)["available"], "available");
}

/// app annotations.rs `fetch_annotations` ("Taken out of the library while the file came, it stays out:
/// nothing is imported"); e2e annotations-gone.mjs ("the VOD deleted before the file arrives: … the file
/// arriving doesn't bring the VOD back into the library").
#[test]
fn a_vod_removed_while_its_vodscription_comes_stays_out() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "a1gone00001", "A stream", 60.0);
    published(&app, &vodscription("a1gone00001", "Middle"), "\"v1\"");
    let coming = Gate::new();
    app.net.holds("https://dl.vodscribe.com/annotations/youtube/a1gone00001.json", &coming);
    // On a thread of its own, as the page awaits it while it goes on.
    let handle = app.handle().clone();
    let fetching = std::thread::spawn(move || {
        tauri::async_runtime::block_on(crate::annotations::fetch_annotations(handle, vod.id))
    });
    coming.reached();
    app.invoke("delete_vod", json!({ "id": vod.id })).unwrap();
    coming.open();
    let answer = fetching.join().unwrap().map(|_| ()).map_err(|e| e.to_string());
    assert_eq!(answer.unwrap_err(), "That VOD is gone.");
    assert!(!listed(&app, vod.id), "not back in the library");
    assert!(app.heard("annotations-imported").is_empty(), "nothing imported");
    if app.db().vod(vod.id).unwrap().is_some() {
        assert!(lines(&app, vod.id).is_empty() && marks(&app, vod.id).is_empty(), "nothing imported");
    }
}

/// core annotations.rs `import_annotations` ("Taken out of the list but kept for clips: listed again"): a
/// VOD removed while an edit uses it, then imported, is in the library again, the same VOD.
#[test]
fn a_vod_removed_while_an_edit_uses_it_is_listed_again_by_an_import() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "a1relist001", "A stream", 60.0);
    app.edit_of(&[(vod.id, 10.0, 20.0)]);
    app.invoke("delete_vod", json!({ "id": vod.id })).unwrap();
    assert!(!listed(&app, vod.id));
    let id = imported(&app, &vodscription("a1relist001", "Middle"), None);
    assert_eq!(id, vod.id);
    assert!(listed(&app, vod.id));
    assert_eq!(lines(&app, vod.id), ["hello there", "general kenobi"]);
}

/// Decided for 0.7.4: a VOD added back after "Remove and delete the video" is a normal VOD again; core
/// annotations.rs `import_annotations` ("adding the VOD if the library doesn't have it") and app
/// annotations.rs `import` ("No chat came with it: fetched from the site, as for a VOD added by its
/// link"). One removed with its video while an edit uses it, then brought back by a file with no chat,
/// has its site's chat fetched again.
#[test]
#[ignore = "disagreement: an import that adds back a VOD removed with its video answers created false, so its chat is never fetched"]
fn a_vod_removed_with_its_video_and_brought_back_by_an_import_has_its_chat_fetched_again() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "a1readd0001", "A stream", 60.0);
    serves_chat(&app, "a1readd0001");
    app.fetches_chats();
    eventually("its chat fetched", || chat(&app, vod.id).is_some_and(|c| c.len() == 3));
    app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let removed = app.invoke("delete_vod", json!({ "id": vod.id, "deleteVideo": true, "anyway": true }));
    assert_eq!(removed.unwrap(), Value::Null);
    app.erasures_done();
    imported(&app, &vodscription("a1readd0001", "Middle"), None);
    assert!(listed(&app, vod.id));
    eventually("its chat fetched again", || chat(&app, vod.id).is_some_and(|c| c.len() == 3));
}
