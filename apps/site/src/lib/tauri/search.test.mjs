// The demo's search and its highlighting against the same cases as the app's Rust search
// (crates/core/tests/phrase-search.json): `node --test apps/site/src/lib/tauri/search.test.mjs`.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { findPhrase, highlight, lastIsPrefix, terms } from "./search.ts";

/** @type {{ about: string, words: string[], query: string, ranges: [number, number][], marked: string }[]} */
const cases = JSON.parse(
  readFileSync(new URL("../../../../../crates/core/tests/phrase-search.json", import.meta.url), "utf8"),
);

for (const c of cases) {
  test(c.about, () => {
    const words = c.words.map((text) => ({ text, terms: terms(text) }));
    const q = terms(c.query);
    assert.deepEqual(findPhrase(words, q, lastIsPrefix(q)), c.ranges);
    // How the result shows it: matched runs in [brackets].
    const marked = highlight(words, c.ranges)
      .map((p) => (p.hit ? `[${p.text}]` : p.text))
      .join("");
    assert.equal(marked, c.marked);
  });
}
