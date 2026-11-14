//! Fetching a chat: slices, carrying on where a fetch stopped, pages asked again, YouTube's file read as
//! yt-dlp writes it. Twitch's and Kick's come from their APIs as the kit's network answers them (`TwitchChat`,
//! `KickVod`), YouTube's from the kit's yt-dlp.

use super::*;
use crate::testkit::sites::{self, TwitchChat};
use crate::testkit::{Gate, Net, yt_dlp};

/// When the fake chats' VOD started (Unix seconds).
const FAKE_START: f64 = 1790631000.0;

/// A message `n` of a fake chat, `per_second` to a second, sent some
/// time within its second (Twitch's offsets are whole seconds).
fn fake(n: usize, per_second: usize) -> (String, ChatMessage) {
    let text = format!("message {n}");
    let time = (n / per_second) as f64;
    let msg = ChatMessage {
        time,
        author: "v".into(),
        color: None,
        parts: vec![ChatPart::Text { text }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: Some(FAKE_START + time + (n % per_second) as f64 / per_second as f64),
    };
    (n.to_string(), msg)
}

/// The store's messages, by their text, in time order.
fn saved(store: &MemoryChat) -> Vec<String> {
    let mut m = store.messages.clone();
    m.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.text().len().cmp(&b.text().len())).then(a.text().cmp(&b.text())));
    m.iter().map(|m| m.text()).collect()
}

/// Every message of `all`, by its text, in time order.
fn all_of(all: &[(String, ChatMessage)]) -> Vec<String> {
    saved(&MemoryChat { messages: all.iter().map(|(_, m)| m.clone()).collect(), positions: None })
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

/// `fetch` of Twitch or Kick VOD `info`'s chat into `store`, with how far it got each time it said.
fn fetched_with_progress(info: &VodInfo, store: &mut MemoryChat) -> (Result<bool>, Vec<f64>) {
    let mut progress = Vec::new();
    let result = fetch(info, None, Path::new("no-yt-dlp-chat"), store, &AtomicBool::new(false), |f| progress.push(f));
    (result, progress)
}

/// A Twitch chat's fetch that stopped partway, the network dropping, carries on from where each slice got to and ends
/// with every message once (chat.rs `ChatStore`: "so one that's interrupted (quitting, a network error) carries on from
/// there"); a clip's is its broadcast's over its stretch, at the clip's times (chat.rs `fetch_twitch_from`: "A clip's
/// chat is its broadcast's over the clip's stretch, at the clip's times"); and a VOD's whose length Twitch didn't say
/// is read to its end.
#[test]
fn a_twitch_fetch_carries_on_where_it_stopped() {
    // 3,000 messages, 7 a second, fetched in one slice; 2,400, one a second, in four.
    let seven_a_second: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
    let one_a_second: Vec<_> = (0..2400).map(|n| fake(n, 1)).collect();
    for (slices, duration, all) in [(1, 430.0, &seven_a_second), (4, 2400.0, &one_a_second)] {
        assert_eq!(slices_for(duration), slices);
        let number = own_twitch_number();
        let info = twitch_vod(&number, Some(duration));
        // Fails partway (the network drops), after some batches were saved.
        let mut store = MemoryChat::default();
        let net = Net::new();
        net.twitch_comments(&number, &TwitchChat::of(comments(all)).failing(|n| n >= 20));
        assert!(fetch_from_site(&info, &mut store, &AtomicBool::new(false)).is_err(), "{slices} slices");
        drop(net);
        let partial = store.messages.len();
        assert!((1000..all.len()).contains(&partial), "{slices} slices: saved {partial} before failing");
        assert_eq!(store.positions.as_ref().map(Vec::len), Some(slices));

        // Carrying on ends with every message, once.
        let net = Net::new();
        net.twitch_comments(&number, &TwitchChat::of(comments(all)));
        let (fetched, progress) = fetched_with_progress(&info, &mut store);
        assert!(fetched.unwrap());
        assert_eq!(saved(&store), all_of(all), "{slices} slices");
        assert_eq!(progress.last(), Some(&1.0));
    }

    // A clip: 30 s from 100 s in, at the clip's own times.
    let all = &seven_a_second;
    let number = own_twitch_number();
    let slug = format!("AClipOf{number}");
    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(all))).twitch_clip(&slug, Some((&number, 100.0)), 30.0);
    let mut store = MemoryChat::default();
    let clip = VodInfo { platform_id: slug, ..twitch_vod(&number, Some(30.0)) };
    assert!(fetch_from_site(&clip, &mut store, &AtomicBool::new(false)).unwrap());
    let in_clip: Vec<ChatMessage> = all
        .iter()
        .filter(|(_, m)| (100.0..130.0).contains(&m.time))
        .map(|(_, m)| ChatMessage { time: m.time - 100.0, ..m.clone() })
        .collect();
    assert_eq!(saved(&store), saved(&MemoryChat { messages: in_clip, positions: None }));
    assert!(store.messages.iter().all(|m| (0.0..30.0).contains(&m.time)));

    // A VOD whose length Twitch didn't say: one slice, read to its end.
    let mut store = MemoryChat::default();
    assert!(fetch_from_site(&twitch_vod(&number, None), &mut store, &AtomicBool::new(false)).unwrap());
    assert_eq!(saved(&store), all_of(all), "every message");
}

/// A Twitch page that fails for a moment is asked again, up to five times, and then the fetch stops with what it had
/// (chat.rs `PAGE_RETRIES`: "five tries over about half a minute, then the fetch stops (and keeps what it saved)").
#[test]
fn a_twitch_page_that_fails_for_a_moment_is_asked_again() {
    let all: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
    let number = own_twitch_number();
    let info = twitch_vod(&number, Some(430.0));

    // Page, "service error", page: the error is asked again, and the fetch ends whole.
    let mut store = MemoryChat::default();
    let net = Net::new();
    let chat = TwitchChat::of(comments(&all)).ending_at(430.0).failing(|n| n == 1);
    net.twitch_comments(&number, &chat);
    assert!(fetch_from_site(&info, &mut store, &AtomicBool::new(false)).unwrap());
    drop(net);
    assert_eq!(saved(&store), all_of(&all));
    assert!(chat.pages_asked() > 30, "paged through");
    // Each message sent within its second, given to the second: the VOD started then.
    let starts: Vec<f64> = store.messages.iter().filter_map(ChatMessage::start).collect();
    assert_eq!(starts.len(), 3000);
    let start = vod_start(&starts).unwrap();
    assert!((FAKE_START..=FAKE_START + 1.0).contains(&start), "{start}");

    // Twitch keeps refusing from the 11th page on: five tries, then the
    // fetch stops with what it had, and carries on from there once Twitch answers.
    let mut store = MemoryChat::default();
    let net = Net::new();
    let chat = TwitchChat::of(comments(&all)).ending_at(430.0).failing(|n| n >= 10);
    net.twitch_comments(&number, &chat);
    let e = fetch_from_site(&info, &mut store, &AtomicBool::new(false)).unwrap_err();
    drop(net);
    assert_eq!(format!("{e:#}"), "Twitch wouldn't give the chat: service error");
    assert_eq!(chat.pages_asked(), 10 + PAGE_RETRIES.len() + 1, "asked five times");
    let partial = store.messages.len();
    assert!((900..3000).contains(&partial), "saved {partial} before it stopped");
    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)).ending_at(430.0));
    assert!(fetch_from_site(&info, &mut store, &AtomicBool::new(false)).unwrap());
    assert_eq!(saved(&store), all_of(&all), "every message, once");
}

/// A Twitch fetch ends at the VOD's end, though Twitch's pages say there are more and refuse to give them (chat.rs
/// `twitch_into`: "the last one runs on to the span's end, or past the VOD's").
#[test]
fn a_twitch_fetch_stops_at_the_vods_end() {
    // 3,024 messages, 7 a second, to 431 s: the chat runs on past the
    // VOD's end, at 418 s.
    let all: Vec<_> = (0..3024).map(|n| fake(n, 7)).collect();
    // The messages before `end` saved, and all of them.
    let before = |store: &MemoryChat, end: f64| {
        let of = |m: &[ChatMessage]| {
            saved(&MemoryChat { messages: m.iter().filter(|m| m.time < end).cloned().collect(), positions: None })
        };
        (of(&store.messages), of(&all.iter().map(|(_, m)| m.clone()).collect::<Vec<_>>()))
    };

    // The page from 406 s ends at 420 s and says there's more, which
    // Twitch refuses to give.
    let number = own_twitch_number();
    let net = Net::new();
    net.twitch_comments(&number, &TwitchChat::of(comments(&all)).ending_at(418.0));
    let mut store = MemoryChat::default();
    assert!(fetch_from_site(&twitch_vod(&number, Some(418.0)), &mut store, &AtomicBool::new(false)).unwrap());
    let (got, all_before) = before(&store, 418.0);
    assert_eq!(got, all_before, "every message before the end");

    // Every page saying there's more, even the empty ones after the chat's
    // last message: still the end.
    for length in [418.0, 440.0] {
        let number = own_twitch_number();
        net.twitch_comments(&number, &TwitchChat::of(comments(&all)).ending_at(length).always_saying_more());
        let mut store = MemoryChat::default();
        assert!(fetch_from_site(&twitch_vod(&number, Some(length)), &mut store, &AtomicBool::new(false)).unwrap());
        let (got, all_before) = before(&store, length);
        assert_eq!(got, all_before, "{length} s");
        // Of unknown length, with the chat ending before the VOD: up to the first empty page.
        if length > 431.0 {
            let mut store = MemoryChat::default();
            assert!(fetch_from_site(&twitch_vod(&number, None), &mut store, &AtomicBool::new(false)).unwrap());
            let (got, all_before) = before(&store, length);
            assert_eq!(got, all_before, "unknown length");
        }
    }
}

/// A Kick chat's fetch that stopped partway carries on from where each slice got to and ends with every message once
/// (chat.rs `ChatStore`: "so one that's interrupted (quitting, a network error) carries on from there"), each saying
/// when the VOD started.
#[test]
fn a_kick_fetch_carries_on_where_it_stopped() {
    // 3,000 messages paged back 25 at a time by cursor, as Kick serves them: over 500 s, fetched in one slice, and over
    // 2,400 s, in four. Kick's cursors are to the microsecond, so no two messages share one.
    for (slices, duration) in [(1, 500.0), (4, 2400.0)] {
        assert_eq!(slices_for(duration), slices);
        let all: Vec<_> = (0..3000)
            .map(|n| {
                let (id, m) = fake(n, 1);
                (id, ChatMessage { time: n as f64 * duration / 3000.0, ..m })
            })
            .collect();
        let history: Vec<Value> = all
            .iter()
            .map(|(id, m)| {
                let mut said = sites::kick_message(FAKE_START + m.time, &m.author, &m.text());
                said["id"] = json!(id);
                said
            })
            .collect();
        let vod = own_kick_vod(duration).started(FAKE_START).chat(history);
        let info = kick_vod(&vod, duration);

        let mut store = MemoryChat::default();
        let net = Net::new();
        net.kick_vod(&vod.clone().failing(|n| n >= 60));
        assert!(fetch_from_site(&info, &mut store, &AtomicBool::new(false)).is_err(), "{slices} slices");
        drop(net);
        assert!(!store.messages.is_empty() && store.messages.len() < 3000, "{slices} slices");
        assert_eq!(store.positions.as_ref().map(Vec::len), Some(slices));

        let net = Net::new();
        net.kick_vod(&vod);
        assert!(fetch_from_site(&info, &mut store, &AtomicBool::new(false)).unwrap());
        assert_eq!(saved(&store), all_of(&all), "{slices} slices");
        // Each message's time sent is Kick's, so each says when the VOD started.
        assert!(store.messages.iter().all(|m| m.start().is_some_and(|s| (s - FAKE_START).abs() < 1e-5)));
    }
}

#[test]
fn tail_reads_whole_lines_as_they_come() {
    let dir = crate::TempDir::new("tail");
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

/// A YouTube chat's fetch stopped while yt-dlp writes it ends promptly, and leaves no files behind; left to finish,
/// the chat is read as it's written, with how far it has got, and its file removed (chat.rs `fetch_youtube`: "reading
/// it into `store` as yt-dlp writes it … and reporting how far into the VOD it has got", "`cancel` stops yt-dlp and the
/// reading").
#[test]
fn youtube_chat_stops_and_fetches() {
    let tools = crate::TempDir::new("youtube-chat-tools");
    let work = crate::TempDir::new("youtube-chat");
    let fake = crate::testkit::YtDlp::at(tools.join("yt-dlp"));
    let yt_dlp = YtDlp { path: fake.path().into(), js_runtime: None };
    let info = VodInfo {
        platform: "youtube".into(),
        platform_id: "x".into(),
        url: "https://www.youtube.com/watch?v=x".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(40.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    // One message at 10 s, 20 s and 30 s.
    fake.serves_chat(&info.url, [10.0, 20.0, 30.0].iter().map(|&at| yt_dlp::chat_line(at, "@a", "hi")).collect());
    let stem = work.join("chat-7");

    // Stopped while yt-dlp writes it: it ends promptly, and leaves no files behind.
    let writing = Gate::new();
    fake.holds_chat(&writing);
    let cancel = AtomicBool::new(false);
    std::thread::scope(|s| {
        let fetching = s.spawn(|| fetch(&info, Some(&yt_dlp), &stem, &mut MemoryChat::default(), &cancel, |_| {}));
        writing.reached();
        let stopped = std::time::Instant::now();
        cancel.store(true, Ordering::Relaxed);
        assert!(fetching.join().unwrap().is_err_and(|e| crate::is_canceled(&e)));
        assert!(stopped.elapsed() < std::time::Duration::from_secs(1), "{:?} after the stop", stopped.elapsed());
    });
    assert_eq!(fs::read_dir(&*work).unwrap().count(), 0, "no files left");

    // Left to finish: the chat is read as it's written, with how far it has
    // got (of a 40 s VOD), and its file removed.
    let writing = Gate::new();
    fake.holds_chat(&writing);
    let mut store = MemoryChat::default();
    let (told, progress) = std::sync::mpsc::channel();
    let fetched = std::thread::scope(|s| {
        let fetching = s.spawn(|| {
            fetch(&info, Some(&yt_dlp), &stem, &mut store, &AtomicBool::new(false), |f| told.send(f).unwrap())
        });
        let first = progress.recv_timeout(std::time::Duration::from_secs(5));
        assert_eq!(first, Ok(0.25), "the first line read while yt-dlp writes the rest");
        writing.open();
        fetching.join().unwrap()
    });
    assert!(fetched.unwrap());
    assert_eq!(store.messages.len(), 3);
    assert_eq!(progress.try_iter().collect::<Vec<f64>>(), [0.5, 0.75]);
    assert_eq!(fs::read_dir(&*work).unwrap().count(), 0, "no files left");
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
