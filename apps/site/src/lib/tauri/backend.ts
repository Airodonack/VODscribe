// The demo's backend: the app's commands answered from the transcripts in
// static/demo (made by scripts/export-demo.py), with the same phrase search as
// the app (crates/core: db.rs `search`, transcript.rs `terms` and `find_phrase`).

import type { Line, SearchHit, Vod } from "$desktop/api";

/** A segment as exported: its text, start and each word's start (words are the text split on spaces). */
type Segment = [text: string, start: number, wordStarts: number[]];

interface Word {
  text: string;
  start: number;
  terms: string[];
}

interface Indexed {
  id: number;
  start: number;
  text: string;
  words: Word[];
}

let vods: Promise<Vod[]> | undefined;
const transcripts = new Map<number, Promise<Indexed[]>>();

async function json<T>(path: string): Promise<T> {
  const res = await fetch(path);
  if (!res.ok) throw new Error(`The demo couldn't load ${path}.`);
  return res.json();
}

export function listVods(): Promise<Vod[]> {
  vods ??= json<Vod[]>("/demo/vods.json");
  return vods;
}

/** Library order, changed by dragging; kept for this visit only. */
export async function reorder(ids: number[]) {
  const all = await listVods();
  vods = Promise.resolve(ids.map((id) => all.find((v) => v.id === id)).filter((v) => v !== undefined));
}

function segments(vodId: number): Promise<Indexed[]> {
  let t = transcripts.get(vodId);
  if (!t) {
    t = json<Segment[]>(`/demo/${vodId}.json`).then((segs) =>
      segs.map(([text, start, starts], i) => ({
        // Segment ids only need to be unique and stable within a visit.
        id: vodId * 100_000 + i,
        start,
        text,
        words: text.split(" ").map((w, j) => ({ text: w, start: starts[j] ?? start, terms: terms(w) })),
      })),
    );
    transcripts.set(vodId, t);
  }
  return t;
}

export async function transcript(vodId: number): Promise<Line[]> {
  return (await segments(vodId)).map(({ start, text }) => ({ start, text }));
}

/** Lowercased alphanumeric runs with accents removed, like the app's `terms`. */
function terms(text: string): string[] {
  return text
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

/** Inclusive word-index ranges where `query` occurs; its last term matches as a prefix. */
function findPhrase(words: Word[], query: string[]): [number, number][] {
  const flat = words.flatMap((w, i) => w.terms.map((t) => [t, i] as const));
  const hits: [number, number][] = [];
  const n = query.length;
  for (let i = 0; i + n <= flat.length;) {
    const matched = query.every((q, j) => (j === n - 1 ? flat[i + j][0].startsWith(q) : flat[i + j][0] === q));
    if (matched) {
      hits.push([flat[i][1], flat[i + n - 1][1]]);
      i += n;
    } else i += 1;
  }
  return hits;
}

/** Text runs with the matched ranges marked; spaces stay out of the marks, as in the app. */
function highlight(words: Word[], ranges: [number, number][]): SearchHit["parts"] {
  const parts: SearchHit["parts"] = [];
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

const LIMIT = 500;

export async function search(query: string, vodId: number | null) {
  const q = terms(query);
  const hits: SearchHit[] = [];
  if (q.length === 0) return { hits, truncated: false };
  for (const vod of await listVods()) {
    if (vodId !== null && vod.id !== vodId) continue;
    for (const seg of await segments(vod.id)) {
      const ranges = findPhrase(seg.words, q);
      if (ranges.length === 0) continue;
      hits.push({
        vod_id: vod.id,
        segment_id: seg.id,
        time: seg.words[ranges[0][0]].start,
        parts: highlight(seg.words, ranges),
      });
      if (hits.length > LIMIT) return { hits: hits.slice(0, LIMIT), truncated: true };
    }
  }
  return { hits, truncated: false };
}
