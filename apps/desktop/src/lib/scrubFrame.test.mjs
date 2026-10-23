// The timeline thumbnail's placement and moments (scrubFrame.ts): node --test apps/desktop/src/lib/scrubFrame.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { edgeShows } from "./media.ts";
import { GAP, nearWithin, place } from "./scrubFrame.ts";

test("the thumbnail and tip sit right of the line, flipping left at the timeline's end", () => {
  // 80 px of frame, a 40 px tip, on an 800 px timeline.
  assert.deepEqual(place(100, 800, 40, 80), { frame: 100 + GAP, tip: 100 + GAP + 80 + GAP, flipped: false });
  // Just fits.
  const fits = 800 - GAP - (80 + GAP + 40);
  assert.equal(place(fits, 800, 40, 80).flipped, false);
  // Past it: on the left, the frame still nearest the line.
  const flipped = place(fits + 1, 800, 40, 80);
  assert.equal(flipped.flipped, true);
  assert.equal((flipped.frame ?? 0) + 80 + GAP, fits + 1);
  assert.equal(flipped.tip + 40 + GAP, flipped.frame);
  // Wider than the timeline's left: kept inside it.
  assert.equal(place(100, 120, 40, 80).tip, 0);
});

test("with no thumbnail the tip is centred on the line, kept off the ends", () => {
  assert.deepEqual(place(100, 800, 40, 0), { frame: null, tip: 80, flipped: false });
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
