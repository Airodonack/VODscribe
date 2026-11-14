//! Fetching a chat: slices, carrying on where a fetch stopped, pages asked again, YouTube's file read as
//! yt-dlp writes it.

use super::*;
use std::sync::atomic::AtomicUsize;

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

#[test]
fn a_twitch_fetch_carries_on_where_it_stopped() {
    // 3,000 messages, 7 a second; pages of 100 from an offset, as Twitch serves them.
    let all: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
    let page = |offset: i64, _: &AtomicBool| {
        let from: Vec<_> = all.iter().filter(|(_, m)| m.time as i64 >= offset).cloned().collect();
        let more = from.len() > 100;
        Ok((from.into_iter().take(100).collect::<Vec<_>>(), more))
    };
    let expected = saved(&MemoryChat { messages: all.iter().map(|(_, m)| m.clone()).collect(), positions: None });
    let no = AtomicBool::new(false);

    for slices in [1, 4] {
        // Fails partway (the network drops), after some batches were saved.
        let mut store = MemoryChat::default();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let failing = |offset: i64, stop: &AtomicBool| {
            if calls.fetch_add(1, Ordering::Relaxed) >= 20 { anyhow::bail!("network down") } else { page(offset, stop) }
        };
        assert!(
            twitch_into(
                &mut store,
                Span { start: 0.0, duration: Some(430.0), end: None },
                slices,
                failing,
                &no,
                &mut |_| {}
            )
            .is_err()
        );
        let partial = store.messages.len();
        assert!((1000..3000).contains(&partial), "{slices} slices: saved {partial} before failing");
        assert_eq!(store.positions.as_ref().map(Vec::len), Some(slices));

        // Carrying on ends with every message, once.
        let mut progress = Vec::new();
        twitch_into(&mut store, Span { start: 0.0, duration: Some(430.0), end: None }, slices, page, &no, &mut |f| {
            progress.push(f)
        })
        .unwrap();
        assert_eq!(saved(&store), expected, "{slices} slices");
        assert_eq!(progress.last(), Some(&1.0));
    }

    // A clip: 30 s from 100 s in, at the clip's own times.
    let mut store = MemoryChat::default();
    let clip = Span { start: 100.0, duration: Some(30.0), end: Some(130.0) };
    twitch_into(&mut store, clip, 1, page, &no, &mut |_| {}).unwrap();
    let in_clip: Vec<ChatMessage> = all
        .iter()
        .filter(|(_, m)| (100.0..130.0).contains(&m.time))
        .map(|(_, m)| ChatMessage { time: m.time - 100.0, ..m.clone() })
        .collect();
    assert_eq!(saved(&store), saved(&MemoryChat { messages: in_clip, positions: None }));
    assert!(store.messages.iter().all(|m| (0.0..30.0).contains(&m.time)));

    // A VOD whose length Twitch didn't say: one slice, read to its end.
    let mut store = MemoryChat::default();
    let unknown = Span { start: 0.0, duration: None, end: None };
    twitch_into(&mut store, unknown, 1, page, &no, &mut |_| {}).unwrap();
    assert_eq!(saved(&store), expected, "every message");
}

/// What Twitch answers for a comments page it won't give, as it did on
/// 2026-10-24 for the test VOD (67 s long) from 69 s on.
fn service_error() -> Value {
    json!({ "errors": [{ "message": "service error", "path": ["video", "comments"] }],
        "data": { "video": { "comments": null } },
        "extensions": { "durationMilliseconds": 41, "requestID": "01M5Q9JGMYCF91S411PH3FN0ES" } })
}

/// Twitch's GraphQL API serving `all` in pages of 100 from an offset, as
/// it does, for a VOD `length` seconds long, with `fail(n)` saying whether
/// its nth comments page (from 0) is answered with an error instead, as
/// Twitch answers now and then. Pages from more than a second past the
/// end are always refused, as Twitch's are. Counts the comments pages
/// asked for in `asked`.
fn twitch_answering<'a>(
    all: &'a [(String, ChatMessage)],
    length: f64,
    asked: &'a AtomicUsize,
    fail: impl Fn(usize) -> bool + Sync + 'a,
) -> impl Fn(&Value, &AtomicBool) -> Result<Value> + Sync + 'a {
    move |body: &Value, _: &AtomicBool| {
        // The cheermotes' list: none beyond the built-in ones.
        let Some(offset) = body.pointer("/variables/offset").and_then(Value::as_i64) else {
            return Ok(json!({ "data": {} }));
        };
        if fail(asked.fetch_add(1, Ordering::Relaxed)) || offset as f64 > length + 1.0 {
            return Ok(service_error());
        }
        let from: Vec<_> = all.iter().filter(|(_, m)| m.time as i64 >= offset).collect();
        let edges: Vec<Value> = from
            .iter()
            .take(100)
            .map(|(id, m)| {
                json!({ "node": { "id": id, "contentOffsetSeconds": m.time, "createdAt": m.at.map(crate::db::utc_time),
                    "commenter": { "displayName": m.author },
                    "message": { "userColor": null, "fragments": [{ "text": m.text(), "emote": null }] } } })
            })
            .collect();
        Ok(json!({ "data": { "video": { "comments": {
            "edges": edges, "pageInfo": { "hasNextPage": from.len() > 100 } } } } }))
    }
}

#[test]
fn a_twitch_page_that_fails_for_a_moment_is_asked_again() {
    let all: Vec<_> = (0..3000).map(|n| fake(n, 7)).collect();
    let expected = saved(&MemoryChat { messages: all.iter().map(|(_, m)| m.clone()).collect(), positions: None });
    let info = VodInfo {
        platform: "twitch".into(),
        platform_id: "2901646294".into(),
        url: String::new(),
        title: String::new(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(430.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let no = AtomicBool::new(false);

    // Page, "service error", page: the error is asked again, and the fetch ends whole.
    let mut store = MemoryChat::default();
    let asked = AtomicUsize::new(0);
    let ask = twitch_answering(&all, 430.0, &asked, |n| n == 1);
    assert!(fetch_twitch_from(&info, &mut store, &no, &mut |_| {}, &ask).unwrap());
    assert_eq!(saved(&store), expected);
    assert!(asked.load(Ordering::Relaxed) > 30, "paged through");
    // Each message sent within its second, given to the second: the VOD started then.
    let starts: Vec<f64> = store.messages.iter().filter_map(ChatMessage::start).collect();
    assert_eq!(starts.len(), 3000);
    let start = vod_start(&starts).unwrap();
    assert!((FAKE_START..=FAKE_START + 1.0).contains(&start), "{start}");

    // Twitch keeps refusing from the 11th page on: five tries, then the
    // fetch stops with what it had, and carries on from there once Twitch answers.
    let mut store = MemoryChat::default();
    let asked = AtomicUsize::new(0);
    let ask = twitch_answering(&all, 430.0, &asked, |n| n >= 10);
    let e = fetch_twitch_from(&info, &mut store, &no, &mut |_| {}, &ask).unwrap_err();
    assert_eq!(format!("{e:#}"), "Twitch wouldn't give the chat: service error");
    assert_eq!(asked.load(Ordering::Relaxed), 10 + PAGE_RETRIES.len() + 1, "asked five times");
    let partial = store.messages.len();
    assert!((900..3000).contains(&partial), "saved {partial} before it stopped");
    let ask = twitch_answering(&all, 430.0, &asked, |_| false);
    assert!(fetch_twitch_from(&info, &mut store, &no, &mut |_| {}, &ask).unwrap());
    assert_eq!(saved(&store), expected, "every message, once");
}

#[test]
fn a_twitch_fetch_stops_at_the_vods_end() {
    // 3,024 messages, 7 a second, to 431 s: the chat runs on past the
    // VOD's end, at 418 s.
    let all: Vec<_> = (0..3024).map(|n| fake(n, 7)).collect();
    let info = VodInfo {
        platform: "twitch".into(),
        platform_id: "2901646294".into(),
        url: String::new(),
        title: String::new(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(418.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let no = AtomicBool::new(false);
    // The messages before `end` saved, and all of them.
    let before = |store: &MemoryChat, end: f64| {
        let of = |m: &[ChatMessage]| {
            saved(&MemoryChat { messages: m.iter().filter(|m| m.time < end).cloned().collect(), positions: None })
        };
        (of(&store.messages), of(&all.iter().map(|(_, m)| m.clone()).collect::<Vec<_>>()))
    };

    // The page from 406 s ends at 420 s and says there's more, which
    // Twitch refuses to give.
    let mut store = MemoryChat::default();
    let asked = AtomicUsize::new(0);
    let ask = twitch_answering(&all, 418.0, &asked, |_| false);
    assert!(fetch_twitch_from(&info, &mut store, &no, &mut |_| {}, &ask).unwrap());
    let (got, all_before) = before(&store, 418.0);
    assert_eq!(got, all_before, "every message before the end");

    // Every page saying there's more, even the empty ones after the chat's
    // last message: still the end.
    for length in [418.0, 440.0] {
        let mut store = MemoryChat::default();
        let asked = AtomicUsize::new(0);
        let honest = twitch_answering(&all, length, &asked, |_| false);
        let ask = |body: &Value, stop: &AtomicBool| {
            let mut page = honest(body, stop)?;
            if let Some(more) = page.pointer_mut("/data/video/comments/pageInfo/hasNextPage") {
                *more = json!(true);
            }
            Ok(page)
        };
        let info = VodInfo { duration: Some(length), ..info.clone() };
        assert!(fetch_twitch_from(&info, &mut store, &no, &mut |_| {}, &ask).unwrap(), "{length} s");
        let (got, all_before) = before(&store, length);
        assert_eq!(got, all_before, "{length} s");
        // Of unknown length, with the chat ending before the VOD: up to the first empty page.
        if length > 431.0 {
            let info = VodInfo { duration: None, ..info.clone() };
            let mut store = MemoryChat::default();
            assert!(fetch_twitch_from(&info, &mut store, &no, &mut |_| {}, &ask).unwrap(), "unknown length");
            let (got, all_before) = before(&store, length);
            assert_eq!(got, all_before, "unknown length");
        }
    }
}

#[test]
fn a_kick_fetch_carries_on_where_it_stopped() {
    // 3,000 messages over 500 s from Unix time 1000, paged back 25 at a time by cursor, as Kick serves them.
    let start = 1000.0;
    // Kick's cursors are to the microsecond, so no two messages share one.
    let all: Vec<_> = (0..3000)
        .map(|n| {
            let (id, m) = fake(n, 1);
            (id, ChatMessage { time: n as f64 / 6.0, ..m })
        })
        .collect();
    let page = |cursor: i64, _: &AtomicBool| {
        let mut older: Vec<_> = all.iter().filter(|(_, m)| ((start + m.time) * 1e6) < cursor as f64).cloned().collect();
        older.sort_by(|a, b| {
            b.1.time.total_cmp(&a.1.time).then(b.0.parse::<usize>().unwrap().cmp(&a.0.parse().unwrap()))
        });
        let page: Vec<_> = older.into_iter().take(25).collect();
        let next = page.last().map(|(_, m)| ((start + m.time) * 1e6) as i64);
        let rows = page.into_iter().map(|(id, m)| (start + m.time, id, ChatMessage { time: 0.0, ..m }, None)).collect();
        Ok((rows, next))
    };
    let expected = saved(&MemoryChat { messages: all.iter().map(|(_, m)| m.clone()).collect(), positions: None });
    let no = AtomicBool::new(false);

    for slices in [1, 4] {
        let mut store = MemoryChat::default();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let failing = |cursor: i64, stop: &AtomicBool| {
            if calls.fetch_add(1, Ordering::Relaxed) >= 60 { anyhow::bail!("network down") } else { page(cursor, stop) }
        };
        assert!(kick_into(&mut store, start, 500.0, slices, failing, &no, &mut |_| {}).is_err());
        assert!(!store.messages.is_empty() && store.messages.len() < 3000, "{slices} slices");
        assert_eq!(store.positions.as_ref().map(Vec::len), Some(slices));

        kick_into(&mut store, start, 500.0, slices, page, &no, &mut |_| {}).unwrap();
        assert_eq!(saved(&store), expected, "{slices} slices");
        // Each message's time sent is Kick's, so each says when the VOD started.
        assert!(store.messages.iter().all(|m| m.start().is_some_and(|s| (s - start).abs() < 1e-6)));
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

/// A stand-in yt-dlp that writes its chat file as yt-dlp does: to a `.part`,
/// a line at a time (one message at 10 s, 20 s, …), `step` seconds apart,
/// then renamed.
#[cfg(unix)]
fn fake_yt_dlp(dir: &Path, lines: u32, step: f64) -> YtDlp {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(format!("yt-dlp-{lines}"));
    let mut script =
        "#!/bin/sh\nwhile [ \"$1\" != -o ]; do shift; done\nf=\"$2.live_chat.json\"\n: > \"$f.part\"\n".to_string();
    for i in 1..=lines {
        let line = format!(
            r#"{{"replayChatItemAction":{{"videoOffsetTimeMsec":"{}","actions":[{{"addChatItemAction":{{"item":{{"liveChatTextMessageRenderer":{{"authorName":{{"simpleText":"@a"}},"message":{{"runs":[{{"text":"hi"}}]}}}}}}}}}}]}}}}"#,
            i * 10_000
        );
        script += &format!("echo '{line}' >> \"$f.part\"\nsleep {step}\n");
    }
    script += "mv \"$f.part\" \"$f\"\n";
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    YtDlp { path, js_runtime: None }
}

#[cfg(unix)]
#[test]
fn youtube_chat_stops_and_fetches() {
    let tools = crate::TempDir::new("youtube-chat-tools");
    let work = crate::TempDir::new("youtube-chat");
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
    // Stopped while yt-dlp runs: it ends promptly, and leaves no files behind.
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let stopper = {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            cancel.store(true, Ordering::Relaxed);
            std::time::Instant::now()
        })
    };
    let yt_dlp = fake_yt_dlp(&tools, 1, 30.0);
    let result = fetch_youtube(&info, &yt_dlp, &work.join("chat-7"), &mut MemoryChat::default(), &cancel, &mut |_| {});
    let stopped = stopper.join().unwrap();
    assert!(result.is_err_and(|e| crate::is_canceled(&e)));
    assert!(stopped.elapsed() < std::time::Duration::from_secs(1), "{:?} after the stop", stopped.elapsed());
    assert_eq!(fs::read_dir(&*work).unwrap().count(), 0, "no files left");
    // Left to finish: the chat is read as it's written, with how far it has
    // got (of a 40 s VOD), and its file removed.
    let mut store = MemoryChat::default();
    let begun = std::time::Instant::now();
    let mut progress = Vec::new();
    let yt_dlp = fake_yt_dlp(&tools, 3, 0.4);
    let fetched = fetch_youtube(&info, &yt_dlp, &work.join("chat-7"), &mut store, &AtomicBool::new(false), &mut |f| {
        progress.push((f, begun.elapsed()))
    });
    let ended = begun.elapsed();
    assert!(fetched.unwrap());
    assert_eq!(store.messages.len(), 3);
    let fractions: Vec<f64> = progress.iter().map(|p| p.0).collect();
    assert_eq!(fractions, [0.25, 0.5, 0.75]);
    // The first line is written 1.2 s before the rename. Measured from the
    // end, not the start: a script just written may take most of a second
    // to start on macOS.
    let before_the_end = ended - progress[0].1;
    assert!(
        before_the_end > std::time::Duration::from_millis(600),
        "the first line read {before_the_end:?} before yt-dlp was done"
    );
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
