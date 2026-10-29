// The transcript view's rows (rows.ts): node --test apps/desktop/src/lib/rows.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { FAR, buildRows, linesFrom, linesSpan, messagesIn, nearestLine } from "./rows.ts";

/** @param {number} start @returns {import("./api").Line} */
const line = (start) => ({ start, text: "hi" });
/** @param {import("./rows").Row[]} rows */
const spans = (rows) => rows.map((r) => [r.line ? "line" : "chat", r.from, r.to]);

test("without chat, a row per line", () => {
  assert.deepEqual(spans(buildRows([line(5), line(9)], 600, false)), [
    ["line", 5, 9],
    ["line", 9, FAR],
  ]);
});

test("with chat and no transcript yet, rows of a minute to the end", () => {
  assert.deepEqual(spans(buildRows([], 150, true)), [
    ["chat", 0, 60],
    ["chat", 60, 120],
    ["chat", 120, FAR],
  ]);
});

test("with chat, chat before the first line, and minutes after the last", () => {
  assert.deepEqual(spans(buildRows([line(10), line(70)], 250, true)), [
    ["chat", -FAR, 10],
    ["line", 10, 70],
    ["line", 70, 120],
    ["chat", 120, 180],
    ["chat", 180, 240],
    ["chat", 240, FAR],
  ]);
});

test("the last line's row is open-ended when the VOD ends in its minute", () => {
  assert.deepEqual(spans(buildRows([line(10), line(100)], 110, true)).at(-1), ["line", 100, FAR]);
});

test("a length that isn't a real number gives no minute rows", () => {
  for (const duration of [NaN, Infinity, -5, 0]) assert.deepEqual(buildRows([], duration, true), []);
});

test("keys are unique, and a line's key changes when its row's end does", () => {
  const before = buildRows([line(10), line(10)], 300, true);
  assert.equal(new Set(before.map((r) => r.key)).size, before.length);
  const after = buildRows([line(10), line(10), line(50)], 300, true);
  assert.notEqual(before[2].key, after[2].key);
  assert.equal(before[1].key, after[1].key);
});

test("with chat, a stretch of more than a minute without speech goes in minutes", () => {
  assert.deepEqual(spans(buildRows([line(10), line(30), line(200)], 260, true)), [
    ["chat", -FAR, 10],
    ["line", 10, 30],
    ["line", 30, 60],
    ["chat", 60, 120],
    ["chat", 120, 180],
    ["chat", 180, 200],
    ["line", 200, 240],
    ["chat", 240, FAR],
  ]);
});

test("a gap within one minute, however long the gap's rows would be, stays one row", () => {
  assert.deepEqual(
    spans(buildRows([line(61), line(119)], 600, true)).find((r) => r[0] === "line"),
    ["line", 61, 119],
  );
});

test("without chat, gaps stay one row", () => {
  assert.equal(buildRows([line(10), line(500)], 600, false).length, 2);
});

test("with chat, a first line more than a minute in has minutes of chat before it", () => {
  assert.deepEqual(spans(buildRows([line(150)], 200, true)).slice(0, 3), [
    ["chat", -FAR, 60],
    ["chat", 60, 120],
    ["chat", 120, 150],
  ]);
  const rows = buildRows([line(150)], 200, true);
  assert.equal(new Set(rows.map((r) => r.key)).size, rows.length, "keys stay unique");
});

test("a run of lines spans from the first one's start to the last one's end", () => {
  const lines = [
    { start: 1, end: 2.5, text: "a" },
    { start: 3, end: 4, text: "b" },
    { start: 6, text: "c" },
    { start: 9, text: "d" },
  ];
  assert.deepEqual(linesSpan(lines, 0, 1), { start: 1, end: 4 });
  assert.deepEqual(linesSpan(lines, 1, 0), { start: 1, end: 4 }, "dragged upwards");
  // The last line with no end known: 5 s on, but not past the VOD's end.
  const last = [
    { start: 1, text: "a" },
    { start: 8, text: "b" },
  ];
  assert.deepEqual(linesSpan(last, 0, 1), { start: 1, end: 13 });
  assert.deepEqual(linesSpan(last, 0, 1, 10), { start: 1, end: 10 }, "within the VOD");
  assert.deepEqual(linesSpan(last, 1, 1, 7.5), { start: 8, end: 13 }, "a line past the stated length: not inverted");
  assert.deepEqual(linesSpan(lines, 2, 2), { start: 6, end: 9 }, "no end: to the next line");
  assert.deepEqual(linesSpan(lines, 3, 3), { start: 9, end: 14 }, "the last, with no end");
});

test("a chapter's heading goes over the line starting nearest it", () => {
  const lines = [0, 8, 15, 30].map(line);
  assert.equal(nearestLine(lines, -3), 0);
  assert.equal(nearestLine(lines, 2), 0);
  assert.equal(nearestLine(lines, 12), 2);
  assert.equal(nearestLine(lines, 11), 1);
  assert.equal(nearestLine(lines, 15), 2);
  assert.equal(nearestLine(lines, 99), 3);
  assert.equal(nearestLine([line(5)], 0), 0);
});

test("a line arriving in the middle keeps the keys of the rows after it", () => {
  const before = buildRows([line(10), line(500)], 600, false);
  const after = buildRows([line(10), line(200), line(500)], 600, false);
  assert.equal(after[2].key, before[1].key);
});

test("a gap has a row where it starts, among the lines", () => {
  const gap = { start: 120, end: 300, request: null };
  assert.deepEqual(
    buildRows([line(10), line(300)], 600, false, [gap]).map((r) => [r.line ? "line" : r.gap ? "gap" : "chat", r.from]),
    [
      ["line", 10],
      ["gap", 120],
      ["line", 300],
    ],
  );
  // With chat, its minutes follow it, as a line's would.
  assert.deepEqual(spans(buildRows([line(10), line(300)], 400, true, [gap])).slice(1, 6), [
    ["line", 10, 60],
    ["chat", 60, 120],
    ["chat", 120, 180],
    ["chat", 180, 240],
    ["chat", 240, 300],
  ]);
  const rows = buildRows([line(10), line(300)], 400, true, [gap]);
  assert.equal(rows[3].gap, gap, "the gap's row holds the chat of its first minute");
});

test("a drag's lines, from its ends' times, stay put as lines arrive above them", () => {
  /** @param {number} start */
  const said = (start) => ({ start, end: start + 2, text: `at ${start}` });
  // Pressed on the line at 40 and dragged to the one at 50, while a stretch
  // above, 2 to 30 s, is being transcribed: its lines come in meanwhile.
  const before = [said(0), said(30), said(40), said(50)];
  const after = [said(0), said(10), said(20), said(30), said(40), said(50)];
  assert.deepEqual(linesFrom(before, 50, 40), [2, 3]);
  const [a, b] = linesFrom(after, 40, 50) ?? [-1, -1];
  assert.deepEqual([after[a].start, after[b].start], [40, 50]);
  // Lines sharing a start both count; none in the span, none.
  assert.deepEqual(linesFrom([said(1), said(5), said(5), said(9)], 5, 5), [1, 2]);
  assert.equal(linesFrom(after, 41, 49), null);
  assert.equal(linesFrom([], 0, 10), null);
});

test("a row's messages, guessed from the chat's histogram", () => {
  // Four bins of 10 s.
  const histogram = [10, 0, 4, 2];
  assert.equal(messagesIn(histogram, 40, -FAR, FAR), 16);
  assert.equal(messagesIn(histogram, 40, 0, 10), 10);
  assert.equal(messagesIn(histogram, 40, 5, 25), 5 + 0 + 2);
  assert.equal(messagesIn(histogram, 40, 12, 18), 0);
  assert.equal(messagesIn([], 40, 0, 10), 0);
  assert.equal(messagesIn(histogram, 0, 0, 10), 0);
});
