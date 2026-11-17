//! Chat kept in the library: each message as it was fetched, whether a VOD's chat is fetched, and what the transcript
//! view, its minimap and Timeline layers read of it. A fetch's own keeping (slices, carrying on, starting over) is in
//! chat/tests/fetch.rs, through `chat::fetch`.

use super::*;
use crate::chat::ChatStore;

/// A library with VODs `a` and `b`, in that order, and their ids.
fn two_vods() -> (Db, i64, i64) {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let a = db.add_vod(&info("a")).unwrap().0.id;
    let b = db.add_vod(&info("b")).unwrap().0.id;
    (db, a, b)
}

/// A message with `kind` and `amount`.
fn event(time: f64, author: &str, text: &str, kind: Kind, amount: Option<f64>) -> ChatMessage {
    ChatMessage { kind, amount, ..message(time, author, text) }
}

/// A message of `emote` alone.
fn emote(time: f64, name: &str, url: &str) -> ChatMessage {
    ChatMessage { parts: vec![ChatPart::Emote { name: name.into(), url: url.into() }], ..message(time, "fan", "") }
}

/// A chat message is kept as it was fetched: its time, who wrote it in their colour, its text and emotes, and what
/// kind it is with its amount (CHANGELOG.md, 0.2.0: "each line with the messages sent while it was said, emotes and
/// all" and "Subs, gifts and tips … are marked in chat"; db.rs `CHAT_COLUMNS`: "Every column of a chat message, as
/// each INSERT INTO chat names them").
#[test]
fn a_chat_message_is_kept_as_it_was_fetched() {
    let (db, a, _) = two_vods();
    let lock = std::sync::Mutex::new(db);
    let messages = [
        ChatMessage {
            parts: vec![
                ChatPart::Text { text: "nice ".into() },
                ChatPart::Emote { name: "barkylove".into(), url: "https://files.kick.com/emotes/1/fullsize".into() },
            ],
            ..message(12.5, "Ada", "")
        },
        event(20.0, "Bo", "Cheer100 gg", Kind::Tip, Some(100.0)),
        ChatMessage { color: None, ..event(30.0, "Cy", "Cy is gifting 5 Tier 1 Subs", Kind::Gift, Some(5.0)) },
    ];
    let mut store = LibraryChat { db: &lock, vod_id: a, on_save: Box::new(|| {}), starts: Vec::new() };
    store.save(&messages, &[0.0]).unwrap();
    let lines = lock.lock().unwrap().chat(a).unwrap();
    let read: Vec<_> = lines
        .into_iter()
        .map(|l| ChatMessage {
            time: l.time,
            author: l.author,
            color: l.color,
            parts: l.parts,
            kind: l.kind,
            amount: l.amount,
            event_time: None,
            at: None,
        })
        .collect();
    assert_eq!(read, messages);
}

/// A VOD's chat reads oldest first (db.rs `chat`: "VOD `vod_id`'s chat, oldest first").
#[test]
fn a_vods_chat_reads_oldest_first() {
    let (mut db, a, _) = two_vods();
    db.add_chat(a, &[message(25.0, "v", "later"), message(5.0, "v", "first")], &[0.0]).unwrap();
    db.add_chat(a, &[message(15.0, "v", "between")], &[0.0]).unwrap();
    let times: Vec<f64> = db.chat(a).unwrap().iter().map(|l| l.time).collect();
    assert_eq!(times, [5.0, 15.0, 25.0]);
}

/// A chat fetched in full is done: it says no error and has nothing to carry on from, and isn't fetched again (db.rs
/// `finish_chat`: "Records VOD `vod_id`'s chat as fetched in full"; `set_chat`: "Replaces VOD `vod_id`'s chat with
/// `messages` and records it as fetched").
#[test]
fn a_chat_fetched_in_full_is_done() {
    let (mut db, a, _) = two_vods();
    db.add_chat(a, &[message(5.0, "v", "partway")], &[300.0]).unwrap();
    db.set_chat_status(a, ChatStatus::Failed, Some("Twitch wouldn't give the chat: service error")).unwrap();
    db.finish_chat(a).unwrap();
    let vod = db.vod(a).unwrap().unwrap();
    assert_eq!((vod.chat, vod.chat_error), (ChatStatus::Done, None));
    assert_eq!(db.chat_slices(a).unwrap(), None, "nothing to carry on from");
    assert_eq!(db.chat_count(a).unwrap(), 1, "what it fetched");
    assert!(!db.chat_pending().unwrap().contains(&a));

    db.set_chat(a, &[message(30.0, "viewer", "gg")]).unwrap();
    let vod = db.vod(a).unwrap().unwrap();
    assert_eq!((vod.chat, db.chat(a).unwrap().len()), (ChatStatus::Done, 1), "replaced, and done");
}

/// The chats fetched are those not fetched yet and those whose fetch failed, in library order, and a failed one says
/// why; one fetched in full or with no chat replay isn't fetched again (db.rs `chat_pending`: "VODs whose chat hasn't
/// been fetched (or failed), in library order"; `ChatStatus::Failed`: "`chat_error` says why, and it's tried again
/// later").
#[test]
fn the_chats_fetched_are_those_not_fetched_yet_or_failed_in_library_order() {
    let (db, a, b) = two_vods();
    let c = db.add_vod(&info("c")).unwrap().0.id;
    let d = db.add_vod(&info("d")).unwrap().0.id;
    assert_eq!(db.vod(a).unwrap().unwrap().chat, ChatStatus::Pending);
    assert_eq!(db.chat_pending().unwrap(), [d, c, b, a], "library order, newest on top");
    db.set_chat_status(b, ChatStatus::Failed, Some("Kick isn't responding")).unwrap();
    db.set_chat_status(c, ChatStatus::None, None).unwrap();
    db.finish_chat(d).unwrap();
    assert_eq!(db.chat_pending().unwrap(), [b, a]);
    assert_eq!(db.vod(b).unwrap().unwrap().chat_error.as_deref(), Some("Kick isn't responding"), "said why");
}

/// When a VOD started goes by the messages a fetch fetched itself: one that carries on from an earlier one goes by
/// its own (db.rs `LibraryChat::starts`: "Only this fetch's: one that carries on from an earlier one goes by the
/// messages it fetched itself").
#[test]
fn when_a_vod_started_goes_by_the_messages_its_fetch_fetched_itself() {
    let (mut db, a, _) = two_vods();
    let sent_at = |time: f64, at: f64| ChatMessage { at: Some(at), ..message(time, "v", "hi") };
    db.add_chat(a, &[sent_at(1.0, 1001.0), sent_at(2.0, 1002.0)], &[2.0]).unwrap();
    let lock = std::sync::Mutex::new(db);
    let mut store = LibraryChat { db: &lock, vod_id: a, on_save: Box::new(|| {}), starts: Vec::new() };
    assert_eq!(store.resume().unwrap(), Some(vec![2.0]), "carrying on");
    store.save(&[sent_at(3.0, 2003.0), sent_at(4.0, 2004.5)], &[4.0]).unwrap();
    assert_eq!(store.starts, [2000.0, 2000.5]);
    store.clear().unwrap();
    assert!(store.starts.is_empty(), "starting over");
}

/// What the transcript view shows of a chat: for each of its rows' stretches, the first messages and how many there
/// are, the stretch's end left out (db.rs `chat_windows`: "For each stretch of VOD `vod_id` (from, to: seconds, the
/// end left out), its first `cap` messages and how many it has").
#[test]
fn each_rows_stretch_shows_its_first_messages_and_how_many() {
    let (mut db, a, b) = two_vods();
    let chat: Vec<_> = [5.0, 10.0, 15.0, 16.0, 25.0].iter().map(|&t| message(t, "v", "hi")).collect();
    db.add_chat(a, &chat, &[0.0]).unwrap();
    db.add_chat(b, &[message(12.0, "v", "elsewhere")], &[0.0]).unwrap();
    let windows = db.chat_windows(a, &[(-1e12, 10.0), (10.0, 20.0), (20.0, 1e12), (30.0, 40.0)], 2).unwrap();
    let shown: Vec<_> =
        windows.iter().map(|w| (w.messages.iter().map(|m| m.time).collect::<Vec<_>>(), w.total)).collect();
    assert_eq!(shown, [(vec![5.0], 1), (vec![10.0, 15.0], 3), (vec![25.0], 1), (vec![], 0)]);
}

/// How busy a chat was over the VOD, for the minimap: how many messages in each equal stretch of it (CHANGELOG.md,
/// 0.2.0: "A timeline beside the transcript shows how busy chat was over the whole VOD."; db.rs `chat_histogram`:
/// "How many of VOD `vod_id`'s chat messages fall in each of `bins` equal stretches of its `duration`").
#[test]
fn how_busy_a_chat_was_is_counted_in_equal_stretches_of_the_vod() {
    let (mut db, a, b) = two_vods();
    let chat: Vec<_> = [0.0, 5.0, 10.0, 15.0, 16.0, 25.0].iter().map(|&t| message(t, "v", "hi")).collect();
    db.add_chat(a, &chat, &[0.0]).unwrap();
    db.add_chat(b, &[message(12.0, "v", "elsewhere")], &[0.0]).unwrap();
    assert_eq!(db.chat_histogram(a, 30.0, 3).unwrap(), [2, 3, 1]);
    assert_eq!(db.chat_count(a).unwrap(), 6, "the header's count");
}

/// The emotes a chat has, each once, by name and image (db.rs `chat_emotes`: "The emotes in VOD `vod_id`'s chat, as
/// (name, image URL), each once").
#[test]
fn a_chats_emotes_are_each_listed_once() {
    let (mut db, a, b) = two_vods();
    let kick = "https://files.kick.com/emotes/1/fullsize";
    let chat = [emote(5.0, "barkylove", kick), emote(6.0, "barkylove", kick), message(7.0, "v", "barkylove")];
    db.add_chat(a, &chat, &[0.0]).unwrap();
    db.add_chat(b, &[emote(5.0, "other", "https://files.kick.com/emotes/2/fullsize")], &[0.0]).unwrap();
    assert_eq!(db.chat_emotes(a).unwrap(), [("barkylove".to_string(), kick.to_string())]);
}

/// A chat's subs, gifts and tips, oldest first, for the chat and the timeline to mark (CHANGELOG.md, 0.2.0: "Subs,
/// gifts and tips (bits, KICKs, Super Chats and memberships) are marked in chat, and can be shown on the timeline";
/// db.rs `chat_events`: "VOD `vod_id`'s subs, gifts and tips, oldest first").
#[test]
fn a_chats_subs_gifts_and_tips_are_listed_oldest_first() {
    let (mut db, a, _) = two_vods();
    let chat = [
        event(30.0, "Cy", "Cy subscribed at Tier 1.", Kind::Sub, Some(1.0)),
        message(5.0, "v", "hi"),
        event(20.0, "Bo", "Cheer100", Kind::Tip, Some(100.0)),
        event(10.0, "Ada", "Ada is gifting 5 Tier 1 Subs", Kind::Gift, Some(5.0)),
    ];
    db.add_chat(a, &chat, &[0.0]).unwrap();
    let events: Vec<_> = db.chat_events(a).unwrap().into_iter().map(|l| (l.time, l.kind, l.amount)).collect();
    assert_eq!(events, [(10.0, Kind::Gift, Some(5.0)), (20.0, Kind::Tip, Some(100.0)), (30.0, Kind::Sub, Some(1.0))]);
}

/// A Timeline layer's messages are those with any of its words or phrases, whole, an emote by its name, in any case
/// (CHANGELOG.md, 0.2.0: "Timeline layers mark the moments you care about: … any words and emotes"; db.rs
/// `chat_matches`: "each a whole word or phrase (an emote by its name), matched as search matches words: ignoring
/// case and accents").
#[test]
fn a_layers_messages_have_any_of_its_words_whole() {
    let (mut db, a, _) = two_vods();
    let chat = [
        message(25.0, "viewer", "CLUTCH PLAY!!"),
        emote(5.0, "barkylove", "https://files.kick.com/emotes/1/fullsize"),
        message(9.0, "viewer", "play clutch"),
        message(12.0, "viewer", "Café time"),
    ];
    db.add_chat(a, &chat, &[0.0]).unwrap();
    let times = |terms: &[&str]| {
        let terms: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
        db.chat_matches(a, &terms).unwrap().into_iter().map(|(_, time)| time).collect::<Vec<_>>()
    };
    assert_eq!(times(&["clutch play"]), [25.0], "a phrase, its words in order");
    assert_eq!(times(&["BARKYLOVE", "nothing"]), [5.0], "an emote by name, any case");
    assert_eq!(times(&["cafe"]), [12.0], "any accent");
    assert_eq!(times(&["clut"]), Vec::<f64>::new(), "whole words, not prefixes");
    assert_eq!(times(&["play", "barkylove"]), [5.0, 9.0, 25.0]);
    assert_eq!(times(&["\" OR *", ""]), Vec::<f64>::new(), "what's typed is words, never search syntax");
}

/// A removed VOD's chat goes a part at a time, the VOD itself staying till the end, and then nothing of it is found
/// (db.rs `delete_some_of`: "A VOD with a busy chat has a hundred thousand rows and more … so they go a part at a time,
/// letting everything else use the library in between"; `delete_vod`).
#[test]
fn a_removed_vods_chat_goes_a_part_at_a_time() {
    let (mut db, a, b) = two_vods();
    let many: Vec<_> = (0..2500).map(|i| message(i as f64, "v", "gg")).collect();
    db.add_chat(a, &many, &[0.0]).unwrap();
    db.add_chat(b, &[message(1.0, "v", "gg elsewhere")], &[0.0]).unwrap();
    let mut parts = 0;
    while db.delete_some_of(a, 1000).unwrap() > 0 {
        parts += 1;
        assert!(db.vod(a).unwrap().is_some(), "the VOD stays till its chat is gone");
    }
    assert_eq!((parts, db.chat_count(a).unwrap()), (3, 0));
    db.delete_vod(a).unwrap();
    let hits = db.search("gg", &SearchFilter::default(), 10).unwrap().hits;
    assert_eq!(hits.iter().map(|h| h.vod_id).collect::<Vec<_>>(), [b], "another VOD's chat stays");
}
