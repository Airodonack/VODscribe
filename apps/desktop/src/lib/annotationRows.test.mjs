// The annotations pane's rows (annotationRows.ts): node --test apps/desktop/src/lib/annotationRows.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { chapterRows, currentChapter, highlightRows, lengthLabel, stepRow } from "./annotationRows.ts";

/** @type {(id: number, kind: import("./api").MarkKind, start: number, extra?: object) => import("./api").Mark} */
const mark = (id, kind, start, extra = {}) => ({ id, vod_id: 1, kind, start, text: null, ...extra });

const marks = [
  mark(1, "chapter", 120, { text: "The door" }),
  mark(2, "highlight", 50, { text: "Later", end: 60, category: "funny", note: " Why. " }),
  mark(3, "cut", 10),
  mark(4, "chapter", 0, { text: "The start" }),
  mark(5, "highlight", 7, { text: "Long", end: 30 }),
  mark(6, "highlight", 7, { text: "Short", end: 14, category: "hype" }),
  mark(7, "chapter", 300, { text: "  " }),
];

test("chapters come in time order, each to the next one's start, the last to the VOD's end", () => {
  const rows = chapterRows(marks, 400);
  assert.deepEqual(
    rows.map((r) => [r.id, r.start, r.end, r.title]),
    [
      [4, 0, 120, "The start"],
      [1, 120, 300, "The door"],
      [7, 300, 400, "Untitled chapter"],
    ],
  );
  assert.equal(chapterRows(marks).at(-1)?.end, null);
  assert.ok(rows.every((r) => r.kind === "chapter"));
});

test("highlights come in time order, the shorter first at one start, with their kind and why", () => {
  const rows = highlightRows(marks);
  assert.deepEqual(
    rows.map((r) => [r.id, r.title, r.end, r.category, r.why]),
    [
      [6, "Short", 14, "hype", null],
      [5, "Long", 30, null, null],
      [2, "Later", 60, "funny", "Why."],
    ],
  );
});

test("the current chapter is the last to start by the playhead, none before the first", () => {
  const rows = [{ start: 5 }, { start: 120 }, { start: 300 }];
  assert.equal(currentChapter(rows, 0), -1);
  assert.equal(currentChapter(rows, 5), 0);
  assert.equal(currentChapter(rows, 119.9), 0);
  assert.equal(currentChapter(rows, 120), 1);
  assert.equal(currentChapter(rows, 4000), 2);
  assert.equal(currentChapter([], 10), -1);
});

test("a highlight's length reads in seconds under a minute, as a time over", () => {
  assert.equal(lengthLabel(7, 14), "7 s");
  assert.equal(lengthLabel(7, 7.2), "1 s");
  assert.equal(lengthLabel(0, 65), "1:05");
  assert.equal(lengthLabel(0, 3725), "1:02:05");
  assert.equal(lengthLabel(7, null), "");
  assert.equal(lengthLabel(7, 5), "");
});

test("arrows, Home and End move between rows; other keys don't", () => {
  assert.equal(stepRow(0, 3, "ArrowUp"), 0);
  assert.equal(stepRow(0, 3, "ArrowDown"), 1);
  assert.equal(stepRow(2, 3, "ArrowDown"), 2);
  assert.equal(stepRow(1, 3, "Home"), 0);
  assert.equal(stepRow(1, 3, "End"), 2);
  assert.equal(stepRow(1, 3, "Enter"), null);
  assert.equal(stepRow(0, 0, "ArrowDown"), null);
});
