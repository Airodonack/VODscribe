// The transcript view's rows (rows.ts): node --test apps/desktop/src/lib/rows.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { FAR, buildRows } from "./rows.ts";

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
