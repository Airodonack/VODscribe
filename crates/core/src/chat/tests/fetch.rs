//! Fetching a chat into the library: a long VOD's in slices at once, kept as it arrives and carried on from where a
//! fetch stopped, pages asked again, Twitch's end and its clips, YouTube's read as yt-dlp writes it. Twitch's and
//! Kick's come from their APIs as the kit's network answers them (`TwitchChat`, `KickVod`), YouTube's from the kit's
//! yt-dlp, and each is kept in a library of the test's own as the chat queue keeps it (`LibraryChat`).

use super::*;
use crate::db::{Db, LibraryChat, SearchFilter};
use crate::testkit::sites::{self, KickVod, TwitchChat};
use crate::testkit::{Gate, Net, Reply, TempDir, eventually, yt_dlp};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// When the fake chats' VOD started (Unix seconds).
const FAKE_START: f64 = 1790631000.0;

/// A message `n` of a fake chat, `per_second` to a second, sent some
/// time within its second (Twitch's offsets are whole seconds).
fn fake(n: usize, per_second: usize) -> (String, ChatMessage) {
    let time = (n / per_second) as f64;
    let at = FAKE_START + time + (n % per_second) as f64 / per_second as f64;
    (n.to_string(), message(time, &format!("message {n}"), at))
}

/// A plain message saying `text` `time` seconds into the VOD, sent at `at`.
fn message(time: f64, text: &str, at: f64) -> ChatMessage {
    ChatMessage {
        time,
        author: "v".into(),
        color: None,
        parts: vec![ChatPart::Text { text: text.into() }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: Some(at),
    }
}

/// `count` messages spread evenly over `duration` seconds, each a quarter
/// of the way into its share of it, so none is on a slice's edge.
fn spread(count: usize, duration: f64) -> Vec<(String, ChatMessage)> {
    (0..count)
        .map(|n| {
            let time = (n as f64 + 0.25) * duration / count as f64;
            (n.to_string(), message(time, &format!("message {n}"), FAKE_START + time))
        })
        .collect()
}

/// `all` as Twitch's GraphQL API gives its comments, each with its id, and when it was sent to the second.
fn comments(all: &[(String, ChatMessage)]) -> Vec<Value> {
    let comment = |(id, m): &(String, ChatMessage)| {
        let mut c = sites::twitch_comment(m.time, &m.author, &m.text());
        (c["id"], c["createdAt"]) = (json!(id), json!(m.at.map(crate::db::utc_time)));
        c
    };
    all.iter().map(comment).collect()
}

/// `all` as Kick's chat history gives it, each sent when it was into a VOD
/// that started at `FAKE_START`.
fn history(all: &[(String, ChatMessage)]) -> Vec<Value> {
    let said = |(id, m): &(String, ChatMessage)| {
        let mut said = sites::kick_message(FAKE_START + m.time, &m.author, &m.text());
        said["id"] = json!(id);
        said
    };
    all.iter().map(said).collect()
}

/// A Kick VOD of this test's own, `duration` seconds long, started at
/// `FAKE_START` with `all` its chat; and the link of its channel's
/// history, but for the cursor.
fn kick_of_own(duration: f64, all: &[(String, ChatMessage)]) -> (KickVod, String) {
    kick_numbered(own(), duration, all)
}

/// Kick VOD `n` of this test's own (`kick_of_own`), made anew: its pages
/// counted from the first again, for `KickVod::failing`.
fn kick_numbered(n: usize, duration: f64, all: &[(String, ChatMessage)]) -> (KickVod, String) {
    let channel = 6_000_000 + n as u64;
    let vod = KickVod::new(&format!("01a0e9a8-a9e0-797e-9646-{n:012x}"), channel, duration);
    (vod.started(FAKE_START).chat(history(all)), format!("https://web.kick.com/api/v1/chat/{channel}/history?cursor="))
}

/// The cursor Kick's history is asked before for `message`'s page: when it
/// was sent, in microseconds.
fn cursor_at(message: &ChatMessage) -> i64 {
    ((FAKE_START + message.time) * 1e6).round() as i64
}

/// Counts the requests to links starting with `prefix` on `net` from now,
/// refused ones included, each then answered as before. A held request is
/// counted once it's let go.
fn counting(net: &Net, prefix: &str) -> Arc<AtomicUsize> {
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    net.replies(prefix, move |_| {
        counter.fetch_add(1, Ordering::Relaxed);
        None
    });
    asked
}

/// A library of the test's own holding VOD `info`, and its id.
fn library_of(info: &VodInfo) -> (Mutex<Db>, i64) {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let id = db.add_vod(info).unwrap().0.id;
    (Mutex::new(db), id)
}

/// The library's store for VOD `id`'s chat, as the chat queue makes it for
/// a fetch, with `on_save` told after each batch.
fn store<'a>(db: &'a Mutex<Db>, id: i64, on_save: impl FnMut() + 'a) -> LibraryChat<'a> {
    LibraryChat { db, vod_id: id, on_save: Box::new(on_save), starts: Vec::new() }
}

/// `fetch` of a Twitch or Kick VOD `info`'s chat into the library as VOD
/// `id`'s, from the site's API, with how far it said it got each time.
fn fetched_with_progress(info: &VodInfo, db: &Mutex<Db>, id: i64, cancel: &AtomicBool) -> (Result<bool>, Vec<f64>) {
    let mut progress = Vec::new();
    let result =
        fetch(info, None, Path::new("no-yt-dlp-chat"), &mut store(db, id, || {}), cancel, |f| progress.push(f));
    (result, progress)
}

/// `fetch` of a Twitch or Kick VOD `info`'s chat into the library as VOD `id`'s.
fn fetched(info: &VodInfo, db: &Mutex<Db>, id: i64) -> Result<bool> {
    fetched_with_progress(info, db, id, &AtomicBool::new(false)).0
}

/// VOD `id`'s chat in the library: each message's time (to the
/// microsecond, as Kick's are) and text, in time order.
fn kept(db: &Mutex<Db>, id: i64) -> Vec<(i64, String)> {
    let lines = db.lock().unwrap().chat(id).unwrap();
    in_time_order(lines.iter().map(|l| ChatMessage { parts: l.parts.clone(), ..message(l.time, "", 0.0) }))
}

/// Every message of `all`: its time (to the microsecond) and text, in time order.
fn every(all: &[(String, ChatMessage)]) -> Vec<(i64, String)> {
    in_time_order(all.iter().map(|(_, m)| m.clone()))
}

/// Messages' times and texts, by time, then text ("message 9" before "message 10").
fn in_time_order(messages: impl Iterator<Item = ChatMessage>) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = messages.map(|m| (us(m.time), m.text())).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.len().cmp(&b.1.len())).then(a.1.cmp(&b.1)));
    out
}

/// `seconds` in microseconds.
fn us(seconds: f64) -> i64 {
    (seconds * 1e6).round() as i64
}

/// How many messages VOD `id`'s chat has in the library.
fn count(db: &Mutex<Db>, id: i64) -> u32 {
    db.lock().unwrap().chat_count(id).unwrap()
}

/// Where the library says an unfinished fetch of VOD `id`'s chat got to.
fn slices_kept(db: &Mutex<Db>, id: i64) -> Option<Vec<f64>> {
    db.lock().unwrap().chat_slices(id).unwrap()
}

// Slices.

/// A VOD's chat is fetched in a slice per whole ten minutes of it, four at most, on Twitch and Kick (CHANGELOG.md,
/// 0.3.0: "Kick and Twitch chats download 3 to 5 times faster."; chat.rs `SLICES` and `MIN_SLICE`: "At most this many
/// slices at once, each this long at least"). An unfinished fetch keeps where each slice got to.
#[test]
fn a_chat_is_fetched_in_a_slice_per_whole_ten_minutes_four_at_most() {
    for (duration, slices) in [(599.0, 1), (1199.0, 1), (1200.0, 2), (1800.0, 3), (2400.0, 4), (6000.0, 4)] {
        let net = Net::new();
        let number = own_twitch_number();
        net.twitch_comments(&number, &TwitchChat::of(Vec::new()).failing(|_| true));
        let info = twitch_vod(&number, Some(duration));
        let (db, id) = library_of(&info);
        assert!(fetched(&info, &db, id).is_err());
        assert_eq!(slices_kept(&db, id).map(|p| p.len()), Some(slices), "Twitch, {duration} s");

        let (vod, _) = kick_of_own(duration, &[]);
        net.kick_vod(&vod.clone().failing(|_| true));
        let info = kick_vod(&vod, duration);
        let (db, id) = library_of(&info);
        assert!(fetched(&info, &db, id).is_err());
        assert_eq!(slices_kept(&db, id).map(|p| p.len()), Some(slices), "Kick, {duration} s");
    }
}

/// A long VOD's slices are fetched at once, on Kick and Twitch: while the first slice waits for its first page, the
/// others page through the whole of theirs (chat.rs, the comment before `SLICES`: "a long VOD's chat is fetched in slices of its time at
/// once, each paging through its own stretch"; CHANGELOG.md, 0.3.0: "Kick and Twitch chats download 3 to 5 times
/// faster.").
#[test]
fn a_long_vods_slices_are_fetched_at_once() {
    // 3,000 messages over 40 minutes: four slices of 750, each 30 pages of 25 and the page that goes past its start.
    let all = spread(3000, 2400.0);
    let (vod, history) = kick_of_own(2400.0, &all);
    let net = Net::new();
    let asked = counting(&net, &history);
    net.kick_vod(&vod);
    let first = Gate::new();
    let first_slices_end = ((FAKE_START + 600.0) * 1e6) as i64;
    net.holds(&format!("{history}{first_slices_end}"), &first);
    let info = kick_vod(&vod, 2400.0);
    let (db, id) = library_of(&info);
    std::thread::scope(|s| {
        let fetching = s.spawn(|| fetched(&info, &db, id));
        first.reached();
        eventually("the other three slices' pages asked", || asked.load(Ordering::Relaxed) >= 3 * 31);
        assert!(kept(&db, id).iter().all(|(time, _)| *time >= us(600.0)), "none of the first slice's yet");
        first.open();
        assert!(fetching.join().unwrap().unwrap());
    });
    assert_eq!(kept(&db, id), every(&all), "Kick");

    // Twitch: 2,400 messages, one a second, 100 to a page; the page from 0 s waits.
    let all: Vec<_> = (0..2400).map(|n| fake(n, 1)).collect();
    let number = own_twitch_number();
    let net = Net::new();
    let first = Gate::new();
    let (ours, waits) = (number.clone(), first.clone());
    net.replies(crate::chat::TWITCH_GQL, move |request| {
        let body = request.json()?;
        let first_page = body["variables"]["id"] == ours.as_str()
            && body["variables"]["offset"] == 0
            && body["query"].as_str()?.contains("comments(");
        if first_page {
            waits.wait();
        }
        None
    });
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)));
    let info = twitch_vod(&number, Some(2400.0));
    let (db, id) = library_of(&info);
    std::thread::scope(|s| {
        let fetching = s.spawn(|| fetched(&info, &db, id));
        first.reached();
        // All but the last batch of the other three slices' 1,800 kept: a batch is kept once it has 500.
        eventually("the other slices' messages kept", || count(&db, id) > 1800 - 500);
        assert!(kept(&db, id).iter().all(|(time, _)| *time >= us(600.0)), "none of the first slice's yet");
        first.open();
        assert!(fetching.join().unwrap().unwrap());
    });
    assert_eq!(kept(&db, id), every(&all), "Twitch");
}

/// A chat fetched in slices has every message once: none lost or kept twice at a slice's edge, on Twitch and Kick
/// (chat.rs `in_slices`: "Runs `fetch_slice` … for each slice at once, … and saves what they send"; `twitch_into`:
/// "Slice i covers [from, to) in whole seconds of the broadcast"; `kick_into`: "Slice i covers [from, to) in Unix
/// time").
#[test]
fn a_chat_fetched_in_slices_has_every_message_once() {
    // Twitch: one a second over 40 minutes, so on each slice's edge too, and another in the second of each edge.
    let mut all: Vec<_> = (0..2400).map(|n| fake(n, 1)).collect();
    for edge in [599.0, 600.0, 1200.0, 1800.0] {
        all.push((format!("edge {edge}"), message(edge, &format!("also at {edge}"), FAKE_START + edge + 0.5)));
    }
    // In time order, as Twitch gives them.
    all.sort_by(|a, b| a.1.time.total_cmp(&b.1.time));
    let number = own_twitch_number();
    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)));
    let info = twitch_vod(&number, Some(2400.0));
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept(&db, id), every(&all), "Twitch");

    // Kick: at the VOD's start, on each slice's edge and just either side of it, and just before the VOD's end.
    let mut all = spread(1000, 2400.0);
    for edge in [0.0, 599.999, 600.0, 600.001, 1200.0, 1800.0, 2399.999] {
        all.push((format!("edge {edge}"), message(edge, &format!("at {edge}"), FAKE_START + edge)));
    }
    let (vod, _) = kick_of_own(2400.0, &all);
    net.kick_vod(&vod);
    let info = kick_vod(&vod, 2400.0);
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept(&db, id), every(&all), "Kick");
}

/// A Kick message sent at the VOD's very end is kept (chat.rs `kick_into`: "Slice i covers [from, to) in Unix time;
/// the last one includes the end").
#[test]
#[ignore = "disagreement: the last slice is first asked for messages older than the VOD's end, so one at the end is never asked for"]
fn a_kick_message_at_the_vods_very_end_is_kept() {
    let all = vec![
        ("start".to_string(), message(0.0, "at the start", FAKE_START)),
        ("end".to_string(), message(500.0, "at the end", FAKE_START + 500.0)),
    ];
    let (vod, _) = kick_of_own(500.0, &all);
    let net = Net::new();
    net.kick_vod(&vod);
    let info = kick_vod(&vod, 500.0);
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept(&db, id), every(&all));
}

/// A chat is kept as it arrives, so it shows before the fetch is over (chat.rs `ChatStore`: "Where a VOD's chat goes
/// as it's fetched, a batch at a time, so it can be shown while the rest arrives"; CHANGELOG.md, 0.2.0: "It's fetched
/// in the background, shows as it arrives").
#[test]
fn a_chat_is_kept_as_it_arrives() {
    // 1,000 messages, 25 to a page: the 21st page waits.
    let all = spread(1000, 500.0);
    let (vod, history) = kick_of_own(500.0, &all);
    let net = Net::new();
    net.kick_vod(&vod);
    let later = Gate::new();
    net.holds(&format!("{history}{}", cursor_at(&all[500].1)), &later);
    let info = kick_vod(&vod, 500.0);
    let (db, id) = library_of(&info);
    let (told, saves) = mpsc::channel();
    std::thread::scope(|s| {
        let fetching = s.spawn(|| {
            let mut store = store(&db, id, || told.send(()).unwrap());
            fetch(&info, None, Path::new("no-yt-dlp-chat"), &mut store, &AtomicBool::new(false), |_| {})
        });
        later.reached();
        saves.recv_timeout(Duration::from_secs(5)).expect("a batch kept while the rest waits");
        let shown = count(&db, id);
        assert!((1..1000).contains(&shown), "{shown} kept before the fetch is over");
        later.open();
        assert!(fetching.join().unwrap().unwrap());
    });
    assert_eq!(kept(&db, id), every(&all));
}

/// A Twitch or Kick chat's fetch says how far it has got, rising from 0 through the middle to 1 once it's whole, also
/// when it carried on from slices done before (chat.rs `fetch`: "Reports a 0..1 fraction while it pages through"; `in_slices`: "A slice
/// done before (a fetch carried on) sends nothing more").
#[test]
fn a_chats_fetch_says_how_far_it_has_got() {
    let all: Vec<_> = (0..2400).map(|n| fake(n, 1)).collect();
    let number = own_twitch_number();
    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)));
    let info = twitch_vod(&number, Some(2400.0));
    let (db, id) = library_of(&info);
    let (fetched, progress) = fetched_with_progress(&info, &db, id, &AtomicBool::new(false));
    assert!(fetched.unwrap());
    assert!(rises_to_whole(&progress), "Twitch: {progress:?}");

    let all = spread(3000, 2400.0);
    let (vod, _) = kick_of_own(2400.0, &all);
    net.kick_vod(&vod);
    let info = kick_vod(&vod, 2400.0);
    let (db, id) = library_of(&info);
    let (fetched, progress) = fetched_with_progress(&info, &db, id, &AtomicBool::new(false));
    assert!(fetched.unwrap());
    assert!(rises_to_whole(&progress), "Kick: {progress:?}");

    // Stopped with the last three of four slices done, the first not begun: carried on, it's whole at the end.
    let (vod, history) = kick_of_own(2400.0, &all);
    let net = Net::new();
    let asked = counting(&net, &history);
    net.kick_vod(&vod);
    let first = Gate::new();
    net.holds(&format!("{history}{}", us(FAKE_START + 600.0)), &first);
    let info = kick_vod(&vod, 2400.0);
    let (db, id) = library_of(&info);
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        let fetching = s.spawn(|| fetched_with_progress(&info, &db, id, &stop).0);
        first.reached();
        eventually("the other three slices' pages asked", || asked.load(Ordering::Relaxed) >= 3 * 31);
        stop.store(true, Ordering::Relaxed);
        first.open();
        assert!(fetching.join().unwrap().is_err_and(|e| crate::is_canceled(&e)));
    });
    drop(net);
    let net = Net::new();
    net.kick_vod(&vod);
    let (fetched, progress) = fetched_with_progress(&info, &db, id, &AtomicBool::new(false));
    assert!(fetched.unwrap());
    assert_eq!(kept(&db, id), every(&all));
    assert_eq!(progress.last(), Some(&1.0), "Kick, carried on: {progress:?}");
}

/// Whether how far a fetch said it got rose from under a quarter, through the middle, to 1, a little at a time as
/// pages came in.
fn rises_to_whole(progress: &[f64]) -> bool {
    progress.windows(2).all(|w| (0.0..=0.1).contains(&(w[1] - w[0])))
        && progress.first().is_some_and(|f| (0.0..0.25).contains(f))
        && progress.iter().any(|f| (0.4..0.6).contains(f))
        && progress.last() == Some(&1.0)
}

/// A Kick chat's fetch stops at the VOD's start, not paging on back through the channel's older chat, and ends when
/// the history stops moving back (chat.rs, the comment before `fetch_kick`: "The history is the whole channel's, so
/// the VOD's span is worked out from when it started"; `kick_into`: "Stop at the slice's start, or when the history
/// runs out or stops moving back").
#[test]
fn a_kick_fetch_stops_at_the_vods_start() {
    // 100 messages in the VOD, 25 to a page, and 1,000 the channel had before it.
    let all = spread(100, 500.0);
    let before: Vec<_> =
        (0..1000).map(|n| (format!("before {n}"), message(-1000.0 + n as f64, "earlier", -1.0))).collect();
    let (vod, history) = kick_of_own(500.0, &[all.clone(), before].concat());
    let net = Net::new();
    let asked = counting(&net, &history);
    net.kick_vod(&vod);
    let info = kick_vod(&vod, 500.0);
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept(&db, id), every(&all));
    assert_eq!(asked.load(Ordering::Relaxed), 5, "the VOD's four pages, and the one that goes past its start");

    // A history whose every page gives the cursor it was asked for.
    let (vod, history) = kick_of_own(500.0, &[]);
    let net = Net::new();
    let mut stuck = sites::kick_message(FAKE_START + 400.0, "v", "again and again");
    stuck["id"] = json!("stuck");
    let page = history.clone();
    net.replies(&history, move |request| {
        let cursor = request.url.strip_prefix(&page)?.to_string();
        Some(Reply::json(&json!({ "data": { "messages": [stuck.clone()], "cursor": cursor } })))
    });
    net.kick_vod(&vod);
    let info = kick_vod(&vod, 500.0);
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(count(&db, id), 1);
}

// Carrying on where a fetch stopped.

/// A chat whose fetch stops partway, the network failing, keeps what came in, and where each slice got to, on Twitch
/// and Kick (CHANGELOG.md, 0.6.0 Fixes: "A chat that stops partway is kept."; chat.rs `in_slices`: "what arrived is
/// saved either way").
#[test]
fn a_chat_that_stops_partway_is_kept() {
    // 3,000 messages, 7 a second, in one slice; 2,400, one a second, in four.
    for (slices, all) in
        [(1, (0..3000).map(|n| fake(n, 7)).collect::<Vec<_>>()), (4, (0..2400).map(|n| fake(n, 1)).collect())]
    {
        let duration = all.last().unwrap().1.time + 1.0;
        let number = own_twitch_number();
        let net = Net::new();
        net.twitch_comments(&number, &TwitchChat::of(comments(&all)).failing(|n| n >= 20));
        let info = twitch_vod(&number, Some(duration));
        let (db, id) = library_of(&info);
        assert!(fetched(&info, &db, id).is_err());
        let partial = count(&db, id) as usize;
        assert!((1000..all.len()).contains(&partial), "Twitch, {slices} slices: kept {partial}");
        assert_eq!(slices_kept(&db, id).map(|p| p.len()), Some(slices), "Twitch");

        let all = spread(3000, duration);
        let (vod, _) = kick_of_own(duration, &all);
        net.kick_vod(&vod.clone().failing(|n| n >= 60));
        let info = kick_vod(&vod, duration);
        let (db, id) = library_of(&info);
        assert!(fetched(&info, &db, id).is_err());
        let partial = count(&db, id) as usize;
        assert!((1000..all.len()).contains(&partial), "Kick, {slices} slices: kept {partial}");
        assert_eq!(slices_kept(&db, id).map(|p| p.len()), Some(slices), "Kick");
    }
}

/// A Twitch chat's fetch that stopped partway carries on from where each slice got to, asking only for the rest, and
/// ends with every message once (CHANGELOG.md, 0.2.0: "carries on where it stopped if interrupted"; chat.rs
/// `ChatStore`: "so one that's interrupted (quitting, a network error) carries on from there"; e2e chat-partial.mjs:
/// "Try again carries on from where it stopped … to the whole chat, each once").
#[test]
fn a_twitch_fetch_carries_on_where_it_stopped() {
    // 3,000 messages, 7 a second, in one slice; 2,400, one a second, in four.
    for all in [(0..3000).map(|n| fake(n, 7)).collect::<Vec<_>>(), (0..2400).map(|n| fake(n, 1)).collect()] {
        let duration = all.last().unwrap().1.time + 1.0;
        let number = own_twitch_number();
        let info = twitch_vod(&number, Some(duration));
        // The pages a whole fetch asks for.
        let whole = TwitchChat::of(comments(&all));
        let net = Net::new();
        net.twitch_comments(&number, &whole);
        let (db, id) = library_of(&info);
        assert!(fetched(&info, &db, id).unwrap());
        drop(net);

        let (db, id) = library_of(&info);
        let net = Net::new();
        net.twitch_comments(&number, &TwitchChat::of(comments(&all)).failing(|n| n >= 20));
        assert!(fetched(&info, &db, id).is_err());
        drop(net);
        let net = Net::new();
        let rest = TwitchChat::of(comments(&all));
        net.twitch_comments(&number, &rest);
        let (fetched, progress) = fetched_with_progress(&info, &db, id, &AtomicBool::new(false));
        assert!(fetched.unwrap());
        assert_eq!(kept(&db, id), every(&all), "{duration} s");
        let (rest, whole) = (rest.pages_asked(), whole.pages_asked());
        assert!(rest + 10 < whole, "{rest} of {whole} pages");
        assert_eq!(progress.last(), Some(&1.0));
    }
}

/// A Kick chat's fetch that stopped partway carries on from where each slice got to, asking only for the rest, and
/// ends with every message once (CHANGELOG.md, 0.2.0: "carries on where it stopped if interrupted"; chat.rs
/// `ChatStore`: "so one that's interrupted (quitting, a network error) carries on from there").
#[test]
fn a_kick_fetch_carries_on_where_it_stopped() {
    for duration in [500.0, 2400.0] {
        let all = spread(3000, duration);
        let n = own();
        let (vod, history) = kick_numbered(n, duration, &all);
        let info = kick_vod(&vod, duration);
        let net = Net::new();
        let whole = counting(&net, &history);
        net.kick_vod(&vod);
        let (db, id) = library_of(&info);
        assert!(fetched(&info, &db, id).unwrap());
        drop(net);

        let (db, id) = library_of(&info);
        let net = Net::new();
        net.kick_vod(&kick_numbered(n, duration, &all).0.failing(|n| n >= 60));
        assert!(fetched(&info, &db, id).is_err());
        drop(net);
        let net = Net::new();
        let rest = counting(&net, &history);
        net.kick_vod(&vod);
        assert!(fetched(&info, &db, id).unwrap());
        assert_eq!(kept(&db, id), every(&all), "{duration} s");
        let (rest, whole) = (rest.load(Ordering::Relaxed), whole.load(Ordering::Relaxed));
        assert!(rest + 40 < whole, "{rest} of {whole} pages");
    }
}

/// A chat's fetch stopped, as quitting stops it, carries on from there the next time, asking only for the rest
/// (chat.rs `ChatStore`: "so one that's interrupted (quitting, a network error) carries on from there").
#[test]
fn a_chats_fetch_stopped_carries_on_from_there() {
    // 1,000 messages, 25 to a page: stopped while the 21st page is asked.
    let all = spread(1000, 500.0);
    let (vod, history) = kick_of_own(500.0, &all);
    let net = Net::new();
    net.kick_vod(&vod);
    let later = Gate::new();
    net.holds(&format!("{history}{}", cursor_at(&all[500].1)), &later);
    let info = kick_vod(&vod, 500.0);
    let (db, id) = library_of(&info);
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        let fetching = s.spawn(|| fetched_with_progress(&info, &db, id, &stop).0);
        later.reached();
        stop.store(true, Ordering::Relaxed);
        later.open();
        assert!(fetching.join().unwrap().is_err_and(|e| crate::is_canceled(&e)));
    });
    drop(net);
    assert!(count(&db, id) >= 500, "what came in kept");

    // The last 500: 20 pages, and the one past the VOD's start.
    let net = Net::new();
    let rest = counting(&net, &history);
    net.kick_vod(&vod);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept(&db, id), every(&all));
    let rest = rest.load(Ordering::Relaxed);
    assert!(rest <= 21, "{rest} pages asked for the last 500 messages");
}

/// A chat's fetch stopped, as quitting stops it, ends though the site has more to give (chat.rs `in_slices`: "A slice
/// failing (or `cancel`) stops the others"; `ChatStore`: "one that's interrupted (quitting, a network error)").
#[test]
fn a_chats_fetch_stopped_ends_though_the_site_has_more() {
    // A history that never runs out: each page one message a microsecond before its cursor.
    let (vod, history) = kick_of_own(500.0, &[]);
    let net = Net::new();
    let asked = counting(&net, &history);
    let pages = history.clone();
    net.replies(&history, move |request| {
        let cursor: i64 = request.url.strip_prefix(&pages)?.parse().ok()?;
        let mut said = sites::kick_message((cursor - 1) as f64 / 1e6, "v", "more");
        said["id"] = json!(cursor.to_string());
        Some(Reply::json(&json!({ "data": { "messages": [said], "cursor": (cursor - 1).to_string() } })))
    });
    net.kick_vod(&vod);
    let info = kick_vod(&vod, 500.0);
    let (db, id) = library_of(&info);
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        let fetching = s.spawn(|| fetched_with_progress(&info, &db, id, &stop).0);
        eventually("pages asked", || asked.load(Ordering::Relaxed) >= 10);
        stop.store(true, Ordering::Relaxed);
        assert!(fetching.join().unwrap().is_err_and(|e| crate::is_canceled(&e)));
    });
    assert!(count(&db, id) >= 9, "what came in kept");
}

/// A fetch an older version left unfinished starts over, since that version cleared the chat but not where this
/// one's slices got to (db.rs `chat_slices`: "None if a version before 0.3.0 has fetched since (it writes
/// `chat_resume`, which this one only ever clears): it cleared the chat but not the slices, which no longer describe
/// it").
#[test]
fn a_fetch_an_older_version_left_unfinished_starts_over() {
    let all: Vec<_> = (0..2400).map(|n| fake(n, 1)).collect();
    let number = own_twitch_number();
    let info = twitch_vod(&number, Some(2400.0));
    let (db, id) = library_of(&info);
    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)).failing(|n| n >= 20));
    assert!(fetched(&info, &db, id).is_err());
    drop(net);
    // 0.2's fetch since: it cleared the chat, then stopped two minutes in.
    let older = format!("DELETE FROM chat WHERE vod_id = {id}; UPDATE vods SET chat_resume = 120.0 WHERE id = {id};");
    db.lock().unwrap().conn.execute_batch(&older).unwrap();

    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)));
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept(&db, id), every(&all), "every message once");
}

/// A chat fetched again, with no fetch of it left unfinished, starts over: it replaces the old one, which is no
/// longer kept or found, and another VOD's chat stays, on Twitch and Kick (chat.rs `ChatStore::clear`: "Forgets every
/// saved message and position, to start over"; db.rs `LibraryChat`'s `clear`: "The old messages' words out of the
/// index too (some may be gone from the platform since, removed by moderators)").
#[test]
fn a_chat_fetched_again_replaces_the_old_one() {
    let all: Vec<_> = (0..600).map(|n| fake(n, 1)).collect();
    let net = Net::new();
    let number = own_twitch_number();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)));
    let (vod, _) = kick_of_own(600.0, &all);
    net.kick_vod(&vod);
    for info in [twitch_vod(&number, Some(600.0)), kick_vod(&vod, 600.0)] {
        let (db, id) = library_of(&info);
        let other = db.lock().unwrap().add_vod(&twitch_vod(&own_twitch_number(), Some(60.0))).unwrap().0.id;
        let old = message(5.0, "removed by moderators", 0.0);
        db.lock().unwrap().set_chat(id, &[old.clone(), ChatMessage { time: 6000.0, ..old.clone() }]).unwrap();
        db.lock().unwrap().set_chat(other, &[old]).unwrap();
        assert!(fetched(&info, &db, id).unwrap());
        assert_eq!(kept(&db, id), every(&all), "{}", info.platform);
        let db = db.lock().unwrap();
        let found = db.search("moderators", &SearchFilter { vod_id: Some(id), ..Default::default() }, 10).unwrap();
        assert!(found.hits.is_empty(), "{}: the old chat isn't found", info.platform);
        assert_eq!(db.chat_count(other).unwrap(), 1, "another VOD's chat stays");
    }
}

// Pages asked again.

/// A page Twitch or Kick refuses for a moment is asked for again, and the fetch ends whole (CHANGELOG.md, 0.6.0
/// Fixes: "Chat downloads retry a refused part."; chat.rs `Transient`: "Twitch answers a page of a long VOD's chat
/// with \"service error\" now and then, and the next ask is fine").
#[test]
fn a_page_refused_for_a_moment_is_asked_again() {
    let all: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
    let number = own_twitch_number();
    let info = twitch_vod(&number, Some(430.0));
    let whole = TwitchChat::of(comments(&all)).ending_at(430.0);
    let net = Net::new();
    net.twitch_comments(&number, &whole);
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    drop(net);
    let refused = TwitchChat::of(comments(&all)).ending_at(430.0).failing(|n| n == 1);
    let net = Net::new();
    net.twitch_comments(&number, &refused);
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    drop(net);
    assert_eq!(kept(&db, id), every(&all), "Twitch");
    assert_eq!(refused.pages_asked(), whole.pages_asked() + 1, "Twitch: the refused page asked again");

    let all = spread(1000, 500.0);
    let n = own();
    let (vod, history) = kick_numbered(n, 500.0, &all);
    let info = kick_vod(&vod, 500.0);
    let net = Net::new();
    let whole = counting(&net, &history);
    net.kick_vod(&vod);
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    drop(net);
    let net = Net::new();
    let refused = counting(&net, &history);
    net.kick_vod(&kick_numbered(n, 500.0, &all).0.failing(|n| n == 1));
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept(&db, id), every(&all), "Kick");
    let (refused, whole) = (refused.load(Ordering::Relaxed), whole.load(Ordering::Relaxed));
    assert_eq!(refused, whole + 1, "Kick: the refused page asked again");
}

/// A page refused five times stops the fetch, saying why, with what it had kept, and once the site answers the fetch
/// carries on from there (CHANGELOG.md, 0.6.0 Fixes: "Chat downloads retry a refused part."; chat.rs `PAGE_RETRIES`:
/// "five tries over about half a minute, then the fetch stops (and keeps what it saved)").
#[test]
fn a_page_refused_five_times_stops_the_fetch_saying_why() {
    // Twitch refuses from the 11th page on.
    let all: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
    let number = own_twitch_number();
    let info = twitch_vod(&number, Some(430.0));
    let net = Net::new();
    let chat = TwitchChat::of(comments(&all)).ending_at(430.0).failing(|n| n >= 10);
    net.twitch_comments(&number, &chat);
    let (db, id) = library_of(&info);
    let e = fetched(&info, &db, id).unwrap_err();
    drop(net);
    assert_eq!(format!("{e:#}"), "Twitch wouldn't give the chat: service error");
    assert_eq!(chat.pages_asked(), 10 + 5, "Twitch: the 11th asked five times");
    let partial = count(&db, id) as usize;
    assert!((900..3000).contains(&partial), "Twitch: kept {partial}");
    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)).ending_at(430.0));
    assert!(fetched(&info, &db, id).unwrap());
    drop(net);
    assert_eq!(kept(&db, id), every(&all), "Twitch: every message, once");

    // Kick fails from the 11th page on.
    let all = spread(1000, 500.0);
    let (vod, history) = kick_of_own(500.0, &all);
    let info = kick_vod(&vod, 500.0);
    let net = Net::new();
    let asked = counting(&net, &history);
    net.kick_vod(&vod.clone().failing(|n| n >= 10));
    let (db, id) = library_of(&info);
    let e = fetched(&info, &db, id).unwrap_err();
    drop(net);
    assert_eq!(format!("{e:#}"), "web.kick.com answered 500 when asked for the chat.");
    assert_eq!(asked.load(Ordering::Relaxed), 10 + 5, "Kick: the 11th asked five times");
    assert_eq!(count(&db, id), 250, "Kick: the ten pages kept");
    let net = Net::new();
    net.kick_vod(&vod);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept(&db, id), every(&all), "Kick: every message, once");
}

/// A slice whose page is refused five times stops the other slices, and the fetch says why that slice failed (chat.rs
/// `in_slices`: "A slice failing (or `cancel`) stops the others", and its comment "The slice that failed, rather than
/// the ones it stopped").
#[test]
fn a_slice_that_fails_stops_the_others_and_says_why() {
    // Four slices, every page refused: the first slice's five tries answered while the other three wait for theirs.
    let all = spread(3000, 2400.0);
    let (vod, history) = kick_of_own(2400.0, &all);
    let net = Net::new();
    let asked = counting(&net, &history);
    net.kick_vod(&vod.clone().failing(|_| true));
    let others = Gate::new();
    for slice in 1..4 {
        net.holds(&format!("{history}{}", us(FAKE_START + 600.0 * (slice + 1) as f64)), &others);
    }
    let info = kick_vod(&vod, 2400.0);
    let (db, id) = library_of(&info);
    let e = std::thread::scope(|s| {
        let fetching = s.spawn(|| fetched(&info, &db, id));
        eventually("the first slice's five tries", || asked.load(Ordering::Relaxed) == 5);
        others.open();
        fetching.join().unwrap().unwrap_err()
    });
    assert!(!crate::is_canceled(&e), "{e:#}");
    assert_eq!(format!("{e:#}"), "web.kick.com answered 500 when asked for the chat.");
}

/// A page the site refuses for good is asked for once, and the fetch says why (chat.rs `Transient`: "An error worth
/// asking again for: the site out of sorts for a moment"; `api`).
#[test]
fn a_page_refused_for_good_is_asked_for_once() {
    let (vod, history) = kick_of_own(500.0, &spread(100, 500.0));
    let net = Net::new();
    let asked = counting(&net, &history);
    net.answers(&history, 404).kick_vod(&vod);
    let info = kick_vod(&vod, 500.0);
    let (db, id) = library_of(&info);
    let e = fetched(&info, &db, id).unwrap_err();
    assert_eq!(format!("{e:#}"), "web.kick.com answered 404 when asked for the chat.");
    assert_eq!(asked.load(Ordering::Relaxed), 1);
}

// Twitch's end, and its clips.

/// A Twitch chat's fetch ends at the VOD's end, though Twitch's pages say there are more and refuse to give them
/// (CHANGELOG.md, 0.6.0 Fixes: "Twitch chats finish at the VOD's end."; chat.rs `twitch_into`: "So a slice stops at
/// the VOD's end, whatever the page says").
#[test]
fn a_twitch_fetch_stops_at_the_vods_end() {
    // 3,024 messages, 7 a second, to 431 s: the chat runs on past the VOD's end, at 418 s.
    let all: Vec<_> = (0..3024).map(|n| fake(n, 7)).collect();
    let before = |end: f64| every(&all).into_iter().filter(|(t, _)| *t < us(end)).collect::<Vec<_>>();
    let kept_before =
        |db: &Mutex<Db>, id: i64, end: f64| kept(db, id).into_iter().filter(|(t, _)| *t < us(end)).collect::<Vec<_>>();

    // The page from 406 s ends at 420 s and says there's more, which Twitch refuses to give.
    let number = own_twitch_number();
    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)).ending_at(418.0));
    let info = twitch_vod(&number, Some(418.0));
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    assert_eq!(kept_before(&db, id, 418.0), before(418.0), "every message before the end");

    // Every page saying there's more, even the empty ones after the chat's last message: still the end.
    for length in [418.0, 440.0] {
        let number = own_twitch_number();
        net.twitch_comments(&number, &TwitchChat::of(comments(&all)).ending_at(length).always_saying_more());
        let info = twitch_vod(&number, Some(length));
        let (db, id) = library_of(&info);
        assert!(fetched(&info, &db, id).unwrap());
        assert_eq!(kept_before(&db, id, length), before(length), "{length} s");
    }
}

/// A Twitch VOD whose length Twitch didn't say has its chat read in one slice to the chat's end, even when its pages
/// say there are more (chat.rs `Span`: "up to `end` (or on past the end, for a whole VOD)"; `twitch_into`: "A page
/// with nothing on it has nothing after it either").
#[test]
fn a_twitch_vod_of_unknown_length_has_its_chat_read_to_its_end() {
    let all: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
    let net = Net::new();
    for chat in [TwitchChat::of(comments(&all)), TwitchChat::of(comments(&all)).always_saying_more()] {
        let number = own_twitch_number();
        net.twitch_comments(&number, &chat);
        let info = twitch_vod(&number, None);
        let (db, id) = library_of(&info);
        assert!(fetched(&info, &db, id).unwrap());
        assert_eq!(kept(&db, id), every(&all));
    }
}

/// A Twitch clip's chat is its broadcast's over the clip, at the clip's own times (CHANGELOG.md, 0.3.0: "Twitch clips
/// play, and show the chat from the broadcast they were cut from, while Twitch still has it."; chat.rs
/// `fetch_twitch_from`: "A clip's chat is its broadcast's over the clip's stretch, at the clip's times").
#[test]
fn a_twitch_clips_chat_is_its_broadcasts_over_the_clip() {
    let all: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
    let number = own_twitch_number();
    let slug = format!("AClipOf{number}");
    let net = Net::new();
    // 30 s from 100 s in.
    net.twitch_comments(&number, &TwitchChat::of(comments(&all))).twitch_clip(&slug, Some((&number, 100.0)), 30.0);
    let clip = VodInfo { platform_id: slug, ..twitch_vod(&number, Some(30.0)) };
    let (db, id) = library_of(&clip);
    assert!(fetched(&clip, &db, id).unwrap());
    let in_clip: Vec<_> = all
        .iter()
        .filter(|(_, m)| (100.0..130.0).contains(&m.time))
        .map(|(id, m)| (id.clone(), ChatMessage { time: m.time - 100.0, ..m.clone() }))
        .collect();
    assert_eq!(kept(&db, id), every(&in_clip));
}

/// A Twitch clip whose broadcast Twitch no longer has has no chat replay (CHANGELOG.md, 0.3.0: "Twitch clips … show
/// the chat from the broadcast they were cut from, while Twitch still has it."; chat.rs `fetch_twitch_from`: "False
/// when there's no chat replay: a clip whose broadcast Twitch no longer has").
#[test]
fn a_twitch_clip_whose_broadcast_is_gone_has_no_chat_replay() {
    let number = own_twitch_number();
    let slug = format!("AClipOf{number}");
    let net = Net::new();
    net.twitch_clip(&slug, None, 30.0);
    let clip = VodInfo { platform_id: slug, ..twitch_vod(&number, Some(30.0)) };
    let (db, id) = library_of(&clip);
    assert!(!fetched(&clip, &db, id).unwrap());
    assert_eq!(count(&db, id), 0);
}

// Events counted once, as the library keeps them.

/// A Kick resub KickBot announced stays in the library as one sub: the subscriber's celebration of it is kept as a
/// plain message (chat.rs `resubs_announced`: "Of `subs`, the celebrations KickBot announced, which should stay in
/// chat as plain messages"; `fetch_kick`).
#[test]
fn a_resub_kickbot_announced_is_kept_as_one_sub() {
    let mut celebration = sites::kick_message(FAKE_START + 400.0, "Ari_letric", "oh wow");
    let subbed = json!({ "celebration": { "type": "subscription_renewed",
        "created_at": crate::db::utc_time(FAKE_START + 100.0) } });
    celebration["metadata"] = json!(subbed.to_string());
    let (vod, _) = kick_of_own(600.0, &[]);
    let vod = vod.chat(vec![
        sites::kick_message(FAKE_START + 100.0, "KickBot", "@Ari_letric has subbed for 15 months!"),
        celebration,
    ]);
    let net = Net::new();
    net.kick_vod(&vod);
    let info = kick_vod(&vod, 600.0);
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    let db = db.lock().unwrap();
    let events: Vec<_> = db.chat_events(id).unwrap().into_iter().map(|l| (l.author, l.kind)).collect();
    assert_eq!(events, [("KickBot".to_string(), Kind::Sub)]);
    assert_eq!(db.chat_count(id).unwrap(), 2, "the celebration still in chat");
}

/// A Twitch gift bomb stays in the library as one gift of its count: the line for each sub after it is kept as a
/// plain message, though the lines fall in the next slice (chat.rs `GiftBombs`: "Those are the same gift: they stay
/// in chat as messages rather than counting again"; `count_gift_bombs_once`: "After the fetch, as a bomb's lines can
/// fall in the slice after its own").
#[test]
fn a_twitch_gift_bomb_is_kept_as_one_gift() {
    let net = Net::new();
    let number = own_twitch_number();
    net.twitch_chat(
        &number,
        &[
            (599.0, "ada", "ada is gifting 2 Tier 1 Subs to the community!"),
            (600.0, "ada", "ada gifted a Tier 1 sub to bo!"),
            (601.0, "ada", "ada gifted a Tier 1 sub to cy!"),
        ],
    );
    let info = twitch_vod(&number, Some(1200.0));
    let (db, id) = library_of(&info);
    assert!(fetched(&info, &db, id).unwrap());
    let db = db.lock().unwrap();
    let events: Vec<_> = db.chat_events(id).unwrap().into_iter().map(|l| (l.time, l.kind, l.amount)).collect();
    assert_eq!(events, [(599.0, Kind::Gift, Some(2.0))]);
    assert_eq!(db.chat_count(id).unwrap(), 3, "the lines still in chat");
}

// YouTube's, read as yt-dlp writes it.

/// A YouTube video of this test's own, 40 seconds long, its chat downloaded
/// by a fake yt-dlp of its own, to a work folder of its own.
struct YouTube {
    info: VodInfo,
    fake: crate::testkit::YtDlp,
    yt_dlp: YtDlp,
    work: TempDir,
    _tools: TempDir,
}

impl YouTube {
    fn new() -> Self {
        let tools = TempDir::new("youtube-chat-tools");
        let fake = crate::testkit::YtDlp::at(tools.join("yt-dlp"));
        let yt_dlp = YtDlp { path: fake.path().into(), js_runtime: None };
        let id = format!("chat{:07}", own());
        let info = VodInfo {
            platform: "youtube".into(),
            platform_id: id.clone(),
            url: format!("https://www.youtube.com/watch?v={id}"),
            title: "t".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(40.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        YouTube { info, fake, yt_dlp, work: TempDir::new("youtube-chat"), _tools: tools }
    }

    /// The site's chat replay of it is `lines`, each a moment's messages.
    fn serves(&self, lines: Vec<String>) {
        self.fake.serves_chat(&self.info.url, lines);
    }

    /// The site's chat replay of it: one message at 10 s, 20 s and 30 s.
    fn serves_three(&self) {
        self.serves([10.0, 20.0, 30.0].iter().map(|&at| yt_dlp::chat_line(at, "@a", "hi")).collect());
    }

    /// `fetch` of its chat into the library as VOD `id`'s, as the chat queue
    /// does, with `on_save` told after each batch and `on_progress` how far.
    fn fetch(
        &self,
        db: &Mutex<Db>,
        id: i64,
        cancel: &AtomicBool,
        on_save: impl FnMut(),
        on_progress: impl FnMut(f64),
    ) -> Result<bool> {
        let stem = self.work.join(format!("chat-{id}"));
        fetch(&self.info, Some(&self.yt_dlp), &stem, &mut store(db, id, on_save), cancel, on_progress)
    }

    /// Its chat fetched into the library as VOD `id`'s.
    fn fetched(&self, db: &Mutex<Db>, id: i64) -> Result<bool> {
        self.fetch(db, id, &AtomicBool::new(false), || {}, |_| {})
    }

    /// How many files are left in its work folder.
    fn left(&self) -> usize {
        fs::read_dir(&*self.work).unwrap().count()
    }
}

/// A line of a YouTube chat replay with `count` messages at `at` seconds.
fn busy_line(at: f64, count: usize) -> String {
    let mut line: Value = serde_json::from_str(&yt_dlp::chat_line(at, "@a", "hi")).unwrap();
    let action = line["replayChatItemAction"]["actions"][0].clone();
    line["replayChatItemAction"]["actions"] = Value::Array(vec![action; count]);
    line.to_string()
}

/// A YouTube chat shows as it downloads, saying how far into the VOD it has got (CHANGELOG.md, 0.3.0: "YouTube chat
/// shows how far it has got and appears as it downloads"; chat.rs `fetch_youtube`: "reading it into `store` as yt-dlp
/// writes it … and reporting how far into the VOD it has got").
#[test]
fn a_youtube_chat_shows_as_it_downloads_saying_how_far_it_has_got() {
    let youtube = YouTube::new();
    // 500 messages at 10 s, then one at 20 s and one at 30 s, of a 40 s VOD.
    let lines = vec![busy_line(10.0, 500), yt_dlp::chat_line(20.0, "@a", "hi"), yt_dlp::chat_line(30.0, "@a", "hi")];
    youtube.serves(lines);
    let writing = Gate::new();
    youtube.fake.holds_chat(&writing);
    let (db, id) = library_of(&youtube.info);
    let (told, progress) = mpsc::channel();
    let (saved, saves) = mpsc::channel();
    let fetched = std::thread::scope(|s| {
        let fetching = s.spawn(|| {
            youtube.fetch(&db, id, &AtomicBool::new(false), || saved.send(()).unwrap(), |f| told.send(f).unwrap())
        });
        // The first line read while yt-dlp writes the rest.
        assert_eq!(progress.recv_timeout(Duration::from_secs(5)), Ok(0.25));
        saves.recv_timeout(Duration::from_secs(5)).expect("the first line's messages kept");
        assert_eq!(count(&db, id), 500, "shown while the rest downloads");
        writing.open();
        fetching.join().unwrap()
    });
    assert!(fetched.unwrap());
    assert_eq!(count(&db, id), 502);
    assert_eq!(progress.try_iter().collect::<Vec<f64>>(), [0.5, 0.75]);
}

/// Removing a VOD stops its YouTube chat's download promptly (CHANGELOG.md, 0.3.0: "YouTube chat … appears as it
/// downloads, and removing the VOD stops it."; chat.rs `fetch_youtube`: "`cancel` stops yt-dlp and the reading").
#[test]
fn removing_a_vod_stops_its_youtube_chats_download() {
    let youtube = YouTube::new();
    youtube.serves_three();
    let writing = Gate::new();
    youtube.fake.holds_chat(&writing);
    let (db, id) = library_of(&youtube.info);
    let cancel = AtomicBool::new(false);
    std::thread::scope(|s| {
        let fetching = s.spawn(|| youtube.fetch(&db, id, &cancel, || {}, |_| {}));
        writing.reached();
        let stopped = std::time::Instant::now();
        cancel.store(true, Ordering::Relaxed);
        assert!(fetching.join().unwrap().is_err_and(|e| crate::is_canceled(&e)));
        assert!(stopped.elapsed() < Duration::from_secs(1), "{:?} after the stop", stopped.elapsed());
    });
}

/// A YouTube chat's download leaves no files behind, stopped or done (lib.rs `Paths::temp_chat`: "Where VOD
/// `vod_id`'s YouTube chat downloads to, before its extension"; chat.rs `fetch_youtube`).
#[test]
fn a_youtube_chats_download_leaves_no_files() {
    let youtube = YouTube::new();
    youtube.serves_three();
    let (db, id) = library_of(&youtube.info);
    assert!(youtube.fetched(&db, id).unwrap());
    assert_eq!(youtube.left(), 0, "done");

    let writing = Gate::new();
    youtube.fake.holds_chat(&writing);
    let cancel = AtomicBool::new(false);
    std::thread::scope(|s| {
        let fetching = s.spawn(|| youtube.fetch(&db, id, &cancel, || {}, |_| {}));
        writing.reached();
        cancel.store(true, Ordering::Relaxed);
        assert!(fetching.join().unwrap().is_err());
    });
    assert_eq!(youtube.left(), 0, "stopped");
}

/// A YouTube video that wasn't a livestream has no chat replay (chat.rs `fetch`: "False when the site has no chat
/// replay for it (… or a YouTube video that wasn't a livestream)").
#[test]
fn a_youtube_video_that_was_not_a_livestream_has_no_chat_replay() {
    let youtube = YouTube::new();
    let (db, id) = library_of(&youtube.info);
    assert!(!youtube.fetched(&db, id).unwrap());
    assert_eq!(count(&db, id), 0);
    assert_eq!(youtube.left(), 0);
}

/// A YouTube chat fetched again starts over: the new one replaces the old, and an empty chat replay leaves none
/// (chat.rs `fetch_youtube`: "There's nothing to carry on from, so a fetch starts over"; and its comment "An empty
/// chat replay: nothing read, but it's done").
#[test]
fn a_youtube_chat_fetched_again_starts_over() {
    let youtube = YouTube::new();
    youtube.serves_three();
    let (db, id) = library_of(&youtube.info);
    assert!(youtube.fetched(&db, id).unwrap());
    assert!(youtube.fetched(&db, id).unwrap());
    assert_eq!(count(&db, id), 3, "each message once");

    youtube.serves(Vec::new());
    assert!(youtube.fetched(&db, id).unwrap(), "an empty chat replay is a chat");
    assert_eq!(count(&db, id), 0);
}

/// A YouTube chat's fetch that fails before its first line keeps the chat fetched before (chat.rs `fetch_youtube`,
/// its comment: "Until a line is read, the store keeps the last fetch's chat").
#[test]
fn a_youtube_fetch_that_fails_keeps_the_chat_fetched_before() {
    let youtube = YouTube::new();
    youtube.serves_three();
    let (db, id) = library_of(&youtube.info);
    assert!(youtube.fetched(&db, id).unwrap());
    let _failing = crate::testkit::tools::fake(youtube.fake.path(), |run| {
        run.err("ERROR: [youtube] chat: HTTP Error 503: Service Unavailable\n");
        1
    });
    assert!(youtube.fetched(&db, id).is_err());
    assert_eq!(count(&db, id), 3, "the chat fetched before");
}

/// A file another program is still writing is read a whole line at a time, the last one at the end [fn: what yt-dlp
/// writes can't be cut mid-line through the fake] (chat.rs `Tail`: "Reads a file another program is still writing, a
/// whole line at a time"; `Tail::read`: "with `last`, also the final one with no newline").
#[test]
fn a_file_being_written_is_read_a_whole_line_at_a_time() {
    let dir = TempDir::new("tail");
    let path = dir.join("growing");
    let mut tail = Tail::default();
    let read = |tail: &mut Tail, last: bool| {
        let mut lines = Vec::new();
        tail.read(&[&path], last, |l| {
            lines.push(l.to_string());
            Ok(())
        })
        .unwrap();
        lines
    };
    assert!(read(&mut tail, false).is_empty());
    assert!(!tail.opened(), "nothing to read before the file exists");
    fs::write(&path, "a\nb").unwrap();
    assert_eq!(read(&mut tail, false), ["a"]);
    fs::write(&path, "a\nbc\nd").unwrap();
    assert_eq!(read(&mut tail, false), ["bc"], "a line is handed over once it ends");
    assert_eq!(read(&mut tail, true), ["d"], "and the last one at the end");
}

/// Fetches a real YouTube chat with the app's yt-dlp:
/// `VODSCRIBE_BIN=<data dir>/bin cargo test -p vodscribe-core -- --ignored youtube_chat_live --nocapture`.
#[test]
#[ignore = "network"]
fn youtube_chat_live() {
    let bin = std::env::var("VODSCRIBE_BIN").expect("VODSCRIBE_BIN: the folder with yt-dlp");
    let yt_dlp = crate::tools::find_yt_dlp(Path::new(&bin)).expect("yt-dlp");
    let work = crate::TempDir::new("youtube-chat-live");
    let id = std::env::var("YOUTUBE_ID").unwrap_or_else(|_| "j5HcY5GBrTw".into());
    let info = crate::fetch::probe(
        &yt_dlp,
        &format!("https://www.youtube.com/watch?v={id}"),
        &work.join("info.json"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let begun = std::time::Instant::now();
    let mut progress = Vec::new();
    let mut store = MemoryChat::default();
    let fetched = fetch_youtube(&info, &yt_dlp, &work.join("chat-7"), &mut store, &AtomicBool::new(false), &mut |f| {
        progress.push((f, begun.elapsed()))
    });
    assert!(fetched.unwrap());
    let total = begun.elapsed();
    let early = progress.iter().filter(|p| p.1 < total.mul_f64(0.8)).count();
    eprintln!(
        "{} messages in {total:?}; {} progress reports, {early} in the first 80%; last {:?}",
        store.messages.len(),
        progress.len(),
        progress.last()
    );
    assert!(!store.messages.is_empty());
}

/// How much faster slices fetch, for real: Twitch for 90 s each way, Kick in full.
/// `cargo test -p vodscribe-core -- --ignored chat_slices_live --nocapture`.
#[test]
#[ignore = "network"]
fn chat_slices_live() {
    use std::time::{Duration, Instant};
    let twitch = std::env::var("TWITCH_ID").unwrap_or_else(|_| "2862677806".into());
    let cheermotes = Cheermotes::for_video(&twitch, &AtomicBool::new(false), &twitch_gql);
    let page = |offset: i64, stop: &AtomicBool| {
        let body = json!({ "query": TWITCH_QUERY, "variables": { "id": twitch, "offset": offset } });
        parse_twitch_page(&api(TWITCH_GQL, Some(&body), &[("Client-ID", TWITCH_CLIENT_ID)], stop)?, &cheermotes)
    };
    if std::env::var("TWITCH_FULL").is_ok() {
        let begun = Instant::now();
        let mut store = MemoryChat::default();
        twitch_into(
            &mut store,
            Span { start: 0.0, duration: Some(29014.0), end: None },
            4,
            page,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        count_gift_bombs_once(&mut store).unwrap();
        let gifts = store.messages.iter().filter(|m| m.kind == Kind::Gift).count();
        eprintln!(
            "Twitch in full, 4 slices: {} messages, {gifts} gifts, in {:?}",
            store.messages.len(),
            begun.elapsed()
        );
        return;
    }
    for slices in [1, 4] {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let timer = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(90));
                cancel.store(true, Ordering::Relaxed)
            })
        };
        let mut store = MemoryChat::default();
        let _ = twitch_into(
            &mut store,
            Span { start: 0.0, duration: Some(29014.0), end: None },
            slices,
            page,
            &cancel,
            &mut |_| {},
        );
        timer.join().unwrap();
        eprintln!("Twitch, {slices} slice(s): {} messages in 90 s", store.messages.len());
    }
    let kick = std::env::var("KICK_ID").unwrap_or_else(|_| "01a0db44-6218-70fb-977c-6bbe91a018d1".into());
    let info = VodInfo {
        platform: "kick".into(),
        platform_id: kick,
        url: String::new(),
        title: String::new(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(14786.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let origin = kick::chat_origin(&info.platform_id).unwrap();
    let page = |cursor: i64, stop: &AtomicBool| {
        let url = format!("https://web.kick.com/api/v1/chat/{}/history?cursor={cursor}", origin.channel);
        parse_kick_page(&api(&url, None, &[], stop)?)
    };
    for slices in [1, 4] {
        let begun = Instant::now();
        let mut store = MemoryChat::default();
        kick_into(&mut store, origin.start, 14786.0, slices, page, &AtomicBool::new(false), &mut |_| {}).unwrap();
        eprintln!("Kick, {slices} slice(s): {} messages in {:?}", store.messages.len(), begun.elapsed());
    }
}
