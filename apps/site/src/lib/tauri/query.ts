// The search box's language for the demo: a port of the app's parser
// (crates/core query.rs), which says what the language is. Keep the two
// together: a change there is made here too, and query.test.mjs runs
// query.rs's tests against this file (same cases, same trees), so
// `node --test apps/site/src/lib/tauri/query.test.mjs` catches a drift.
// No imports, so Node can test it directly.

/** How far apart a match's pieces may be when `within:` isn't typed, in seconds. */
export const DEFAULT_WITHIN = 20;

/** The widest window, typed or the filter's: an hour, as in the app. */
export const MOST_WITHIN = 3600;

/** What's said of a window over `MOST_WITHIN`. */
export const TOO_WIDE = "within: is at most 1h.";

/** More alternatives than this is refused rather than run one by one. */
const MOST_ALTERNATIVES = 64;

/** Brackets inside each other deeper than this are refused. */
export const MOST_BRACKETS = 32;

export type Key = "from" | "in" | "within" | "after" | "before";

/** The parsed query, as the page shows its grouping (desktop api.ts `QueryNode`). */
export type Node =
  | { kind: "or"; items: Node[] }
  | { kind: "and"; items: Node[] }
  | { kind: "phrase"; text: string; strict: boolean; prefix: boolean }
  | { kind: "not"; item: Node }
  | { kind: "filter"; key: Key; value: string };

export interface Sources {
  audio: boolean;
  chat: boolean;
}

/** A phrase to find: its words folded as the index folds them. */
export interface Piece {
  terms: string[];
  /** The last term matches as the start of a word. */
  prefix: boolean;
}

/** One alternative of a query: everything in it must hold together. */
export interface Conjunction {
  pieces: Piece[];
  /** Phrases that mustn't be in the window around a match (in chat: in the message). */
  not: Piece[];
  /** Who: any of these names (a channel's VODs, or a chatter's messages). */
  from: string[];
  /** Typed `in:`, which takes the place of the filter's sources. */
  sources: Sources | null;
  after: number | null;
  before: number | null;
}

export interface Query {
  /** Null for a query with nothing in it. */
  tree: Node | null;
  /** The window, if typed: the last `within:` anywhere in the query. */
  within: number | null;
}

/** What the typed text set (desktop api.ts `SearchQuery`). */
export interface QueryInfo {
  tree: Node | null;
  within: number | null;
  from: string | null;
  sources: Sources | null;
  after: number | null;
  before: number | null;
  error: string | null;
}

/** A query that can't be run: the message says what to fix. */
export class QueryError extends Error {}

/** Lowercased alphanumeric runs with accents removed, like the app's `terms`. */
export function terms(text: string): string[] {
  return text
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

/** Whether a search's last term matches as a prefix: once it's two characters
 *  or more, as in the app (crates/core transcript.rs `last_is_prefix`). */
export function lastIsPrefix(query: string[]): boolean {
  return [...(query.at(-1) ?? "")].length >= 2;
}

function keyOf(name: string): Key | null {
  const k = name.toLowerCase();
  return k === "from" || k === "in" || k === "within" || k === "after" || k === "before" ? k : null;
}

export function emptyConjunction(): Conjunction {
  return { pieces: [], not: [], from: [], sources: null, after: null, before: null };
}

/** Whether a typed filter narrows it, so that with no pieces it still lists what passes. */
export function filtered(c: Conjunction): boolean {
  return c.from.length > 0 || c.sources !== null || c.after !== null || c.before !== null;
}

const maxOf = (a: number | null, b: number | null) => (a !== null && b !== null ? Math.max(a, b) : (a ?? b));
const minOf = (a: number | null, b: number | null) => (a !== null && b !== null ? Math.min(a, b) : (a ?? b));

function both(a: Conjunction, b: Conjunction): Conjunction {
  return {
    pieces: [...a.pieces, ...b.pieces],
    not: [...a.not, ...b.not],
    from: [...a.from, ...b.from],
    sources:
      a.sources && b.sources
        ? { audio: a.sources.audio || b.sources.audio, chat: a.sources.chat || b.sources.chat }
        : (a.sources ?? b.sources),
    // Both hold: the later start and the earlier end.
    after: maxOf(a.after, b.after),
    before: minOf(a.before, b.before),
  };
}

/** The filters that apply to the whole query (not inside an OR or brackets). */
export function topFilters(query: Query): [Key, string][] {
  const t = query.tree;
  const items = t === null ? [] : t.kind === "and" ? t.items : [t];
  return items.flatMap((n) => (n.kind === "filter" && n.value !== "" ? [[n.key, n.value] as [Key, string]] : []));
}

/** The query as alternatives, each run on its own: `(a OR b) c` is `a c` and `b c`. */
export function conjunctions(query: Query): Conjunction[] {
  return query.tree ? alternatives(query.tree) : [];
}

export function queryInfo(query: Query): QueryInfo {
  const info: QueryInfo = { ...noInfo(), tree: query.tree, within: query.within };
  for (const [key, value] of topFilters(query)) {
    if (key === "from") info.from = value;
    else if (key === "in") {
      const s = sources(value);
      const was = info.sources ?? { audio: false, chat: false };
      info.sources = { audio: was.audio || s.audio, chat: was.chat || s.chat };
    } else if (key === "after") info.after = maxOf(info.after, seconds(value));
    else if (key === "before") info.before = minOf(info.before, seconds(value));
  }
  return info;
}

export function noInfo(error: string | null = null): QueryInfo {
  return { tree: null, within: null, from: null, sources: null, after: null, before: null, error };
}

function alternatives(node: Node): Conjunction[] {
  switch (node.kind) {
    case "phrase":
      return [{ ...emptyConjunction(), pieces: [piece(node.text, node.prefix)] }];
    case "not":
      // The parser only negates phrases.
      if (node.item.kind !== "phrase") throw new Error("the parser only negates phrases");
      return [{ ...emptyConjunction(), not: [piece(node.item.text, false)] }];
    case "filter":
      return [filter(node.key, node.value)];
    case "or": {
      const out: Conjunction[] = [];
      for (const item of node.items) {
        out.push(...alternatives(item));
        if (out.length > MOST_ALTERNATIVES) throw new QueryError(tooMany());
      }
      return out;
    }
    case "and": {
      let out = [emptyConjunction()];
      for (const item of node.items) {
        const next = alternatives(item);
        if (out.length * next.length > MOST_ALTERNATIVES) throw new QueryError(tooMany());
        out = out.flatMap((a) => next.map((b) => both(a, b)));
      }
      return out;
    }
  }
}

const tooMany = () => `This search has more than ${MOST_ALTERNATIVES} alternatives; use fewer ORs.`;

function piece(text: string, prefix: boolean): Piece {
  return { terms: terms(text), prefix };
}

function filter(key: Key, value: string): Conjunction {
  const c = emptyConjunction();
  // Still being typed: nothing yet.
  if (value === "") return c;
  const time = () => {
    const s = seconds(value);
    if (s === null)
      throw new QueryError(`${key}: takes a time into the VOD, such as ${key}:1:20:00, ${key}:90m or ${key}:2h.`);
    return s;
  };
  if (key === "from") c.from.push(value);
  else if (key === "in") c.sources = sources(value);
  else if (key === "after") c.after = time();
  else if (key === "before") c.before = time();
  // within: is global, read by `parse`.
  return c;
}

function sources(value: string): Sources {
  const v = value.toLowerCase();
  if (v === "chat") return { audio: false, chat: true };
  if (v === "audio") return { audio: true, chat: false };
  throw new QueryError(`in: is in:audio or in:chat, not in:${value}.`);
}

/** Seconds from `1:20:00`, `20:00`, `90`, `90s`, `2m`, `1h` or `1h30m`; a bare number is seconds. */
export function seconds(input: string): number | null {
  const text = input.trim().toLowerCase();
  if (text === "") return null;
  const finite = (n: number) => (Number.isFinite(n) ? n : null);
  if (text.includes(":")) {
    const parts = text.split(":").map(number);
    if (parts.some((p) => p === null) || parts.length > 3) return null;
    return finite((parts as number[]).reduce((acc, p) => acc * 60 + p, 0));
  }
  const n = number(text);
  if (n !== null) return n;
  // Numbers each followed by a unit, larger units first: 1h30m.
  let total = 0;
  let rest = text;
  let lastUnit = Infinity;
  while (rest !== "") {
    const digits = rest.match(/^[0-9.]*/)![0].length;
    const n = number(rest.slice(0, digits));
    if (n === null) return null;
    rest = rest.slice(digits);
    const size = ({ h: 3600, m: 60, s: 1 } as Record<string, number>)[rest[0]];
    if (size === undefined || size >= lastUnit) return null;
    lastUnit = size;
    total += n * size;
    rest = rest.slice(1);
  }
  return finite(total);
}

function number(text: string): number | null {
  if (!/^[0-9.]+$/.test(text)) return null;
  const n = Number(text);
  return Number.isFinite(n) ? n : null;
}

type Token =
  | { t: "open" }
  | { t: "close" }
  | { t: "or" }
  | { t: "and" }
  | { t: "word"; text: string }
  | { t: "quoted"; text: string }
  | { t: "notWord"; text: string }
  | { t: "notQuoted"; text: string }
  | { t: "filter"; key: Key; value: string };

const isSpace = (c: string) => /\s/u.test(c);
/** Characters that end a word: brackets and quotes stand on their own. */
const endsWord = (c: string) => isSpace(c) || c === "(" || c === ")" || c === '"';

function tokens(text: string): Token[] {
  const chars = Array.from(text);
  const out: Token[] = [];
  let i = 0;
  // `i` is just past the opening quote.
  const quoted = () => {
    const start = i;
    while (i < chars.length && chars[i] !== '"') i++;
    if (i === chars.length) throw new QueryError("A quote isn't closed: add the other \".");
    const s = chars.slice(start, i).join("");
    i++;
    return s;
  };
  const word = () => {
    const start = i;
    while (i < chars.length && !endsWord(chars[i])) i++;
    return chars.slice(start, i).join("");
  };
  while (i < chars.length) {
    const c = chars[i];
    if (isSpace(c)) i++;
    else if (c === "(") {
      out.push({ t: "open" });
      i++;
    } else if (c === ")") {
      out.push({ t: "close" });
      i++;
    } else if (c === '"') {
      i++;
      out.push({ t: "quoted", text: quoted() });
    } else if (c === "-") {
      i++;
      const next = chars[i];
      if (next === '"') {
        i++;
        out.push({ t: "notQuoted", text: quoted() });
      } else if (next === "(") {
        throw new QueryError("A minus goes before a word or a quoted phrase, not brackets.");
      } else if (next !== undefined && !endsWord(next)) {
        const w = word();
        const colon = w.indexOf(":");
        if (colon >= 0 && keyOf(w.slice(0, colon)) !== null)
          throw new QueryError(`A minus goes before a word or a quoted phrase, not a filter (${w}).`);
        out.push({ t: "notWord", text: w });
      }
      // A dash on its own is punctuation.
    } else {
      const w = word();
      const colon = w.indexOf(":");
      const key = colon >= 0 ? keyOf(w.slice(0, colon)) : null;
      if (w === "OR") out.push({ t: "or" });
      else if (w === "AND") out.push({ t: "and" });
      else if (key !== null) {
        let value = w.slice(colon + 1);
        // A quoted value, for a name with spaces: from:"Some One".
        if (value === "" && chars[i] === '"') {
          i++;
          value = quoted();
        }
        out.push({ t: "filter", key, value });
      } else out.push({ t: "word", text: w });
    }
  }
  return out;
}

class Parser {
  at = 0;
  /** How many brackets are open where it's reading. */
  depth = 0;
  readonly tokens: Token[];
  constructor(tokens: Token[]) {
    this.tokens = tokens;
  }

  peek(): Token | undefined {
    return this.tokens[this.at];
  }

  is(t: Token["t"]): boolean {
    return this.peek()?.t === t;
  }

  /** An OR or AND with nothing on one side (while typing `a OR`) is left out. */
  skipOperators() {
    while (this.is("or") || this.is("and")) this.at++;
  }

  /** Pieces side by side, up to a closing bracket or the end: the implicit AND. */
  sequence(): Node | null {
    const items: Node[] = [];
    for (;;) {
      this.skipOperators();
      if (this.peek() === undefined || this.is("close")) break;
      const n = this.or();
      if (n) items.push(n);
    }
    return andOf(items);
  }

  or(): Node | null {
    const first = this.and();
    const items = first ? [first] : [];
    while (this.is("or")) {
      this.skipOperators();
      const n = this.and();
      if (n) items.push(n);
    }
    return orOf(items);
  }

  and(): Node | null {
    const first = this.atom();
    const items = first ? [first] : [];
    while (this.is("and")) {
      this.at++;
      // `a AND OR b`: the OR is the one meant.
      if (this.is("or")) break;
      const n = this.atom();
      if (n) items.push(n);
    }
    return andOf(items);
  }

  atom(): Node | null {
    const token = this.peek();
    if (token === undefined) return null;
    this.at++;
    switch (token.t) {
      case "word": {
        // Words run on into one phrase until something else comes.
        const words = [token.text];
        for (let next = this.peek(); next?.t === "word"; next = this.peek()) {
          words.push(next.text);
          this.at++;
        }
        const text = words.join(" ");
        const t = terms(text);
        // The phrase the query ends with is the one being typed.
        const prefix = this.at === this.tokens.length && lastIsPrefix(t);
        return t.length > 0 ? { kind: "phrase", text, strict: false, prefix } : null;
      }
      case "quoted":
        return terms(token.text).length > 0
          ? { kind: "phrase", text: token.text.trim(), strict: true, prefix: false }
          : null;
      case "notWord":
      case "notQuoted":
        if (terms(token.text).length === 0) return null;
        return {
          kind: "not",
          item:
            token.t === "notWord"
              ? { kind: "phrase", text: token.text, strict: false, prefix: false }
              : { kind: "phrase", text: token.text.trim(), strict: true, prefix: false },
        };
      case "filter":
        return { kind: "filter", key: token.key, value: token.value };
      case "open": {
        if (this.depth === MOST_BRACKETS)
          throw new QueryError(`More than ${MOST_BRACKETS} brackets inside each other: take some out.`);
        this.depth++;
        const inside = this.sequence();
        this.depth--;
        if (!this.is("close")) throw new QueryError("A bracket isn't closed: add a ).");
        this.at++;
        return inside;
      }
      case "close":
        throw new Error("sequence stops at a closing bracket");
      case "or":
      case "and":
        return null;
    }
  }
}

/** Items with any of the same kind inside merged into them; null for none. */
function groupOf(kind: "and" | "or", items: Node[]): Node | null {
  const out = items.flatMap((n) => (n.kind === kind ? n.items : [n]));
  if (out.length === 0) return null;
  return out.length === 1 ? out[0] : { kind, items: out };
}
const andOf = (items: Node[]) => groupOf("and", items);
const orOf = (items: Node[]) => groupOf("or", items);

/** The query typed into the search box, parsed; throws a `QueryError` saying what to fix. */
export function parse(text: string): Query {
  const parser = new Parser(tokens(text));
  const tree = parser.sequence();
  if (parser.is("close")) throw new QueryError("A bracket is closed that wasn't opened: take out a ).");
  const query: Query = { tree, within: null };
  // Each filter's value checked, and the window read: the last `within:` typed.
  let error: string | null = null;
  const visit = (node: Node) => {
    if (node.kind === "filter" && node.value !== "") {
      if (node.key === "within") {
        const s = seconds(node.value);
        if (s === null || !(s > 0)) error = "within: takes a length of time, such as within:20s, 2m or 1h.";
        else if (s > MOST_WITHIN) error = TOO_WIDE;
        else query.within = s;
      } else {
        try {
          filter(node.key, node.value);
        } catch (e) {
          if (!(e instanceof QueryError)) throw e;
          error = e.message;
        }
      }
    } else if (node.kind === "or" || node.kind === "and") node.items.forEach(visit);
    else if (node.kind === "not") visit(node.item);
  };
  if (tree) visit(tree);
  if (error !== null) throw new QueryError(error);
  return query;
}

/** Back to text that parses to the same tree (query.rs `Display for Node`). */
export function show(node: Node): string {
  const inner = (n: Node) => (n.kind === "or" || n.kind === "and" ? `(${show(n)})` : show(n));
  const loose = (n: Node) => n.kind === "phrase" && !n.strict;
  switch (node.kind) {
    case "or":
      return node.items.map(inner).join(" OR ");
    case "and":
      return node.items
        .map((item, i) => (i === 0 ? "" : loose(node.items[i - 1]) && loose(item) ? " AND " : " ") + inner(item))
        .join("");
    case "phrase":
      return node.strict ? `"${node.text}"` : node.text;
    case "not":
      return `-${show(node.item)}`;
    case "filter":
      return /\s/u.test(node.value) ? `${node.key}:"${node.value}"` : `${node.key}:${node.value}`;
  }
}
