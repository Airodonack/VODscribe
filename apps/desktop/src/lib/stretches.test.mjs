// What of a VOD is transcribed (stretches.ts): node --test apps/desktop/src/lib/stretches.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { covered, gaps } from "./stretches.ts";

let ids = 0;
/**
 * @param {number} start @param {number | null} end
 * @param {import("./api").Stretch["status"]} status @param {number | null} reached
 * @returns {import("./api").Stretch}
 */
const stretch = (start, end, status = "done", reached = null) => ({
  id: ++ids,
  vod_id: 1,
  start,
  end,
  status,
  progress: 0,
  reached,
  error: null,
  model: null,
});
/** @param {import("./stretches").Gap[]} list */
const spans = (list) => list.map((g) => [g.start, g.end, g.request ? g.request.status : null]);

test("done stretches and what requests reached are covered, touching ones as one", () => {
  assert.deepEqual(covered([stretch(0, 60), stretch(60.2, 120), stretch(300, null, "transcribing", 400)]), [
    [0, 120],
    [300, 400],
  ]);
  assert.deepEqual(covered([stretch(0, null)]), [[0, Infinity]]);
});

test("nothing transcribed is one gap, to the end", () => {
  assert.deepEqual(spans(gaps([], 600)), [[0, null, null]]);
  assert.deepEqual(spans(gaps([stretch(0, null)], 600)), []);
  assert.deepEqual(spans(gaps([stretch(0, 599.8)], 600)), [], "near enough the end");
});

test("gaps between and after stretches", () => {
  assert.deepEqual(spans(gaps([stretch(120, 180), stretch(300, 400)], 600)), [
    [0, 120, null],
    [180, 300, null],
    [400, null, null],
  ]);
});

test("a request splits its gap: its part waits, the rest is still not asked for", () => {
  const ask = stretch(200, 250, "queued");
  assert.deepEqual(spans(gaps([stretch(0, 100), ask], 600)), [
    [100, 200, null],
    [200, 250, "queued"],
    [250, null, null],
  ]);
  // One running: from where it reached.
  const running = stretch(100, null, "transcribing", 160);
  assert.deepEqual(spans(gaps([stretch(0, 100), running], 600)), [[160, null, "transcribing"]]);
});

test("a request over lines already there is no gap", () => {
  assert.deepEqual(spans(gaps([stretch(0, null), stretch(0, null, "queued")], 600)), []);
});

test("an unknown length: the last gap is open", () => {
  assert.deepEqual(spans(gaps([stretch(0, 60)], null)), [[60, null, null]]);
});
