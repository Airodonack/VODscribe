// Captions' line breaking: node --test apps/desktop/src/lib/textWrap.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { wrapWith } from "./textWrap.ts";

// Ten pixels a character.
const width = (/** @type {string} */ s) => s.length * 10;

test("words fill lines up to the width", () => {
  assert.deepEqual(wrapWith("one two three four", 90, width), ["one two", "three", "four"]);
  assert.deepEqual(wrapWith("  ", 90, width), []);
});

test("a word wider than a line is broken, not run over", () => {
  const lines = wrapWith("ok AAAAAAAAAAAAAAAAAAAAAAAAA then", 100, width);
  assert.deepEqual(lines, ["ok", "AAAAAAAAAA", "AAAAAAAAAA", "AAAAA then"]);
  for (const line of lines) assert.ok(width(line) <= 100, line);
});
