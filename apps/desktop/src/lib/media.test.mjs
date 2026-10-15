// The local player's arithmetic (media.ts): node --test apps/desktop/src/lib/media.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { columns, cueAt, stepTime } from "./media.ts";

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
