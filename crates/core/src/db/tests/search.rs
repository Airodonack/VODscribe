//! Search over the library's transcripts and chat.

use super::*;

/// How search results mark their matches, against the cases the website's
/// demo is tested against too (crates/core/tests/phrase-search.json):
/// matched runs in [brackets].
#[test]
fn shared_highlight_cases() {
    #[derive(serde::Deserialize)]
    struct Case {
        about: String,
        words: Vec<String>,
        ranges: Vec<(usize, usize)>,
        marked: String,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../../tests/phrase-search.json")).unwrap();
    for c in cases {
        let words: Vec<Word> = c.words.into_iter().map(|text| Word { text, start: 0.0, end: 0.0 }).collect();
        let marked: String = crate::search::highlight(&words, &c.ranges)
            .into_iter()
            .map(|p| if p.hit { format!("[{}]", p.text) } else { p.text })
            .collect();
        assert_eq!(marked, c.marked, "{}", c.about);
    }
}

#[test]
fn index_and_search() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let (vod, created) = db.add_vod(&info("a")).unwrap();
    assert!(created);
    assert!(!db.add_vod(&info("a")).unwrap().1);

    let mut seg = Segmenter::new();
    let mut segs = seg.push(words("Welcome back everyone.", 0.0));
    segs.extend(seg.push(words("That was a clutch play, honestly.", 10.0)));
    db.insert_segments(vod.id, &segs).unwrap();

    let r = db.search("CLUTCH play", &SearchFilter::default(), 10).unwrap();
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].time, 13.0);
    let parts: Vec<_> = r.hits[0].parts.iter().map(|p| (p.text.as_str(), p.hit)).collect();
    assert_eq!(parts, [("That was a ", false), ("clutch play,", true), (" honestly.", false)]);

    assert_eq!(db.search("welc", &SearchFilter::default(), 10).unwrap().hits.len(), 1, "prefix match");
    assert!(db.search("play clutch", &SearchFilter::default(), 10).unwrap().hits.is_empty(), "phrase order");
    assert!(db.search("\" OR *", &SearchFilter::default(), 10).unwrap().hits.is_empty(), "no FTS syntax injection");

    db.delete_vod(vod.id).unwrap();
    assert!(db.search("clutch", &SearchFilter::default(), 10).unwrap().hits.is_empty(), "cascade clears index");
}

#[test]
fn an_edits_chat_is_its_pieces_vods_where_they_play() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
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

#[test]
fn one_letter_matches_a_word_and_a_vods_search_keeps_to_it() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let a = db.add_vod(&info("a")).unwrap().0.id;
    let b = db.add_vod(&info("b")).unwrap().0.id;
    // Saved in turns, so b's messages sit between a's ids.
    db.add_chat(a, &[message(1.0, "v", "an apple a day")], &[1.0]).unwrap();
    db.add_chat(b, &[message(2.0, "v", "apple pie")], &[2.0]).unwrap();
    db.add_chat(a, &[message(3.0, "v", "apples again")], &[3.0]).unwrap();
    let chat = |query: &str, vod_id: Option<i64>| {
        let filter = SearchFilter { vod_id, speech: false, chat: true, ..Default::default() };
        db.search(query, &filter, 10).unwrap().hits.iter().map(|h| (h.vod_id, h.time)).collect::<Vec<_>>()
    };
    assert_eq!(chat("a", None), [(a, 1.0)], "one letter: the word, not every word starting with it");
    assert_eq!(chat("ap", Some(a)), [(a, 1.0), (a, 3.0)], "a's own, though b's lie between them");
    assert_eq!(chat("ap", Some(b)), [(b, 2.0)]);
    let ids: Vec<f64> = db.chat_matches(a, &["apple".into()]).unwrap().into_iter().map(|(_, t)| t).collect();
    assert_eq!(ids, [1.0], "matches keep to the VOD too");
}

#[test]
fn a_chat_search_is_limited_too() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let a = db.add_vod(&info("a")).unwrap().0.id;
    let chat: Vec<_> = (0..5).map(|i| message(i as f64, "v", "hype")).collect();
    db.set_chat(a, &chat).unwrap();
    let r = db.search("hype", &SearchFilter::default(), 3).unwrap();
    assert_eq!((r.hits.len(), r.truncated), (3, true));
}
