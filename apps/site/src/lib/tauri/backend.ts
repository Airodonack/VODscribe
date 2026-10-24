// The demo's backend: the app's commands answered from the transcripts in
// static/demo (made by scripts/export-demo.py), searched as the app searches
// (search.ts, ported from crates/core search.rs and query.rs).

import type {
  AuthorSuggestion,
  ChatLine,
  ChatOverview,
  ChatWindow,
  Line,
  SearchFilter,
  SearchResponse,
  Stretch,
  Vod,
} from "desktop/lib/api.ts";
import { authorSuggestions as suggest, searchLibrary, terms, type LibraryVod } from "./search";

/** A segment as exported: its text, start and each word's start (words are the text split on spaces). */
type Segment = [text: string, start: number, wordStarts: number[]];

interface Word {
  text: string;
  start: number;
  /** The demo's words have no ends: each ends where it starts. */
  end: number;
  terms: string[];
}

interface Indexed {
  id: number;
  start: number;
  end: number;
  text: string;
  words: Word[];
}

const library = new Map<"vods", Promise<Vod[]>>();
const transcripts = new Map<number, Promise<Indexed[]>>();

/** Remembers `promise` under `key` until it fails, so a failed load is tried again next time. */
function cached<K, T>(cache: Map<K, Promise<T>>, key: K, load: () => Promise<T>): Promise<T> {
  let p = cache.get(key);
  if (!p) {
    p = load();
    cache.set(key, p);
    p.catch(() => cache.delete(key));
  }
  return p;
}

async function json<T>(path: string): Promise<T> {
  const res = await fetch(path);
  if (!res.ok) throw new Error(`The demo couldn't load ${path}.`);
  return res.json();
}

// The demo's VODs are transcribed whole, as the export made them (it
// predates stretches, so their fields are filled in here).
export function listVods(): Promise<Vod[]> {
  return cached(library, "vods", async () =>
    (await json<Vod[]>("/demo/vods.json")).map((v) => ({
      ...v,
      status: "done",
      stretch: null,
      transcribed: "all",
      waiting: 0,
    })),
  );
}

/** A demo VOD's stretches: one, done, the whole of it. */
export function transcriptStretches(vodId: number): Stretch[] {
  return [
    {
      id: vodId,
      vod_id: vodId,
      start: 0,
      end: null,
      status: "done",
      progress: 1,
      reached: null,
      error: null,
      model: null,
    },
  ];
}

/** Library order, changed by dragging; kept for this visit only. */
export async function reorder(ids: number[]) {
  const all = await listVods();
  library.set("vods", Promise.resolve(ids.map((id) => all.find((v) => v.id === id)).filter((v) => v !== undefined)));
}

function segments(vodId: number): Promise<Indexed[]> {
  return cached(transcripts, vodId, () =>
    json<Segment[]>(`/demo/${vodId}.json`).then((segs) =>
      segs.map(([text, start, starts], i) => {
        const words = text.split(" ").map((w, j) => {
          const at = starts[j] ?? start;
          return { text: w, start: at, end: at, terms: terms(w) };
        });
        // Segment ids only need to be unique and stable within a visit.
        return { id: vodId * 100_000 + i, start, end: words.at(-1)?.start ?? start, text, words };
      }),
    ),
  );
}

/** A demo VOD's lines: all, or from `from` to before `to` (null: on to the end). */
export async function transcript(vodId: number, from?: number, to?: number | null): Promise<Line[]> {
  return (await segments(vodId))
    .filter(({ start }) => from === undefined || (start >= from && (to == null || start < to)))
    .map(({ start, text }) => ({ start, text }));
}

// Chat, for the demo VODs that have it (static/demo/<id>.chat.json, made by
// scripts/export-demo-chat.py with every chatter's name made up), answered as
// the app's chat commands are (desktop chats.rs, crates/core db.rs).

interface IndexedChat {
  line: ChatLine;
  /** Its text as words, emotes by name, as the app indexes it (chat.rs `ChatMessage::text`). */
  words: { text: string; terms: string[] }[];
}

const chats = new Map<number, Promise<IndexedChat[]>>();

function chat(vodId: number): Promise<IndexedChat[]> {
  return cached(chats, vodId, async () => {
    const res = await fetch(`/demo/${vodId}.chat.json`);
    const lines: ChatLine[] = res.ok ? await res.json() : [];
    return lines.map((line) => {
      const text = line.parts.map((p) => (p.type === "text" ? p.text : ` ${p.name} `)).join("");
      const words = text.split(/\s+/).filter(Boolean);
      return { line, words: words.map((w) => ({ text: w, terms: terms(w) })) };
    });
  });
}

/** The index of the first message at or after `time` (messages are in time order). */
function at(messages: IndexedChat[], time: number): number {
  let lo = 0;
  let hi = messages.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (messages[mid].line.time < time) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export async function chatOverview(vodId: number, duration: number, bins: number): Promise<ChatOverview> {
  const messages = await chat(vodId);
  const histogram = new Array<number>(bins).fill(0);
  if (duration > 0)
    for (const { line } of messages)
      histogram[Math.min(bins - 1, Math.max(0, Math.floor((line.time * bins) / duration)))]++;
  return {
    count: messages.length,
    histogram,
    events: messages.filter((m) => m.line.kind !== "message").map((m) => m.line),
  };
}

export async function chatWindows(vodId: number, ranges: [number, number][], cap: number): Promise<ChatWindow[]> {
  const messages = await chat(vodId);
  return ranges.map(([from, to]) => {
    const [i, j] = [at(messages, from), at(messages, to)];
    return { messages: messages.slice(i, Math.min(j, i + cap)).map((m) => m.line), total: j - i };
  });
}

export async function chatEmotes(vodId: number): Promise<[string, string][]> {
  const seen = new Map<string, string>();
  for (const { line } of await chat(vodId)) for (const p of line.parts) if (p.type === "emote") seen.set(p.name, p.url);
  return [...seen];
}

/** For each list of terms, the [id, time] of messages with any of them as whole words, in order. */
export async function chatMatches(vodId: number, lists: string[][]): Promise<[number, number][][]> {
  const messages = await chat(vodId);
  return lists.map((list) => {
    const phrases = list.map(terms).filter((t) => t.length > 0);
    return messages
      .filter(({ words }) => {
        const flat = words.flatMap((w) => w.terms);
        return phrases.some((q) => flat.some((_, i) => q.every((t, j) => flat[i + j] === t)));
      })
      .map(({ line }) => [line.id, line.time]);
  });
}

/** As many hits as the app shows (desktop vods.rs `search`). */
const LIMIT = 500;

const searchables = new Map<string, Promise<LibraryVod[]>>();

/** The library as the search reads it, in its order: every VOD's speech and chat. */
async function searchable(): Promise<LibraryVod[]> {
  const vods = await listVods();
  return cached(searchables, vods.map((v) => v.id).join(" "), () =>
    Promise.all(
      vods.map(async (v) => ({ id: v.id, channel: v.channel, lines: await segments(v.id), chat: await chat(v.id) })),
    ),
  );
}

export async function search(query: string, filter: SearchFilter): Promise<SearchResponse> {
  return searchLibrary(await searchable(), query, filter, LIMIT);
}

export async function authorSuggestions(prefix: string, vodId: number | null): Promise<AuthorSuggestion[]> {
  return suggest(await searchable(), prefix, vodId);
}
