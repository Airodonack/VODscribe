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

/// Whether a search's last term matches as a prefix (so results update while
/// typing): only once it's two characters or more, since one begins too many
/// words to look through. The website demo's search does the same (search.ts).
pub fn last_is_prefix(terms: &[String]) -> bool {
    terms.last().is_some_and(|t| t.chars().count() >= 2)
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

/// A caption: words shown together over the picture, at most two short lines.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Characters a cue holds: two lines of the usual subtitle width.
const CUE_CHARS: usize = 84;
/// A cue ends at a pause this long, or once it has been up this long.
const CUE_GAP: f64 = 1.0;
const CUE_LONGEST: f64 = 6.0;
/// How long a cue stays up after its last word, unless the next one comes first.
pub const CUE_HOLD: f64 = 0.6;

/// Captions for `segments`, cut from their words: at a sentence's end, at a
/// pause, before running past `CUE_CHARS`, or at a comma once a line is full.
pub fn cues(segments: &[Segment]) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    for segment in segments {
        if segment.words.is_empty() {
            out.push(Cue { start: segment.start, end: segment.end, text: segment.text.clone() });
            continue;
        }
        let mut current: Vec<&Word> = Vec::new();
        let len = |words: &[&Word]| words.iter().map(|w| w.text.chars().count() + 1).sum::<usize>();
        for word in &segment.words {
            if let (Some(first), Some(last)) = (current.first(), current.last()) {
                let full = len(&current) + word.text.chars().count() > CUE_CHARS;
                let pause = word.start - last.end > CUE_GAP;
                let long = word.end - first.start > CUE_LONGEST;
                let comma = last.text.ends_with([',', ';', ':']) && len(&current) > CUE_CHARS / 2;
                if full || pause || long || comma {
                    out.push(cue(&current));
                    current.clear();
                }
            }
            current.push(word);
            if word.text.ends_with(['.', '?', '!', '…']) {
                out.push(cue(&current));
                current.clear();
            }
        }
        if !current.is_empty() {
            out.push(cue(&current));
        }
    }
    // Held a moment after the last word, but never over the next cue.
    for i in 0..out.len() {
        let next = out.get(i + 1).map_or(f64::INFINITY, |c| c.start);
        out[i].end = (out[i].end + CUE_HOLD).min(next).max(out[i].start);
    }
    out
}

fn cue(words: &[&Word]) -> Cue {
    let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    Cue { start: words[0].start, end: words[words.len() - 1].end, text }
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
            let q = terms(&c.query);
            assert_eq!(find_phrase(&words, &q, last_is_prefix(&q)), c.ranges, "{}", c.about);
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
    #[test]
    fn cues_break_at_sentences_pauses_and_length() {
        let w = |text: &str, start: f64| Word { text: text.into(), start, end: start + 0.3 };
        let seg = |words: Vec<Word>| Segment {
            start: words[0].start,
            end: words.last().unwrap().end,
            text: words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
            words,
        };
        let long: Vec<Word> = (0..30).map(|i| w("wordy", 20.0 + i as f64 * 0.15)).collect();
        let segments = [
            seg(vec![w("Hello", 0.0), w("there.", 0.4), w("How", 0.8), w("are", 1.2), w("you", 3.0)]),
            seg(vec![w("Next", 3.5)]),
            seg(long),
            Segment { start: 40.0, end: 41.0, text: "no words".into(), words: Vec::new() },
        ];
        let cues = cues(&segments);
        let texts: Vec<&str> = cues.iter().map(|c| c.text.as_str()).collect();
        let wordy = |n: usize| vec!["wordy"; n].join(" ");
        assert_eq!(
            texts,
            ["Hello there.", "How are", "you", "Next", &wordy(14), &wordy(14), "wordy wordy", "no words"]
        );
        assert_eq!((cues[0].start, cues[0].end), (0.0, 0.8), "held until the next cue");
        assert!((cues[1].end - 2.1).abs() < 1e-9, "held after its last word: {}", cues[1].end);
        assert_eq!(cues[2].end, 3.5);
    }
}
