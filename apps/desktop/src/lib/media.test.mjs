// The player's and timeline's arithmetic (media.ts): node --test apps/desktop/src/lib/media.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  columns,
  cueAt,
  isUnplayable,
  unplayable,
  playedSpans,
  frameStart,
  inFrame,
  nudge,
  stepTime,
  stretchIn,
  stretches,
  tickStep,
} from "./media.ts";

test("the caption up is the last one started, until it ends", () => {
  const cues = [
    { start: 1, end: 2, text: "a" },
    { start: 2, end: 3.5, text: "b" },
    { start: 5, end: 6, text: "c" },
  ];
  assert.equal(cueAt(cues, 0.5), null);
  assert.equal(cueAt(cues, 1)?.text, "a");
  assert.equal(cueAt(cues, 2)?.text, "b", "the next one takes over at its start");
  assert.equal(cueAt(cues, 4), null, "a gap between two");
  assert.equal(cueAt(cues, 5.9)?.text, "c");
  assert.equal(cueAt(cues, 6), null);
  assert.equal(cueAt([], 1), null);
});

test("each column is the loudest level in its stretch", () => {
  // Two seconds: twenty levels, into four columns of five.
  const levels = Uint8Array.from({ length: 20 }, (_, i) => (i === 7 ? 255 : i));
  assert.deepEqual(Array.from(columns(levels, 2, 4)), [4 / 255, 1, 14 / 255, 19 / 255].map(Math.fround));
  // More columns than levels: each still reads one.
  assert.equal(columns(levels, 2, 40)[39], Math.fround(19 / 255));
  // The video runs on past the levels (read before it was all here): silence.
  assert.deepEqual(Array.from(columns(levels, 4, 4)).slice(2), [0, 0]);
  assert.deepEqual(Array.from(columns(new Uint8Array(), 2, 3)), [0, 0, 0]);
});

test("frame steps land inside the frame and count from it", () => {
  const fps = 30;
  let t = 10;
  t = stepTime(t, 1, fps);
  assert.ok(Math.abs(t - 301.25 / 30) < 1e-9);
  for (let i = 0; i < 100; i++) t = stepTime(t, 1, fps);
  assert.equal(Math.floor(t * fps), 401, "no drift");
  t = stepTime(t, -1, fps);
  assert.equal(Math.floor(t * fps), 400);
  assert.equal(stepTime(0, -1, fps), 0);
  // 29.97: a time on a frame's start counts as that frame.
  assert.equal(Math.floor(stepTime(100 / 29.97, 1, 29.97) * 29.97), 101);
});

test("ticks are spaced for their labels", () => {
  assert.equal(tickStep(120, 800), 10, "two minutes: every 10 s");
  assert.equal(tickStep(30, 800), 5);
  assert.equal(tickStep(10 * 3600, 800), 3600);
  assert.equal(tickStep(100 * 3600, 400), 14400, "the longest step at most");
});

test("a mark goes where the frame on show starts", () => {
  assert.equal(frameStart(10.02, 30), 10);
  assert.equal(frameStart(10.04, 30), 301 / 30);
  assert.equal(frameStart(301 / 30, 30), 301 / 30, "a time on a frame's start is that frame");
  assert.equal(frameStart(0.01, 60), 0);
});

test("marks move a frame at a time, within the video", () => {
  assert.equal(nudge(10, 1, 30, 100), 301 / 30);
  assert.equal(nudge(10, -10, 30, 100), 290 / 30);
  assert.equal(nudge(0, -1, 30, 100), 0);
  assert.equal(nudge(100, 5, 30, 100), 100);
  // From a time a hair off the frame (stored as a float): still whole frames.
  assert.equal(nudge(301 / 30 + 1e-9, 1, 30, 100), 302 / 30);
});

test("cuts split the video into stretches", () => {
  assert.deepEqual(stretches([], 60), [{ start: 0, end: 60 }]);
  assert.deepEqual(stretches([30], 60), [
    { start: 0, end: 30 },
    { start: 30, end: 60 },
  ]);
  assert.deepEqual(stretches([40, 10, 10, 0, 60, 70], 60), [
    { start: 0, end: 10 },
    { start: 10, end: 40 },
    { start: 40, end: 60 },
  ]);
});

test("a seek to a cut lands a quarter of a frame into the frame it starts, short of the end", () => {
  assert.equal(inFrame(1, 4, 10), 1.0625);
  assert.equal(frameStart(inFrame(2 / 30, 30, 10), 30), 2 / 30);
  assert.equal(inFrame(10, 30, 10), 10);
});

test("an edit's piece plays what isn't struck, as the render does", () => {
  assert.deepEqual(playedSpans(0, 10, []), [[0, 10]]);
  assert.deepEqual(
    playedSpans(0, 10, [
      [6, 7],
      [2, 3],
    ]),
    [
      [0, 2],
      [3, 6],
      [7, 10],
    ],
  );
  assert.deepEqual(
    playedSpans(0, 10, [
      [-1, 1],
      [9, 12],
    ]),
    [[1, 9]],
  );
  assert.deepEqual(
    playedSpans(0, 10, [
      [2, 5],
      [4, 6],
    ]),
    [
      [0, 2],
      [6, 10],
    ],
  );
  assert.deepEqual(playedSpans(0, 10, [[0, 10]]), []);
  assert.deepEqual(
    playedSpans(0, 10, [
      [2, 5],
      [5.02, 8],
    ]),
    [
      [0, 2],
      [8, 10],
    ],
  );
});

test("the stretch the playhead is in, the last one at the very end", () => {
  assert.deepEqual(stretchIn([2], 5, 1), { start: 0, end: 2 });
  assert.deepEqual(stretchIn([2], 5, 2), { start: 2, end: 5 });
  // Played to the end (the time stops at the length), or a site's player past it.
  assert.deepEqual(stretchIn([2], 5, 5), { start: 2, end: 5 });
  assert.deepEqual(stretchIn([], 5, 5.3), { start: 0, end: 5 });
  assert.equal(stretchIn([], 0, 0), undefined);
});

test("a file that won't play says why, and is known as such", () => {
  assert.equal(unplayable({ code: 4 }), "This video's format can't be played here.");
  assert.equal(unplayable(null), "This video couldn't be played.");
  assert.ok(isUnplayable(unplayable({ code: 4 })) && isUnplayable(unplayable(undefined)));
  assert.ok(!isUnplayable("The file is missing."));
});
