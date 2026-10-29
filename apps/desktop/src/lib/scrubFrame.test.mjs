// The timeline thumbnail's placement and moments (scrubFrame.ts): node --test apps/desktop/src/lib/scrubFrame.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { edgeShows } from "./media.ts";
import { nearWithin, place } from "./scrubFrame.ts";

test("the thumbnail sits to the left of the line, off it, and to the right only at the left end", () => {
  // 80 px of frame, a 40 px tip, on an 800 px timeline: the tip centred on the line.
  assert.deepEqual(place(200, 800, 40, 80), { frame: 112, tip: 180 });
  // Too close to the left end for a frame on the left: on the right instead.
  assert.deepEqual(place(50, 800, 40, 80), { frame: 58, tip: 30 });
  // At the right end the frame still fits on the left; the tip is kept off the end.
  assert.deepEqual(place(795, 800, 40, 80), { frame: 707, tip: 752 });
  // A timeline narrower than the frame: its left end wins.
  assert.equal(place(30, 60, 40, 80).frame, 0);
});

test("with no thumbnail the tip is centred on the line, kept off the ends", () => {
  assert.deepEqual(place(100, 800, 40, 0), { frame: null, tip: 80 });
  assert.equal(place(3, 800, 40, 0).tip, 28 - 20);
  assert.equal(place(799, 800, 40, 0).tip, 800 - 28 - 20);
});

test("a selection's end shows the frame before it, its start its own", () => {
  assert.equal(edgeShows("start", 2, 30), 2);
  assert.equal(edgeShows("end", 4, 30), 119 / 30);
  assert.equal(edgeShows("end", 0, 30), 0);
});

test("a stored frame counts as near within 30 pixels of the view's time", () => {
  assert.equal(nearWithin(600, 1200), 15);
  assert.equal(nearWithin(10, 0), 0);
});
