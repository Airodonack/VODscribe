// Captions' line breaking: node --test apps/desktop/src/lib/textWrap.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { wrapWith } from "./textWrap.ts";

// Ten pixels a character.
const width = (/** @type {string} */ s) => s.length * 10;

// Source: textWrap.ts `wrapWith`.
test("words fill lines up to the width", () => {
  assert.deepEqual(wrapWith("one two three four", 90, width), ["one two", "three", "four"]);
  assert.deepEqual(wrapWith("  ", 90, width), []);
});

// Source: textWrap.ts `wrapWith` ("rather than run over").
test("a word wider than a line is broken, not run over", () => {
  const lines = wrapWith("ok AAAAAAAAAAAAAAAAAAAAAAAAA then", 100, width);
  assert.deepEqual(lines, ["ok", "AAAAAAAAAA", "AAAAAAAAAA", "AAAAA then"]);
  for (const line of lines) assert.ok(width(line) <= 100, line);
});

// Source: textWrap.ts `wrapWith` ("By characters, not UTF-16 units").
test("a broken word keeps each emoji whole", () => {
  // An emoji twice as wide as a letter; half of one (a lone surrogate) as one letter.
  const wide = (/** @type {string} */ s) => Array.from(s).reduce((w, c) => w + (c.length === 2 ? 20 : 10), 0);
  const lines = wrapWith("😀😀😀", 30, wide);
  assert.deepEqual(lines, ["😀", "😀", "😀"]);
});

// Source for the tests below: textWrap.ts's `wrapWith` doc comment ("words
// onto lines no wider than `most` by `width`; a word wider than that alone is
// broken where it has to be, rather than run over").

test("no text, no lines", () => {
  assert.deepEqual(wrapWith("", 90, width), []);
});

test("a line exactly as wide as the width fits; one more character wraps", () => {
  assert.deepEqual(wrapWith("abcd efgh", 90, width), ["abcd efgh"]);
  assert.deepEqual(wrapWith("abcd efghi", 90, width), ["abcd", "efghi"]);
});

test("any run of spaces and line breaks between words is one space", () => {
  assert.deepEqual(wrapWith("a \n\t  b", 90, width), ["a b"]);
});

test("a letter wider than the width stands alone on a line, not dropped or looped on", () => {
  assert.deepEqual(wrapWith("abc", 5, width), ["a", "b", "c"]);
});

test("a word broken across lines is followed by the next word on the last piece's line when it fits", () => {
  assert.deepEqual(wrapWith("AAAAAAAAAAAA b", 100, width), ["AAAAAAAAAA", "AA b"]);
});

test("a wide width keeps every word", () => {
  assert.deepEqual(wrapWith("a b c", 1000, width), ["a b c"]);
  assert.deepEqual(wrapWith("one", 1000, width), ["one"]);
});

test("a word exactly as wide as the line isn't broken", () => {
  assert.deepEqual(wrapWith("AAAAAAAAAA", 100, width), ["AAAAAAAAAA"]);
});
