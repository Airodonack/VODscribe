//! The evaluator's own: the word looked up first, and the ways it passes
//! work over that must find what the long way finds, with the switches the
//! code reads to go the long way. The specs of what a search finds are at
//! its seam, `Db::search`, in db/tests/search.rs.

use super::*;
use crate::chat::{ChatMessage, ChatPart, Kind};
use crate::transcript::Segment;
use std::path::Path;

thread_local! {
    /// Every window read, none passed over: what `may_match` must agree with.
    pub(super) static NO_SHORTCUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The exclusions looked for in each window afresh, as they were till
    /// 0.7.2: what `Stream::look_through` must agree with.
    pub(super) static EACH_WINDOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// An empty library in memory, for the search specs here and in
/// db/tests/search.rs.
pub(crate) fn library() -> Db {
    Db::open(Path::new(":memory:")).unwrap()
}

/// A VOD of a site, `id`, an hour long, streamed on `channel`.
pub(crate) fn vod(db: &Db, id: &str, channel: &str) -> i64 {
    crate::testkit::library::add_site_vod_of(db, "twitch", id, id, 3600.0, Some(channel)).id
}

/// A line of `text` from `start`, a word every 0.3 s, each 0.25 s long, as
/// the website's demo search says its lines (search.test.mjs).
pub(crate) fn say(db: &mut Db, vod: i64, start: f64, text: &str) {
    crate::testkit::library::says(db, vod, crate::testkit::library::spoken(start, text, 0.3, 0.25));
}

/// The anchor, the word a search looks up first, is the pieces' rarest
/// whole word, by how many lines the index has it in; the word being
/// typed only if it's the only one (decided for 0.6.0: the word looked up
/// first is the rarest whole word of all the pieces, by the index's own
/// counts, the prefix being typed only if it is the only word; search.rs,
/// the doc comment of `Db::anchor`; db.rs `MIGRATE_V28`: "search by who and
/// by the rarest word").
/// The function is the spec: which word comes first changes how quick a
/// search is, never what it finds.
#[test]
fn the_rarest_word_is_looked_up() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    for i in 0..20 {
        say(&mut db, v, i as f64 * 30.0, if i == 7 { "the the zebra" } else { "the the the" });
    }
    let pieces = |q: &str| crate::query::parse(q).unwrap().conjunctions().unwrap().remove(0).pieces;
    let anchor = |q: &str| db.anchor(&pieces(q)).unwrap().map(|a| (a.term, a.prefix));
    assert_eq!(anchor("the zebra the"), Some(("zebra".into(), false)));
    assert_eq!(anchor("the zeb"), Some(("the".into(), false)), "not the word being typed");
    assert_eq!(anchor("zeb"), Some(("zeb".into(), true)), "unless it's the only one");
    assert_eq!(anchor("the \"okapi\""), None, "a word in no line");
    let found = db.search("the zeb", &SearchFilter::default(), 500).unwrap();
    assert_eq!(found.hits[0].time, 210.3);
}

/// The windows passed over unread find what reading every one finds, on
/// random lines and queries (search.rs, the doc comment of
/// `Stream::may_match`: "Whether the window ... could hold a match";
/// decided for 0.6.0: the lines around each line the anchor is in are read
/// and matched word by word).
#[test]
fn passing_windows_over_never_loses_a_match() {
    const WORDS: [&str; 6] = ["ah", "be", "cee", "dee", "eh", "eff"];
    let mut seed: u64 = 7;
    let mut next = move |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n
    };
    let mut db = library();
    for v in 0..3 {
        let id = vod(&db, &format!("v{v}"), "s");
        let mut t = 0.0;
        for _ in 0..300 {
            let text = match next(10) {
                0 => "…".to_string(),
                _ => (0..1 + next(5)).map(|_| WORDS[next(WORDS.len())]).collect::<Vec<_>>().join(" "),
            };
            say(&mut db, id, t, &text);
            t += 0.5 + next(12) as f64 * 0.5 + text.split(' ').count() as f64 * 0.3;
        }
    }
    let shown = |r: &SearchResults| -> Vec<String> {
        r.hits.iter().map(|h| format!("{} {} {} {} {:?}", h.vod_id, h.time, h.end, h.lines, marked_parts(h))).collect()
    };
    let mut found = 0;
    for _ in 0..300 {
        let mut q = Vec::new();
        for _ in 0..1 + next(3) {
            let phrase: Vec<&str> = (0..1 + next(3)).map(|_| WORDS[next(WORDS.len())]).collect();
            q.push(match next(4) {
                0 => format!("\"{}\"", phrase.join(" ")),
                1 => format!("-{}", phrase[0]),
                _ => format!("{} AND", phrase.join(" ")),
            });
        }
        // Half end in a word, which is then a prefix being typed.
        let q = q.join(" ").trim_end_matches(" AND").to_string();
        let q = if next(2) == 0 { format!("{q} within:{}", 3 + next(20)) } else { q };
        let quick = db.search(&q, &SearchFilter::default(), 10_000).unwrap();
        NO_SHORTCUT.set(true);
        let every = db.search(&q, &SearchFilter::default(), 10_000).unwrap();
        NO_SHORTCUT.set(false);
        assert_eq!(shown(&quick), shown(&every), "{q}");
        found += every.hits.len();
    }
    assert!(found > 1000, "the queries find things: {found}");
}

/// The exclusions found once per stretch of lines (`Stream::look_through`)
/// leave out what looking in each window afresh did, on a library of
/// lines with and without words: minus words and phrases, small windows
/// and the widest, with phrases common to most lines, with a name alone,
/// with a time (CHANGELOG.md, 0.7.2: "Searches leaving words out are
/// quicker.", what they find kept; its budget is the e2e perf-library's).
#[test]
fn exclusions_found_once_leave_out_what_each_window_did() {
    const WORDS: [&str; 6] = ["ah", "be", "cee", "dee", "eh", "eff"];
    let mut seed: u64 = 11;
    let mut next = move |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n
    };
    let mut db = library();
    for v in 0..3 {
        let id = vod(&db, &format!("v{v}"), if v == 0 { "streamer" } else { "other" });
        let mut t = 0.0;
        for _ in 0..300 {
            let text = match next(10) {
                0 => "…".to_string(),
                _ => (0..1 + next(5)).map(|_| WORDS[next(WORDS.len())]).collect::<Vec<_>>().join(" "),
            };
            say(&mut db, id, t, &text);
            t += 0.5 + next(30) as f64 * 0.5 + text.split(' ').count() as f64 * 0.3;
        }
    }
    let shown = |r: &SearchResults| -> Vec<String> {
        r.hits.iter().map(|h| format!("{} {} {} {} {:?}", h.vod_id, h.time, h.end, h.lines, marked_parts(h))).collect()
    };
    let mut queries: Vec<String> = [
        "ah be cee",
        "ah be cee within:1h",
        "\"ah\" \"be\" within:1h",
        "ah -be",
        "ah -be within:1h",
        "ah be -cee",
        "\"ah\" \"dee\" -\"be cee\" within:30",
        "ah -\"be cee dee\" within:2",
        "eff OR dee -ah",
        "from:streamer -ah",
        "from:streamer in:audio -ah within:1h",
        "from:streamer in:audio -\"eh eff\" -cee within:10",
        "after:5:00 in:audio -be",
        "after:5:00 in:audio -be -cee -dee within:1",
    ]
    .map(String::from)
    .to_vec();
    for _ in 0..200 {
        let minus: Vec<String> = (0..1 + next(2))
            .map(|_| {
                let phrase: Vec<&str> = (0..1 + next(3)).map(|_| WORDS[next(WORDS.len())]).collect();
                if phrase.len() == 1 { format!("-{}", phrase[0]) } else { format!("-\"{}\"", phrase.join(" ")) }
            })
            .collect();
        let find = match next(4) {
            0 => "from:streamer in:audio".to_string(),
            _ => (0..1 + next(3)).map(|_| WORDS[next(WORDS.len())]).collect::<Vec<_>>().join(" "),
        };
        let within = [3, 10, 40, 600, 3600][next(5)];
        queries.push(format!("{find} {} within:{within}", minus.join(" ")));
    }
    let mut found = 0;
    for q in &queries {
        let once = db.search(q, &SearchFilter::default(), 10_000).unwrap();
        EACH_WINDOW.set(true);
        let each = db.search(q, &SearchFilter::default(), 10_000).unwrap();
        EACH_WINDOW.set(false);
        assert_eq!(once.query.error, None, "{q}");
        assert_eq!(shown(&once), shown(&each), "{q}");
        found += each.hits.len();
    }
    assert!(found > 1000, "the queries find things: {found}");
}

fn marked_parts(h: &SearchHit) -> String {
    h.parts.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect()
}

/// The cases timed, as (what, query): the e2e perf pass's three, and
/// what this search adds. The library has no "i" or "mean", so the
/// all-common phrase is one of its own common words.
const BENCHMARK: [(&str, &str); 19] = [
    ("one line a VOD", "tranquility base relay"),
    ("many lines", "chat"),
    ("none", "zyzzyva"),
    ("all-common phrase", "you know what"),
    ("all-common word pair", "the game"),
    ("three-line phrase", "eagle has landed safely"),
    ("prefix, the only word", "cha"),
    ("prefix after words", "tranquility base rel"),
    ("quoted, any order", "\"relay\" \"tranquility\""),
    ("from: a chatter", "from:viewer7"),
    ("from: a chatter, a word", "from:viewer7 good"),
    ("from: a streamer, a phrase", "from:streamer_a tranquility base"),
    // A name alone: a scan of their lines and messages, no word to look up.
    ("from: a streamer alone", "from:streamer_a"),
    ("from: a streamer alone, late", "from:streamer_a after:1:50:00"),
    ("from: two names", "from:streamer_b,viewer7"),
    ("from: a streamer alone, -word", "from:streamer_a -the"),
    // The minus, an hour either way, leaves out every line, so the scan
    // reads all of the channel's: the slowest a name alone can be.
    ("from: a streamer alone, -word everywhere", "from:streamer_a in:audio -the within:1h"),
    ("-word", "chat -the"),
    // The widest window (MOST_WITHIN) over common words, the slowest a
    // search can be: about 1.4 s on 10 VODs of 10 hours (grown with
    // `hours: 10`), where within:3h took 15 s and within:10h over a minute.
    ("widest window, common words", "\"the\" \"a\" \"game\" within:1h"),
];

/// Times `BENCHMARK` on a large synthetic library, never a real one: a
/// copy of tests/e2e/fixtures/library-0.5.0.sqlite grown by
/// tests/e2e/fixtures/large-library.mjs (`grow(file, { vods: 60, hours: 2 })`,
/// as scenarios/search-large.mjs does), then
///   VODSCRIBE_SEARCH_BENCH=<file> cargo test --release -p vodscribe-core search_benchmark -- --ignored --nocapture
/// The first run adds what the grown library lacks: a phrase over three
/// lines in each VOD, channels, and chat from 200 chatters.
#[test]
#[ignore = "needs a library grown by tests/e2e/fixtures/large-library.mjs"]
fn search_benchmark() {
    let path = std::env::var("VODSCRIBE_SEARCH_BENCH").expect("VODSCRIBE_SEARCH_BENCH: the grown library");
    let mut db = Db::open(Path::new(&path)).unwrap();
    plant(&mut db);
    drop(db);
    let db = Db::open(Path::new(&path)).unwrap();
    println!("| case | query | hits | ms (median of 7) | slowest |");
    println!("|---|---|---|---|---|");
    for (what, query) in BENCHMARK {
        let r = db.search(query, &SearchFilter::default(), 500).unwrap();
        assert_eq!(r.query.error, None, "{query}");
        let mut times: Vec<f64> = (0..7)
            .map(|_| {
                let started = std::time::Instant::now();
                db.search(query, &SearchFilter::default(), 500).unwrap();
                started.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        times.sort_by(f64::total_cmp);
        let hits = format!("{}{}", r.hits.len(), if r.truncated { "+" } else { "" });
        println!("| {what} | `{query}` | {hits} | {:.1} | {:.1} |", times[3], times[6]);
    }
}

/// What `search_benchmark` needs that the grown library lacks, added once.
fn plant(db: &mut Db) {
    let grown: Vec<i64> = {
        let mut stmt = db.conn.prepare("SELECT id FROM vods WHERE title LIKE 'Long stream %' ORDER BY id").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
    };
    assert!(grown.len() >= 10, "not a library grown by large-library.mjs");
    if db.conn.query_row("SELECT count(*) FROM chat", [], |r| r.get::<_, i64>(0)).unwrap() > 0 {
        return;
    }
    const WORDS: [&str; 12] = ["good", "nice", "lol", "gg", "the", "play", "wow", "chat", "what", "yes", "no", "hi"];
    let mut seed: u64 = 1969;
    let mut next = move |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n
    };
    for (n, &vod) in grown.iter().enumerate() {
        let channel = if n % 2 == 0 { "streamer_a" } else { "streamer_b" };
        db.conn.execute("UPDATE vods SET channel = ?2 WHERE id = ?1", params![vod, channel]).unwrap();
        // "… eagle / has landed / safely …" over the lines at 1000, 1004 and 1008 s.
        let mut lines = Vec::new();
        for (start, words) in
            [(1000.0, vec![(-1, "eagle")]), (1004.0, vec![(0, "has"), (1, "landed")]), (1008.0, vec![(0, "safely")])]
        {
            let (id, end, json): (i64, f64, String) = db
                .conn
                .query_row(
                    "SELECT id, end, words FROM segments WHERE vod_id = ?1 AND start = ?2",
                    params![vod, start],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let mut said: Vec<Word> = serde_json::from_str(&json).unwrap();
            if start == 1004.0 {
                said.truncate(2);
            }
            for (i, text) in words {
                let i = if i < 0 { said.len() - 1 } else { i as usize };
                said[i].text = text.into();
            }
            db.conn.execute("DELETE FROM segments WHERE id = ?1", [id]).unwrap();
            let text = said.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            lines.push(Segment { start, end, text, words: said });
        }
        db.insert_segments(vod, &lines).unwrap();
        let messages: Vec<ChatMessage> = (0..1440)
            .map(|i| ChatMessage {
                time: i as f64 * 5.0,
                author: format!("viewer{}", next(200)),
                color: None,
                parts: vec![ChatPart::Text {
                    text: (0..1 + next(6)).map(|_| WORDS[next(WORDS.len())]).collect::<Vec<_>>().join(" "),
                }],
                kind: Kind::Message,
                amount: None,
                event_time: None,
                at: None,
            })
            .collect();
        db.add_chat(vod, &messages, &[]).unwrap();
    }
}
