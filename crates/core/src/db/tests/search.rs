//! Search over the library's transcripts and chat, at its seam: what
//! `Db::search` finds for what's typed and the filter's choices, the names
//! `from:` suggests, and the search's own connection (`Searcher`).
//! The evaluator's shortcuts are checked against the long way in
//! search/tests.rs. The website's demo search (apps/site search.ts) is
//! tested against the same cases (its search.test.mjs).

use std::sync::Arc;

use super::*;
use crate::edits::LibraryItem;
use crate::query::{MOST_WITHIN, TOO_WIDE};
use crate::search::tests::{library, say, vod};
use crate::searcher::Searcher;
use crate::testkit::Gate;

/// `author` writes `text` in VOD `vod`'s chat at `time`.
fn chat(db: &mut Db, vod: i64, time: f64, author: &str, text: &str) {
    db.add_chat(vod, &[message(time, author, text)], &[]).unwrap();
}

fn find(db: &Db, query: &str) -> SearchResults {
    db.search(query, &SearchFilter::default(), 500).unwrap()
}

/// `find` in what was said only, or in chat only.
fn find_in(db: &Db, query: &str, speech: bool) -> SearchResults {
    db.search(query, &SearchFilter { speech, chat: !speech, ..Default::default() }, 500).unwrap()
}

/// Each hit as its text with the marked runs in [brackets].
fn marked(r: &SearchResults) -> Vec<String> {
    r.hits
        .iter()
        .map(|h| h.parts.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect())
        .collect()
}

/// Each hit as (VOD, where it was found, its time).
fn hits(r: &SearchResults) -> Vec<(i64, Source, f64)> {
    r.hits.iter().map(|h| (h.vod_id, h.source, h.time)).collect()
}

// Words and phrases.

/// The cases the website's demo search is tested against too, each said as
/// a line and written as a message: found, and marked, the same in both;
/// a case marking nothing finds nothing. Accents are ignored, hyphens and
/// apostrophes split words, numbers are words, a phrase's repeats don't
/// overlap (crates/core/tests/phrase-search.json; search.rs, the module
/// doc: "The website's demo searches its fixtures the same way ... a change
/// to what matches is made there too").
#[test]
fn the_shared_phrase_cases_are_found_and_marked_in_speech_and_chat() {
    #[derive(serde::Deserialize)]
    struct Case {
        about: String,
        words: Vec<String>,
        query: String,
        ranges: Vec<(usize, usize)>,
        marked: String,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../tests/phrase-search.json")).unwrap();
    for c in cases {
        let mut db = library();
        let v = vod(&db, "a", "s");
        let text = c.words.join(" ");
        say(&mut db, v, 0.0, &text);
        chat(&mut db, v, 0.0, "viewer", &text);
        let shown = if c.ranges.is_empty() { vec![] } else { vec![c.marked.clone()] };
        assert_eq!(marked(&find_in(&db, &c.query, true)), shown, "said: {}", c.about);
        assert_eq!(marked(&find_in(&db, &c.query, false)), shown, "in chat: {}", c.about);
    }
}

/// Unquoted words are a phrase, in order, in any case and with any
/// punctuation, the last word a prefix while it's typed (CHANGELOG.md,
/// 0.6.0: "Quoted words are found in any order.": "unquoted words are still
/// a phrase, in order"; query.rs, the module doc: "words typed one after
/// another are one loose phrase in order (the last word a prefix while it's
/// being typed)").
#[test]
fn unquoted_words_are_a_phrase_in_order_the_last_a_prefix() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "Welcome back everyone.");
    say(&mut db, v, 10.0, "That was a clutch play, honestly.");
    let r = find(&db, "CLUTCH play");
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].time, 10.9);
    let parts: Vec<_> = r.hits[0].parts.iter().map(|p| (p.text.as_str(), p.hit)).collect();
    assert_eq!(parts, [("That was a ", false), ("clutch play,", true), (" honestly.", false)]);
    assert_eq!(find(&db, "welc").hits.len(), 1, "the last word a prefix");
    assert!(find(&db, "play clutch").hits.is_empty(), "in order");
    assert!(find(&db, "\" OR *").hits.is_empty(), "no FTS syntax injection");
}

/// A phrase and quoted words mix: `nice "clutch"` finds "nice" with
/// "clutch" anywhere near it, before or after, and not without it
/// (CHANGELOG.md, 0.6.0: "Quoted words are found in any order.": "the two
/// mix: `nice "clutch"` finds "nice" with "clutch" anywhere near it").
#[test]
fn a_phrase_and_a_quoted_word_mix_the_word_anywhere_near() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "nice try");
    say(&mut db, v, 5.0, "what a clutch");
    say(&mut db, v, 100.0, "clutch again");
    say(&mut db, v, 104.0, "so nice");
    say(&mut db, v, 200.0, "nice alone");
    say(&mut db, v, 300.0, "clutch alone");
    let r = find(&db, "nice \"clutch\"");
    assert_eq!(r.hits.iter().map(|h| (h.time, h.lines)).collect::<Vec<_>>(), [(0.0, 2), (100.0, 2)]);
    assert_eq!(marked(&r), ["[nice] try what a [clutch]", "[clutch] again so [nice]"]);
}

/// A one-letter search finds that word, not every word starting with the
/// letter (CHANGELOG.md, 0.2.1: "A one-letter search now finds that word,
/// rather than every word starting with the letter.").
#[test]
fn a_one_letter_search_finds_that_word_not_every_word_it_begins() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "an apple a day");
    chat(&mut db, v, 5.0, "viewer", "an apple a day");
    assert_eq!(marked(&find(&db, "a")), ["an apple [a] day", "an apple [a] day"]);
}

// Chat.

/// Search covers what was said and what chat wrote: audio, chat or both, in
/// this VOD or all of them, in library order (CHANGELOG.md, 0.2.0: "Search
/// covers chat too.": "The From button by the search box chooses audio, chat
/// or both, and this VOD or all of them").
#[test]
fn search_covers_audio_chat_or_both_in_this_vod_or_all() {
    let mut db = library();
    let a = vod(&db, "a", "s");
    let b = vod(&db, "b", "s");
    say(&mut db, a, 20.0, "what a clutch play");
    chat(&mut db, a, 25.0, "viewer", "CLUTCH PLAY!!");
    chat(&mut db, b, 1.0, "other", "clutch play elsewhere");
    let search = |filter: SearchFilter| hits(&db.search("clutch play", &filter, 10).unwrap());
    let (s, c) = (Source::Speech, Source::Chat);
    assert_eq!(search(SearchFilter::default()), [(b, c, 1.0), (a, s, 20.6), (a, c, 25.0)]);
    assert_eq!(search(SearchFilter { chat: false, ..Default::default() }), [(a, s, 20.6)]);
    assert_eq!(search(SearchFilter { speech: false, ..Default::default() }), [(b, c, 1.0), (a, c, 25.0)]);
    assert_eq!(search(SearchFilter { vod_id: Some(a), ..Default::default() }), [(a, s, 20.6), (a, c, 25.0)]);
    assert_eq!(search(SearchFilter { speech: false, vod_id: Some(a), ..Default::default() }), [(a, c, 25.0)]);
    assert_eq!(search(SearchFilter { speech: false, chat: false, ..Default::default() }), []);
}

/// In chat, a match is in one message: all its pieces there, never some in
/// one message and the rest in the next, however close (decided for 0.6.0:
/// in chat, all the pieces are in one message, never across messages, with
/// no window; search.rs, the module doc: "A chat message is a unit of its
/// own: all the pieces in one message, no window").
#[test]
fn a_chat_match_is_in_one_message_never_across_messages() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    chat(&mut db, v, 1.0, "viewer", "hello");
    chat(&mut db, v, 1.5, "viewer", "world");
    chat(&mut db, v, 9.0, "viewer", "world says hello");
    let found = |q: &str| find_in(&db, q, false).hits.iter().map(|h| h.time).collect::<Vec<_>>();
    assert_eq!(found("\"hello\" \"world\""), [9.0]);
    assert_eq!(found("hello world"), Vec::<f64>::new());
    assert_eq!(found("world says hello within:1"), [9.0], "no window in a message");
}

/// A match in chat is its message: at the time it was sent, to the end too,
/// one line, with who wrote it and their name's colour (db.rs, the doc
/// comments of `SearchHit`: "For chat, when the message was sent", "for
/// chat, `time`", "For chat, who wrote it, and their name color").
#[test]
fn a_chat_match_is_its_message_at_its_time_by_its_author() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    chat(&mut db, v, 12.5, "Ada", "what a clutch play");
    let hit = find(&db, "clutch").hits.remove(0);
    assert_eq!((hit.source, hit.time, hit.end, hit.lines), (Source::Chat, 12.5, 12.5, 1));
    assert_eq!((hit.author.as_deref(), hit.color.as_deref()), (Some("Ada"), Some("#ff0000")));
}

/// `in:chat` alone lists every message, all VODs' in library order, and
/// nothing said (e2e chat-search.mjs: "`in:chat` alone lists chat";
/// decided for 0.6.0: filters alone list everything they pass).
#[test]
fn in_chat_alone_lists_every_message() {
    let mut db = library();
    let a = vod(&db, "a", "s");
    let b = vod(&db, "b", "s");
    say(&mut db, a, 0.0, "hello chat");
    chat(&mut db, a, 4.0, "viewer", "gg");
    chat(&mut db, a, 2.0, "other", "hi");
    chat(&mut db, b, 9.0, "viewer", "lol");
    let c = Source::Chat;
    assert_eq!(hits(&find(&db, "in:chat")), [(b, c, 9.0), (a, c, 2.0), (a, c, 4.0)]);
}

// Who.

/// `from:` with a quoted name finds the one whose name has a space, and not
/// someone named as its first word (CHANGELOG.md, 0.6.0: "Search learns from,
/// in, within, after, before.": "`from:"Some One"` for a name with a space";
/// e2e chat-search.mjs: "a name with a space needs its quotes").
#[test]
fn from_a_quoted_name_finds_the_one_whose_name_has_a_space() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    chat(&mut db, v, 1.0, "Some One", "hello there");
    chat(&mut db, v, 2.0, "Some", "hello again");
    let r = find(&db, "from:\"Some One\" hello");
    assert_eq!(
        r.hits.iter().map(|h| (h.time, h.author.clone().unwrap())).collect::<Vec<_>>(),
        [(1.0, "Some One".into())]
    );
}

/// A filter beside an OR is the whole query's: `from:x "a" OR "b"` is from x,
/// a or b (query.rs, the module doc: "Pieces and filters side by side (the
/// implicit AND) bind looser than `OR`, so `from:x "a" OR "b"` is from x, a
/// or b").
#[test]
fn a_filter_beside_an_or_is_the_whole_querys() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    chat(&mut db, v, 1.0, "x", "apple");
    chat(&mut db, v, 2.0, "y", "banana");
    chat(&mut db, v, 3.0, "x", "banana");
    chat(&mut db, v, 4.0, "y", "apple");
    let times = |q: &str| find(&db, q).hits.iter().map(|h| h.time).collect::<Vec<_>>();
    assert_eq!(times("from:x \"apple\" OR \"banana\""), [1.0, 3.0]);
    assert_eq!(times("\"apple\" OR \"banana\" from:x"), [1.0, 3.0]);
}

/// `from:` suggests names starting with what's typed, in any case: the
/// channels first, then the people in chat by how much they wrote, at most
/// eight (search.rs, the doc comment of `author_suggestions`: "starting with
/// `prefix` in any case: the channels of the library's VODs first, then
/// chatters with the most messages ... At most `MOST_SUGGESTIONS`"; e2e
/// chat-search.mjs: "the box's suggestions name the channel first, then
/// chatters by how much they wrote"). The command's own test is app
/// vods/tests/reading.rs `from_suggests_channels_first_then_chatters`.
#[test]
fn from_suggests_at_most_eight_names_starting_with_the_prefix_in_any_case() {
    let mut db = library();
    let v = vod(&db, "a", "Viewers");
    for i in 0..10 {
        for n in 0..=i {
            chat(&mut db, v, n as f64, &format!("viewer{i}"), "hi");
        }
    }
    chat(&mut db, v, 0.0, "someone", "hi");
    let names = |prefix: &str| -> Vec<(String, bool, i64)> {
        db.author_suggestions(prefix, None).unwrap().into_iter().map(|s| (s.name, s.streamer, s.messages)).collect()
    };
    let mut expected = vec![("Viewers".to_string(), true, 0)];
    expected.extend((3..10).rev().map(|i| (format!("viewer{i}"), false, i + 1)));
    assert_eq!(names("VIEW"), expected);
    assert_eq!(names("Viewer1"), [("viewer1".to_string(), false, 2)]);
}

// Results.

/// Results go VOD by VOD in the library's order, then by time within each
/// (db.rs, the doc comment of `Db::search`: "Hits are ordered by VOD, in
/// library order, then by time within the VOD"; decided for 0.6.0: no
/// ranking).
#[test]
fn results_go_by_the_librarys_order_then_by_time() {
    let mut db = library();
    let (a, b, c) = (vod(&db, "a", "s"), vod(&db, "b", "s"), vod(&db, "c", "s"));
    for v in [a, b, c] {
        say(&mut db, v, 5.0, "gg");
        chat(&mut db, v, 1.0, "viewer", "gg");
    }
    let order = |db: &Db| find(db, "gg").hits.iter().map(|h| (h.vod_id, h.time)).collect::<Vec<_>>();
    assert_eq!(order(&db), [(c, 1.0), (c, 5.0), (b, 1.0), (b, 5.0), (a, 1.0), (a, 5.0)], "newest first");
    db.reorder_library(&[LibraryItem::Vod(b), LibraryItem::Vod(a), LibraryItem::Vod(c)]).unwrap();
    assert_eq!(order(&db), [(b, 1.0), (b, 5.0), (a, 1.0), (a, 5.0), (c, 1.0), (c, 5.0)]);
}

/// A line saved without its words' times is found all the same, its words
/// at the line's own times (search.rs, `Line::read`: "A line saved without
/// its word timings: its words at its own times").
#[test]
fn a_line_without_its_words_times_is_found_at_its_own() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    let line = Segment { start: 30.0, end: 34.0, text: "the eagle has landed".into(), words: vec![] };
    db.insert_segments(v, &[line]).unwrap();
    let r = find(&db, "eagle has");
    assert_eq!(marked(&r), ["the [eagle has] landed"]);
    assert_eq!((r.hits[0].time, r.hits[0].end), (30.0, 34.0));
}

/// A VOD taken out of the library has no matches, said or in chat, though
/// an edit still plays a piece of it: search covers the VODs whose own clip
/// is there (decided for 0.7.4: search covers only VODs whose clip of the
/// whole video is alive; CHANGELOG.md, 0.5.1: "A VOD removed since has no
/// matches there").
#[test]
fn a_vod_out_of_the_library_has_no_matches_while_an_edit_plays_it() {
    let mut db = library();
    let gone = vod(&db, "gone", "s");
    let kept = vod(&db, "kept", "s");
    for v in [gone, kept] {
        say(&mut db, v, 0.0, "secret plan");
        chat(&mut db, v, 1.0, "viewer", "secret plan");
    }
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, gone, (0.0, 10.0), "p", None).unwrap();
    assert!(!db.remove_entry(gone).unwrap(), "still used by the edit");
    let (s, c) = (Source::Speech, Source::Chat);
    assert_eq!(hits(&find(&db, "secret plan")), [(kept, s, 0.0), (kept, c, 1.0)]);
    assert_eq!(hits(&find(&db, "in:chat")), [(kept, c, 1.0)]);
    let only = SearchFilter { vod_id: Some(gone), ..Default::default() };
    assert!(db.search("secret plan", &only, 10).unwrap().hits.is_empty());
}

/// A search of one VOD keeps to it, though other VODs' lines and messages
/// were saved between its own (CHANGELOG.md, 0.2.0: "Search covers chat
/// too.": "this VOD or all of them"; 0.2.1: "Faster with big chats:
/// searching one VOD of a large library").
#[test]
fn a_vods_search_keeps_to_it() {
    let mut db = library();
    let a = vod(&db, "a", "s");
    let b = vod(&db, "b", "s");
    // Saved in turns, so b's sit between a's ids; said ten seconds after written.
    for (v, t, text) in [(a, 1.0, "an apple a day"), (b, 2.0, "apple pie"), (a, 3.0, "apples again")] {
        db.add_chat(v, &[message(t, "v", text)], &[t]).unwrap();
        say(&mut db, v, t + 10.0, text);
    }
    let found = |vod_id| {
        let r = db.search("ap", &SearchFilter { vod_id: Some(vod_id), ..Default::default() }, 10).unwrap();
        hits(&r)
    };
    let (s, c) = (Source::Speech, Source::Chat);
    assert_eq!(found(a), [(a, c, 1.0), (a, c, 3.0), (a, s, 11.3), (a, s, 13.0)]);
    assert_eq!(found(b), [(b, c, 2.0), (b, s, 12.0)]);
}

/// A timeline layer's chat matches keep to their VOD too (db.rs, the doc
/// comment of `chat_matches`: "The ids and times of VOD `vod_id`'s chat
/// messages that contain any of `terms`").
#[test]
fn a_layers_chat_matches_keep_to_its_vod() {
    let mut db = library();
    let a = vod(&db, "a", "s");
    let b = vod(&db, "b", "s");
    db.add_chat(a, &[message(1.0, "v", "an apple a day")], &[1.0]).unwrap();
    db.add_chat(b, &[message(2.0, "v", "apple pie")], &[2.0]).unwrap();
    db.add_chat(a, &[message(3.0, "v", "apples again")], &[3.0]).unwrap();
    let times: Vec<f64> = db.chat_matches(a, &["apple".into()]).unwrap().into_iter().map(|(_, t)| t).collect();
    assert_eq!(times, [1.0]);
}

/// A search asked for its first results gives the first of all it finds,
/// in order, and says when there were more: in speech and chat, with a
/// minus, and for names alone (db.rs, the doc comment of `Db::search`:
/// "Hits are ordered by VOD, in library order, then by time within the VOD;
/// at most `limit`, and `truncated` if there were more").
#[test]
fn a_limited_search_gives_the_first_of_all_it_finds() {
    let mut db = library();
    for (n, channel) in ["s", "t", "s"].iter().enumerate() {
        let v = vod(&db, &format!("v{n}"), channel);
        for i in 0..6 {
            let t = i as f64 * 40.0;
            say(&mut db, v, t, if i % 3 == 0 { "gg lol" } else { "gg well played" });
            chat(&mut db, v, t + 1.0, if i % 2 == 0 { "viewer" } else { "fan" }, "gg");
        }
    }
    let shown = |r: &SearchResults| -> Vec<String> {
        r.hits.iter().map(|h| format!("{} {:?} {} {:?}", h.vod_id, h.source, h.time, h.parts)).collect()
    };
    for q in ["gg", "gg -lol", "in:chat", "in:audio", "from:s", "from:viewer", "\"gg\" \"played\""] {
        let all = db.search(q, &SearchFilter::default(), 10_000).unwrap();
        let total = all.hits.len();
        assert!(total > 3 && !all.truncated, "{q}: {total}");
        for limit in 1..=total + 1 {
            let r = db.search(q, &SearchFilter::default(), limit).unwrap();
            assert_eq!(shown(&r), shown(&all)[..limit.min(total)], "{q}, the first {limit}");
            assert_eq!(r.truncated, total > limit, "{q}, the first {limit}");
        }
    }
}

/// A match's window is at most `within:`: from its first word's start to
/// its last word's end, exactly the window is in it (search.rs, the module
/// doc: "the last matched word's end minus the first's start at most
/// `within:`"; CHANGELOG.md, 0.6.0: "Quoted words are found in any order.":
/// "said within 20 seconds of each other").
#[test]
fn a_match_spanning_exactly_the_window_is_in_it() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    // "first" from 0 s to 0.25 s; "last" from 19.75 s to 20 s.
    say(&mut db, v, 0.0, "first");
    say(&mut db, v, 19.75, "last");
    say(&mut db, v, 100.0, "first");
    say(&mut db, v, 120.0, "last");
    let times = |q: &str| find(&db, q).hits.iter().map(|h| (h.time, h.end)).collect::<Vec<_>>();
    assert_eq!(times("\"first\" \"last\""), [(0.0, 20.0)], "20 s exactly, and 20.25 s");
}

/// A minus phrase said across two lines leaves out the match near it, as
/// the windows move on through the lines (CHANGELOG.md, 0.6.0: "A minus
/// leaves out matches.": "Put it before a word or a quoted phrase"; search.rs,
/// the module doc: "each phrase contiguous in the stream (across any number
/// of lines)").
#[test]
fn a_minus_phrase_across_lines_leaves_out_the_match_near_it() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "ah");
    say(&mut db, v, 3.0, "zz be");
    say(&mut db, v, 6.0, "cee");
    say(&mut db, v, 8.0, "ah");
    // The first ah's window (5 s either side) ends before cee; the second's has be cee.
    let times = find(&db, "ah -\"be cee\" within:10").hits.iter().map(|h| h.time).collect::<Vec<_>>();
    assert_eq!(times, [0.0]);
}

/// A search with more alternatives than it will run one by one says so and
/// finds nothing (query.rs, the doc comment of `MOST_ALTERNATIVES`: "More
/// alternatives than this ... is refused rather than run one by one").
#[test]
fn a_search_of_too_many_alternatives_says_so() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "a c e g i k m");
    let q = "(a OR b) (c OR d) (e OR f) (g OR h) (i OR j) (k OR l) (m OR n)";
    let r = find(&db, q);
    assert!(r.hits.is_empty());
    assert!(r.query.error.as_deref().is_some_and(|e| e.contains("alternatives")), "{:?}", r.query.error);
    assert_eq!(find(&db, "(a OR b) (c OR d) (e OR f)").hits.len(), 1, "fewer run");
}

/// A search of chat gives at most as many results as it's asked for too,
/// saying there were more (db.rs, the doc comment of `Db::search`: "at most
/// `limit`, and `truncated` if there were more").
#[test]
fn a_chat_search_is_limited_too() {
    let mut db = library();
    let a = vod(&db, "a", "s");
    let chat: Vec<_> = (0..5).map(|i| message(i as f64, "v", "hype")).collect();
    db.set_chat(a, &chat).unwrap();
    let r = db.search("hype", &SearchFilter::default(), 3).unwrap();
    assert_eq!((r.hits.len(), r.truncated), (3, true));
}

/// A phrase runs from one line into the next, and its result shows every
/// line it runs over with each part found marked (CHANGELOG.md, 0.6.0:
/// "Search finds phrases across lines.": "a result over several lines shows
/// them all, each part found marked"; decided for 0.6.0, as a worked
/// example: `well that was a good shot "damn"` finds `damn / well that was /
/// a good shot` across its three lines).
#[test]
fn a_match_runs_across_lines() {
    let mut db = library();
    let v = vod(&db, "a", "streamer");
    say(&mut db, v, 10.0, "damn");
    say(&mut db, v, 12.0, "well that was");
    say(&mut db, v, 14.0, "a good shot");
    say(&mut db, v, 100.0, "well that was a good shot");
    let r = find(&db, "well that was a good shot \"damn\"");
    assert_eq!(marked(&r), ["[damn well that was a good shot]"], "three lines, every piece marked");
    let hit = &r.hits[0];
    assert_eq!((hit.time, hit.lines), (10.0, 3));
    assert!((hit.end - 14.85).abs() < 1e-9, "the last word's end: {}", hit.end);
    assert_eq!(marked(&find(&db, "(well that was a good shot \"damn\")")), marked(&r));
    // Without damn, the phrase is found twice: across two lines, and in one.
    let r = find(&db, "well that was a good shot");
    assert_eq!(marked(&r), ["[well that was a good shot]", "[well that was a good shot]"]);
    assert_eq!(r.hits.iter().map(|h| h.lines).collect::<Vec<_>>(), [2, 1]);
}

/// A phrase runs over any number of lines, past a line with no words, the
/// last word a prefix while it's typed; a quoted one is whole words
/// (search.rs, the module doc: "each phrase contiguous in the stream
/// (across any number of lines)"; query.rs, the module doc: "the last word
/// a prefix while it's being typed), a quoted string a strict phrase of
/// whole words").
#[test]
fn a_phrase_over_many_lines() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    for (i, w) in ["you", "know", "what", "i", "really", "mean"].iter().enumerate() {
        say(&mut db, v, i as f64, w);
    }
    let r = find(&db, "you know what i really mean");
    assert_eq!(r.hits.iter().map(|h| h.lines).collect::<Vec<_>>(), [6]);
    assert_eq!(marked(&r), ["[you know what i really mean]"]);
    assert_eq!(find(&db, "what i real").hits[0].lines, 3, "the last word a prefix, while typing");
    let w = vod(&db, "b", "s");
    say(&mut db, w, 0.0, "sure you know");
    say(&mut db, w, 1.0, "…");
    say(&mut db, w, 2.0, "what");
    let r = find(&db, "you know what");
    assert_eq!(
        r.hits.iter().map(|h| (h.vod_id, h.lines)).collect::<Vec<_>>(),
        [(w, 3), (v, 3)],
        "past a line of no words"
    );
    assert!(find(&db, "\"what i real\"").hits.is_empty(), "quoted: whole words");
}

/// A minus leaves out a match with the word or phrase in the window centred
/// on it, and only there (CHANGELOG.md, 0.6.0: "A minus leaves out
/// matches.", "as in `i did it -frick`"; search.rs, the doc comment of
/// `Stream::excluded`: "the match widened equally on both sides to the
/// window's full size"; decided for 0.6.0, as worked examples: frick within
/// about 10 s of "i did it", before, after or in its line, leaves it out
/// with a 20 s window).
#[test]
fn a_minus_leaves_out_matches_with_the_word_near() {
    let mut db = library();
    // The four layouts, each frick within about 10 s of "i did it": all out.
    let layouts = [
        vec![(0.0, "frick"), (3.0, "i did it")],
        vec![(0.0, "i did it"), (3.0, "frick")],
        vec![(0.0, "frick i did it")],
        vec![(0.0, "i did it frick")],
        vec![(0.0, "frick"), (9.0, "i did it")],
    ];
    for (n, lines) in layouts.iter().enumerate() {
        let v = vod(&db, &format!("near{n}"), "s");
        for &(start, text) in lines {
            say(&mut db, v, start, text);
        }
    }
    assert_eq!(find(&db, "i did it").hits.len(), 5);
    assert!(find(&db, "i did it -frick").hits.is_empty(), "{:?}", marked(&find(&db, "i did it -frick")));
    // Far enough away it doesn't count: 30 s, or 12 s (past the centred window's 9.6 s).
    let far = vod(&db, "far", "s");
    say(&mut db, far, 0.0, "frick");
    say(&mut db, far, 30.0, "i did it");
    let edge = vod(&db, "edge", "s");
    say(&mut db, edge, 0.0, "frick");
    say(&mut db, edge, 12.0, "i did it");
    let r = find(&db, "i did it -frick");
    assert_eq!(r.hits.iter().map(|h| h.vod_id).collect::<Vec<_>>(), [edge, far]);
    // A narrower window narrows what's near; a phrase can be left out too.
    assert_eq!(find(&db, "i did it -frick within:4").hits.len(), 5, "only same-line fricks are near");
    assert_eq!(find(&db, "frick -\"i did it\"").hits.len(), 2, "the far and edge ones");
    assert_eq!(find(&db, "frick -\"did i\"").hits.len(), 7);
}

/// Quoted words are found in any order, all said within 20 seconds unless
/// `within:` says otherwise, and the filter's Within is the default the typed
/// one overrides (CHANGELOG.md, 0.6.0: "Quoted words are found in any
/// order.": "They need only all be said within 20 seconds of each other, as
/// in `"all" "words" "any" "order"`"; "Search learns from, in, within, after,
/// before.": "`within:2m` ... (seconds, minutes or hours; 20 seconds unless
/// you say)"; "The search filter gains Within and From whom.": "What you
/// type wins over the filter").
#[test]
fn quoted_words_in_any_order_within_the_window() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "order matters any words at all");
    say(&mut db, v, 100.0, "all");
    say(&mut db, v, 105.0, "the words");
    say(&mut db, v, 110.0, "in any");
    say(&mut db, v, 119.0, "order");
    say(&mut db, v, 200.0, "all the words in any");
    say(&mut db, v, 225.0, "order");
    let q = "\"all\" \"words\" \"any\" \"order\"";
    let r = find(&db, q);
    assert_eq!(r.hits.iter().map(|h| h.time).collect::<Vec<_>>(), [0.0, 100.0], "225 s is too far from 200 s");
    assert_eq!(marked(&r)[0], "[order] matters [any words] at [all]");
    assert_eq!(r.hits[1].lines, 4);
    assert_eq!(find(&db, &format!("{q} within:10")).hits.len(), 1);
    assert_eq!(find(&db, &format!("{q} within:1m")).hits.len(), 3, "200 s to 225 s is in a minute");
    assert_eq!(r.query.within, None);
    assert_eq!(find(&db, &format!("{q} within:1m")).query.within, Some(60.0));
    // The filter's window is the default the typed one overrides.
    let filter = |within| SearchFilter { within: Some(within), ..Default::default() };
    assert_eq!(db.search(q, &filter(60.0), 500).unwrap().hits.len(), 3);
    assert_eq!(db.search(&format!("{q} within:10"), &filter(60.0), 500).unwrap().hits.len(), 1);
}

/// The tightest set of a match's pieces is the match, and a match is shown
/// once (decided for 0.6.0: the tightest set is kept, and a match is kept
/// only if none of its words is in one kept already; search.rs, the doc
/// comments of `keep` and `Found::key`: "a line saying the phrase twice is
/// one hit").
#[test]
fn the_tightest_set_and_no_match_twice() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "damn");
    say(&mut db, v, 8.0, "damn");
    say(&mut db, v, 10.0, "nice shot");
    let r = find(&db, "nice shot \"damn\"");
    assert_eq!(r.hits.iter().map(|h| (h.time, h.lines)).collect::<Vec<_>>(), [(8.0, 2)], "the nearer damn");
    // The same where damn is the rarest word, looked up first: each damn
    // makes a match with the phrase, and the tighter one is kept.
    let u = vod(&db, "d", "s");
    say(&mut db, u, 0.0, "damn");
    say(&mut db, u, 8.0, "damn");
    say(&mut db, u, 10.0, "nice shot");
    for t in [500.0, 600.0, 700.0, 800.0] {
        say(&mut db, u, t, "nice shot");
    }
    let filter = SearchFilter { vod_id: Some(u), ..Default::default() };
    let r = db.search("nice shot \"damn\"", &filter, 10).unwrap();
    assert_eq!(r.hits.iter().map(|h| (h.time, h.lines)).collect::<Vec<_>>(), [(8.0, 2)]);
    // Two pieces, each twice in a row: two matches, not four.
    let w = vod(&db, "b", "s");
    say(&mut db, w, 0.0, "ping pong ping pong");
    let r = find(&db, "\"ping\" \"pong\"");
    assert_eq!(marked(&r), ["[ping pong ping pong]"], "two matches of one line: one hit, both marked");
    // Saying the phrase twice in a line is one hit, as before.
    let x = vod(&db, "c", "s");
    say(&mut db, x, 0.0, "clutch play and another clutch play");
    assert_eq!(marked(&find(&db, "clutch play")), ["[clutch play] and another [clutch play]"]);
}

/// A word given as two pieces is two occurrences of it: pieces never
/// share a matched word (search.rs, the doc comment of `Stream::tightest`:
/// "no two sharing a word (`"nice" "nice"` is two of them)").
#[test]
fn pieces_never_share_a_word() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "nice shot");
    say(&mut db, v, 100.0, "nice");
    say(&mut db, v, 105.0, "very nice");
    say(&mut db, v, 200.0, "good shot");
    let times = |q: &str| find(&db, q).hits.iter().map(|h| (h.time, h.lines)).collect::<Vec<_>>();
    assert_eq!(times("\"nice\" \"nice\""), [(100.0, 2)], "not the nice at 0 s, alone");
    assert_eq!(marked(&find(&db, "\"nice\" \"nice\"")), ["[nice] very [nice]"]);
    assert!(find(&db, "\"nice\" \"nice\" within:3").hits.is_empty(), "the two are 5 s apart");
    assert!(find(&db, "\"good shot\" \"shot\"").hits.is_empty(), "one shot can't be both");
    assert_eq!(times("\"good\" \"shot\""), [(200.0, 1)], "two pieces, two words");
}

/// A search gives at most as many results as it's asked for, saying when
/// there were more, and a line found twice counts once (db.rs, the doc
/// comment of `Db::search`: "at most `limit`, and `truncated` if there were
/// more"; search.rs, the doc comment of `keep`: "`out` counts hits as
/// they're shown (a line saying the word twice is one)").
#[test]
fn a_word_twice_in_a_line_is_one_hit_toward_the_limit() {
    let mut db = library();
    let (a, b) = (vod(&db, "a", "s"), vod(&db, "b", "s"));
    for v in [a, b] {
        for t in [0.0, 30.0, 60.0] {
            say(&mut db, v, t, "hype hype");
        }
    }
    let r = db.search("hype", &SearchFilter::default(), 4).unwrap();
    assert_eq!((r.hits.len(), r.truncated), (4, true), "on into the second VOD, and more there");
    let r = db.search("hype", &SearchFilter::default(), 6).unwrap();
    assert_eq!((r.hits.len(), r.truncated), (6, false));
}

/// OR finds either side, AND binds more tightly than OR, and brackets group
/// (CHANGELOG.md, 0.6.0: "OR finds either side.": "AND binds more tightly
/// than OR, and brackets group, as in `well that was OR damn`").
#[test]
fn or_and_brackets() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "well that was");
    say(&mut db, v, 50.0, "oh damn");
    say(&mut db, v, 100.0, "nice clutch");
    say(&mut db, v, 150.0, "good play");
    let times = |q: &str| find(&db, q).hits.iter().map(|h| h.time).collect::<Vec<_>>();
    assert_eq!(times("well that was OR damn"), [0.0, 50.3]);
    assert_eq!(times("nice (\"clutch\" OR \"play\")"), [100.0]);
    assert_eq!(times("\"good\" AND \"play\" OR \"oh\""), [50.0, 150.0]);
    assert_eq!(times("\"oh\" OR \"oh\""), [50.0], "the same match once");
    assert_eq!(times("\"nice\" OR \"clutch\""), [100.0], "matches in the same line are one hit");
}

/// `from:` a channel finds what it said and its own chat, `from:` a chatter
/// their chat, in any case; `@name` is words; a filter in a group is that
/// alternative's; the typed `in:` and `from:` win over the filter's; and the
/// names suggested (CHANGELOG.md, 0.6.0: "Search learns from, in, within,
/// after, before.": "`from:name` for what a channel said and wrote or a
/// chatter wrote", "`in:chat` or `in:audio`"; "OR finds either side.":
/// "`(from:alice "hi") OR (from:bob "gg")`"; "The search filter gains Within
/// and From whom.": "What you type wins over the filter"; e2e chat-search.mjs:
/// "`@Ada` is words to find (a mention), not a filter").
#[test]
fn who_said_it() {
    let mut db = library();
    let mine = vod(&db, "mine", "Streamer");
    let theirs = vod(&db, "theirs", "other");
    say(&mut db, mine, 5.0, "hello chat");
    say(&mut db, theirs, 5.0, "hello chat");
    chat(&mut db, mine, 6.0, "streamer", "hello everyone");
    chat(&mut db, mine, 7.0, "Bob", "hello streamer");
    chat(&mut db, mine, 8.0, "bob", "gg");
    chat(&mut db, theirs, 9.0, "BOB", "hello again");
    chat(&mut db, theirs, 10.0, "alice", "hi bob");
    let hits = |q: &str| {
        let r = find(&db, q);
        assert_eq!(r.query.error, None, "{q}");
        r.hits.iter().map(|h| (h.vod_id, h.source, h.time)).collect::<Vec<_>>()
    };
    let (s, c) = (Source::Speech, Source::Chat);
    assert_eq!(hits("from:streamer hello"), [(mine, s, 5.0), (mine, c, 6.0)], "their speech and their chat");
    assert_eq!(hits("from:bob hello"), [(theirs, c, 9.0), (mine, c, 7.0)], "any case");
    assert_eq!(hits("@bob"), [(theirs, c, 10.0)], "@name is words");
    assert_eq!(hits("from:bob"), [(theirs, c, 9.0), (mine, c, 7.0), (mine, c, 8.0)], "a filter alone lists all");
    assert_eq!(hits("from:bob -hello"), [(mine, c, 8.0)]);
    assert_eq!(hits("(from:alice \"hi\") OR (from:bob \"gg\")"), [(theirs, c, 10.0), (mine, c, 8.0)]);
    assert_eq!(hits("hello in:audio"), [(theirs, s, 5.0), (mine, s, 5.0)]);
    assert_eq!(hits("from:bob from:alice -hello"), [(theirs, c, 10.0), (mine, c, 8.0)], "either name");
    // The typed in: wins over the filter's sources.
    let filter = SearchFilter { speech: true, chat: false, vod_id: Some(mine), ..Default::default() };
    let r = db.search("hello in:chat", &filter, 10).unwrap();
    assert_eq!(r.hits.iter().map(|h| h.time).collect::<Vec<_>>(), [6.0, 7.0]);
    assert_eq!(r.query.sources, Some(crate::query::Sources { audio: false, chat: true }));
    assert_eq!(find(&db, "from:bob in:chat").query.from.as_deref(), Some("bob"));
    // The filter's who, for alternatives that don't type their own.
    let from = |name: &str| SearchFilter { from: Some(name.into()), ..Default::default() };
    let by = |q: &str, f: &SearchFilter| {
        db.search(q, f, 10).unwrap().hits.iter().map(|h| (h.vod_id, h.source, h.time)).collect::<Vec<_>>()
    };
    assert_eq!(by("hello", &from("bob")), [(theirs, c, 9.0), (mine, c, 7.0)]);
    assert_eq!(by("hello", &from("streamer")), [(mine, s, 5.0), (mine, c, 6.0)]);
    assert_eq!(by("hello from:streamer", &from("bob")), [(mine, s, 5.0), (mine, c, 6.0)], "typed wins");
    assert_eq!(by("-hello", &from("bob")), [(mine, c, 8.0)], "a filter, so a minus alone lists");
    assert_eq!(by("(from:alice \"hi\") OR \"gg\"", &from("bob")), [(theirs, c, 10.0), (mine, c, 8.0)]);
    let names = |prefix: &str, vod_id| {
        let s = db.author_suggestions(prefix, vod_id).unwrap();
        s.into_iter().map(|s| (s.name.to_lowercase(), s.streamer, s.messages)).collect::<Vec<_>>()
    };
    assert_eq!(names("B", None), [("bob".to_string(), false, 3)]);
    assert_eq!(names("st", None), [("streamer".to_string(), true, 1)], "the channel, and its own chat");
    assert_eq!(names("", Some(theirs)).len(), 3, "other, bob and alice");
    assert!(names("zz", None).is_empty());
}

/// A name alone lists everything it passes: a channel's speech (every
/// line of its VODs) and its own chat, a chatter's chat; `in:` narrows
/// it, and several names mean any of them, typed or in the filter's From
/// whom (CHANGELOG.md, 0.6.0: "Search learns from, in, within, after,
/// before.": "`from:ada,marlowe` for several; alone it lists everything they
/// said and wrote"; "The search filter gains Within and From whom.": "takes
/// several separated by commas, and alone lists everything they said and
/// wrote"; decided for 0.6.0: filters alone list everything they pass,
/// capped as results are).
#[test]
fn a_name_alone_lists_what_they_said_and_wrote() {
    let mut db = library();
    let mine = vod(&db, "mine", "Streamer");
    let theirs = vod(&db, "theirs", "other");
    say(&mut db, mine, 5.0, "hello chat");
    say(&mut db, mine, 30.0, "what a play");
    say(&mut db, theirs, 5.0, "hello chat");
    chat(&mut db, mine, 6.0, "streamer", "hello everyone");
    chat(&mut db, mine, 7.0, "Bob", "hello streamer");
    chat(&mut db, theirs, 9.0, "alice", "hi bob");
    let hits = |q: &str| {
        let r = find(&db, q);
        assert_eq!(r.query.error, None, "{q}");
        r.hits.iter().map(|h| (h.vod_id, h.source, h.time)).collect::<Vec<_>>()
    };
    let (s, c) = (Source::Speech, Source::Chat);
    assert_eq!(hits("from:streamer"), [(mine, s, 5.0), (mine, c, 6.0), (mine, s, 30.0)]);
    assert_eq!(hits("from:streamer in:audio"), [(mine, s, 5.0), (mine, s, 30.0)]);
    assert_eq!(hits("from:streamer in:chat"), [(mine, c, 6.0)]);
    assert_eq!(hits("from:streamer after:10"), [(mine, s, 30.0)]);
    assert_eq!(hits("from:streamer -play"), [(mine, s, 5.0), (mine, c, 6.0)]);
    assert_eq!(hits("from:streamer within:1m -hello"), [], "the window, centred on each line, that the minus clears");
    assert_eq!(hits("from:streamer -hello"), [(mine, s, 30.0)]);
    let either = [(theirs, c, 9.0), (mine, s, 5.0), (mine, c, 6.0), (mine, s, 30.0)];
    assert_eq!(hits("from:streamer,alice"), either);
    assert_eq!(hits("from:alice from:streamer"), either);
    assert_eq!(hits("from:Bob,\"alice\" in:chat"), [(theirs, c, 9.0), (mine, c, 7.0)]);
    // The filter's From whom takes the same list, spaces round the commas allowed.
    let from = |names: &str| SearchFilter { from: Some(names.into()), ..Default::default() };
    let by = |q: &str, f: &SearchFilter| {
        db.search(q, f, 10).unwrap().hits.iter().map(|h| (h.vod_id, h.source, h.time)).collect::<Vec<_>>()
    };
    assert_eq!(by("in:chat", &from("Bob, \"alice\"")), [(theirs, c, 9.0), (mine, c, 7.0)]);
    assert_eq!(by("hello in:audio", &from("streamer,alice")), [(mine, s, 5.0)]);
    assert_eq!(by("in:chat from:alice", &from("Bob")), [(theirs, c, 9.0)], "typed wins");
    // With nothing typed, the filter's names alone list as typed ones do.
    assert_eq!(by("", &from("streamer")), hits("from:streamer"));
    assert_eq!(by(" ", &from("Bob, alice")), [(theirs, c, 9.0), (mine, c, 7.0)]);
    let chat_only = SearchFilter { speech: false, ..from("streamer") };
    assert_eq!(by("", &chat_only), [(mine, c, 6.0)]);
    assert!(find(&db, "").hits.is_empty(), "nothing typed, nothing set: no search");
    assert!(by("", &from(" , ")).is_empty());
    // Capped as every search is, VOD by VOD.
    for i in 0..20 {
        say(&mut db, theirs, 100.0 + i as f64, "more talk");
    }
    let r = db.search("from:other,streamer", &SearchFilter::default(), 10).unwrap();
    assert_eq!((r.hits.len(), r.truncated), (10, true));
    assert!(r.hits.iter().all(|h| h.vod_id == theirs && h.source == s), "the first VOD's lines first");
}

/// `after:` and `before:` are times into the VOD: where a match in speech
/// starts, when a message was sent (CHANGELOG.md, 0.6.0: "Search learns
/// from, in, within, after, before.": "`after:1:20:00` or `before:2h` for
/// where in the VOD"; decided for 0.6.0: a match in speech by its start, a
/// message by when it was sent).
#[test]
fn times_into_the_vod() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    for t in [10.0, 4000.0, 8000.0] {
        say(&mut db, v, t, "gg");
        chat(&mut db, v, t, "x", "gg");
    }
    let times = |q: &str| find(&db, q).hits.iter().map(|h| h.time).collect::<Vec<_>>();
    assert_eq!(times("gg after:1h before:2h"), [4000.0, 4000.0]);
    assert_eq!(times("gg after:1:00:00 in:audio"), [4000.0, 8000.0]);
    assert_eq!(times("before:20 in:chat"), [10.0], "filters alone");
    let r = find(&db, "after:1:20:00 gg");
    assert_eq!((r.query.after, r.query.before), (Some(4800.0), None));
}

/// A search that can't be read says what to fix and finds nothing; a minus
/// alone finds nothing and says to add a word or a filter; whatever is
/// typed, only words reach the index (CHANGELOG.md, 0.6.0: "Searches show
/// how they were read.": "a search VODscribe can't read, such as one with a
/// quote left open, says what to fix there instead"; decided for 0.6.0: a
/// minus alone, with no word or filter, finds nothing and says to add a word
/// or a filter; search.rs `Db::search_speech`: "quoting one can't inject FTS
/// syntax").
#[test]
fn errors_and_nothing_to_look_for() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "a clutch play or two");
    for q in ["\"clutch", "(clutch OR play", "clutch)", "in:video clutch"] {
        let r = find(&db, q);
        assert!(r.hits.is_empty() && r.query.error.is_some(), "{q}: {:?}", r.query.error);
    }
    let r = find(&db, "-clutch");
    assert!(r.hits.is_empty());
    assert!(r.query.error.as_deref().unwrap().contains("Add a word or a filter"));
    let r = find(&db, "");
    assert!(r.hits.is_empty() && r.query.error.is_none() && r.query.tree.is_none());
    // Whatever is typed, only folded words reach the index.
    for q in ["NEAR(clutch play)", "clutch* play", "{text}: clutch", "clutch ^play", "c:lutch"] {
        assert!(find(&db, q).query.error.is_none(), "{q}");
    }
    assert_eq!(find(&db, "clutch* play").hits.len(), 1);
    assert!(find(&db, "zyzzyva clutch").hits.is_empty(), "a word in no line");
}

/// The window is at most an hour, typed or the filter's: wider says so and
/// finds nothing (query.rs, the doc comment of `MOST_WITHIN`: "The widest
/// window, typed or the filter's: an hour"; `TOO_WIDE`).
#[test]
fn the_filters_window_is_at_most_an_hour_too() {
    let mut db = library();
    let v = vod(&db, "a", "s");
    say(&mut db, v, 0.0, "a clutch play");
    say(&mut db, v, 3000.0, "and another play");
    let filter = |within| SearchFilter { within: Some(within), ..Default::default() };
    let r = db.search("\"clutch\" \"another\"", &filter(MOST_WITHIN), 500).unwrap();
    assert_eq!((r.hits.len(), r.query.error), (1, None));
    let r = db.search("clutch", &filter(MOST_WITHIN + 1.0), 500).unwrap();
    assert!(r.hits.is_empty());
    assert_eq!(r.query.error.as_deref(), Some(TOO_WIDE));
    assert_eq!(find(&db, "clutch within:2h").query.error.as_deref(), Some(TOO_WIDE));
}

// An edit's chat.

/// An edit's chat is each piece's VOD's chat where that piece plays, its
/// struck stretches left out: the windows beside its transcript, how busy it
/// is, and its subs, gifts and tips (CHANGELOG.md, 0.5.0: "An edit's
/// transcript has chat beside it: each piece's VOD's chat where that piece
/// plays, and the minimap shows how busy it is over the edit", "subs, gifts
/// and tips ... show where they play"; edits.rs, the doc comments of
/// `edit_chat_windows`, `edit_chat_histogram` and `edit_chat_events`).
#[test]
fn an_edits_chat_is_its_pieces_vods_where_they_play() {
    let mut db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let at = [5.0, 11.0, 13.0, 15.0, 31.0];
    let messages: Vec<_> = at.iter().map(|&t| message(t, "a", &format!("at {t}"))).collect();
    db.add_chat(vod, &messages, &[]).unwrap();
    let edit = db.add_edit("e").unwrap().id;
    // 10-20 s with 12-14 taken out (plays 0-2, 2-8), then 30-35 s (8-13).
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
    db.remove_edit_stretch(edit, 2.0, 4.0).unwrap();
    let w = db.edit_chat_windows(edit, &[(0.0, 13.0), (2.0, 9.5)], 2).unwrap();
    let times = |i: usize| w[i].messages.iter().map(|m| m.time).collect::<Vec<_>>();
    assert_eq!((times(0), w[0].total), (vec![1.0, 3.0], 3), "11 and 15 s; 31 s counted; 13 s taken out");
    assert_eq!((times(1), w[1].total), (vec![3.0, 9.0], 2));
    assert_eq!(w[0].messages[0].platform.as_deref(), Some("youtube"));
    let (count, histogram) = db.edit_chat_histogram(edit, 13.0, 13).unwrap();
    assert_eq!(count, 3);
    assert_eq!((histogram[1], histogram[3], histogram[9]), (1, 1, 1));
    // Before the page knows the edit's length, it still learns there's chat.
    assert_eq!(db.edit_chat_histogram(edit, 0.0, 13).unwrap(), (3, vec![0; 13]));
    // Its events, where they play: a sub at 16 s (4 s in) and one at 13 s (taken out).
    let sub = |time| ChatMessage { kind: Kind::Sub, amount: Some(1.0), ..message(time, "b", "subbed") };
    db.add_chat(vod, &[sub(16.0), sub(13.0)], &[]).unwrap();
    let events = db.edit_chat_events(edit).unwrap();
    assert_eq!(events.iter().map(|e| e.time).collect::<Vec<_>>(), [4.0]);
    assert_eq!(events[0].platform.as_deref(), Some("youtube"));
}

// The search's own connection.

/// SQL that runs for a long while (seconds), unless cut short.
fn long(db: &Db) -> anyhow::Result<i64> {
    Ok(db.conn.query_row(
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 200000000) SELECT count(*) FROM n",
        [],
        |r| r.get(0),
    )?)
}

/// SQL of a few million steps: a search cut short partway, or by a handler
/// left behind, ends in an error.
fn long_enough(db: &Db) -> anyhow::Result<i64> {
    Ok(db.conn.query_row(
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 2000000) SELECT count(*) FROM n",
        [],
        |r| r.get(0),
    )?)
}

/// `searcher.run(typed, f)` on a thread of its own.
fn run_apart<T: Send + 'static>(
    searcher: &Arc<Searcher>,
    typed: bool,
    f: impl FnOnce(&Db) -> anyhow::Result<T> + Send + 'static,
) -> std::thread::JoinHandle<Option<T>> {
    let searcher = searcher.clone();
    std::thread::spawn(move || searcher.run(typed, f).unwrap())
}

/// A typed search overtaken by a newer one while it waits for the
/// connection doesn't run; the newest does; and one not typed (a test's,
/// say) runs whole while typed ones come (searcher.rs, the module doc:
/// "only the newest is wanted: a typed search overtaken by a newer one while
/// it waits for the connection doesn't run"; the doc comment of `run`:
/// "Others (a test's, say) always run whole").
#[test]
fn a_typed_search_overtaken_while_it_waits_never_runs() {
    let searcher = Arc::new(Searcher::new(library()));
    // One not typed holds the connection till the test lets it go on.
    let on = Gate::new();
    let busy = {
        let on = on.clone();
        run_apart(&searcher, false, move |db| {
            on.wait();
            long_enough(db)
        })
    };
    on.reached();
    let ran = Arc::new(std::sync::Mutex::new(Vec::new()));
    let typed = |n: u32| {
        let ran = ran.clone();
        run_apart(&searcher, true, move |_| {
            ran.lock().unwrap().push(n);
            Ok(n)
        })
    };
    // Each typed one asked, then held before the connection, the second after the first.
    let (first_asked, second_asked) = (Gate::new(), Gate::new());
    searcher.holds(&first_asked);
    let first = typed(1);
    first_asked.reached();
    searcher.holds(&second_asked);
    let second = typed(2);
    second_asked.reached();
    first_asked.open();
    second_asked.open();
    on.open();
    assert_eq!(busy.join().unwrap(), Some(2_000_000), "the one not typed ran whole");
    assert_eq!(first.join().unwrap(), None);
    assert_eq!(second.join().unwrap(), Some(2));
    assert_eq!(*ran.lock().unwrap(), [2], "only the newest ran");
}

/// A typed search that fails, not overtaken, says why (searcher.rs, the doc
/// comment of `run`: "Ok(None) if that came before it began, or while it
/// ran", any other failure its own).
#[test]
fn a_typed_search_that_fails_says_why() {
    let searcher = Searcher::new(library());
    let failed =
        searcher.run(true, |db| Ok(db.conn.query_row("SELECT * FROM no_such_table", [], |r| r.get::<_, i64>(0))?));
    assert!(failed.unwrap_err().to_string().contains("no_such_table"));
}

/// A typed search overtaken by a newer one while it runs is cut short, and
/// the newer one runs; the cut leaves the connection as it was, no statement
/// or transaction open and nothing left to cut the next search short
/// (searcher.rs, the module doc: "one overtaken while it runs is cut
/// short"; the doc comment of `run`: "Ok(None) if that came before it began,
/// or while it ran").
#[test]
fn a_typed_search_overtaken_while_it_runs_is_cut_short() {
    let searcher = Arc::new(Searcher::new(library()));
    let on = Gate::new();
    let first = {
        let on = on.clone();
        run_apart(&searcher, true, move |db| {
            on.wait();
            long(db)
        })
    };
    // The first is on the connection, about to run, when the second is typed.
    on.reached();
    let asked = Gate::new();
    searcher.holds(&asked);
    let second = run_apart(&searcher, true, |_| Ok(7));
    asked.reached();
    on.open();
    assert_eq!(first.join().unwrap(), None, "cut short, with no answer");
    searcher.lets_go();
    assert_eq!(second.join().unwrap(), Some(7), "the newest runs");
    // A newer typed one asked, then one not typed run: a handler left behind
    // by either typed search would cut it short.
    let third_asked = Gate::new();
    searcher.holds(&third_asked);
    let third = run_apart(&searcher, true, |_| Ok(3));
    third_asked.reached();
    searcher.holds(&Gate::opened());
    let after = searcher.run(false, |db| Ok((db.conn.is_autocommit(), db.conn.is_busy(), long_enough(db)?))).unwrap();
    assert_eq!(after, Some((true, false, 2_000_000)));
    third_asked.open();
    assert_eq!(third.join().unwrap(), Some(3));
}
