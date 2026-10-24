//! The search box's language. A query is pieces, all of which must match:
//! words typed one after another are one loose phrase in order (the last word
//! a prefix while it's being typed), a quoted string a strict phrase of whole
//! words, `-word` or `-"a phrase"` something that mustn't be near. `OR`
//! offers alternatives and `AND` joins pieces explicitly, binding tighter
//! than `OR`; brackets group. Pieces and filters side by side (the implicit
//! AND) bind looser than `OR`, so `from:x "a" OR "b"` is from x, a or b.
//! Filters: `from:name`, `in:chat` / `in:audio`, `within:20s`, `after:1:20:00`,
//! `before:2h`. Anything else is words.
//!
//! Parsing is pure: `parse` gives the tree the page shows, and
//! `Query::conjunctions` the alternatives `Db::search` runs, one by one.

use std::fmt;

use serde::Serialize;

use crate::transcript::{last_is_prefix, terms};

/// How far apart a match's pieces may be when `within:` isn't typed: from
/// the first matched word's start to the last one's end, in seconds.
pub const DEFAULT_WITHIN: f64 = 20.0;

/// More alternatives than this (as `(a OR b) (c OR d) …` multiplies them) is
/// refused rather than run one by one.
const MOST_ALTERNATIVES: usize = 64;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Key {
    From,
    In,
    Within,
    After,
    Before,
}

impl Key {
    fn parse(name: &str) -> Option<Key> {
        match name.to_lowercase().as_str() {
            "from" => Some(Key::From),
            "in" => Some(Key::In),
            "within" => Some(Key::Within),
            "after" => Some(Key::After),
            "before" => Some(Key::Before),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Key::From => "from",
            Key::In => "in",
            Key::Within => "within",
            Key::After => "after",
            Key::Before => "before",
        }
    }
}

/// The parsed query, as the page shows its grouping.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Node {
    Or {
        items: Vec<Node>,
    },
    And {
        items: Vec<Node>,
    },
    /// `text` as typed. Strict: quoted, whole words. `prefix`: its last word
    /// matches as the start of a word (it's the one being typed).
    Phrase {
        text: String,
        strict: bool,
        prefix: bool,
    },
    Not {
        item: Box<Node>,
    },
    /// `value` as typed; empty while it's being typed, and then it does nothing.
    Filter {
        key: Key,
        value: String,
    },
}

/// Which sources `in:` asked for.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Sources {
    pub audio: bool,
    pub chat: bool,
}

/// A phrase to find: its words folded as the index folds them.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub terms: Vec<String>,
    /// The last term matches as the start of a word.
    pub prefix: bool,
}

/// One alternative of a query: everything in it must hold together.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Conjunction {
    pub pieces: Vec<Piece>,
    /// Phrases that mustn't be in the window around a match (in chat: in the message).
    pub not: Vec<Piece>,
    /// Who: any of these names (a channel's VODs, or a chatter's messages).
    pub from: Vec<String>,
    /// Typed `in:`, which takes the place of the filter's sources.
    pub sources: Option<Sources>,
    /// A match starts at or after `after` and before `before`, seconds into the VOD.
    pub after: Option<f64>,
    pub before: Option<f64>,
}

impl Conjunction {
    /// Whether a typed filter narrows it, so that with no pieces it still
    /// lists what passes (all of a chatter's messages, say).
    pub fn filtered(&self) -> bool {
        !self.from.is_empty() || self.sources.is_some() || self.after.is_some() || self.before.is_some()
    }

    fn and(mut self, other: &Conjunction) -> Conjunction {
        self.pieces.extend(other.pieces.iter().cloned());
        self.not.extend(other.not.iter().cloned());
        self.from.extend(other.from.iter().cloned());
        self.sources = match (self.sources, other.sources) {
            (Some(a), Some(b)) => Some(Sources { audio: a.audio || b.audio, chat: a.chat || b.chat }),
            (a, b) => a.or(b),
        };
        // Both hold: the later start and the earlier end.
        self.after = max_of(self.after, other.after);
        self.before = min_of(self.before, other.before);
        self
    }
}

fn max_of(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn min_of(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Query {
    /// None for a query with nothing in it.
    pub tree: Option<Node>,
    /// The window, if typed: the last `within:` anywhere in the query.
    pub within: Option<f64>,
}

impl Query {
    /// The filters that apply to the whole query (those not inside an `OR`
    /// or brackets), for the filter's controls to show.
    pub fn top_filters(&self) -> Vec<(Key, &str)> {
        let items: Vec<&Node> = match &self.tree {
            Some(Node::And { items }) => items.iter().collect(),
            Some(node) => vec![node],
            None => Vec::new(),
        };
        items
            .into_iter()
            .filter_map(|n| match n {
                Node::Filter { key, value } if !value.is_empty() => Some((*key, value.as_str())),
                _ => None,
            })
            .collect()
    }

    /// The query as alternatives, each run on its own: `(a OR b) c` is
    /// `a c` and `b c`, so a filter inside brackets is that alternative's.
    pub fn conjunctions(&self) -> Result<Vec<Conjunction>, String> {
        match &self.tree {
            Some(node) => alternatives(node),
            None => Ok(Vec::new()),
        }
    }
}

/// What the typed text set, as the page shows it: the grouping, and the
/// filters that apply to the whole query, which the filter's controls then
/// show as set by the text. `error` (an unbalanced quote, say) comes with no hits.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct QueryInfo {
    pub tree: Option<Node>,
    pub within: Option<f64>,
    pub from: Option<String>,
    pub sources: Option<Sources>,
    pub after: Option<f64>,
    pub before: Option<f64>,
    pub error: Option<String>,
}

impl QueryInfo {
    pub fn of(query: &Query) -> QueryInfo {
        let mut info = QueryInfo { tree: query.tree.clone(), within: query.within, ..Default::default() };
        // Values were checked by `parse`; where one is typed twice the last shows.
        for (key, value) in query.top_filters() {
            match key {
                Key::From => info.from = Some(value.to_string()),
                Key::In => {
                    let s = sources(value).unwrap_or_default();
                    let was = info.sources.unwrap_or_default();
                    info.sources = Some(Sources { audio: was.audio || s.audio, chat: was.chat || s.chat });
                }
                Key::Within => {}
                Key::After => info.after = seconds(value),
                Key::Before => info.before = seconds(value),
            }
        }
        info
    }

    pub fn error(message: String) -> QueryInfo {
        QueryInfo { error: Some(message), ..Default::default() }
    }
}

fn alternatives(node: &Node) -> Result<Vec<Conjunction>, String> {
    let one = |c: Conjunction| Ok(vec![c]);
    match node {
        Node::Phrase { text, prefix, .. } => {
            one(Conjunction { pieces: vec![piece(text, *prefix)], ..Default::default() })
        }
        Node::Not { item } => match item.as_ref() {
            Node::Phrase { text, .. } => one(Conjunction { not: vec![piece(text, false)], ..Default::default() }),
            _ => unreachable!("the parser only negates phrases"),
        },
        Node::Filter { key, value } => one(filter(*key, value)?),
        Node::Or { items } => {
            let mut out = Vec::new();
            for item in items {
                out.extend(alternatives(item)?);
                if out.len() > MOST_ALTERNATIVES {
                    return Err(too_many());
                }
            }
            Ok(out)
        }
        Node::And { items } => {
            let mut out = vec![Conjunction::default()];
            for item in items {
                let next = alternatives(item)?;
                if out.len() * next.len() > MOST_ALTERNATIVES {
                    return Err(too_many());
                }
                out = out.iter().flat_map(|a| next.iter().map(|b| a.clone().and(b))).collect();
            }
            Ok(out)
        }
    }
}

fn too_many() -> String {
    format!("This search has more than {MOST_ALTERNATIVES} alternatives; use fewer ORs.")
}

fn piece(text: &str, prefix: bool) -> Piece {
    Piece { terms: terms(text), prefix }
}

fn filter(key: Key, value: &str) -> Result<Conjunction, String> {
    let mut c = Conjunction::default();
    // Still being typed: nothing yet.
    if value.is_empty() {
        return Ok(c);
    }
    match key {
        Key::From => c.from.push(value.to_string()),
        Key::In => c.sources = Some(sources(value)?),
        // Global, read by `parse`.
        Key::Within => {}
        Key::After => c.after = Some(seconds(value).ok_or_else(|| time_error(key))?),
        Key::Before => c.before = Some(seconds(value).ok_or_else(|| time_error(key))?),
    }
    Ok(c)
}

fn sources(value: &str) -> Result<Sources, String> {
    match value.to_lowercase().as_str() {
        "chat" => Ok(Sources { audio: false, chat: true }),
        "audio" => Ok(Sources { audio: true, chat: false }),
        _ => Err(format!("in: is in:audio or in:chat, not in:{value}.")),
    }
}

fn time_error(key: Key) -> String {
    let name = key.name();
    format!("{name}: takes a time into the VOD, such as {name}:1:20:00, {name}:90m or {name}:2h.")
}

/// Seconds from `1:20:00`, `20:00`, `90`, `90s`, `2m`, `1h` or `1h30m`; a
/// bare number is seconds.
pub fn seconds(text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    if text.contains(':') {
        let parts: Vec<f64> = text.split(':').map(number).collect::<Option<_>>()?;
        if parts.len() > 3 {
            return None;
        }
        return Some(parts.iter().fold(0.0, |acc, p| acc * 60.0 + p));
    }
    if let Some(n) = number(&text) {
        return Some(n);
    }
    // Numbers each followed by a unit, larger units first: 1h30m.
    let mut total = 0.0;
    let mut rest = text.as_str();
    let mut last_unit = f64::INFINITY;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        let n = number(&rest[..digits])?;
        rest = &rest[digits..];
        let unit = rest.chars().next()?;
        let size = match unit {
            'h' => 3600.0,
            'm' => 60.0,
            's' => 1.0,
            _ => return None,
        };
        if size >= last_unit {
            return None;
        }
        last_unit = size;
        total += n * size;
        rest = &rest[1..];
    }
    Some(total)
}

fn number(text: &str) -> Option<f64> {
    if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    text.parse().ok().filter(|n: &f64| n.is_finite())
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Open,
    Close,
    Or,
    And,
    Word(String),
    Quoted(String),
    NotWord(String),
    NotQuoted(String),
    Filter(Key, String),
}

/// Characters that end a word: brackets and quotes stand on their own.
fn ends_word(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | '"')
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let quoted = |i: &mut usize| -> Result<String, String> {
        // `*i` is just past the opening quote.
        let start = *i;
        while *i < chars.len() && chars[*i] != '"' {
            *i += 1;
        }
        if *i == chars.len() {
            return Err("A quote isn't closed: add the other \".".into());
        }
        let s: String = chars[start..*i].iter().collect();
        *i += 1;
        Ok(s)
    };
    let word = |i: &mut usize| -> String {
        let start = *i;
        while *i < chars.len() && !ends_word(chars[*i]) {
            *i += 1;
        }
        chars[start..*i].iter().collect()
    };
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            out.push(Token::Open);
            i += 1;
        } else if c == ')' {
            out.push(Token::Close);
            i += 1;
        } else if c == '"' {
            i += 1;
            out.push(Token::Quoted(quoted(&mut i)?));
        } else if c == '-' {
            i += 1;
            match chars.get(i) {
                Some('"') => {
                    i += 1;
                    out.push(Token::NotQuoted(quoted(&mut i)?));
                }
                Some('(') => return Err("A minus goes before a word or a quoted phrase, not brackets.".into()),
                Some(&c) if !ends_word(c) => {
                    let w = word(&mut i);
                    if w.split_once(':').is_some_and(|(k, _)| Key::parse(k).is_some()) {
                        return Err(format!("A minus goes before a word or a quoted phrase, not a filter ({w})."));
                    }
                    out.push(Token::NotWord(w));
                }
                // A dash on its own is punctuation.
                _ => {}
            }
        } else {
            let w = word(&mut i);
            if w == "OR" {
                out.push(Token::Or);
            } else if w == "AND" {
                out.push(Token::And);
            } else if let Some((key, value)) = w.split_once(':').and_then(|(k, v)| Some((Key::parse(k)?, v))) {
                // A quoted value, for a name with spaces: from:"Some One".
                let value = if value.is_empty() && chars.get(i) == Some(&'"') {
                    i += 1;
                    quoted(&mut i)?
                } else {
                    value.to_string()
                };
                out.push(Token::Filter(key, value));
            } else {
                out.push(Token::Word(w));
            }
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    /// An OR or AND with nothing on one side (while typing `a OR`) is left out.
    fn skip_operators(&mut self) {
        while matches!(self.peek(), Some(Token::Or | Token::And)) {
            self.at += 1;
        }
    }

    /// Pieces side by side, up to a closing bracket or the end: the implicit AND.
    fn sequence(&mut self) -> Result<Option<Node>, String> {
        let mut items = Vec::new();
        loop {
            self.skip_operators();
            match self.peek() {
                None | Some(Token::Close) => break,
                _ => items.extend(self.or()?),
            }
        }
        Ok(and_of(items))
    }

    fn or(&mut self) -> Result<Option<Node>, String> {
        let mut items: Vec<Node> = self.and()?.into_iter().collect();
        while self.peek() == Some(&Token::Or) {
            self.skip_operators();
            items.extend(self.and()?);
        }
        Ok(or_of(items))
    }

    fn and(&mut self) -> Result<Option<Node>, String> {
        let mut items: Vec<Node> = self.atom()?.into_iter().collect();
        while self.peek() == Some(&Token::And) {
            self.at += 1;
            // `a AND OR b`: the OR is the one meant.
            if self.peek() == Some(&Token::Or) {
                break;
            }
            items.extend(self.atom()?);
        }
        Ok(and_of(items))
    }

    fn atom(&mut self) -> Result<Option<Node>, String> {
        let Some(token) = self.peek().cloned() else { return Ok(None) };
        self.at += 1;
        Ok(match token {
            Token::Word(first) => {
                // Words run on into one phrase until something else comes.
                let mut words = vec![first];
                while let Some(Token::Word(w)) = self.peek() {
                    words.push(w.clone());
                    self.at += 1;
                }
                let text = words.join(" ");
                let t = terms(&text);
                // The phrase the query ends with is the one being typed.
                let prefix = self.at == self.tokens.len() && last_is_prefix(&t);
                (!t.is_empty()).then_some(Node::Phrase { text, strict: false, prefix })
            }
            Token::Quoted(text) => (!terms(&text).is_empty()).then(|| Node::Phrase {
                text: text.trim().to_string(),
                strict: true,
                prefix: false,
            }),
            Token::NotWord(text) | Token::NotQuoted(text) if terms(&text).is_empty() => None,
            Token::NotWord(text) => {
                Some(Node::Not { item: Box::new(Node::Phrase { text, strict: false, prefix: false }) })
            }
            Token::NotQuoted(text) => Some(Node::Not {
                item: Box::new(Node::Phrase { text: text.trim().to_string(), strict: true, prefix: false }),
            }),
            Token::Filter(key, value) => Some(Node::Filter { key, value }),
            Token::Open => {
                let inside = self.sequence()?;
                if self.peek() != Some(&Token::Close) {
                    return Err("A bracket isn't closed: add a ).".into());
                }
                self.at += 1;
                inside
            }
            Token::Close => unreachable!("sequence stops at a closing bracket"),
            Token::Or | Token::And => None,
        })
    }
}

fn and_of(items: Vec<Node>) -> Option<Node> {
    group(items, |n| match n {
        Node::And { items } => Ok(items),
        n => Err(n),
    })
    .map(|mut items| if items.len() == 1 { items.remove(0) } else { Node::And { items } })
}

fn or_of(items: Vec<Node>) -> Option<Node> {
    group(items, |n| match n {
        Node::Or { items } => Ok(items),
        n => Err(n),
    })
    .map(|mut items| if items.len() == 1 { items.remove(0) } else { Node::Or { items } })
}

/// Items with any of the same kind inside merged into them (`(a AND b) AND
/// c` is `a AND b AND c`); None for none.
fn group(items: Vec<Node>, same: impl Fn(Node) -> Result<Vec<Node>, Node>) -> Option<Vec<Node>> {
    let mut out = Vec::new();
    for item in items {
        match same(item) {
            Ok(inner) => out.extend(inner),
            Err(item) => out.push(item),
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The query typed into the search box, parsed; an unbalanced quote or
/// bracket, or a filter's value that isn't one, is the message to show.
pub fn parse(text: &str) -> Result<Query, String> {
    let mut parser = Parser { tokens: tokens(text)?, at: 0 };
    let tree = parser.sequence()?;
    if parser.peek() == Some(&Token::Close) {
        return Err("A bracket is closed that wasn't opened: take out a ).".into());
    }
    let mut query = Query { tree, within: None };
    // Each filter's value checked, and the window read: the last `within:` typed.
    let mut error = None;
    if let Some(tree) = &query.tree {
        visit(tree, &mut |node| {
            if let Node::Filter { key, value } = node
                && !value.is_empty()
            {
                if *key == Key::Within {
                    match seconds(value).filter(|&s| s > 0.0) {
                        Some(s) => query.within = Some(s),
                        None => error = Some("within: takes a length of time, such as within:20s, 2m or 1h.".into()),
                    }
                } else if let Err(e) = filter(*key, value) {
                    error = Some(e);
                }
            }
        });
    }
    match error {
        Some(e) => Err(e),
        None => Ok(query),
    }
}

fn visit(node: &Node, f: &mut impl FnMut(&Node)) {
    f(node);
    match node {
        Node::Or { items } | Node::And { items } => items.iter().for_each(|n| visit(n, f)),
        Node::Not { item } => visit(item, f),
        Node::Phrase { .. } | Node::Filter { .. } => {}
    }
}

/// Back to text that parses to the same tree: brackets around every group
/// inside another, and AND between two loose phrases (side by side they
/// would run into one).
impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = |n: &Node| match n {
            Node::Or { .. } | Node::And { .. } => format!("({n})"),
            n => n.to_string(),
        };
        let loose = |n: &Node| matches!(n, Node::Phrase { strict: false, .. });
        match self {
            Node::Or { items } => f.write_str(&items.iter().map(inner).collect::<Vec<_>>().join(" OR ")),
            Node::And { items } => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(if loose(&items[i - 1]) && loose(item) { " AND " } else { " " })?;
                    }
                    f.write_str(&inner(item))?;
                }
                Ok(())
            }
            Node::Phrase { text, strict: true, .. } => write!(f, "\"{text}\""),
            Node::Phrase { text, .. } => f.write_str(text),
            Node::Not { item } => write!(f, "-{item}"),
            Node::Filter { key, value } if value.contains(char::is_whitespace) => {
                write!(f, "{}:\"{value}\"", key.name())
            }
            Node::Filter { key, value } => write!(f, "{}:{value}", key.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loose(text: &str) -> Node {
        Node::Phrase { text: text.into(), strict: false, prefix: false }
    }
    fn typing(text: &str) -> Node {
        Node::Phrase { text: text.into(), strict: false, prefix: true }
    }
    fn strict(text: &str) -> Node {
        Node::Phrase { text: text.into(), strict: true, prefix: false }
    }
    fn not(item: Node) -> Node {
        Node::Not { item: Box::new(item) }
    }
    fn and(items: Vec<Node>) -> Node {
        Node::And { items }
    }
    fn or(items: Vec<Node>) -> Node {
        Node::Or { items }
    }
    fn filter(key: Key, value: &str) -> Node {
        Node::Filter { key, value: value.into() }
    }
    fn tree(text: &str) -> Node {
        parse(text).unwrap().tree.unwrap()
    }
    fn error(text: &str) -> String {
        parse(text).unwrap_err()
    }

    #[test]
    fn words_are_one_phrase_the_last_word_being_typed() {
        assert_eq!(tree("clutch play"), typing("clutch play"));
        assert_eq!(tree("clutch play "), typing("clutch play"), "a space after doesn't end the typing");
        assert_eq!(tree("a"), loose("a"), "one letter is a word, not a prefix");
        assert_eq!(tree("don't stop"), typing("don't stop"));
        assert_eq!(parse("").unwrap().tree, None);
        assert_eq!(parse("  !! - ").unwrap().tree, None, "nothing to search for");
        // As before the query language: `"` OR *` has a quote left open.
        assert!(parse("\" OR *").is_err());
    }

    #[test]
    fn quotes_make_strict_pieces() {
        assert_eq!(tree("nice \"clutch\""), and(vec![loose("nice"), strict("clutch")]));
        assert_eq!(
            tree("\"all\" \"words\" \"any\" \"order\""),
            and(vec![strict("all"), strict("words"), strict("any"), strict("order")])
        );
        assert_eq!(tree("\"a good shot\""), strict("a good shot"));
        assert_eq!(tree("\"\" hello"), typing("hello"), "an empty quote is nothing");
    }

    #[test]
    fn the_owners_examples() {
        // damn / well that was / a good shot: found by
        assert_eq!(
            tree("well that was a good shot \"damn\""),
            and(vec![loose("well that was a good shot"), strict("damn")])
        );
        assert_eq!(tree("(well that was a good shot \"damn\")"), tree("well that was a good shot \"damn\""));
        assert_eq!(tree("i did it -frick"), and(vec![loose("i did it"), not(loose("frick"))]));
        assert_eq!(tree("well that was OR damn"), or(vec![loose("well that was"), typing("damn")]));
    }

    #[test]
    fn and_binds_tighter_than_or_and_side_by_side_looser() {
        assert_eq!(
            tree("\"a\" AND \"b\" OR \"c\""),
            or(vec![and(vec![strict("a"), strict("b")]), strict("c")]),
            "AND before OR"
        );
        assert_eq!(tree("\"a\" OR \"b\" AND \"c\""), or(vec![strict("a"), and(vec![strict("b"), strict("c")])]));
        assert_eq!(
            tree("from:x \"a\" OR \"b\""),
            and(vec![filter(Key::From, "x"), or(vec![strict("a"), strict("b")])]),
            "typed filters apply to the whole query"
        );
        assert_eq!(
            tree("nice \"clutch\" OR \"play\" \"good\""),
            and(vec![loose("nice"), or(vec![strict("clutch"), strict("play")]), strict("good")])
        );
        assert_eq!(tree("(\"a\" OR \"b\") \"c\""), and(vec![or(vec![strict("a"), strict("b")]), strict("c")]));
        assert_eq!(
            tree("(from:a \"x\") OR (from:b \"y\")"),
            or(vec![and(vec![filter(Key::From, "a"), strict("x")]), and(vec![filter(Key::From, "b"), strict("y")])])
        );
        assert_eq!(tree("a b AND c"), and(vec![loose("a b"), loose("c")]), "AND splits a phrase");
        assert_eq!(tree("cats or dogs"), typing("cats or dogs"), "lowercase or is a word");
    }

    #[test]
    fn half_typed_operators_are_left_out() {
        assert_eq!(tree("well that was OR"), loose("well that was"));
        assert_eq!(tree("OR damn"), typing("damn"));
        assert_eq!(tree("a OR OR bb"), or(vec![loose("a"), typing("bb")]));
        assert_eq!(tree("a AND"), loose("a"));
        assert_eq!(tree("\"a\" AND OR \"b\""), or(vec![strict("a"), strict("b")]));
        assert_eq!(tree("hello -"), typing("hello"), "a dash on its own is punctuation, as before");
        assert_eq!(tree("from:"), filter(Key::From, ""), "a filter being typed");
        assert_eq!(parse("()").unwrap().tree, None);
    }

    #[test]
    fn negation_and_filters() {
        assert_eq!(tree("-\"a phrase\" bb"), and(vec![not(strict("a phrase")), typing("bb")]));
        assert_eq!(tree("well-known"), typing("well-known"), "a dash inside a word is the word's");
        assert_eq!(
            tree("from:\"Some One\" in:chat hi"),
            and(vec![filter(Key::From, "Some One"), filter(Key::In, "chat"), typing("hi")])
        );
        assert_eq!(tree("re:zero"), typing("re:zero"), "an unknown key is words");
        assert_eq!(tree("FROM:Bob"), filter(Key::From, "Bob"), "keys in any case");
        let q = parse("within:5 a within:2m after:1:20:00 before:2h in:audio from:bob").unwrap();
        assert_eq!(q.within, Some(120.0), "the last within: wins");
        let c = q.conjunctions().unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].after, c[0].before), (Some(4800.0), Some(7200.0)));
        assert_eq!(c[0].sources, Some(Sources { audio: true, chat: false }));
        assert_eq!(c[0].from, ["bob"]);
        assert_eq!(q.top_filters().len(), 6);
    }

    #[test]
    fn errors_say_what_to_fix() {
        assert!(error("\"unclosed").contains("quote"));
        assert!(error("-\"unclosed").contains("quote"));
        assert!(error("(a OR b").contains("bracket isn't closed"));
        assert!(error("a OR b)").contains("wasn't opened"));
        assert!(error("((a) b").contains("bracket"));
        assert!(error("in:video").contains("in:audio"));
        assert!(error("within:soon").contains("within:"));
        assert!(error("within:0").contains("within:"));
        assert!(error("after:yesterday").contains("after:"));
        assert!(error("-(a b)").contains("minus"));
        assert!(error("-from:bob").contains("minus"));
        let many = (0..7).map(|i| format!("(\"a{i}\" OR \"b{i}\")")).collect::<Vec<_>>().join(" ");
        assert!(parse(&many).unwrap().conjunctions().unwrap_err().contains("alternatives"));
    }

    #[test]
    fn alternatives_carry_their_own_filters() {
        let c = parse("(from:a \"x\") OR (from:b \"y\")").unwrap().conjunctions().unwrap();
        assert_eq!(
            c.iter().map(|c| (c.from.clone(), c.pieces[0].terms.clone())).collect::<Vec<_>>(),
            [(vec!["a".to_string()], vec!["x".to_string()]), (vec!["b".to_string()], vec!["y".to_string()])]
        );
        let c = parse("from:x (\"a\" OR \"b\") \"c\" -d").unwrap().conjunctions().unwrap();
        assert_eq!(c.len(), 2);
        assert!(c.iter().all(|c| c.from == ["x"] && c.pieces.len() == 2 && c.not.len() == 1));
        let c = parse("in:chat in:audio after:10 after:20 before:50 before:40").unwrap().conjunctions().unwrap();
        assert_eq!(c[0].sources, Some(Sources { audio: true, chat: true }), "either in:");
        assert_eq!((c[0].after, c[0].before), (Some(20.0), Some(40.0)), "all times hold");
        let c = parse("Café don't").unwrap().conjunctions().unwrap();
        assert_eq!(c[0].pieces, [Piece { terms: vec!["cafe".into(), "don".into(), "t".into()], prefix: false }]);
        assert!(c[0].pieces[0].terms.iter().all(|t| t.chars().all(char::is_alphanumeric)), "nothing FTS reads");
        let c = parse("-frick").unwrap().conjunctions().unwrap();
        assert!(c[0].pieces.is_empty() && !c[0].filtered(), "nothing to look for");
    }

    #[test]
    fn times() {
        assert_eq!(seconds("20"), Some(20.0));
        assert_eq!(seconds("20s"), Some(20.0));
        assert_eq!(seconds("2m"), Some(120.0));
        assert_eq!(seconds("1h"), Some(3600.0));
        assert_eq!(seconds("1h30m"), Some(5400.0));
        assert_eq!(seconds("1.5m"), Some(90.0));
        assert_eq!(seconds("1:20:00"), Some(4800.0));
        assert_eq!(seconds("20:00"), Some(1200.0));
        assert_eq!(seconds("30m1h"), None);
        assert_eq!(seconds("2x"), None);
        assert_eq!(seconds("h"), None);
        assert_eq!(seconds("1:2:3:4"), None);
    }

    #[test]
    fn shown_as_text_it_parses_the_same() {
        for text in [
            "well that was a good shot \"damn\"",
            "i did it -frick",
            "\"all\" \"words\" \"any\" \"order\"",
            "from:x \"a\" OR \"b\"",
            "\"a\" AND \"b\" OR \"c\"",
            "(from:a \"x\") OR (from:b \"y\")",
            "a b AND c",
            "nice \"clutch\" OR \"play\" good",
            "from:\"Some One\" -\"a phrase\" within:2m",
        ] {
            let parsed = tree(text);
            let shown = parsed.to_string();
            // Shown, the last phrase isn't being typed any more; else the same.
            assert_eq!(strip_prefix(tree(&format!("{shown} ()"))), strip_prefix(parsed), "{text} → {shown}");
        }
        assert_eq!(tree("from:x \"a\" OR \"b\"").to_string(), "from:x (\"a\" OR \"b\")");
    }

    fn strip_prefix(node: Node) -> Node {
        match node {
            Node::Phrase { text, strict, .. } => Node::Phrase { text, strict, prefix: false },
            Node::Or { items } => Node::Or { items: items.into_iter().map(strip_prefix).collect() },
            Node::And { items } => Node::And { items: items.into_iter().map(strip_prefix).collect() },
            n => n,
        }
    }

    #[test]
    fn the_tree_as_the_page_reads_it() {
        let json = serde_json::to_value(tree("from:x (\"a\" OR -b) c")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "and", "items": [
                {"kind": "filter", "key": "from", "value": "x"},
                {"kind": "or", "items": [
                    {"kind": "phrase", "text": "a", "strict": true, "prefix": false},
                    {"kind": "not", "item": {"kind": "phrase", "text": "b", "strict": false, "prefix": false}}
                ]},
                {"kind": "phrase", "text": "c", "strict": false, "prefix": false}
            ]})
        );
    }
}
