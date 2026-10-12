// The demo's search against the same cases as the app's Rust search
// (crates/core/tests/phrase-search.json): `node --test apps/site/src/lib/tauri/search.test.mjs`.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { findPhrase, terms } from "./search.ts";

const cases = JSON.parse(readFileSync(new URL("../../../../../crates/core/tests/phrase-search.json", import.meta.url)));

for (const c of cases) {
  test(c.about, () => {
    const words = c.words.map((text) => ({ text, terms: terms(text) }));
    assert.deepEqual(findPhrase(words, terms(c.query)), c.ranges);
  });
}
