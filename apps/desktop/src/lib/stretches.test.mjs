// What of a VOD is transcribed (stretches.ts): node --test apps/desktop/src/lib/stretches.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { formatTime } from "./platform.ts";
import {
  covered,
  gaps,
  requestDetail,
  showsProgress,
  shownGaps,
  statusLabel,
  transcribeChoices,
  transcribeRest,
  transcribeStretchOf,
} from "./stretches.ts";

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
  audio: false,
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

test("the rest of a VOD with nothing transcribed is all of it, once: not again while that's asked for", async () => {
  /** @type {[number, number, number | null][]} */
  const asked = [];
  /** @type {import("./api").Stretch[]} */
  let have = [];
  const calls = {
    transcriptStretches: async () => have,
    transcribeStretch: async (
      /** @type {number} */ id,
      /** @type {number} */ start,
      /** @type {number | null} */ end,
    ) => {
      asked.push([id, start, end]);
      const s = stretch(start, end, "queued");
      have = [...have, s];
      return s;
    },
  };
  // @ts-ignore: a VOD with only what's read of it.
  await transcribeRest({ id: 7, duration: 600 }, calls);
  // @ts-ignore: pressed again while it waits.
  await transcribeRest({ id: 7, duration: 600 }, calls);
  assert.deepEqual(asked, [[7, 0, null]]);
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

test("once some is transcribed, its gaps show though it has no lines yet", () => {
  // 2:00–3:00 transcribed, all silence: no lines, nothing asked.
  const silent = [stretch(120, 180)];
  assert.deepEqual(spans(shownGaps(silent, 600, "some", 0)), [
    [0, 120, null],
    [180, null, null],
  ]);
  // Before anything is transcribed or asked for, the empty state says so instead.
  assert.deepEqual(shownGaps([], 600, "none", 0), []);
  assert.equal(shownGaps([stretch(0, null, "queued")], 600, "none", 0).length, 1, "a request shows");
  assert.deepEqual(shownGaps([stretch(0, null)], 600, "all", 0), [], "all of it: no speech found");
});

test("a row's request says its range and how many more wait", () => {
  assert.equal(requestDetail({ start: 720, end: 1320 }, 3600, 0, formatTime), " · 12:00–22:00");
  assert.equal(requestDetail({ start: 600, end: null }, 3600, 2, formatTime), " · 10:00–1:00:00 · 2 more waiting");
  assert.equal(requestDetail({ start: 0, end: null }, 3600, 0, formatTime), "", "all of it needs no saying");
  assert.equal(requestDetail({ start: 0, end: null }, 3600, 1, formatTime), " · 1 more waiting");
  assert.equal(requestDetail({ start: 600, end: null }, null, 0, formatTime), " · from 10:00", "to an end not known");
  assert.equal(requestDetail(null, 3600, 0, formatTime), "");
});

test("the Transcribe menu offers each stretch once, near the end as well", () => {
  /** @param {number} from @param {number | null} duration */
  const offered = (from, duration) =>
    transcribeChoices(from, duration).map((c) => `${c.key} ${JSON.stringify(transcribeStretchOf(c, from, duration))}`);
  // Far from the end, every choice is a stretch of its own.
  assert.equal(offered(600, 7200).length, 5);
  // Ten minutes before the end, 15 min, 1 hour and to the end are all the rest: to the end says it.
  assert.deepEqual(offered(6600, 7200), [
    '300 {"start":6600,"end":6900}',
    'end {"start":6600,"end":null}',
    'whole {"start":0,"end":null}',
  ]);
  // From the start, to the end is the whole VOD.
  assert.deepEqual(offered(0, 600), ['300 {"start":0,"end":300}', 'whole {"start":0,"end":null}']);
  // With no length known, nothing reaches the end but to the end.
  assert.equal(offered(600, null).length, 5);
});

/**
 * stretches.ts `covered`: "The ranges `stretches` transcribed, in order and
 * apart: the done ones, and what requests have reached".
 */
test("covered ranges come in time order, whatever order the stretches are in", () => {
  assert.deepEqual(covered([stretch(300, 400), stretch(0, 60), stretch(120, 180)]), [
    [0, 60],
    [120, 180],
    [300, 400],
  ]);
  assert.deepEqual(covered([]), []);
});

/** stretches.ts `covered`: "what requests have reached" (a request that has reached nothing has covered nothing). */
test("a request that hasn't reached anything covers nothing, one that has covers as far as it reached", () => {
  assert.deepEqual(covered([stretch(0, 600, "queued"), stretch(100, 200, "downloading", null)]), []);
  assert.deepEqual(covered([stretch(100, 200, "transcribing", 100)]), [], "reached its own start");
  assert.deepEqual(covered([stretch(100, 200, "transcribing", 130), stretch(300, null, "paused", 340)]), [
    [100, 130],
    [300, 340],
  ]);
  assert.deepEqual(covered([stretch(100, 200, "error", 150)]), [[100, 150]]);
});

/** stretches.ts `covered`: "ranges this close (EDGE) count as touching", and ranges that overlap are one. */
test("stretches that overlap are covered as one range", () => {
  assert.deepEqual(covered([stretch(0, 100), stretch(50, 80), stretch(90, 150)]), [[0, 150]]);
  assert.deepEqual(covered([stretch(0, null), stretch(500, 700)]), [[0, Infinity]]);
});

/**
 * 0.7.1 Fixes: "The Transcribe menu lists each stretch once. Near a VOD's end,
 * lengths that would run past the end are left out, as To the end covers them."
 */
test("near the end, the menu offers only what reaches the end once, as to the end", () => {
  /** @param {number} from @param {number | null} duration */
  const keys = (from, duration) => transcribeChoices(from, duration).map((c) => c.key);
  assert.deepEqual(keys(7000, 7200), ["end", "whole"], "even 5 min would run past the end");
  assert.deepEqual(keys(6000, 7200), ["300", "900", "end", "whole"], "an hour runs past it, 15 min doesn't");
  assert.deepEqual(keys(0, null), ["300", "900", "3600", "whole"], "from the start the whole VOD is all of it");
});

/**
 * stretches.ts `gaps`: "The parts of a VOD that `stretches` don't cover, split
 * where requests will transcribe them, in time order."
 */
test("gaps come in time order, whatever order the requests are in", () => {
  const late = stretch(400, 500, "queued");
  const early = stretch(100, 200, "queued");
  assert.deepEqual(spans(gaps([late, early], 600)), [
    [0, 100, null],
    [100, 200, "queued"],
    [200, 400, null],
    [400, 500, "queued"],
    [500, null, null],
  ]);
});

/** stretches.ts `EDGE`: "Ranges this close count as touching". */
test("what's left within half a second of the start or of a request's edge isn't a gap of its own", () => {
  // Transcribed from 0.3: the 0.3 s before it touches the start.
  assert.deepEqual(spans(gaps([stretch(0.3, 600)], 600)), []);
  // A request starting 0.3 s into the gap takes all of it; one ending 0.2 s short of the end leaves nothing after.
  assert.deepEqual(spans(gaps([stretch(0.3, 100, "queued")], 600)), [
    [0, 100, "queued"],
    [100, null, null],
  ]);
  assert.deepEqual(spans(gaps([stretch(0, 599.8, "queued")], 600)), [[0, 599.8, "queued"]]);
});

/**
 * stretches.ts `statusLabel`: "A request's status in words, as a VOD's row and
 * the transcript's rows say it; none for done or nothing asked"; e2e
 * transcribe-stretch.mjs (`AT_WORK`): "Waiting to download", "Downloading",
 * "Waiting to transcribe", "Transcribing".
 */
test("a request's status is said in words, and a done one or none asked says nothing", () => {
  assert.deepEqual(statusLabel, {
    queued: "Waiting to download",
    downloading: "Downloading",
    ready: "Waiting to transcribe",
    transcribing: "Transcribing",
    paused: "Paused",
    done: "",
    error: "Failed",
    none: "",
  });
});

/**
 * stretches.ts `requestDetail`: "the stretch's range, its times put as
 * `format` says, unless it's all of it"; 0.6.0: "a part waiting or being
 * transcribed says how far it is".
 */
test("a row's range is said for any stretch but all of the VOD, and for one from the start that ends", () => {
  assert.equal(requestDetail({ start: 0, end: 600 }, 3600, 0, formatTime), " · 0:00–10:00");
  assert.equal(requestDetail({ start: 0, end: null }, null, 0, formatTime), "", "all of it, its length unknown");
  assert.equal(requestDetail({ start: 10, end: null }, 3600, 0, formatTime), " · 0:10–1:00:00", "to the VOD's end");
  assert.equal(requestDetail(null, 3600, 3, formatTime), " · 3 more waiting", "nothing shown, others waiting");
});

/**
 * 0.6.0: "Hold Transcribe to start at the playhead. Pick 5 minutes, 15
 * minutes, an hour, to the end, or the whole VOD"; stretches.ts
 * `transcribeStretchOf`: "a length in seconds from where it opened, on to the
 * end, or all of it from the start".
 */
test("a length asks for that long from the playhead, to the end for the rest, the whole VOD for all of it", () => {
  /** @param {string} key @param {number} from @param {number | null} duration */
  const asks = (key, from, duration) => {
    // The menu far from the end offers all five; the choice is asked for at `from`.
    const choice = transcribeChoices(600, 100000).find((c) => c.key === key);
    assert.ok(choice, key);
    return transcribeStretchOf(choice, from, duration);
  };
  assert.deepEqual(asks("300", 600, 7200), { start: 600, end: 900 });
  assert.deepEqual(asks("900", 600, 7200), { start: 600, end: 1500 });
  assert.deepEqual(asks("3600", 600, 7200), { start: 600, end: 4200 });
  assert.deepEqual(asks("end", 600, 7200), { start: 600, end: null });
  assert.deepEqual(asks("whole", 600, 7200), { start: 0, end: null }, "from the start, wherever the playhead is");
  assert.deepEqual(asks("300", 600, null), { start: 600, end: 900 }, "a length not known reaches nothing");
  assert.deepEqual(asks("3600", 6000, 7200), { start: 6000, end: null }, "an hour past the end is to it");
});

/**
 * stretches.ts `gaps`, `shownGaps`: "once some is [transcribed or asked for],
 * even with no lines, so the rest can be asked for"; 0.6.0: "a part waiting or
 * being transcribed says how far it is, with Stop".
 */
test("a request over part of a VOD shows all its gaps, the parts not asked for too", () => {
  const found = shownGaps([stretch(0, 100, "queued")], 600, "none", 0);
  assert.deepEqual(spans(found), [
    [0, 100, "queued"],
    [100, null, null],
  ]);
});

/** stretches.ts `gaps`, `covered`: "(null: unknown)": a length of 0, as a site or ffmpeg may give, isn't known either. */
test("a VOD whose length is 0 is of a length not known", () => {
  assert.deepEqual(spans(gaps([], 0)), [[0, null, null]]);
  assert.deepEqual(spans(gaps([stretch(0, 60)], 0)), [[60, null, null]]);
});

/** 0.6.0: "Pick 5 minutes, 15 minutes, an hour, to the end, or the whole VOD", in that order, each in words. */
test("the Transcribe menu offers 5 minutes, 15 minutes, an hour, to the end and the whole VOD, each with words", () => {
  const menu = transcribeChoices(600, 7200);
  assert.deepEqual(
    menu.map((c) => [c.key, c.length ?? null, c.whole ?? false]),
    [
      ["300", 300, false],
      ["900", 900, false],
      ["3600", 3600, false],
      ["end", null, false],
      ["whole", null, true],
    ],
  );
  assert.ok(menu.every((c) => c.label.length > 0));
  assert.equal(new Set(menu.map((c) => c.label)).size, menu.length, "no two say the same");
});

/** 0.7.1 Fixes: "The Transcribe menu lists each stretch once": a length ending exactly at the VOD's end is to the end. */
test("a length that ends exactly at the VOD's end is not offered beside to the end", () => {
  assert.deepEqual(
    transcribeChoices(300, 600).map((c) => c.key),
    ["end", "whole"],
  );
  assert.deepEqual(transcribeStretchOf({ key: "300", label: "5 min", length: 300 }, 300, 600), {
    start: 300,
    end: null,
  });
});
