// The demo's phrase search, ported from the app's (crates/core transcript.rs
// `terms` and `find_phrase`, db.rs `highlight`). No imports, so Node can test
// it directly: search.test.mjs checks it against the same cases as the Rust
// (crates/core/tests/phrase-search.json).

/** A run of a search hit's text, marked if it's part of the match. */
export interface Part {
  text: string;
  hit: boolean;
}

/** Lowercased alphanumeric runs with accents removed, like the app's `terms`. */
export function terms(text: string): string[] {
  return text
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

/** Inclusive word-index ranges where `query` occurs; its last term matches as a prefix. */
export function findPhrase(words: { terms: string[] }[], query: string[]): [number, number][] {
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
