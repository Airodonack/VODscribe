//! The specs of transcript.rs: the model's tokens made words, words made
//! the lines the library keeps, the terms a search matches, and captions.

use super::*;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// A word said at `start`, for 0.3 s.
fn w(text: &str, start: f64) -> Word {
    Word { text: text.into(), start, end: start + 0.3 }
}

/// A line of the transcript made of `words`, as the library keeps one.
fn line(words: Vec<Word>) -> Segment {
    Segment {
        start: words[0].start,
        end: words.last().unwrap().end,
        text: words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
        words,
    }
}

fn texts(cues: &[Cue]) -> Vec<&str> {
    cues.iter().map(|c| c.text.as_str()).collect()
}

// Tokens to words.

/// Parakeet's tokens make a word from one that starts with a space up to
/// the next that does (transcript.rs `tokens_to_words`: "Merges sub-word
/// tokens into words. Parakeet marks the start of a word with a leading
/// space").
#[test]
fn a_token_after_a_space_starts_a_word_and_the_rest_join_it() {
    let words = tokens_to_words(&s(&[" A", "sk", " not", " what", "ever"]), &[0.0, 0.2, 0.4, 0.6, 0.8], None, 0.0);
    let texts: Vec<_> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, ["Ask", "not", "whatever"]);
}

/// Punctuation goes with the word before it (transcript.rs
/// `tokens_to_words`: "punctuation tokens attach to the preceding word").
#[test]
fn punctuation_goes_with_the_word_before_it() {
    let words = tokens_to_words(&s(&[" not", ",", " what", "?"]), &[0.0, 0.2, 0.4, 0.6], None, 0.0);
    let texts: Vec<_> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, ["not,", "what?"]);
}

/// A word is said from its first token's start to its last token's end, at
/// the VOD's time its audio began (transcript.rs `tokens_to_words`: "Merges
/// sub-word tokens into words"; asr.rs `words_of` passes the chunk's start as
/// `offset`).
#[test]
fn a_word_lasts_from_its_first_token_to_its_last_at_the_vods_time() {
    let tokens = s(&[" A", "sk", " not", ",", " what"]);
    let words = tokens_to_words(&tokens, &[0.0, 0.24, 0.4, 0.6, 0.64], Some(&[0.24, 0.16, 0.2, 0.04, 0.16]), 10.0);
    assert_eq!(words[0].start, 10.0);
    assert!((words[0].end - 10.4).abs() < 1e-6, "{}", words[0].end);
    assert!((words[1].start - 10.4).abs() < 1e-6, "{}", words[1].start);
    assert!((words[1].end - 10.64).abs() < 1e-6, "{}", words[1].end);
    assert!((words[2].end - 10.8).abs() < 1e-6, "{}", words[2].end);
}

// Words to lines.

/// Lines end at a sentence's end, a sentence said without a pause is one
/// line however many words it has, and a long pause starts a new one
/// (transcript.rs `Segmenter`: "Groups a stream of words into
/// sentence-sized segments for indexing"). The pause's length (1.5 s), the
/// three words a sentence needs and the 40 words a line holds at most are
/// the code's alone, said nowhere.
#[test]
fn segments_on_sentences_and_gaps() {
    let w = |t: &str, start: f64| Word { text: t.into(), start, end: start + 0.2 };
    let mut seg = Segmenter::new();
    let mut out = seg.push(vec![w("Hi", 0.0), w("there", 0.3), w("friend.", 0.6), w("Next", 0.9)]);
    out.extend(seg.push(vec![w("after", 5.0), w("gap", 5.3)]));
    out.extend(seg.finish());
    let texts: Vec<_> = out.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["Hi there friend.", "Next", "after gap"]);

    let mut seg = Segmenter::new();
    let words = ["So", "the", "eagle", "has", "landed."].iter().enumerate();
    let out = seg.push(words.map(|(i, t)| w(t, i as f64 * 0.35)).collect());
    let texts: Vec<_> = out.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["So the eagle has landed."], "one sentence, said without a pause");
}

/// Words given a few at a time, as the worker sends them, make the lines
/// they make given all at once (transcript.rs `Segmenter`: "Groups a stream
/// of words").
#[test]
fn words_streamed_a_few_at_a_time_make_the_lines_they_make_at_once() {
    let words: Vec<Word> = "So the eagle has landed. It really has! And then , after a long while , we went home ."
        .split(' ')
        .enumerate()
        .map(|(i, t)| w(t, i as f64 * 0.4 + if i >= 10 { 3.0 } else { 0.0 }))
        .collect();
    let lines = |batch: usize| {
        let mut seg = Segmenter::new();
        let mut out: Vec<String> = words.chunks(batch).flat_map(|b| seg.push(b.to_vec())).map(|s| s.text).collect();
        out.extend(seg.finish().map(|s| s.text));
        out
    };
    let at_once = lines(words.len());
    assert!(at_once.len() >= 3, "{at_once:?}");
    for batch in [1, 2, 3, 7] {
        assert_eq!(lines(batch), at_once, "{batch} at a time");
    }
}

/// What a stop resumes from: the words held back for the next line start
/// where `pending_start` says, and every word before is in the lines given
/// out (transcript.rs `Segmenter::pending_start`: "Where the words held back
/// for the next segment start, if any: what's before is all in the segments
/// given out").
#[test]
fn the_words_held_back_start_where_pending_start_says() {
    let mut seg = Segmenter::new();
    assert_eq!(seg.pending_start(), None, "nothing held yet");
    let out = seg.push(vec![w("Hi", 0.0), w("there", 0.4), w("friend.", 0.8), w("And", 1.2), w("then", 1.6)]);
    let given: Vec<f64> = out.iter().flat_map(|s| s.words.iter().map(|w| w.start)).collect();
    assert_eq!(given, [0.0, 0.4, 0.8]);
    assert_eq!(seg.pending_start(), Some(1.2));
    let out = seg.push(vec![w("home.", 2.0)]);
    assert_eq!(out.len(), 1);
    assert_eq!(seg.pending_start(), None, "all given out");
}

// Terms and phrases.

/// The cases the website's demo search is tested against too
/// (apps/site/src/lib/tauri/search.test.mjs), so the two stay the same
/// (crates/core/tests/phrase-search.json; its specs are 3A2's).
#[test]
fn shared_phrase_search_cases() {
    #[derive(serde::Deserialize)]
    struct Case {
        about: String,
        words: Vec<String>,
        query: String,
        ranges: Vec<(usize, usize)>,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("../../tests/phrase-search.json")).unwrap();
    for c in cases {
        let words: Vec<Word> = c.words.into_iter().map(|text| Word { text, start: 0.0, end: 0.0 }).collect();
        let q = terms(&c.query);
        assert_eq!(find_phrase(&words, &q, last_is_prefix(&q)), c.ranges, "{}", c.about);
    }
}

/// Terms are lowercase, without accents, split at what isn't a letter or a
/// digit; a phrase is found by its terms, as inclusive ranges of the words
/// they're in, its last term a prefix when asked (transcript.rs `terms`:
/// "Splits text into lowercase, diacritic-free alphanumeric terms";
/// `find_phrase`: "Finds each occurrence of `query` (a phrase whose last term
/// may be a prefix) in `words`, returning inclusive word-index ranges").
#[test]
fn finds_phrases_like_fts() {
    let words: Vec<Word> = ["That's", "a", "Café", "moment,", "don't", "you", "think?"]
        .iter()
        .enumerate()
        .map(|(i, t)| Word { text: t.to_string(), start: i as f64, end: i as f64 + 0.5 })
        .collect();
    assert_eq!(terms("Don't CAFÉ!"), s(&["don", "t", "cafe"]));
    assert_eq!(find_phrase(&words, &terms("cafe moment"), false), [(2, 3)]);
    assert_eq!(find_phrase(&words, &terms("don't you th"), true), [(4, 6)]);
    assert_eq!(find_phrase(&words, &terms("don't you th"), false), []);
    assert_eq!(find_phrase(&words, &terms("s a"), false), [(0, 1)]);
}

/// The texts among `texts` whose terms aren't the ones the library's search
/// index makes of them (db.rs's tokenizer, in SQLite itself), each with both.
fn unlike_the_index(texts: &[&str]) -> Vec<String> {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE VIRTUAL TABLE t USING fts5 (text, tokenize = 'unicode61 remove_diacritics 2');
         CREATE VIRTUAL TABLE v USING fts5vocab (t, 'instance');",
    )
    .unwrap();
    let mut differ = Vec::new();
    for text in texts {
        db.execute("DELETE FROM t", []).unwrap();
        db.execute("INSERT INTO t (text) VALUES (?1)", [text]).unwrap();
        let mut stmt = db.prepare("SELECT term FROM v ORDER BY offset").unwrap();
        let indexed: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        if terms(text) != indexed {
            differ.push(format!("{text}: {:?}, the index {indexed:?}", terms(text)));
        }
    }
    differ
}

/// A line's terms are the ones the library's search index makes of it, so
/// what the index finds is found again in the line's words: accents, case,
/// apostrophes, hyphens and numbers (transcript.rs `terms`: "mirroring
/// SQLite's `unicode61 remove_diacritics 2` tokenizer closely enough to
/// locate FTS matches inside a segment").
#[test]
fn terms_are_the_ones_the_search_index_makes() {
    let differ = unlike_the_index(&[
        "Don't CAFÉ!",
        "Le café était fermé.",
        "an e-sports team, launched in 2026",
        "Ångström naïve façade Ørsted crème brûlée",
        "ÉCOLE Ça ŞEHİR ĞÜÇ",
        "under_score dot.ted slash/ed (brackets) \"quotes\"",
        "Straße ŒUVRE Кириллица",
        "x1 2b 3.5 4,000 #5 @six 🚀🚀",
    ]);
    assert!(differ.is_empty(), "{differ:#?}");
}

/// The same in other scripts: Greek with its accents, Korean, Japanese
/// kana with their voicing marks, and the newer emoji (transcript.rs
/// `terms`, as above). Today `terms` takes Greek's accents and kana's marks
/// off where the index keeps them, leaves Korean's syllables in pieces, and
/// drops emoji the index keeps as words; a search reaches the index with
/// `terms`' own (search.rs `fts_phrase`), so such words can't be found.
#[test]
#[ignore = "disagreement: terms differs from the search index for Greek accents, Hangul, kana voicing marks and newer emoji"]
fn terms_are_the_ones_the_search_index_makes_in_every_script() {
    let differ = unlike_the_index(&["Ελληνικά", "한국어 방송", "ガンダム だよ", "the 🦅 landed"]);
    assert!(differ.is_empty(), "{differ:#?}");
}

// Captions.

/// A caption ends with its sentence (transcript.rs `cues`: "cut from their
/// words: at a sentence's end").
#[test]
fn a_caption_ends_at_a_sentences_end() {
    let cues = cues(&[line(vec![w("Hello", 0.0), w("there.", 0.4), w("How", 0.8), w("are", 1.2), w("you?", 1.6)])]);
    assert_eq!(texts(&cues), ["Hello there.", "How are you?"]);
    let cues = super::cues(&[line(vec![w("Wait!", 0.0), w("What", 0.4), w("now…", 0.8), w("ok", 1.2)])]);
    assert_eq!(texts(&cues), ["Wait!", "What now…", "ok"]);
}

/// A caption ends at a pause of more than a second (transcript.rs `cues`:
/// "at a pause"; `CUE_GAP`: "A cue ends at a pause this long").
#[test]
fn a_caption_ends_at_a_pause_of_over_a_second() {
    // "are" ends at 1.5: "you" a second later is the same caption, a moment more isn't.
    let cues = cues(&[line(vec![w("How", 0.8), w("are", 1.2), w("you", 2.5), w("then", 3.9)])]);
    assert_eq!(texts(&cues), ["How are you", "then"]);
}

/// A caption holds two lines of the usual subtitle width, 84 characters,
/// and the next word starts the next one (transcript.rs `Cue`: "at most two
/// short lines"; `cues`: "before running past `CUE_CHARS`"; `CUE_CHARS`:
/// "two lines of the usual subtitle width").
#[test]
fn a_caption_holds_at_most_84_characters() {
    let long: Vec<Word> = (0..30).map(|i| w("wordy", 20.0 + i as f64 * 0.15)).collect();
    let cues = cues(&[line(long)]);
    let wordy = |n: usize| vec!["wordy"; n].join(" ");
    assert_eq!(texts(&cues), [wordy(14), wordy(14), wordy(2)]);
    assert!(cues.iter().all(|c| c.text.chars().count() <= 84));
    // 84 exactly is still one caption.
    let mut words: Vec<Word> = (0..12).map(|i| w("abcdef", i as f64 * 0.15)).collect();
    words[11].text = "abcdefg".into();
    assert_eq!(super::cues(&[line(words)]).len(), 1, "84 characters fit");
    // Counted in characters, not bytes.
    let wide: Vec<Word> = (0..30).map(|i| w("éèàçù", 20.0 + i as f64 * 0.15)).collect();
    assert_eq!(super::cues(&[line(wide)]).len(), 3);
}

/// A word too long for a caption is a caption of its own, the words around
/// it in theirs (transcript.rs `cues`: "before running past `CUE_CHARS`";
/// 0.4.1 Fixes: "Captions blurred at 125% or 150% display scale, and a very
/// long word ran out of them", whose wrapping within the caption is
/// textWrap.ts's).
#[test]
fn a_word_too_long_for_a_caption_is_a_caption_of_its_own() {
    let long = "a".repeat(100);
    let cues = cues(&[line(vec![w("so", 0.0), w("it", 0.3), w(&long, 0.6), w("then", 0.9), w("ok", 1.2)])]);
    assert_eq!(texts(&cues), ["so it", long.as_str(), "then ok"]);
}

/// Past half a caption's room, a comma, semicolon or colon ends it; before,
/// it doesn't (transcript.rs `cues`: "or at a comma once a line is full").
#[test]
fn a_caption_ends_at_a_comma_once_a_line_is_full() {
    // Seven words of 5 characters, a space each, are 42: a line's worth.
    let words = |stop: &str| {
        let mut words: Vec<Word> = (0..12).map(|i| w("abcde", i as f64 * 0.2)).collect();
        words[2].text = format!("abcd{stop}");
        words[7].text = format!("abcd{stop}");
        words
    };
    for stop in [",", ";", ":"] {
        let cues = cues(&[line(words(stop))]);
        let ends = format!("abcd{stop}");
        assert_eq!(cues.len(), 2, "{stop}: {:?}", texts(&cues));
        assert!(cues[0].text.ends_with(&ends) && cues[0].text.split(' ').count() == 8, "{:?}", texts(&cues));
    }
    assert_eq!(cues(&[line(words("x"))]).len(), 1, "no comma, no cut");
}

/// A caption is up 6 seconds at most: a word that would keep it up longer
/// starts the next (transcript.rs `CUE_GAP` and `CUE_LONGEST`: "A cue ends
/// at a pause this long, or once it has been up this long").
#[test]
fn a_caption_ends_once_it_has_been_up_six_seconds() {
    let words: Vec<Word> = (0..16).map(|i| w("a", i as f64 * 0.5)).collect();
    let cues = cues(&[line(words)]);
    assert_eq!(texts(&cues), [["a"; 12].join(" "), ["a"; 4].join(" ")]);
    assert_eq!(cues[1].start, 6.0);
}

/// A caption stays up 0.6 s after its last word is said (transcript.rs
/// `CUE_HOLD`: "How long a cue stays up after its last word, unless the
/// next one comes first").
#[test]
fn a_caption_stays_up_a_moment_after_its_last_word() {
    let cues = cues(&[line(vec![w("How", 0.8), w("are", 1.2), w("you", 3.0)])]);
    assert!((cues[0].end - (1.5 + CUE_HOLD)).abs() < 1e-9, "{}", cues[0].end);
    assert!((cues[1].end - (3.3 + CUE_HOLD)).abs() < 1e-9, "{}", cues[1].end);
}

/// A caption held after its last word gives way to the next as it starts
/// (transcript.rs `cues`: "Held a moment after the last word, but never
/// over the next cue").
#[test]
fn a_caption_never_stays_over_the_next() {
    let cues = cues(&[line(vec![w("Hello", 0.0), w("there.", 0.4), w("How", 0.8)])]);
    assert_eq!((cues[0].start, cues[0].end), (0.0, 0.8));
    assert_eq!(cues[1].start, 0.8);
}

/// A line kept without its words' times is one caption of its text, for
/// as long as the line (transcript.rs `cues`: "Captions for `segments`").
#[test]
fn a_line_without_its_words_is_one_caption() {
    let cues = cues(&[Segment { start: 40.0, end: 41.0, text: "no words".into(), words: Vec::new() }]);
    assert_eq!(cues, [Cue { start: 40.0, end: 41.0 + CUE_HOLD, text: "no words".into() }]);
}
