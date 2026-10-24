// The demo's search, ported from the app's (crates/core search.rs, with
// transcript.rs `find_phrase` and db.rs `highlight`) over the demo's
// transcripts and chat, and its query parsed by query.ts (a port of
// query.rs). search.test.mjs checks the phrase search against the Rust's
// cases (crates/core/tests/phrase-search.json) and `searchLibrary` against
// search.rs's own tests; a change to search.rs's semantics is made here too.
// Imports only query.ts, so Node can test it directly.

import {
  conjunctions,
  DEFAULT_WITHIN,
  filtered,
  lastIsPrefix,
  MOST_WITHIN,
  noInfo,
  parse,
  QueryError,
  queryInfo,
  terms,
  TOO_WIDE,
  type Conjunction,
  type Piece,
  type Query,
  type QueryInfo,
} from "./query.ts";

export { lastIsPrefix, terms };

/** A run of a search hit's text, marked if it's part of the match. */
export interface Part {
  text: string;
  hit: boolean;
}

/** Inclusive word-index ranges where `query` occurs; its last term matches as a prefix if `lastIsPrefix`. */
export function findPhrase(words: { terms: string[] }[], query: string[], lastIsPrefix = true): [number, number][] {
  const flat = words.flatMap((w, i) => w.terms.map((t) => [t, i] as const));
  const hits: [number, number][] = [];
  const n = query.length;
  for (let i = 0; i + n <= flat.length;) {
    const matched = query.every((q, j) =>
      j === n - 1 && lastIsPrefix ? flat[i + j][0].startsWith(q) : flat[i + j][0] === q,
    );
    if (matched) {
      hits.push([flat[i][1], flat[i + n - 1][1]]);
      i += n;
    } else i += 1;
  }
  return hits;
}

/** Text runs with the matched ranges marked; spaces stay out of the marks, as in the app. */
export function highlight(words: { text: string }[], ranges: [number, number][]): Part[] {
  const parts: Part[] = [];
  words.forEach((w, i) => {
    const hit = ranges.some(([a, b]) => i >= a && i <= b);
    const sep = i === 0 ? "" : " ";
    const last = parts.at(-1);
    if (last && last.hit === hit) last.text += sep + w.text;
    else if (last && hit) {
      last.text += sep;
      parts.push({ text: w.text, hit });
    } else parts.push({ text: sep + w.text, hit });
  });
  return parts;
}

// The search over the library (crates/core search.rs `Db::search`): each
// alternative of the query run against what was said and what chat wrote.
// What was said is one stream of timed words per VOD (its lines are only how
// it's shown); a match is one occurrence of every piece, each phrase
// contiguous in the stream, the first matched word's start to the last one's
// end within the window. Candidates are found through the pieces' rarest
// word, as the app does, so the same matches come out. A chat message is a
// unit of its own: all the pieces in one message, no window. What the app
// does only to be fast (the index's shortcuts, stopping early) isn't here:
// the demo's library is two VODs.

/** How many names `from:` suggests at most. */
const MOST_SUGGESTIONS = 8;

/** How many occurrences `tightest` tries at most for one first word. */
const MOST_TRIES = 10_000;

export interface SpeechWord {
  text: string;
  start: number;
  end: number;
  terms: string[];
}

export interface SpeechLine {
  id: number;
  start: number;
  end: number;
  words: SpeechWord[];
}

export interface ChatMessage {
  line: { id: number; time: number; author: string; color: string | null };
  /** Its text split on spaces, emotes by name. */
  words: { text: string; terms: string[] }[];
}

/** A VOD as the search reads it; the library's VODs come in its order. */
export interface LibraryVod {
  id: number;
  channel: string | null;
  lines: SpeechLine[];
  chat: ChatMessage[];
}

/** What a search looks through (desktop api.ts `SearchFilter`). Fields left out don't filter. */
export interface Filter {
  vodId?: number | null;
  speech?: boolean;
  chat?: boolean;
  within?: number | null;
  from?: string | null;
}

export interface Hit {
  vod_id: number;
  source: "speech" | "chat";
  id: number;
  time: number;
  end: number;
  lines: number;
  parts: Part[];
  author: string | null;
  color: string | null;
}

export interface Results {
  hits: Hit[];
  truncated: boolean;
  query: QueryInfo;
}

export interface AuthorSuggestion {
  name: string;
  streamer: boolean;
  messages: number;
}

/** A hit before it's shown: the lines it covers and which of their words are marked. */
interface Found {
  position: number;
  vodId: number;
  source: "speech" | "chat";
  time: number;
  end: number;
  lines: { id: number; words: { text: string }[] }[];
  /** `line:word`, the line counted from the hit's first. */
  marked: Set<string>;
  author: string | null;
  color: string | null;
}

const foundKey = (f: Found) => `${f.source} ${f.vodId} ${f.lines[0].id} ${f.lines[f.lines.length - 1].id}`;

function intoHit(f: Found): Hit {
  const words: { text: string }[] = [];
  const ranges: [number, number][] = [];
  f.lines.forEach((line, l) =>
    line.words.forEach((w, i) => {
      if (f.marked.has(`${l}:${i}`)) ranges.push([words.length, words.length]);
      words.push(w);
    }),
  );
  return {
    vod_id: f.vodId,
    source: f.source,
    id: f.lines[0].id,
    time: f.time,
    end: f.end,
    lines: f.lines.length,
    parts: highlight(words, ranges),
    author: f.author,
    color: f.color,
  };
}

/** An occurrence of a piece: indices [from, to) into a window's places. */
type Range = [number, number];

/** A match in what was said, and its words as `line:word`. */
interface Candidate {
  start: number;
  words: string[];
  found: Found;
}

/** A VOD's speech as one stream of terms (a word can have several: don't). */
class Stream {
  readonly term: string[] = [];
  readonly line: number[] = [];
  readonly word: number[] = [];
  readonly start: number[] = [];
  readonly end: number[] = [];
  /** The longest word, so a window's first place can be looked up by start. */
  readonly longest: number = 0;
  readonly vod: LibraryVod;
  readonly position: number;

  constructor(vod: LibraryVod, position: number) {
    this.vod = vod;
    this.position = position;
    let longest = 0;
    vod.lines.forEach((line, l) =>
      line.words.forEach((w, i) => {
        longest = Math.max(longest, w.end - w.start);
        for (const t of w.terms) {
          this.term.push(t);
          this.line.push(l);
          this.word.push(i);
          this.start.push(w.start);
          this.end.push(w.end);
        }
      }),
    );
    this.longest = longest;
  }

  /** The first place starting at or after `time` (the words are in time order). */
  private from(time: number): number {
    let lo = 0;
    let hi = this.start.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.start[mid] < time) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** The places whose words overlap `lo..hi`, in order. */
  places(lo: number, hi: number): number[] {
    const out: number[] = [];
    for (let i = this.from(lo - this.longest); i < this.start.length && this.start[i] <= hi; i++)
      if (this.end[i] >= lo) out.push(i);
    return out;
  }

  /** Each occurrence of `piece` in `places`. */
  occurrences(places: number[], piece: Piece): Range[] {
    const n = piece.terms.length;
    const out: Range[] = [];
    for (let i = 0; i + n <= places.length; i++) {
      const found = piece.terms.every((q, j) => {
        const t = this.term[places[i + j]];
        return piece.prefix && j === n - 1 ? t.startsWith(q) : t === q;
      });
      if (found) out.push([i, i + n]);
    }
    return out;
  }

  span(places: number[], [a, b]: Range): [number, number] {
    return [this.start[places[a]], this.end[places[b - 1]]];
  }

  /** Whether any of `not` is in the window centred on `start..end`. */
  excluded(not: Piece[], start: number, end: number, within: number): boolean {
    const centre = (start + end) / 2;
    const places = this.places(centre - within / 2, centre + within / 2);
    return not.some((p) => this.occurrences(places, p).length > 0);
  }

  /** The tightest match through anchor place `a`, if any: every piece once,
   *  the span within the window, nothing excluded near it. */
  match(a: number, c: Conjunction, within: number): Candidate | null {
    const places = this.places(this.end[a] - within, this.start[a] + within);
    const at = places.indexOf(a);
    if (at < 0) return null;
    const found = c.pieces.map((p) => this.occurrences(places, p));
    let best: { span: number; set: Range[] } | null = null;
    // Each piece's occurrence through the anchor word, with the others around it.
    for (const [k, occurrences] of found.entries())
      for (const fixed of occurrences.filter(([x, y]) => x <= at && at < y)) {
        const set = this.tightest(places, found, k, fixed, within);
        if (set && (!best || set.span < best.span)) best = set;
      }
    if (!best) return null;
    const { set } = best;
    const spans = set.map((r) => this.span(places, r));
    const start = Math.min(...spans.map((s) => s[0]));
    const end = Math.max(...spans.map((s) => s[1]));
    if (c.not.length > 0 && this.excluded(c.not, start, end, within)) return null;
    return this.candidate(places, set, start, end);
  }

  /** The occurrences, one per piece, with piece `k`'s `fixed`, spanning the
   *  least time; null if none fits in the window (search.rs `tightest`). */
  private tightest(
    places: number[],
    found: Range[][],
    k: number,
    fixed: Range,
    within: number,
  ): { span: number; set: Range[] } | null {
    const [fixedStart, fixedEnd] = this.span(places, fixed);
    const firsts = found
      .flat()
      .map((r) => this.span(places, r)[0])
      .filter((s) => s < fixedStart && s >= fixedEnd - within);
    firsts.push(fixedStart);
    // Each piece's occurrences with their times, soonest-ending first.
    const byEnd = found.map((occurrences) =>
      occurrences
        .map((range) => {
          const [start, end] = this.span(places, range);
          return { start, end, range };
        })
        .sort((a, b) => a.end - b.end),
    );
    let best: { span: number; set: Range[] } | null = null;
    for (const first of firsts) {
      const chosen: (Range | null)[] = found.map(() => null);
      chosen[k] = fixed;
      const soonest: { end: number; set: Range[] | null } = { end: Infinity, set: null };
      const budget = { left: MOST_TRIES };
      // Times are seconds as floats: a hair over the window is in it.
      const window = { start: first, end: first + within + 1e-9 };
      const assign = (j: number, end: number) => {
        if (j === byEnd.length) {
          if (soonest.set === null || end < soonest.end) {
            soonest.end = end;
            soonest.set = chosen.map((r) => r!);
          }
          return;
        }
        if (chosen[j] !== null) return assign(j + 1, end);
        for (const t of byEnd[j]) {
          if (t.start < window.start) continue;
          const e = Math.max(end, t.end);
          // Sorted by their ends: none after this one ends any sooner, nor fits.
          if (t.end > window.end || (soonest.set !== null && e >= soonest.end) || budget.left === 0) break;
          budget.left--;
          if (chosen.some((r) => r !== null && this.share(places, r, t.range))) continue;
          chosen[j] = t.range;
          assign(j + 1, e);
          chosen[j] = null;
        }
      };
      assign(0, fixedEnd);
      if (soonest.set !== null) {
        const span = soonest.end - first;
        if (!best || span < best.span) best = { span, set: soonest.set };
      }
    }
    return best;
  }

  /** Whether occurrences `a` and `b` have a word in common. */
  private share(places: number[], a: Range, b: Range): boolean {
    for (let i = a[0]; i < a[1]; i++)
      for (let j = b[0]; j < b[1]; j++) {
        const [p, q] = [places[i], places[j]];
        if (this.line[p] === this.line[q] && this.word[p] === this.word[q]) return true;
      }
    return false;
  }

  /** The hit for a set of occurrences: the lines from its first word's to its last's, each piece's words marked. */
  private candidate(places: number[], set: Range[], start: number, end: number): Candidate {
    const matched = set.flatMap(([a, b]) => places.slice(a, b));
    const first = Math.min(...matched.map((p) => this.line[p]));
    const last = Math.max(...matched.map((p) => this.line[p]));
    const lines = this.vod.lines.slice(first, last + 1);
    return {
      start,
      words: matched.map((p) => `${this.line[p]}:${this.word[p]}`),
      found: {
        position: this.position,
        vodId: this.vod.id,
        source: "speech",
        time: start,
        end,
        lines,
        marked: new Set(matched.map((p) => `${this.line[p] - first}:${this.word[p]}`)),
        author: null,
        color: null,
      },
    };
  }
}

/** Kept from `pending`: the tightest first (then the earliest), each match
 *  none of whose words is in one kept already; a match showing the same
 *  lines as one kept is marked in it (search.rs `keep`). */
function keep(pending: Candidate[], out: Found[]) {
  const span = (c: Candidate) => c.found.end - c.found.time;
  pending.sort((a, b) => span(a) - span(b) || a.found.time - b.found.time);
  const words = new Set<string>();
  const hits = new Map<string, Found>();
  for (const c of pending) {
    if (c.words.some((w) => words.has(w))) continue;
    c.words.forEach((w) => words.add(w));
    const key = foundKey(c.found);
    const hit = hits.get(key);
    if (hit) {
      hit.time = Math.min(hit.time, c.found.time);
      hit.end = Math.max(hit.end, c.found.end);
      c.found.marked.forEach((m) => hit.marked.add(m));
    } else {
      hits.set(key, c.found);
      out.push(c.found);
    }
  }
}

/** A line's first word's start to its last word's end. */
function lineSpan(line: SpeechLine): [number, number] {
  return line.words.length > 0 ? [line.words[0].start, line.words[line.words.length - 1].end] : [line.start, line.end];
}

/** The library as a search reads it, with what's worked out once per library. */
class Library {
  readonly streams: Stream[];
  /** For each term, how many lines have it, as the app's index counts them. */
  readonly lineCounts = new Map<string, number>();
  readonly vods: LibraryVod[];

  constructor(vods: LibraryVod[]) {
    this.vods = vods;
    this.streams = vods.map((v, i) => new Stream(v, i));
    for (const v of vods)
      for (const line of v.lines)
        for (const t of new Set(line.words.flatMap((w) => w.terms)))
          this.lineCounts.set(t, (this.lineCounts.get(t) ?? 0) + 1);
  }

  /** The VODs `vodId` and `from` (channels, any case) allow, as streams. */
  vodsOf(vodId: number | null, from: string[]): Stream[] {
    const names = from.map((n) => n.toLowerCase());
    return this.streams.filter(
      (s) =>
        (vodId === null || s.vod.id === vodId) &&
        (names.length === 0 || (s.vod.channel !== null && names.includes(s.vod.channel.toLowerCase()))),
    );
  }

  /** The word to look up first: the pieces' rarest whole word (the one being
   *  typed only if it's the only word); null if a whole word is in no line. */
  anchor(pieces: Piece[]): { term: string; prefix: boolean } | null {
    const words = pieces.flatMap((p) => p.terms.slice(0, p.terms.length - (p.prefix ? 1 : 0)));
    if (words.length === 0) return { term: pieces[0].terms[pieces[0].terms.length - 1], prefix: true };
    let best: { lines: number; term: string } | null = null;
    for (const term of words) {
      const lines = this.lineCounts.get(term) ?? 0;
      if (lines === 0) return null;
      if (!best || lines < best.lines) best = { lines, term };
    }
    return { term: best!.term, prefix: false };
  }

  speech(c: Conjunction, vodId: number | null, within: number, limit: number): Found[] {
    const streams = this.vodsOf(vodId, c.from);
    if (c.pieces.length === 0) return this.listSpeech(c, streams, within, limit);
    const anchor = this.anchor(c.pieces);
    if (!anchor) return [];
    const [after, before] = [c.after ?? -Infinity, c.before ?? Infinity];
    const out: Found[] = [];
    for (const stream of streams) {
      const pending: Candidate[] = [];
      stream.term.forEach((t, a) => {
        if (anchor.prefix ? !t.startsWith(anchor.term) : t !== anchor.term) return;
        const candidate = stream.match(a, c, within);
        if (candidate && candidate.start >= after && candidate.start < before) pending.push(candidate);
      });
      keep(pending, out);
      if (out.length > limit) break;
    }
    return out;
  }

  /** Speech with no pieces to find (`from:x`, `after:1h -word`): each line passing the filters. */
  private listSpeech(c: Conjunction, streams: Stream[], within: number, limit: number): Found[] {
    const [after, before] = [c.after ?? -Infinity, c.before ?? Infinity];
    const out: Found[] = [];
    for (const stream of streams)
      for (const line of stream.vod.lines) {
        if (!(line.start >= after && line.start < before)) continue;
        const [time, end] = lineSpan(line);
        if (c.not.length > 0 && stream.excluded(c.not, time, end, within)) continue;
        out.push({
          position: stream.position,
          vodId: stream.vod.id,
          source: "speech",
          time,
          end,
          lines: [line],
          marked: new Set(),
          author: null,
          color: null,
        });
        if (out.length > limit) return out;
      }
    return out;
  }

  /** What chat wrote that matches `c`: every piece in one message, none of
   *  the exclusions; `from:` by the message's author, in any case. */
  chat(c: Conjunction, vodId: number | null, limit: number): Found[] {
    const [after, before] = [c.after ?? -Infinity, c.before ?? Infinity];
    const names = c.from.map((n) => n.toLowerCase());
    const out: Found[] = [];
    for (const stream of this.vodsOf(vodId, [])) {
      for (const { line, words } of stream.vod.chat) {
        if (!(line.time >= after && line.time < before)) continue;
        if (names.length > 0 && !names.includes(line.author.toLowerCase())) continue;
        const marked = new Set<string>();
        const all = c.pieces.every((p) => {
          const ranges = findPhrase(words, p.terms, p.prefix);
          for (const [a, b] of ranges) for (let w = a; w <= b; w++) marked.add(`0:${w}`);
          return ranges.length > 0;
        });
        if (!all || c.not.some((p) => findPhrase(words, p.terms, false).length > 0)) continue;
        out.push({
          position: stream.position,
          vodId: stream.vod.id,
          source: "chat",
          time: line.time,
          end: line.time,
          lines: [{ id: line.id, words }],
          marked,
          author: line.author,
          color: line.color,
        });
        if (out.length > limit) return out;
      }
    }
    return out;
  }
}

const libraries = new WeakMap<LibraryVod[], Library>();

function libraryOf(vods: LibraryVod[]): Library {
  let lib = libraries.get(vods);
  if (!lib) {
    lib = new Library(vods);
    libraries.set(vods, lib);
  }
  return lib;
}

/** Searches `vods` (in library order) for `query` as the app does
 *  (search.rs `Db::search`): the filter's VOD, sources, window and who are
 *  defaults the typed text overrides. At most `limit` hits, by VOD, then time. */
export function searchLibrary(vods: LibraryVod[], query: string, filter: Filter, limit: number): Results {
  const none = (info: QueryInfo): Results => ({ hits: [], truncated: false, query: info });
  if (filter.within != null && filter.within > MOST_WITHIN) return none(noInfo(TOO_WIDE));
  let parsed: Query;
  try {
    parsed = parse(query);
  } catch (e) {
    if (e instanceof QueryError) return none(noInfo(e.message));
    throw e;
  }
  const info = queryInfo(parsed);
  let alternatives: Conjunction[];
  try {
    alternatives = conjunctions(parsed);
  } catch (e) {
    if (e instanceof QueryError) return none({ ...info, error: e.message });
    throw e;
  }
  const within = parsed.within ?? (filter.within != null && filter.within > 0 ? filter.within : null) ?? DEFAULT_WITHIN;
  const from = filter.from?.trim() || null;
  const lib = libraryOf(vods);
  const vodId = filter.vodId ?? null;
  const found: Found[] = [];
  let ran = false;
  for (const typed of alternatives) {
    // The filter's who, for an alternative that doesn't name its own.
    const c = from && typed.from.length === 0 ? { ...typed, from: [from] } : typed;
    // Exclusions alone have nothing to leave things out of.
    if (c.pieces.length === 0 && !filtered(c)) continue;
    ran = true;
    const speech = c.sources ? c.sources.audio : filter.speech !== false;
    const chat = c.sources ? c.sources.chat : filter.chat !== false;
    if (speech) found.push(...lib.speech(c, vodId, within, limit));
    if (chat) found.push(...lib.chat(c, vodId, limit));
  }
  if (!ran && alternatives.some((c) => c.not.length > 0))
    info.error = "Add a word or a filter: a minus only leaves things out.";
  // One hit per stretch of lines, across alternatives too.
  const merged: Found[] = [];
  const at = new Map<string, Found>();
  for (const f of found) {
    const m = at.get(foundKey(f));
    if (m) {
      m.time = Math.min(m.time, f.time);
      m.end = Math.max(m.end, f.end);
      f.marked.forEach((x) => m.marked.add(x));
    } else {
      at.set(foundKey(f), f);
      merged.push(f);
    }
  }
  merged.sort((a, b) => a.position - b.position || b.vodId - a.vodId || a.time - b.time);
  return { hits: merged.slice(0, limit).map(intoHit), truncated: merged.length > limit, query: info };
}

/** Names `from:` could mean, starting with `prefix` in any case: the
 *  channels first, then chatters with the most messages (in VOD `vodId`, if
 *  given), at most `MOST_SUGGESTIONS` (search.rs `author_suggestions`). */
export function authorSuggestions(vods: LibraryVod[], prefix: string, vodId: number | null): AuthorSuggestion[] {
  const want = prefix.trim().toLowerCase();
  const fits = (name: string) => name.toLowerCase().startsWith(want);
  const mine = vods.filter((v) => vodId === null || v.id === vodId);
  const out: AuthorSuggestion[] = [];
  const index = (name: string) => out.findIndex((s) => s.name.toLowerCase() === name.toLowerCase());
  const channels = [...new Set(mine.flatMap((v) => (v.channel !== null && fits(v.channel) ? [v.channel] : [])))];
  for (const name of channels.sort((a, b) => cmp(a.toLowerCase(), b.toLowerCase())).slice(0, MOST_SUGGESTIONS))
    if (index(name) < 0) out.push({ name, streamer: true, messages: 0 });
  const counts = new Map<string, { name: string; messages: number }>();
  for (const v of mine)
    for (const { line } of v.chat)
      if (fits(line.author)) {
        const key = line.author.toLowerCase();
        const seen = counts.get(key) ?? { name: line.author, messages: 0 };
        seen.messages++;
        counts.set(key, seen);
      }
  const chatters = [...counts.values()]
    .sort((a, b) => b.messages - a.messages || cmp(a.name.toLowerCase(), b.name.toLowerCase()))
    .slice(0, MOST_SUGGESTIONS);
  for (const { name, messages } of chatters) {
    const i = index(name);
    if (i >= 0) out[i].messages = messages;
    else if (out.length < MOST_SUGGESTIONS) out.push({ name, streamer: false, messages });
  }
  return out;
}

/** Strings compared as SQLite compares them: by code unit, not by locale. */
const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
