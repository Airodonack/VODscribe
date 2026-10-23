// What of a VOD is transcribed (stretches.ts): node --test apps/desktop/src/lib/stretches.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { covered, gaps, showsProgress, transcribeRest } from "./stretches.ts";

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

test("transcribing the rest asks for each gap not asked for, and nothing else", async () => {
  /** @type {[number, number, number | null][]} */
  const asked = [];
  const calls = {
    transcriptStretches: async () => [stretch(120, 180), stretch(300, 360, "queued")],
    transcribeStretch: async (
      /** @type {number} */ id,
      /** @type {number} */ start,
      /** @type {number | null} */ end,
    ) => {
      asked.push([id, start, end]);
      return stretch(start, end, "queued");
    },
  };
  // @ts-ignore: a VOD with only what's read of it.
  await transcribeRest({ id: 7, duration: 600 }, calls);
  assert.deepEqual(asked, [
    [7, 0, 120],
    [7, 180, 300],
    [7, 360, null],
  ]);
});

test("a VOD's row keeps the request it shows while another runs beside it", () => {
  // Request 1 transcribing, request 2 of the same VOD downloading: each
  // sends its progress every 250 ms.
  const row = {
    stretch: /** @type {number | null} */ (1),
    status: /** @type {import("./api").Status} */ ("transcribing"),
  };
  let turns = 0;
  for (let tick = 0; tick < 8; tick++) {
    for (const [stretch, status] of /** @type {const} */ ([
      [1, "transcribing"],
      [2, "downloading"],
    ])) {
      if (!showsProgress(row.stretch, row.status, stretch, status)) continue;
      turns += Number(row.stretch !== stretch || row.status !== status);
      Object.assign(row, { stretch, status });
    }
  }
  assert.deepEqual([row.stretch, row.status, turns], [1, "transcribing", 0]);
  // Once the one shown isn't running (done, before the VOD's own event), a running one shows.
  assert.equal(showsProgress(1, "done", 2, "downloading"), true);
  assert.equal(showsProgress(1, "queued", 2, "transcribing"), true);
  assert.equal(showsProgress(null, "none", 2, "downloading"), true);
  // The one shown, whatever it now is.
  assert.equal(showsProgress(1, "transcribing", 1, "done"), true);
  assert.equal(showsProgress(1, "transcribing", 2, "ready"), false);
});
