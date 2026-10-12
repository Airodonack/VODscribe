// The demo's backend: the app's commands answered from the transcripts in
// static/demo (made by scripts/export-demo.py), with the same phrase search as
// the app (crates/core: db.rs `search`, transcript.rs `terms` and `find_phrase`).

import type { Line, SearchHit, Vod } from "$desktop/api";
import { findPhrase, highlight, terms } from "./search";

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

export function listVods(): Promise<Vod[]> {
  return cached(library, "vods", () => json<Vod[]>("/demo/vods.json"));
}

/** Library order, changed by dragging; kept for this visit only. */
export async function reorder(ids: number[]) {
  const all = await listVods();
  library.set("vods", Promise.resolve(ids.map((id) => all.find((v) => v.id === id)).filter((v) => v !== undefined)));
}

function segments(vodId: number): Promise<Indexed[]> {
  return cached(transcripts, vodId, () =>
    json<Segment[]>(`/demo/${vodId}.json`).then((segs) =>
      segs.map(([text, start, starts], i) => ({
        // Segment ids only need to be unique and stable within a visit.
        id: vodId * 100_000 + i,
        start,
        text,
        words: text.split(" ").map((w, j) => ({ text: w, start: starts[j] ?? start, terms: terms(w) })),
      })),
    ),
  );
}

export async function transcript(vodId: number): Promise<Line[]> {
  return (await segments(vodId)).map(({ start, text }) => ({ start, text }));
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
