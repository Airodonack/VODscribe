// The transcript view's rows (rows.ts): node --test apps/desktop/src/lib/rows.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { FAR, buildRows, linesFrom, linesSpan, messagesIn, nearestLine } from "./rows.ts";

/** @param {number} start @returns {import("./api").Line} */
const line = (start) => ({ start, text: "hi" });
/** @param {import("./rows").Row[]} rows */
const spans = (rows) => rows.map((r) => [r.line ? "line" : "chat", r.from, r.to]);

// Source: rows.ts `buildRows` ("without chat, just the lines and gaps").
test("without chat, a row per line", () => {
  assert.deepEqual(spans(buildRows([line(5), line(9)], 600, false)), [
    ["line", 5, 9],
    ["line", 9, FAR],
  ]);
});

// Source: rows.ts header ("Where there's no transcript (yet), chat goes in rows of a minute each").
test("with chat and no transcript yet, rows of a minute to the end", () => {
  assert.deepEqual(spans(buildRows([], 150, true)), [
    ["chat", 0, 60],
    ["chat", 60, 120],
    ["chat", 120, FAR],
  ]);
});

// Source: rows.ts header ("before the first line arrives ... after the last line while the rest is transcribed").
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

// Source: rows.ts `minuteRows` ("the last one open-ended").
test("the last line's row is open-ended when the VOD ends in its minute", () => {
  assert.deepEqual(spans(buildRows([line(10), line(100)], 110, true)).at(-1), ["line", 100, FAR]);
});

// Source: rows.ts `minuteRows` ("A length the platform got wrong (missing, negative, endless) gives none").
test("a length that isn't a real number gives no minute rows", () => {
  for (const duration of [NaN, Infinity, -5, 0]) assert.deepEqual(buildRows([], duration, true), []);
});

// Source: rows.ts `Row.key`.
test("keys are unique, and a line's key changes when its row's end does", () => {
  const before = buildRows([line(10), line(10)], 300, true);
  assert.equal(new Set(before.map((r) => r.key)).size, before.length);
  const after = buildRows([line(10), line(10), line(50)], 300, true);
  assert.notEqual(before[2].key, after[2].key);
  assert.equal(before[1].key, after[1].key);
});

// Source: rows.ts header ("in a stretch of more than a minute without speech").
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

// Source: rows.ts `buildRows` ("A minute or less to the next line: one row").
test("a gap within one minute, however long the gap's rows would be, stays one row", () => {
  assert.deepEqual(
    spans(buildRows([line(61), line(119)], 600, true)).find((r) => r[0] === "line"),
    ["line", 61, 119],
  );
});

// Source: rows.ts `buildRows` (without chat, one row per line).
test("without chat, gaps stay one row", () => {
  assert.equal(buildRows([line(10), line(500)], 600, false).length, 2);
});

// Source: rows.ts header ("before a first line more than a minute in").
test("with chat, a first line more than a minute in has minutes of chat before it", () => {
  assert.deepEqual(spans(buildRows([line(150)], 200, true)).slice(0, 3), [
    ["chat", -FAR, 60],
    ["chat", 60, 120],
    ["chat", 120, 150],
  ]);
  const rows = buildRows([line(150)], 200, true);
  assert.equal(new Set(rows.map((r) => r.key)).size, rows.length, "keys stay unique");
});

// Source: rows.ts `linesSpan`.
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

// Source: rows.ts `nearestLine`; 0.7.0 ("The speaker and chapter stay in view.").
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

// Source: rows.ts `Row.key` ("By time, not place").
test("a line arriving in the middle keeps the keys of the rows after it", () => {
  const before = buildRows([line(10), line(500)], 600, false);
  const after = buildRows([line(10), line(200), line(500)], 600, false);
  assert.equal(after[2].key, before[1].key);
});

// Source: rows.ts header ("A part of the VOD not transcribed has a row of its own where it starts").
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

// Source: rows.ts `linesFrom`.
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

// Source: rows.ts `messagesIn`.
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

// Source for the tests below: rows.ts's header ("chat goes in rows of a minute
// each ... all of it before the first line arrives, before a first line more
// than a minute in ... after the last line while the rest is transcribed, and
// in a stretch of more than a minute without speech") and its `buildRows`,
// `linesSpan`, `nearestLine` and `messagesIn` doc comments.

test("a VOD a whole number of minutes long ends its last minute row open-ended", () => {
  assert.deepEqual(spans(buildRows([], 120, true)), [
    ["chat", 0, 60],
    ["chat", 60, FAR],
  ]);
});

test("a first line exactly a minute in has one row of chat before it", () => {
  assert.deepEqual(spans(buildRows([line(60)], 61, true)).slice(0, 2), [
    ["chat", -FAR, 60],
    ["line", 60, FAR],
  ]);
});

test("a line exactly a minute before the next is one row, not minutes", () => {
  assert.deepEqual(spans(buildRows([line(10), line(70)], 100, true)).slice(0, 3), [
    ["chat", -FAR, 10],
    ["line", 10, 70],
    ["line", 70, FAR],
  ]);
});

test("a line comes before a gap starting with it", () => {
  const gap = { start: 10, end: 50, request: null };
  assert.deepEqual(
    buildRows([line(10)], 600, false, [gap]).map((r) => [r.line ? "line" : "gap", r.from, r.to]),
    [
      ["line", 10, 10],
      ["gap", 10, FAR],
    ],
  );
});

test("a gap's key is its start; the same after a line arrives before it", () => {
  const gap = { start: 100, end: null, request: null };
  const before = buildRows([line(10)], 600, false, [gap]);
  const after = buildRows([line(10), line(20)], 600, false, [gap]);
  assert.equal(before.at(-1)?.key, "gap 100 to 1000000000000");
  assert.equal(after.at(-1)?.key, before.at(-1)?.key);
});

test("lines and gaps given without chat and none of either give no rows", () => {
  assert.deepEqual(buildRows([], 600, false), []);
});

test("a run of lines in a VOD of unknown length isn't cut short", () => {
  const lines = [{ start: 90, text: "a" }];
  assert.deepEqual(linesSpan(lines, 0, 0, null), { start: 90, end: 95 });
  assert.deepEqual(linesSpan(lines, 0, 0, 0), { start: 90, end: 95 });
});

test("a line starting exactly between two others goes to the earlier one", () => {
  assert.equal(nearestLine([line(0), line(10)], 5), 0);
  assert.equal(nearestLine([line(0), line(10), line(20)], 15), 1);
});

test("a row past the VOD's end holds the messages of the bins it covers", () => {
  assert.equal(messagesIn([10, 0, 4, 2], 40, 25, 500), 4 * 0.5 + 2);
  assert.equal(messagesIn([10, 0, 4, 2], 40, 45, 50), 0);
});

test("a line sharing its start with the one before has its own key; so does a third", () => {
  const keys = buildRows([line(10), line(10), line(10)], 0, false).map((r) => r.key);
  assert.deepEqual(keys, [`line 10 to 10`, `line 10 #1 to 10`, `line 10 #2 to ${FAR}`]);
});

test("the chat before a first line is one row keyed by where it ends, or minutes keyed by theirs", () => {
  assert.equal(buildRows([line(60)], 61, true)[0].key, "before to 60");
  assert.deepEqual(
    buildRows([line(120)], 130, true)
      .slice(0, 2)
      .map((r) => r.key),
    ["minute 0 to 60", "minute 1 to 120"],
  );
});

test("a first line at a whole number of minutes has no empty row of chat before the line", () => {
  assert.deepEqual(spans(buildRows([line(120)], 130, true)).slice(0, 3), [
    ["chat", -FAR, 60],
    ["chat", 60, 120],
    ["line", 120, FAR],
  ]);
  const rows = buildRows([line(150)], 160, true);
  assert.deepEqual(spans(rows), [
    ["chat", -FAR, 60],
    ["chat", 60, 120],
    ["chat", 120, 150],
    ["line", 150, FAR],
  ]);
});

test("a stretch without speech has its minutes keyed by the minute they start", () => {
  const keys = buildRows([line(10), line(200)], 260, true).map((r) => r.key);
  assert.deepEqual(keys.slice(2, 5), ["minute 1 to 120", "minute 2 to 180", "minute 3 to 200"]);
});

test("a line starting exactly where the VOD's stated length ends isn't cut to nothing", () => {
  assert.deepEqual(linesSpan([{ start: 10, text: "a" }], 0, 0, 10), { start: 10, end: 15 });
});

test("a drag starting on the first line finds it", () => {
  assert.deepEqual(linesFrom([line(0), line(5)], 0, 3), [0, 0]);
});
