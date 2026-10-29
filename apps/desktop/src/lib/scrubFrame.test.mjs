// The timeline thumbnail's placement and moments (scrubFrame.ts): node --test apps/desktop/src/lib/scrubFrame.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { edgeShows } from "./media.ts";
import { nearWithin, place } from "./scrubFrame.ts";

test("the thumbnail and tip are centred on the line, shifted in whole at the timeline's ends", () => {
  // 80 px of frame, a 40 px tip, on an 800 px timeline.
  assert.deepEqual(place(100, 800, 40, 80), { frame: 60, tip: 80 });
  // Near the left end: the frame's left edge on it, the tip still centred on the frame.
  assert.deepEqual(place(10, 800, 40, 80), { frame: 0, tip: 20 });
  // Near the right end, likewise.
  assert.deepEqual(place(795, 800, 40, 80), { frame: 720, tip: 740 });
  // A tip wider than the frame (a narrow picture) is what's kept inside.
  assert.deepEqual(place(5, 800, 60, 20), { frame: 20, tip: 0 });
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
