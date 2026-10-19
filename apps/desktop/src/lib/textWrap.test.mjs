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

test("a broken word keeps each emoji whole", () => {
  // An emoji twice as wide as a letter; half of one (a lone surrogate) as one letter.
  const wide = (/** @type {string} */ s) => Array.from(s).reduce((w, c) => w + (c.length === 2 ? 20 : 10), 0);
  const lines = wrapWith("😀😀😀", 30, wide);
  assert.deepEqual(lines, ["😀", "😀", "😀"]);
});
