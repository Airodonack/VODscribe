use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<Word>,
}

/// Merges sub-word tokens into words. Parakeet marks the start of a word with a
/// leading space; punctuation tokens attach to the preceding word.
pub fn tokens_to_words(tokens: &[String], starts: &[f32], durations: Option<&[f32]>, offset: f64) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    for (i, (token, &start)) in tokens.iter().zip(starts).enumerate() {
        let start = offset + start as f64;
        let end = match durations.and_then(|d| d.get(i)) {
            Some(&d) => start + d as f64,
            None => starts.get(i + 1).map_or(start + 0.1, |&s| offset + s as f64),
        };
        let starts_word = token.starts_with(' ') || words.is_empty();
        let text = token.trim_start();
        if text.is_empty() {
            continue;
        }
        match words.last_mut() {
            Some(w) if !starts_word => {
                w.text.push_str(text);
                w.end = end;
            }
            _ => words.push(Word { text: text.to_string(), start, end }),
        }
    }
    words
}

/// Groups a stream of words into sentence-sized segments for indexing.
pub struct Segmenter {
    current: Vec<Word>,
}

const MAX_WORDS: usize = 40;
const MAX_GAP: f64 = 1.5;

impl Segmenter {
    pub fn new() -> Self {
        Self { current: Vec::new() }
    }

    pub fn push(&mut self, words: Vec<Word>) -> Vec<Segment> {
        let mut out = Vec::new();
        for word in words {
            if let Some(last) = self.current.last()
                && word.start - last.end > MAX_GAP
            {
                out.extend(self.flush());
            }
            let ends_sentence = word.text.ends_with(['.', '?', '!', '…']);
            self.current.push(word);
            if (ends_sentence && self.current.len() >= 3) || self.current.len() >= MAX_WORDS {
                out.extend(self.flush());
            }
        }
        out
    }

    pub fn finish(mut self) -> Option<Segment> {
        self.flush()
    }

    fn flush(&mut self) -> Option<Segment> {
        if self.current.is_empty() {
            return None;
        }
        let words = std::mem::take(&mut self.current);
        let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        Some(Segment { start: words[0].start, end: words[words.len() - 1].end, text, words })
    }
}

impl Default for Segmenter {
    fn default() -> Self {
        Self::new()
    }
}

/// Splits text into lowercase, diacritic-free alphanumeric terms, mirroring
/// SQLite's `unicode61 remove_diacritics 2` tokenizer closely enough to locate
/// FTS matches inside a segment.
pub fn terms(text: &str) -> Vec<String> {
    let folded: String = text.nfd().filter(|c| !is_combining_mark(*c)).flat_map(char::to_lowercase).collect();
    folded.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// Finds each occurrence of `query` (a phrase whose last term may be a prefix)
/// in `words`, returning inclusive word-index ranges.
pub fn find_phrase(words: &[Word], query: &[String], last_is_prefix: bool) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    // Flatten words into terms, remembering which word each term came from.
    let mut flat: Vec<(String, usize)> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        flat.extend(terms(&w.text).into_iter().map(|t| (t, i)));
    }
    let mut hits = Vec::new();
    let n = query.len();
    let mut i = 0;
    while i + n <= flat.len() {
        let matched = query.iter().enumerate().all(|(j, q)| {
            let t = &flat[i + j].0;
            if last_is_prefix && j == n - 1 { t.starts_with(q.as_str()) } else { t == q }
        });
        if matched {
            hits.push((flat[i].1, flat[i + n - 1].1));
            i += n;
        } else {
            i += 1;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cases the website's demo search is tested against too
    /// (apps/site/src/lib/tauri/search.test.mjs), so the two stay the same.
    #[test]
    fn shared_phrase_search_cases() {
        #[derive(serde::Deserialize)]
        struct Case {
            about: String,
            words: Vec<String>,
            query: String,
            ranges: Vec<(usize, usize)>,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../tests/phrase-search.json")).unwrap();
        for c in cases {
            let words: Vec<Word> = c.words.into_iter().map(|text| Word { text, start: 0.0, end: 0.0 }).collect();
            assert_eq!(find_phrase(&words, &terms(&c.query), true), c.ranges, "{}", c.about);
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn merges_subword_tokens() {
        let tokens = s(&[" A", "sk", " not", ",", " what"]);
        let words = tokens_to_words(&tokens, &[0.0, 0.24, 0.4, 0.6, 0.64], Some(&[0.24, 0.16, 0.2, 0.04, 0.16]), 10.0);
        let texts: Vec<_> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["Ask", "not,", "what"]);
        assert_eq!(words[0].start, 10.0);
        assert!((words[0].end - 10.4).abs() < 1e-6);
        assert!((words[1].end - 10.64).abs() < 1e-6);
    }

    #[test]
    fn segments_on_sentences_and_gaps() {
        let w = |t: &str, start: f64| Word { text: t.into(), start, end: start + 0.2 };
        let mut seg = Segmenter::new();
        let mut out = seg.push(vec![w("Hi", 0.0), w("there", 0.3), w("friend.", 0.6), w("Next", 0.9)]);
        out.extend(seg.push(vec![w("after", 5.0), w("gap", 5.3)]));
        out.extend(seg.finish());
        let texts: Vec<_> = out.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Hi there friend.", "Next", "after gap"]);
    }

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
}
