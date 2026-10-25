// Where the transcript's view goes to follow the playhead (follow.ts): node --test apps/desktop/src/lib/follow.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { followTarget } from "./follow.ts";

// A view 600 px tall, scrolled to 1000.
const top = 1000;
const height = 600;

test("a line well in view: the view holds still", () => {
  assert.equal(followTarget(1100, 1130, top, height), null);
  // Down to nine tenths of the way.
  assert.equal(followTarget(1500, 1540, top, height), null);
});

test("a line nearing the bottom: the view moves it a third of the way down, gliding", () => {
  assert.deepEqual(followTarget(1530, 1560, top, height), { top: 1330, far: false });
});

test("a line just above the view: a third of the way down, gliding", () => {
  assert.deepEqual(followTarget(900, 930, top, height), { top: 700, far: false });
});

test("a jump far below or above: a third of the way down, at once", () => {
  assert.deepEqual(followTarget(5000, 5030, top, height), { top: 4800, far: true });
  assert.deepEqual(followTarget(100, 130, top, height), { top: -100, far: true });
});

test("a line taller than the rest of the view, starting high enough: the view holds still", () => {
  assert.equal(followTarget(1150, 2000, top, height), null);
  assert.deepEqual(followTarget(1400, 2000, top, height), { top: 1200, far: false });
});

test("no view yet: nothing to do", () => {
  assert.equal(followTarget(5000, 5030, top, 0), null);
});
