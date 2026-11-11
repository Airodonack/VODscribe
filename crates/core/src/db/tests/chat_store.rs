//! Chat kept in the library.

use super::*;

#[test]
fn a_chat_fetched_again_is_cleared_in_parts_and_resubs_demoted_at_once() {
    use crate::chat::ChatStore;
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let a = db.add_vod(&info("a")).unwrap().0.id;
    let b = db.add_vod(&info("b")).unwrap().0.id;
    let many: Vec<_> = (0..2500).map(|i| message(i as f64, "v", "hi")).collect();
    db.add_chat(a, &many, &[2500.0]).unwrap();
    let mut subs = message(1.0, "s", "resub");
    subs.kind = Kind::Sub;
    db.add_chat(b, &[subs.clone(), subs.clone(), subs], &[2.0]).unwrap();
    let lock = std::sync::Mutex::new(db);
    let mut store = LibraryChat { db: &lock, vod_id: b, on_save: Box::new(|| {}), starts: Vec::new() };
    let ids: Vec<i64> = store.of_kind(Kind::Sub).unwrap().into_iter().map(|(id, _)| id).collect();
    assert_eq!(ids.len(), 3);
    store.demote(&ids[..2]).unwrap();
    assert_eq!(store.of_kind(Kind::Sub).unwrap().len(), 1);
    let mut store = LibraryChat { db: &lock, vod_id: a, on_save: Box::new(|| {}), starts: Vec::new() };
    store.clear().unwrap();
    let db = lock.lock().unwrap();
    assert_eq!(db.chat_count(a).unwrap(), 0);
    assert_eq!(db.chat_slices(a).unwrap(), None);
    assert_eq!(db.chat_count(b).unwrap(), 3, "another VOD's chat stays");
}

#[test]
fn stores_and_searches_chat() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let a = db.add_vod(&info("a")).unwrap().0.id;
    let b = db.add_vod(&info("b")).unwrap().0.id;
    assert_eq!(db.vod(a).unwrap().unwrap().chat, ChatStatus::Pending);
    assert_eq!(db.chat_pending().unwrap(), [b, a], "library order");

    let mut seg = Segmenter::new();
    let mut segs = seg.push(words("what a clutch play", 20.0));
    segs.extend(seg.finish());
    db.insert_segments(a, &segs).unwrap();
    let emote = ChatPart::Emote { name: "barkylove".into(), url: "https://files.kick.com/emotes/1/fullsize".into() };
    let chat = [
        message(25.0, "viewer", "CLUTCH PLAY!!"),
        ChatMessage { parts: vec![emote.clone()], ..message(5.0, "fan", "") },
    ];
    db.set_chat(a, &chat).unwrap();
    db.set_chat(b, &[message(1.0, "other", "clutch play elsewhere")]).unwrap();
    assert_eq!(db.vod(a).unwrap().unwrap().chat, ChatStatus::Done);
    assert!(db.chat_pending().unwrap().is_empty());

    let lines = db.chat(a).unwrap();
    assert_eq!(lines.iter().map(|l| l.time).collect::<Vec<_>>(), [5.0, 25.0], "oldest first");

    // What the transcript view asks for: rows' windows, counts per stretch, events.
    db.add_chat(a, &[message(15.0, "late", "arrived later"), message(16.0, "later", "and again")], &[0.0]).unwrap();
    let windows = db.chat_windows(a, &[(-1e12, 10.0), (10.0, 20.0), (20.0, 1e12)], 1).unwrap();
    let shown: Vec<_> =
        windows.iter().map(|w| (w.messages.iter().map(|m| m.time).collect::<Vec<_>>(), w.total)).collect();
    assert_eq!(shown, [(vec![5.0], 1), (vec![15.0], 2), (vec![25.0], 1)], "the first of each, and how many");
    assert_eq!(db.chat_histogram(a, 30.0, 3).unwrap(), [1, 2, 1]);
    assert_eq!(db.chat_count(a).unwrap(), 4);
    assert_eq!(
        db.chat_emotes(a).unwrap(),
        [("barkylove".to_string(), "https://files.kick.com/emotes/1/fullsize".to_string())]
    );
    db.set_chat(a, &chat).unwrap();
    let lines = db.chat(a).unwrap();
    assert_eq!(lines[1].kind, Kind::Message);
    assert_eq!(lines[0].parts, [emote]);

    let search = |filter: SearchFilter| {
        let hits = db.search("clutch play", &filter, 10).unwrap().hits;
        hits.into_iter().map(|h| (h.vod_id, h.source, h.time)).collect::<Vec<_>>()
    };
    assert_eq!(
        search(SearchFilter::default()),
        [(b, Source::Chat, 1.0), (a, Source::Speech, 22.0), (a, Source::Chat, 25.0)],
        "library order (newest on top), then time"
    );
    assert_eq!(search(SearchFilter { chat: false, ..Default::default() }), [(a, Source::Speech, 22.0)]);
    assert_eq!(
        search(SearchFilter { speech: false, vod_id: Some(a), ..Default::default() }),
        [(a, Source::Chat, 25.0)]
    );
    assert!(search(SearchFilter { speech: false, chat: false, ..Default::default() }).is_empty());
    let hit = db.search("barkylove", &SearchFilter::default(), 10).unwrap().hits.remove(0);
    assert_eq!((hit.author.as_deref(), hit.color.as_deref()), (Some("fan"), Some("#ff0000")));
    let parts: Vec<_> =
        db.search("clutch", &SearchFilter { speech: false, vod_id: Some(a), ..Default::default() }, 10).unwrap().hits
            [0]
        .parts
        .iter()
        .map(|p| (p.text.clone(), p.hit))
        .collect();
    assert_eq!(parts, [("CLUTCH".to_string(), true), (" PLAY!!".to_string(), false)]);

    // Layers match whole words and phrases, in any of their terms.
    let times = |terms: &[&str]| {
        let terms: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
        db.chat_matches(a, &terms).unwrap().into_iter().map(|(_, time)| time).collect::<Vec<_>>()
    };
    assert_eq!(times(&["clutch play"]), [25.0]);
    assert_eq!(times(&["BARKYLOVE", "nothing"]), [5.0], "an emote by name, any case");
    assert_eq!(times(&["clut"]), Vec::<f64>::new(), "whole words, not prefixes");
    assert_eq!(times(&["play", "barkylove"]), [5.0, 25.0]);
    assert_eq!(times(&["\" OR *", ""]), Vec::<f64>::new(), "no FTS syntax injection");

    // Fetching again replaces the chat; a failure is retried; deleting the VOD clears it.
    db.set_chat(a, &[message(30.0, "viewer", "gg")]).unwrap();
    assert_eq!(db.chat(a).unwrap().len(), 1);
    db.set_chat_status(b, ChatStatus::Failed, Some("Kick isn't responding")).unwrap();
    assert_eq!(db.chat_pending().unwrap(), [b]);
    assert_eq!(db.vod(b).unwrap().unwrap().chat_error.as_deref(), Some("Kick isn't responding"), "said why");
    while db.delete_some_of(a, 1).unwrap() > 0 {}
    assert!(db.chat(a).unwrap().is_empty() && db.lines(a).unwrap().is_empty(), "deleted a part at a time");
    assert!(db.vod(a).unwrap().is_some(), "the VOD itself stays until delete_vod");
    db.delete_vod(a).unwrap();
    assert!(db.search("gg", &SearchFilter::default(), 10).unwrap().hits.is_empty());
}
