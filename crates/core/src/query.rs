//! The search box's language. A query is pieces, all of which must match:
//! words typed one after another are one loose phrase in order (the last word
//! a prefix while it's being typed), a quoted string a strict phrase of whole
//! words, `-word` or `-"a phrase"` something that mustn't be near. `OR`
//! offers alternatives and `AND` joins pieces explicitly, binding tighter
//! than `OR`; brackets group. Pieces and filters side by side (the implicit
//! AND) bind looser than `OR`, so `from:x "a" OR "b"` is from x, a or b.
//! Filters: `from:name` (`from:a,b` for either), `in:chat` / `in:audio`, `within:20s`, `after:1:20:00`,
//! `before:2h`. Anything else is words.
//!
//! Parsing is pure: `parse` gives the tree the page shows, and
//! `Query::conjunctions` the alternatives `Db::search` runs, one by one.
//!
//! The website's demo has a TypeScript port of this parser
//! (apps/site/src/lib/tauri/query.ts), and its query.test.mjs runs these
//! tests (query/tests.rs) against it, case for case: a change here is made there too.

use std::fmt;

use serde::Serialize;

use crate::transcript::{last_is_prefix, terms};

/// How far apart a match's pieces may be when `within:` isn't typed: from
/// the first matched word's start to the last one's end, in seconds.
pub const DEFAULT_WITHIN: f64 = 20.0;

/// The widest window, typed or the filter's: an hour. Each match through a
/// common word looks at every word a window either side of it, so a window
/// of hours took minutes over a long VOD (`search_benchmark`), holding up
/// everything else the app reads meanwhile. The page's MOST_WITHIN
/// (queryChips.ts) is the same.
pub const MOST_WITHIN: f64 = 3600.0;

/// What's said of a window over `MOST_WITHIN`.
pub const TOO_WIDE: &str = "within: is at most 1h.";

/// More alternatives than this (as `(a OR b) (c OR d) …` multiplies them) is
/// refused rather than run one by one.
const MOST_ALTERNATIVES: usize = 64;

/// Brackets inside each other deeper than this are refused: the parser goes
/// down one level of recursion per bracket, and thousands of them would run
/// out of stack, which aborts the app.
const MOST_BRACKETS: usize = 32;

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
    /// Every name `from:` gives, each once, separated by ", " (one with a
    /// comma in quotes), as the From whom control takes them.
    pub from: Option<String>,
    pub sources: Option<Sources>,
    pub after: Option<f64>,
    pub before: Option<f64>,
    pub error: Option<String>,
}

impl QueryInfo {
    pub fn of(query: &Query) -> QueryInfo {
        let mut info = QueryInfo { tree: query.tree.clone(), within: query.within, ..Default::default() };
        // Values were checked by `parse`. Every name `from:` gives shows, as
        // any of them is searched for; times typed twice all hold
        // (`Conjunction::and`), so the latest start and the earliest end
        // show, as the search applies them.
        let mut from: Vec<String> = Vec::new();
        for (key, value) in query.top_filters() {
            match key {
                Key::From => {
                    for name in names(value) {
                        if !from.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                            from.push(name);
                        }
                    }
                }
                Key::In => {
                    let s = sources(value).unwrap_or_default();
                    let was = info.sources.unwrap_or_default();
                    info.sources = Some(Sources { audio: was.audio || s.audio, chat: was.chat || s.chat });
                }
                Key::Within => {}
                Key::After => info.after = max_of(info.after, seconds(value)),
                Key::Before => info.before = min_of(info.before, seconds(value)),
            }
        }
        // As the From whom control takes them: a name with a comma in quotes.
        let shown = |n: &String| if n.contains(',') { format!("\"{n}\"") } else { n.clone() };
        info.from = (!from.is_empty()).then(|| from.iter().map(shown).collect::<Vec<_>>().join(", "));
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
        Key::From => c.from.extend(names(value)),
        Key::In => c.sources = Some(sources(value)?),
        // Global, read by `parse`.
        Key::Within => {}
        Key::After => c.after = Some(seconds(value).ok_or_else(|| time_error(key))?),
        Key::Before => c.before = Some(seconds(value).ok_or_else(|| time_error(key))?),
    }
    Ok(c)
}

/// The names in a `from:` value (`tokens`), or in the filter's From whom:
/// one, or several separated by commas, any of them in quotes; a comma
/// inside quotes is the name's.
pub fn names(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut name = String::new();
    let mut quoted = false;
    for c in value.chars().chain([',']) {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                let n = name.trim();
                if !n.is_empty() {
                    out.push(n.to_string());
                }
                name.clear();
            }
            c => name.push(c),
        }
    }
    out
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
        return Some(parts.iter().fold(0.0, |acc, p| acc * 60.0 + p)).filter(|s| s.is_finite());
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
    Some(total).filter(|s| s.is_finite())
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

/// The text up to the closing quote, from `*i` just past the opening one;
/// `*i` ends past the closing one.
fn quoted(chars: &[char], i: &mut usize) -> Result<String, String> {
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
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
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
            out.push(Token::Quoted(quoted(&chars, &mut i)?));
        } else if c == '-' {
            i += 1;
            match chars.get(i) {
                Some('"') => {
                    i += 1;
                    out.push(Token::NotQuoted(quoted(&chars, &mut i)?));
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
                let value = if key == Key::From {
                    // Read again from the colon, as the names it lists.
                    i -= value.chars().count();
                    from_value(&chars, &mut i)?
                } else if value.is_empty() && chars.get(i) == Some(&'"') {
                    // A quoted value: within:"2m".
                    i += 1;
                    quoted(&chars, &mut i)?
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

/// A `from:` value, from `*i` just past the colon: one name, or several
/// separated by commas (`from:a,b`), each bare or in quotes for one with a
/// space or a comma (`from:a,"Some One"`). The value is a name alone as
/// itself; several, or one with a comma, as typed, quotes and all, which
/// `names` reads.
fn from_value(chars: &[char], i: &mut usize) -> Result<String, String> {
    let start = *i;
    let mut names = Vec::new();
    loop {
        if chars.get(*i) == Some(&'"') {
            *i += 1;
            names.push(quoted(chars, i)?);
        } else {
            let from = *i;
            while *i < chars.len() && !ends_word(chars[*i]) && chars[*i] != ',' {
                *i += 1;
            }
            names.push(chars[from..*i].iter().collect());
        }
        if chars.get(*i) != Some(&',') {
            break;
        }
        *i += 1;
    }
    Ok(match &names[..] {
        [name] if !name.contains(',') => name.clone(),
        _ => chars[start..*i].iter().collect(),
    })
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    /// How many brackets are open where it's reading.
    depth: usize,
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
                if self.depth == MOST_BRACKETS {
                    return Err(format!("More than {MOST_BRACKETS} brackets inside each other: take some out."));
                }
                self.depth += 1;
                let inside = self.sequence()?;
                self.depth -= 1;
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
    let mut parser = Parser { tokens: tokens(text)?, at: 0, depth: 0 };
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
                        Some(s) if s > MOST_WITHIN => error = Some(TOO_WIDE.into()),
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
            // A list of names (`from_value`) has its own quotes.
            Node::Filter { key, value } if value.contains(char::is_whitespace) && !value.contains('"') => {
                write!(f, "{}:\"{value}\"", key.name())
            }
            Node::Filter { key, value } => write!(f, "{}:{value}", key.name()),
        }
    }
}

#[cfg(test)]
mod tests;
