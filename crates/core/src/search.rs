//! Search over the library: the query (`query.rs`) run, alternative by
//! alternative, against what was said and what chat wrote.
//!
//! What was said is one stream of timed words per VOD; its lines are only
//! how it's shown. A match is one occurrence of every piece, each phrase
//! contiguous in the stream (across any number of lines), all within the
//! window: the last matched word's end minus the first's start at most
//! `within:`. To find candidates, the rarest word of the pieces (by the
//! index's own counts) is looked up alone: any match holds it, so the lines
//! around each line it's in are read, and matched here. A chat message is a
//! unit of its own: all the pieces in one message, no window.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use anyhow::Result;
use rusqlite::{OptionalExtension, ToSql, params};
use serde::Serialize;

use crate::db::{Db, SearchFilter, SearchHit, SearchResults, Source, TextPart};
use crate::query::{Conjunction, DEFAULT_WITHIN, Piece, QueryInfo};
use crate::transcript::{Word, find_phrase, terms};

/// Bounds for a time filter that isn't set (finite, so SQLite compares them).
const EARLIEST: f64 = f64::MIN;
const LATEST: f64 = f64::MAX;

/// A name `from:` can take, as the box suggests it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AuthorSuggestion {
    pub name: String,
    /// A VOD's channel: their speech and their own chat.
    pub streamer: bool,
    /// Chat messages under the name.
    pub messages: i64,
}

/// A hit before it's shown: the lines it covers and which of their words
/// are marked.
struct Found {
    position: i64,
    vod_id: i64,
    source: Source,
    time: f64,
    end: f64,
    /// (line or message id, its words), first to last.
    lines: Vec<(i64, Vec<Word>)>,
    /// (index into `lines`, word index).
    marked: HashSet<(usize, usize)>,
    author: Option<String>,
    color: Option<String>,
}

impl Found {
    /// Hits that show the same lines are one, with every match in them
    /// marked: a line saying the phrase twice is one hit, as it always was.
    fn key(&self) -> (Source, i64, i64, i64) {
        (self.source, self.vod_id, self.lines[0].0, self.lines[self.lines.len() - 1].0)
    }

    fn into_hit(self) -> SearchHit {
        let mut words = Vec::new();
        let mut ranges = Vec::new();
        for (l, (_, line)) in self.lines.iter().enumerate() {
            for (w, word) in line.iter().enumerate() {
                if self.marked.contains(&(l, w)) {
                    ranges.push((words.len(), words.len()));
                }
                words.push(word.clone());
            }
        }
        SearchHit {
            vod_id: self.vod_id,
            source: self.source,
            id: self.lines[0].0,
            time: self.time,
            end: self.end,
            lines: self.lines.len(),
            parts: highlight(&words, &ranges),
            author: self.author,
            color: self.color,
        }
    }
}

impl Db {
    /// Searches the library for `query` in the language of `query.rs`. The
    /// filter's VOD, sources, window and who are defaults the typed text
    /// overrides (`in:`, `within:`, `from:`).
    /// Hits are ordered by VOD, in library order, then by time within the
    /// VOD; at most `limit`, and `truncated` if there were more.
    pub fn search(&self, query: &str, filter: &SearchFilter, limit: usize) -> Result<SearchResults> {
        let none = |info: QueryInfo| SearchResults { hits: Vec::new(), truncated: false, query: info };
        let parsed = match crate::query::parse(query) {
            Ok(q) => q,
            Err(e) => return Ok(none(QueryInfo::error(e))),
        };
        let mut info = QueryInfo::of(&parsed);
        let conjunctions = match parsed.conjunctions() {
            Ok(c) => c,
            Err(e) => return Ok(none(QueryInfo { error: Some(e), ..info })),
        };
        let within = parsed.within.or(filter.within.filter(|&w| w > 0.0)).unwrap_or(DEFAULT_WITHIN);
        let from = filter.from.as_deref().map(str::trim).filter(|n| !n.is_empty());
        let mut found: Vec<Found> = Vec::new();
        let mut ran = false;
        for c in &conjunctions {
            // The filter's who, for an alternative that doesn't name its own.
            let mut c = c.clone();
            if let Some(name) = from
                && c.from.is_empty()
            {
                c.from.push(name.to_string());
            }
            let c = &c;
            // Exclusions alone have nothing to leave things out of.
            if c.pieces.is_empty() && !c.filtered() {
                continue;
            }
            ran = true;
            let (speech, chat) = match c.sources {
                Some(s) => (s.audio, s.chat),
                None => (filter.speech, filter.chat),
            };
            if speech {
                found.extend(self.search_speech(c, filter.vod_id, within, limit)?);
            }
            if chat {
                found.extend(self.search_chat(c, filter.vod_id, limit)?);
            }
        }
        if !ran && conjunctions.iter().any(|c| !c.not.is_empty()) {
            info.error = Some("Add a word or a filter: a minus only leaves things out.".into());
        }
        // One hit per stretch of lines, across alternatives too.
        let mut merged: Vec<Found> = Vec::new();
        let mut at: HashMap<(Source, i64, i64, i64), usize> = HashMap::new();
        for f in found {
            match at.get(&f.key()) {
                Some(&i) => {
                    let m = &mut merged[i];
                    m.time = m.time.min(f.time);
                    m.end = m.end.max(f.end);
                    m.marked.extend(f.marked);
                }
                None => {
                    at.insert(f.key(), merged.len());
                    merged.push(f);
                }
            }
        }
        // Library position, newest VOD first, then time.
        merged
            .sort_by(|a, b| a.position.cmp(&b.position).then(b.vod_id.cmp(&a.vod_id)).then(a.time.total_cmp(&b.time)));
        let truncated = merged.len() > limit;
        let hits = merged.into_iter().take(limit).map(Found::into_hit).collect();
        Ok(SearchResults { hits, truncated, query: info })
    }

    /// Names `from:` could mean, starting with `prefix` in any case: the
    /// channels of the library's VODs first, then chatters with the most
    /// messages (in VOD `vod_id`, if given). At most eight.
    pub fn author_suggestions(&self, prefix: &str, vod_id: Option<i64>) -> Result<Vec<AuthorSuggestion>> {
        const MOST: usize = 8;
        let prefix = prefix.trim();
        // Every name from `prefix` on that still starts with it, as NOCASE
        // compares (ASCII folded, then bytes): the highest character ends it.
        let upto = format!("{prefix}\u{10FFFF}");
        let mut out: Vec<AuthorSuggestion> = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT channel FROM vods JOIN vod_order o ON o.vod_id = vods.id
             WHERE channel COLLATE NOCASE >= ?1 AND channel COLLATE NOCASE < ?2 AND (?3 IS NULL OR vods.id = ?3)
             ORDER BY channel COLLATE NOCASE LIMIT ?4",
        )?;
        for name in stmt.query_map(params![prefix, upto, vod_id, MOST as i64], |r| r.get::<_, String>(0))? {
            let name = name?;
            if !out.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
                out.push(AuthorSuggestion { name, streamer: true, messages: 0 });
            }
        }
        let mut stmt = self.conn.prepare(
            "SELECT author, count(*) AS n FROM chat
             WHERE author COLLATE NOCASE >= ?1 AND author COLLATE NOCASE < ?2 AND (?3 IS NULL OR vod_id = ?3)
             GROUP BY author COLLATE NOCASE ORDER BY n DESC, author COLLATE NOCASE LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![prefix, upto, vod_id, MOST as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (name, messages) = row?;
            match out.iter().position(|s| s.name.eq_ignore_ascii_case(&name)) {
                Some(i) => out[i].messages = messages,
                None if out.len() < MOST => out.push(AuthorSuggestion { name, streamer: false, messages }),
                None => {}
            }
        }
        Ok(out)
    }

    /// The library's VODs, in its order, with their positions: the
    /// filter's one or all, and if `channels` names any, only theirs.
    fn library_vods(&self, vod_id: Option<i64>, channels: &[String]) -> Result<Vec<(i64, i64)>> {
        let mut sql = "SELECT o.vod_id, o.position FROM vod_order o JOIN vods ON vods.id = o.vod_id
                       WHERE (?1 IS NULL OR o.vod_id = ?1)"
            .to_string();
        let mut args: Vec<&dyn ToSql> = vec![&vod_id];
        if !channels.is_empty() {
            sql += &format!(" AND vods.channel COLLATE NOCASE IN ({})", numbered(2, channels.len()));
            args.extend(channels.iter().map(|n| n as &dyn ToSql));
        }
        sql += " ORDER BY o.position, o.vod_id DESC";
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(&args[..], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// What was said that matches `c`. With `from:`, only the VODs of the
    /// channels named: all their speech is the streamer's (there's no telling
    /// voices apart yet).
    fn search_speech(&self, c: &Conjunction, vod_id: Option<i64>, within: f64, limit: usize) -> Result<Vec<Found>> {
        let vods = self.library_vods(vod_id, &c.from)?;
        if vods.is_empty() {
            return Ok(Vec::new());
        }
        if c.pieces.is_empty() {
            return self.list_speech(c, &vods, within, limit);
        }
        let Some(anchor) = self.anchor(&c.pieces)? else { return Ok(Vec::new()) };
        let positions: HashMap<i64, i64> = vods.into_iter().collect();
        // Terms are alphanumeric only, so quoting one can't inject FTS syntax.
        let fts = format!("\"{}\"{}", anchor.term, if anchor.prefix { "*" } else { "" });
        let (lo, hi) = self.id_range("segments", vod_id)?;
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.vod_id, s.start, s.end FROM segments_fts f
             JOIN segments s ON s.id = f.rowid
             JOIN vod_order v ON v.vod_id = s.vod_id
             WHERE segments_fts MATCH ?1 AND f.rowid BETWEEN ?2 AND ?3 AND (?4 IS NULL OR s.vod_id = ?4)
             ORDER BY v.position, v.vod_id DESC, s.start, s.id",
        )?;
        let rows = stmt.query_map(params![fts, lo, hi, vod_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?))
        })?;
        let lines_with = self.lines_with(&c.pieces, &anchor, (lo, hi))?;
        let (after, before) = (c.after.unwrap_or(EARLIEST), c.before.unwrap_or(LATEST));
        let mut out: Vec<Found> = Vec::new();
        // The VOD under way: its lines read so far, matches not yet kept, and
        // the words of those kept (a match sharing one isn't reported again).
        let mut stream: Option<Stream> = None;
        let mut pending: Vec<Candidate> = Vec::new();
        let mut kept = Kept::default();
        // Once there are more than enough, the anchors still able to change
        // which come first are read (those up to a window past), then no more.
        let mut stop_after: Option<f64> = None;
        for row in rows {
            let (line_id, vod, start, end) = row?;
            let Some(&position) = positions.get(&vod) else { continue };
            let new_vod = stream.as_ref().is_none_or(|s| s.vod_id != vod);
            if new_vod || stop_after.is_some_and(|t| start > t) {
                keep(&mut pending, &mut kept, &mut out);
                if out.len() > limit {
                    break;
                }
                stop_after = None;
            }
            if new_vod {
                stream = Some(Stream::new(vod, position));
                kept = Kept::default();
            }
            let stream = stream.as_mut().expect("set above");
            let (lo, hi) = (start - within, end + within);
            stream.cover(self, lo, hi)?;
            let Some(scope) = stream.may_match(line_id, lo, hi, c, &anchor, &lines_with) else { continue };
            stream.read_lines(self, scope.clone())?;
            for candidate in stream.matches(line_id, &anchor, c, within, scope) {
                if candidate.start >= after && candidate.start < before {
                    pending.push(candidate);
                }
            }
            if stop_after.is_none() && out.len() + pending.len() > limit {
                stop_after = Some(end + within);
            }
        }
        keep(&mut pending, &mut kept, &mut out);
        Ok(out)
    }

    /// Speech with no pieces to find (`from:x`, `after:1h -word`): each line
    /// passing the filters is a hit.
    fn list_speech(&self, c: &Conjunction, vods: &[(i64, i64)], within: f64, limit: usize) -> Result<Vec<Found>> {
        let (after, before) = (c.after.unwrap_or(EARLIEST), c.before.unwrap_or(LATEST));
        let mut stmt = self.conn.prepare(
            "SELECT id, start, end, text, words FROM segments WHERE vod_id = ?1 AND start >= ?2 AND start < ?3
             ORDER BY start, id",
        )?;
        let mut out = Vec::new();
        for &(vod_id, position) in vods {
            let mut stream = Stream::new(vod_id, position);
            let rows = stmt.query_map(params![vod_id, after, before], |r| {
                Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
            })?;
            for row in rows {
                let (id, start, end, text, words): (i64, f64, f64, String, String) = row?;
                let mut line = Line::new(id, start, end, text, Some(words));
                line.read(self)?;
                if !c.not.is_empty() {
                    stream.cover(self, start - within, end + within)?;
                    stream.read(self, start - within, end + within)?;
                    let (first, last) = line.span();
                    if stream.excluded(&c.not, first, last, within) {
                        continue;
                    }
                }
                let (time, end) = line.span();
                out.push(Found {
                    position,
                    vod_id,
                    source: Source::Speech,
                    time,
                    end,
                    lines: vec![(id, line.words)],
                    marked: HashSet::new(),
                    author: None,
                    color: None,
                });
                if out.len() > limit {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }

    /// The word to look up first: the pieces' rarest whole word, by how many
    /// lines the index has it in. The word being typed is only a prefix, too
    /// common to start from, unless it's the only word. None if a whole word
    /// is in no line at all: nothing can match.
    fn anchor(&self, pieces: &[Piece]) -> Result<Option<Anchor>> {
        let words: Vec<&String> = pieces.iter().flat_map(whole).collect();
        if words.is_empty() {
            let typed = pieces.iter().find_map(|p| p.terms.last()).expect("pieces have terms");
            return Ok(Some(Anchor { term: typed.clone(), prefix: true }));
        }
        let mut stmt = self.conn.prepare("SELECT doc FROM segments_vocab WHERE term = ?1")?;
        let mut best: Option<(i64, &String)> = None;
        for term in words {
            let lines: i64 = stmt.query_row([term], |r| r.get(0)).optional()?.unwrap_or(0);
            if lines == 0 {
                return Ok(None);
            }
            if best.is_none_or(|(n, _)| lines < n) {
                best = Some((lines, term));
            }
        }
        Ok(best.map(|(_, term)| Anchor { term: term.clone(), prefix: false }))
    }

    /// The lines each word of the pieces is in (the one being typed: any
    /// word it begins), but the anchor's, in the lines with ids `range`;
    /// for `Stream::may_match`.
    fn lines_with(&self, pieces: &[Piece], anchor: &Anchor, range: (i64, i64)) -> Result<LinesWith> {
        let mut out = LinesWith::new();
        #[cfg(test)]
        if tests::NO_SHORTCUT.get() {
            return Ok(out);
        }
        let mut stmt = self
            .conn
            .prepare("SELECT rowid FROM segments_fts WHERE segments_fts MATCH ?1 AND rowid BETWEEN ?2 AND ?3")?;
        let own = anchor.key();
        for p in pieces {
            for i in 0..p.terms.len() {
                let key = key(p, i);
                if key == own || out.contains_key(&key) {
                    continue;
                }
                // Alphanumeric, as `fts_phrase`.
                let fts = format!("\"{}\"{}", p.terms[i], if key.ends_with('*') { "*" } else { "" });
                let ids = stmt.query_map(params![fts, range.0, range.1], |r| r.get(0))?;
                out.insert(key, ids.collect::<rusqlite::Result<_>>()?);
            }
        }
        Ok(out)
    }

    /// What chat wrote that matches `c`: every piece in one message, none
    /// of the exclusions; `from:` by the message's author, in any case.
    fn search_chat(&self, c: &Conjunction, vod_id: Option<i64>, limit: usize) -> Result<Vec<Found>> {
        if c.pieces.is_empty() && c.from.is_empty() {
            return self.list_chat(c, vod_id, limit);
        }
        let (after, before) = (c.after.unwrap_or(EARLIEST), c.before.unwrap_or(LATEST));
        // All the pieces in the message, as far as the index can tell.
        let fts = c.pieces.iter().map(fts_phrase).collect::<Vec<_>>().join(" AND ");
        let (lo, hi) = if c.pieces.is_empty() { (0, 0) } else { self.id_range("chat", vod_id)? };
        let mut args: Vec<&dyn ToSql> = vec![&vod_id, &after, &before];
        let mut sql = if c.pieces.is_empty() {
            // By the author index.
            "SELECT c.id, c.vod_id, c.time, c.text, c.author, c.color, v.position FROM chat c
             JOIN vod_order v ON v.vod_id = c.vod_id WHERE 1"
                .to_string()
        } else {
            args.extend([&fts as &dyn ToSql, &lo, &hi]);
            "SELECT c.id, c.vod_id, c.time, c.text, c.author, c.color, v.position FROM chat_fts f
             JOIN chat c ON c.id = f.rowid JOIN vod_order v ON v.vod_id = c.vod_id
             WHERE chat_fts MATCH ?4 AND f.rowid BETWEEN ?5 AND ?6"
                .to_string()
        };
        sql += " AND (?1 IS NULL OR c.vod_id = ?1) AND c.time >= ?2 AND c.time < ?3";
        if !c.from.is_empty() {
            sql += &format!(" AND c.author COLLATE NOCASE IN ({})", numbered(args.len() + 1, c.from.len()));
            args.extend(c.from.iter().map(|n| n as &dyn ToSql));
        }
        sql += " ORDER BY v.position, v.vod_id DESC, c.time, c.id";
        // Exclusions are checked here, so only without them does SQLite know
        // how many rows are enough.
        if c.not.is_empty() {
            sql += &format!(" LIMIT {}", limit + 1);
        }
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(&args[..])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let message = Message::read(r, r.get(1)?, r.get(6)?)?;
            out.extend(message.found(c));
            if out.len() > limit {
                break;
            }
        }
        Ok(out)
    }

    /// Chat with no pieces and no author (`in:chat`, `after:1h -gg`): every
    /// message passing the filters, VOD by VOD so that only as many are read
    /// as are shown.
    fn list_chat(&self, c: &Conjunction, vod_id: Option<i64>, limit: usize) -> Result<Vec<Found>> {
        let (after, before) = (c.after.unwrap_or(EARLIEST), c.before.unwrap_or(LATEST));
        let mut stmt = self.conn.prepare(
            "SELECT id, vod_id, time, text, author, color FROM chat WHERE vod_id = ?1 AND time >= ?2 AND time < ?3
             ORDER BY time, id",
        )?;
        let mut out = Vec::new();
        for (vod, position) in self.library_vods(vod_id, &[])? {
            let mut rows = stmt.query(params![vod, after, before])?;
            while let Some(r) = rows.next()? {
                out.extend(Message::read(r, vod, position)?.found(c));
                if out.len() > limit {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }
}

/// A chat message as a search reads it: id, VOD, time, text, author and
/// color first, in that order.
struct Message {
    id: i64,
    vod_id: i64,
    position: i64,
    time: f64,
    text: String,
    author: String,
    color: Option<String>,
}

impl Message {
    fn read(r: &rusqlite::Row, vod_id: i64, position: i64) -> rusqlite::Result<Message> {
        Ok(Message {
            id: r.get(0)?,
            vod_id,
            position,
            time: r.get(2)?,
            text: r.get(3)?,
            author: r.get(4)?,
            color: r.get(5)?,
        })
    }

    /// The hit, if it has every piece of `c` and none of its exclusions.
    fn found(self, c: &Conjunction) -> Option<Found> {
        // Chat has no word timings; every word is at the message's time.
        let time = self.time;
        let words: Vec<Word> =
            self.text.split_whitespace().map(|w| Word { text: w.to_string(), start: time, end: time }).collect();
        let mut marked = HashSet::new();
        for p in &c.pieces {
            let ranges = find_phrase(&words, &p.terms, p.prefix);
            if ranges.is_empty() {
                return None;
            }
            marked.extend(ranges.iter().flat_map(|&(a, b)| (a..=b).map(|w| (0, w))));
        }
        if c.not.iter().any(|p| !find_phrase(&words, &p.terms, false).is_empty()) {
            return None;
        }
        Some(Found {
            position: self.position,
            vod_id: self.vod_id,
            source: Source::Chat,
            time,
            end: time,
            lines: vec![(self.id, words)],
            marked,
            author: Some(self.author),
            color: self.color,
        })
    }
}

/// `?first, ?first+1, …`: `count` numbered parameters for an IN list.
fn numbered(first: usize, count: usize) -> String {
    (first..first + count).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ")
}

fn fts_phrase(p: &Piece) -> String {
    format!("\"{}\"{}", p.terms.join(" "), if p.prefix { "*" } else { "" })
}

/// The matches kept so far in the VOD under way.
#[derive(Default)]
struct Kept {
    /// Their words, as (line id, word index).
    words: HashSet<(i64, usize)>,
    /// Where in the hits each stretch of lines (first and last line id) is.
    hits: HashMap<(i64, i64), usize>,
}

/// Kept from `pending`: the tightest first (then the earliest), each match
/// none of whose words is in one kept already. So where a word repeats
/// near a phrase, the set with the nearer one is the match. A match showing
/// the same lines as one kept is marked in it, so `out` counts hits as
/// they're shown (a line saying the word twice is one).
fn keep(pending: &mut Vec<Candidate>, kept: &mut Kept, out: &mut Vec<Found>) {
    let span = |c: &Candidate| c.found.end - c.found.time;
    pending.sort_by(|a, b| span(a).total_cmp(&span(b)).then(a.found.time.total_cmp(&b.found.time)));
    for c in pending.drain(..) {
        if c.words.iter().any(|w| kept.words.contains(w)) {
            continue;
        }
        kept.words.extend(c.words);
        let (_, _, first, last) = c.found.key();
        match kept.hits.get(&(first, last)) {
            Some(&i) => {
                let hit = &mut out[i];
                hit.time = hit.time.min(c.found.time);
                hit.end = hit.end.max(c.found.end);
                hit.marked.extend(c.found.marked);
            }
            None => {
                kept.hits.insert((first, last), out.len());
                out.push(c.found);
            }
        }
    }
}

struct Anchor {
    term: String,
    prefix: bool,
}

/// For words of the pieces (`key`), the ids of the lines they're in.
type LinesWith = HashMap<String, HashSet<i64>>;

/// Piece `p`'s word `i` as `LinesWith` knows it: the word, or the word
/// being typed with a `*`, as FTS writes a prefix.
fn key(p: &Piece, i: usize) -> String {
    let typed = p.prefix && i == p.terms.len() - 1;
    format!("{}{}", p.terms[i], if typed { "*" } else { "" })
}

impl Anchor {
    fn key(&self) -> String {
        format!("{}{}", self.term, if self.prefix { "*" } else { "" })
    }
}

/// A piece's whole words: all but the one being typed.
fn whole(p: &Piece) -> &[String] {
    &p.terms[..p.terms.len() - usize::from(p.prefix)]
}

/// A match in what was said, and its words as (line id, word index).
struct Candidate {
    start: f64,
    words: Vec<(i64, usize)>,
    found: Found,
}

/// A line of a VOD's transcript, its words folded as the index folds them.
/// Its words are read from their JSON only when a window it's in could
/// hold a match (`Stream::may_match`): most lines around a common word's
/// lines never are.
struct Line {
    id: i64,
    start: f64,
    end: f64,
    /// Whether it has any words to match (a line of "…" doesn't).
    has_terms: bool,
    /// Until read: its text, and its words' JSON if it came with it (else
    /// it's fetched when the line is read).
    saved: Option<(String, Option<String>)>,
    words: Vec<Word>,
    /// (term, the word it's from).
    terms: Vec<(String, usize)>,
}

impl Line {
    fn new(id: i64, start: f64, end: f64, text: String, words: Option<String>) -> Line {
        let has_terms = text.chars().any(char::is_alphanumeric);
        Line { id, start, end, has_terms, saved: Some((text, words)), words: Vec::new(), terms: Vec::new() }
    }

    fn read(&mut self, db: &Db) -> Result<()> {
        let Some((text, words)) = self.saved.take() else { return Ok(()) };
        let words: String = match words {
            Some(words) => words,
            None => db
                .conn
                .prepare_cached("SELECT words FROM segments WHERE id = ?1")?
                .query_row([self.id], |r| r.get(0))?,
        };
        let (start, end) = (self.start, self.end);
        self.words = serde_json::from_str(&words)?;
        // A line saved without its word timings: its words at its own times.
        if self.words.is_empty() {
            self.words = text.split_whitespace().map(|w| Word { text: w.to_string(), start, end }).collect();
        }
        for (i, w) in self.words.iter().enumerate() {
            fold(&w.text, |t| self.terms.push((t, i)));
        }
        Ok(())
    }

    /// From its first word's start to its last word's end.
    fn span(&self) -> (f64, f64) {
        match (self.words.first(), self.words.last()) {
            (Some(a), Some(b)) => (a.start, b.end),
            _ => (self.start, self.end),
        }
    }
}

/// `terms` of one word, each given to `f`; plain ASCII (most words) without
/// the Unicode decomposition `terms` does, the same terms faster.
fn fold(word: &str, mut f: impl FnMut(String)) {
    if word.is_ascii() {
        for t in word.split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()) {
            f(t.to_ascii_lowercase());
        }
    } else {
        terms(word).into_iter().for_each(f);
    }
}

/// A place in the stream: (line index in the stream, term index in the line).
type Place = (usize, usize);

/// A VOD's transcript as one stream of words, read a stretch at a time as
/// the anchors move through it: each line read and folded once however
/// many windows it's in.
struct Stream {
    vod_id: i64,
    position: i64,
    lines: Vec<Line>,
    /// How far the lines read reach: every line starting up to here.
    reached: f64,
}

impl Stream {
    fn new(vod_id: i64, position: i64) -> Stream {
        Stream { vod_id, position, lines: Vec::new(), reached: f64::MIN }
    }

    /// Reads the lines with words from `lo` to `hi` that aren't read yet.
    fn cover(&mut self, db: &Db, lo: f64, hi: f64) -> Result<()> {
        let last = self.lines.last().map(|l| (l.start, l.id));
        let mut stmt;
        let rows = match last {
            // On from the last line read.
            Some((start, id)) if lo <= self.reached => {
                if hi <= self.reached {
                    return Ok(());
                }
                stmt = db.conn.prepare_cached(
                    "SELECT id, start, end, text FROM segments
                     WHERE vod_id = ?1 AND (start, id) > (?2, ?3) AND start <= ?4 ORDER BY start, id",
                )?;
                stmt.query(params![self.vod_id, start, id, hi])?
            }
            // A fresh stretch, from the line `lo` falls in (the last to start by then).
            _ => {
                self.lines.clear();
                stmt = db.conn.prepare_cached(
                    "SELECT id, start, end, text FROM segments
                     WHERE vod_id = ?1 AND start <= ?3 AND start >= coalesce(
                         (SELECT start FROM segments WHERE vod_id = ?1 AND start <= ?2 ORDER BY start DESC LIMIT 1), ?2)
                     ORDER BY start, id",
                )?;
                stmt.query(params![self.vod_id, lo, hi])?
            }
        };
        let mut rows = rows;
        while let Some(r) = rows.next()? {
            // The words, the most of a line to read, only once it's needed.
            self.lines.push(Line::new(r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, None));
        }
        self.reached = self.reached.max(hi);
        Ok(())
    }

    /// The lines overlapping `lo..hi`, by index.
    fn overlapping(&self, lo: f64, hi: f64) -> Range<usize> {
        // Lines are in time order; one starting after `hi` ends the stretch.
        let first = self.lines.partition_point(|l| l.end < lo).saturating_sub(1);
        let last = first + self.lines[first..].partition_point(|l| l.start <= hi);
        first..last
    }

    /// Reads the words of the lines overlapping `lo..hi`.
    fn read(&mut self, db: &Db, lo: f64, hi: f64) -> Result<()> {
        let range = self.overlapping(lo, hi);
        self.read_lines(db, range)
    }

    fn read_lines(&mut self, db: &Db, lines: Range<usize>) -> Result<()> {
        self.lines[lines].iter_mut().try_for_each(|l| l.read(db))
    }

    /// Whether the window `lo..hi` around anchor line `line_id` could hold
    /// a match, by which lines the pieces' other words are in (`lines_with`;
    /// a word not in it may be anywhere): a piece through the anchor word
    /// has all its words within as many lines of it as it has words, and
    /// every piece has its words in the window. If so, the lines a match
    /// through the line can be in: for a piece alone with nothing left out,
    /// only those next to the anchor line.
    fn may_match(
        &self,
        line_id: i64,
        lo: f64,
        hi: f64,
        c: &Conjunction,
        anchor: &Anchor,
        lines_with: &LinesWith,
    ) -> Option<Range<usize>> {
        let lines = self.overlapping(lo, hi);
        let a = self.lines[lines.clone()].iter().rposition(|l| l.id == line_id)? + lines.start;
        let has = |l: usize, key: &str| lines_with.get(key).is_none_or(|ids| ids.contains(&self.lines[l].id));
        let all_in = |p: &Piece, lines: &[usize]| (0..p.terms.len()).all(|i| lines.iter().any(|&l| has(l, &key(p, i))));
        let window: Vec<usize> = lines.clone().filter(|&l| self.lines[l].has_terms).collect();
        if !c.pieces.iter().all(|p| all_in(p, &window)) {
            return None;
        }
        let at = window.iter().position(|&l| l == a)?;
        let own = anchor.key();
        let near = |p: &Piece| {
            let n = p.terms.len() - 1;
            &window[at.saturating_sub(n)..(at + n + 1).min(window.len())]
        };
        // A piece can hold the anchor word as one of its words, or as the start of the word being typed.
        let through = |p: &&Piece| {
            (0..p.terms.len()).any(|i| {
                let k = key(p, i);
                k == own || (!anchor.prefix && k.ends_with('*') && anchor.term.starts_with(&p.terms[i]))
            })
        };
        if !c.pieces.iter().filter(through).any(|p| all_in(p, near(p))) {
            return None;
        }
        match &c.pieces[..] {
            [p] if c.not.is_empty() => {
                let near = near(p);
                Some(near[0]..near[near.len() - 1] + 1)
            }
            _ => Some(lines),
        }
    }

    fn term(&self, (l, t): Place) -> &str {
        &self.lines[l].terms[t].0
    }

    fn word(&self, (l, t): Place) -> &Word {
        &self.lines[l].words[self.lines[l].terms[t].1]
    }

    /// The places whose words overlap `lo..hi`, in order, in lines `scope`
    /// (which are read).
    fn places(&self, lo: f64, hi: f64, scope: &Range<usize>) -> Vec<Place> {
        let mut out = Vec::new();
        let lines = self.overlapping(lo, hi);
        for l in lines.start.max(scope.start)..lines.end.min(scope.end) {
            let line = &self.lines[l];
            debug_assert!(line.saved.is_none(), "a line matched before it's read");
            for (t, &(_, w)) in line.terms.iter().enumerate() {
                let word = &line.words[w];
                if word.end >= lo && word.start <= hi {
                    out.push((l, t));
                }
            }
        }
        out
    }

    /// Each occurrence of `piece` in `places`, as a range of indices into it.
    fn occurrences(&self, places: &[Place], piece: &Piece) -> Vec<Range<usize>> {
        let n = piece.terms.len();
        let mut out = Vec::new();
        for i in 0..places.len().saturating_sub(n - 1) {
            let found = piece.terms.iter().enumerate().all(|(j, q)| {
                let t = self.term(places[i + j]);
                if piece.prefix && j == n - 1 { t.starts_with(q.as_str()) } else { t == q }
            });
            if found {
                out.push(i..i + n);
            }
        }
        out
    }

    fn span(&self, places: &[Place], r: &Range<usize>) -> (f64, f64) {
        (self.word(places[r.start]).start, self.word(places[r.end - 1]).end)
    }

    /// Whether any of `not` is in the window centred on `start..end`: the
    /// match widened equally on both sides to the window's full size.
    fn excluded(&self, not: &[Piece], start: f64, end: f64, within: f64) -> bool {
        let centre = (start + end) / 2.0;
        let (lo, hi) = (centre - within / 2.0, centre + within / 2.0);
        let places = self.places(lo, hi, &self.overlapping(lo, hi));
        not.iter().any(|p| !self.occurrences(&places, p).is_empty())
    }

    /// The tightest match through each of `line_id`'s anchor words: every
    /// piece once, the span within the window, nothing excluded near it.
    fn matches(
        &self,
        line_id: i64,
        anchor: &Anchor,
        c: &Conjunction,
        within: f64,
        scope: Range<usize>,
    ) -> Vec<Candidate> {
        let Some(l) = self.lines.iter().rposition(|line| line.id == line_id) else { return Vec::new() };
        let mut out = Vec::new();
        for (t, (term, _)) in self.lines[l].terms.iter().enumerate() {
            let is_anchor = if anchor.prefix { term.starts_with(&anchor.term) } else { *term == anchor.term };
            if !is_anchor {
                continue;
            }
            let word = self.word((l, t));
            // Any match through this word lies in a window's size either side of it.
            let places = self.places(word.end - within, word.start + within, &scope);
            let Some(at) = places.iter().position(|&p| p == (l, t)) else { continue };
            let found: Vec<Vec<Range<usize>>> = c.pieces.iter().map(|p| self.occurrences(&places, p)).collect();
            let mut best: Option<(f64, Vec<Range<usize>>)> = None;
            // Each piece's occurrence through the anchor word, with the others around it.
            for (k, occurrences) in found.iter().enumerate() {
                for fixed in occurrences.iter().filter(|r| r.contains(&at)) {
                    if let Some(set) = self.tightest(&places, &found, k, fixed, within)
                        && best.as_ref().is_none_or(|(span, _)| set.0 < *span)
                    {
                        best = Some(set);
                    }
                }
            }
            let Some((_, set)) = best else { continue };
            let start = set.iter().map(|r| self.span(&places, r).0).fold(f64::INFINITY, f64::min);
            let end = set.iter().map(|r| self.span(&places, r).1).fold(f64::NEG_INFINITY, f64::max);
            if !c.not.is_empty() && self.excluded(&c.not, start, end, within) {
                continue;
            }
            out.push(self.candidate(&places, &set, start, end));
        }
        out
    }

    /// The occurrences, one per piece, with piece `k`'s `fixed`, spanning the
    /// least time; None if no such set fits in the window. Each start of an
    /// occurrence before the fixed one's (and its own) is tried as the
    /// match's first word; for it, each other piece takes the occurrence
    /// from there on that ends soonest.
    fn tightest(
        &self,
        places: &[Place],
        found: &[Vec<Range<usize>>],
        k: usize,
        fixed: &Range<usize>,
        within: f64,
    ) -> Option<(f64, Vec<Range<usize>>)> {
        let (fixed_start, fixed_end) = self.span(places, fixed);
        let mut firsts: Vec<f64> = found
            .iter()
            .flatten()
            .map(|r| self.span(places, r).0)
            .filter(|&s| s < fixed_start && s >= fixed_end - within)
            .collect();
        firsts.push(fixed_start);
        let mut best: Option<(f64, Vec<Range<usize>>)> = None;
        for first in firsts {
            let mut set = Vec::with_capacity(found.len());
            let mut end = fixed_end;
            for (j, occurrences) in found.iter().enumerate() {
                if j == k {
                    set.push(fixed.clone());
                    continue;
                }
                let soonest = occurrences
                    .iter()
                    .filter(|r| self.span(places, r).0 >= first)
                    .min_by(|a, b| self.span(places, a).1.total_cmp(&self.span(places, b).1));
                match soonest {
                    Some(r) => {
                        end = end.max(self.span(places, r).1);
                        set.push(r.clone());
                    }
                    None => break,
                }
            }
            let span = end - first;
            // Times are seconds as floats: a hair over the window is in it.
            if set.len() == found.len() && span <= within + 1e-9 && best.as_ref().is_none_or(|(s, _)| span < *s) {
                best = Some((span, set));
            }
        }
        best
    }

    /// The hit for a set of occurrences: the lines from its first word's to
    /// its last's, each piece's words marked.
    fn candidate(&self, places: &[Place], set: &[Range<usize>], start: f64, end: f64) -> Candidate {
        let matched: Vec<Place> = set.iter().flat_map(|r| places[r.clone()].iter().copied()).collect();
        let first = matched.iter().map(|p| p.0).min().expect("pieces have words");
        let last = matched.iter().map(|p| p.0).max().expect("pieces have words");
        let words: Vec<(i64, usize)> =
            matched.iter().map(|&(l, t)| (self.lines[l].id, self.lines[l].terms[t].1)).collect();
        let marked = matched.iter().map(|&(l, t)| (l - first, self.lines[l].terms[t].1)).collect();
        let lines = self.lines[first..=last].iter().map(|l| (l.id, l.words.clone())).collect();
        let found = Found {
            position: self.position,
            vod_id: self.vod_id,
            source: Source::Speech,
            time: start,
            end,
            lines,
            marked,
            author: None,
            color: None,
        };
        Candidate { start, words, found }
    }
}

/// Renders words as text runs, marking the matched ranges.
pub(crate) fn highlight(words: &[Word], ranges: &[(usize, usize)]) -> Vec<TextPart> {
    let mut parts: Vec<TextPart> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let hit = ranges.iter().any(|&(a, b)| i >= a && i <= b);
        let sep = if i == 0 { "" } else { " " };
        match parts.last_mut() {
            Some(p) if p.hit == hit => {
                p.text.push_str(sep);
                p.text.push_str(&w.text);
            }
            // Keep spaces out of highlighted runs so the marks hug the words.
            Some(p) if hit => {
                p.text.push_str(sep);
                parts.push(TextPart { text: w.text.clone(), hit });
            }
            _ => parts.push(TextPart { text: format!("{sep}{}", w.text), hit }),
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// Every window read, none passed over: what `may_match` must agree with.
        pub(super) static NO_SHORTCUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    use crate::chat::{ChatMessage, ChatPart, Kind};
    use crate::fetch::VodInfo;
    use crate::transcript::Segment;
    use std::path::Path;

    fn library() -> Db {
        Db::open(Path::new(":memory:")).unwrap()
    }

    fn vod(db: &Db, id: &str, channel: &str) -> i64 {
        let info = VodInfo {
            platform: "twitch".into(),
            platform_id: id.into(),
            url: format!("https://www.twitch.tv/videos/{id}"),
            title: id.into(),
            channel: Some(channel.into()),
            duration: Some(3600.0),
            thumbnail: None,
            published: None,
        };
        db.add_vod(&info).unwrap().0.id
    }

    /// A line of `text` from `start`, a word every 0.3 s, each 0.25 s long.
    fn say(db: &mut Db, vod: i64, start: f64, text: &str) {
        let words: Vec<Word> = text
            .split(' ')
            .enumerate()
            .map(|(i, t)| Word { text: t.into(), start: start + i as f64 * 0.3, end: start + i as f64 * 0.3 + 0.25 })
            .collect();
        let end = words.last().unwrap().end;
        db.insert_segments(vod, &[Segment { start, end, text: text.into(), words }]).unwrap();
    }

    fn chat(db: &mut Db, vod: i64, time: f64, author: &str, text: &str) {
        let m = ChatMessage {
            time,
            author: author.into(),
            color: None,
            parts: vec![ChatPart::Text { text: text.into() }],
            kind: Kind::Message,
            amount: None,
            event_time: None,
        };
        db.add_chat(vod, &[m], &[]).unwrap();
    }

    fn find(db: &Db, query: &str) -> SearchResults {
        db.search(query, &SearchFilter::default(), 500).unwrap()
    }

    /// Each hit as its text with the marked runs in [brackets].
    fn marked(r: &SearchResults) -> Vec<String> {
        r.hits
            .iter()
            .map(|h| h.parts.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect())
            .collect()
    }

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
        assert_eq!(find(&db, "the zeb").hits[0].time, 210.3);
    }

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
            r.hits
                .iter()
                .map(|h| format!("{} {} {} {} {:?}", h.vod_id, h.time, h.end, h.lines, marked_parts(h)))
                .collect()
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

    fn marked_parts(h: &SearchHit) -> String {
        h.parts.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect()
    }

    /// The cases timed, as (what, query): the e2e perf pass's three, and
    /// what this search adds. The library has no "i" or "mean", so the
    /// all-common phrase is one of its own common words.
    pub(crate) const BENCHMARK: [(&str, &str); 13] = [
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
        ("-word", "chat -the"),
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
        let db = Db::open_reader(Path::new(&path)).unwrap();
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
        const WORDS: [&str; 12] =
            ["good", "nice", "lol", "gg", "the", "play", "wow", "chat", "what", "yes", "no", "hi"];
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
            for (start, words) in [
                (1000.0, vec![(-1, "eagle")]),
                (1004.0, vec![(0, "has"), (1, "landed")]),
                (1008.0, vec![(0, "safely")]),
            ] {
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
                })
                .collect();
            db.add_chat(vod, &messages, &[]).unwrap();
        }
    }
}
