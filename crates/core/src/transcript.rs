use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

mod unicode61;

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

    /// Where the words held back for the next segment start, if any: what's
    /// before is all in the segments given out.
    pub fn pending_start(&self) -> Option<f64> {
        self.current.first().map(|w| w.start)
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

/// Splits text into the terms the search index makes of it: SQLite's
/// `unicode61 remove_diacritics 2` tokenizer, character by character, so
/// lowercase, accents off Latin letters, and every other script as written.
/// A search's words reach the index as these terms, and FTS matches are
/// found again inside a segment by them.
pub fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut term = String::new();
    for c in text.chars() {
        match fold(c) {
            Fold::Into(f) => term.push(f),
            Fold::Dropped => {}
            Fold::Parts => {
                if !term.is_empty() {
                    out.push(std::mem::take(&mut term));
                }
            }
        }
    }
    if !term.is_empty() {
        out.push(term);
    }
    out
}

/// What a character is to the index's tokenizer.
enum Fold {
    /// It parts words.
    Parts,
    /// It's left out of the word it's in (a combining accent).
    Dropped,
    /// It's this in its word.
    Into(char),
}

fn fold(c: char) -> Fold {
    if c.is_ascii() {
        return if c.is_ascii_alphanumeric() { Fold::Into(c.to_ascii_lowercase()) } else { Fold::Parts };
    }
    let n = c as u32;
    let parts = unicode61::SEPARATORS.binary_search_by(|&(a, b)| {
        if b < n {
            Ordering::Less
        } else if a > n {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
    if parts.is_ok() {
        return Fold::Parts;
    }
    match unicode61::FOLDS.binary_search_by_key(&n, |&(from, _)| from) {
        Ok(i) => char::from_u32(unicode61::FOLDS[i].1).filter(|&f| f != '\0').map_or(Fold::Dropped, Fold::Into),
        Err(_) => Fold::Into(c),
    }
}

/// Whether a search's last term matches as a prefix (so results update while
/// typing): only once it's two characters or more, since one begins too many
/// words to look through. The website demo's search does the same (query.ts).
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
mod tests;
