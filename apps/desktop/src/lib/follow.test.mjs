// Where the transcript's view goes to follow the playhead (follow.ts): node --test apps/desktop/src/lib/follow.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { followTarget } from "./follow.ts";

// A view 600 px tall, scrolled to 1000.
const top = 1000;
const height = 600;

// Source: follow.ts header ("It holds still while the line is well in view"); 0.5.1 ("the page moving on once the line nears the bottom rather than with every line").
test("a line well in view: the view holds still", () => {
  assert.equal(followTarget(1100, 1130, top, height), null);
  // Down to nine tenths of the way.
  assert.equal(followTarget(1500, 1540, top, height), null);
});

// Source: follow.ts header ("puts the line a third of the way down").
test("a line nearing the bottom: the view moves it a third of the way down, gliding", () => {
  assert.deepEqual(followTarget(1530, 1560, top, height), { top: 1330, far: false });
});

// Source: follow.ts `followTarget`.
test("a line just above the view: a third of the way down, gliding", () => {
  assert.deepEqual(followTarget(900, 930, top, height), { top: 700, far: false });
});

// Source: follow.ts header ("a jump of the playhead, anywhere, lands there too").
test("a jump far below or above: a third of the way down, at once", () => {
  assert.deepEqual(followTarget(5000, 5030, top, height), { top: 4800, far: true });
  assert.deepEqual(followTarget(100, 130, top, height), { top: -100, far: true });
});

// Source: follow.ts `followTarget` ("a tall row starting high enough").
test("a line taller than the rest of the view, starting high enough: the view holds still", () => {
  assert.equal(followTarget(1150, 2000, top, height), null);
  assert.deepEqual(followTarget(1400, 2000, top, height), { top: 1200, far: false });
});

// Source: follow.ts `followTarget` (height 0).
test("no view yet: nothing to do", () => {
  assert.equal(followTarget(5000, 5030, top, 0), null);
});

// Source for the tests below: follow.ts's header and `followTarget` doc
// comment ("It holds still while the line is well in view ... otherwise puts
// the line a third of the way down"), and 0.5.1 ("the page moving on once the
// line nears the bottom rather than with every line").

test("a line starting exactly at the view's top, or ending exactly nine tenths down, holds still", () => {
  assert.equal(followTarget(1000, 1030, top, height), null);
  assert.equal(followTarget(1500, 1540, top, height), null);
  assert.deepEqual(followTarget(1500, 1541, top, height), { top: 1300, far: false });
});

test("a line taller than the view starting exactly a third down holds still; below that it moves", () => {
  assert.equal(followTarget(1200, 3000, top, height), null);
  assert.notEqual(followTarget(1201, 3000, top, height), null);
});

test("a move of exactly a screen glides; more than that goes at once", () => {
  // The target is `height` from the top: 1000 + 600 = 1600, a - 200.
  assert.deepEqual(followTarget(1800, 1830, top, height), { top: 1600, far: false });
  assert.deepEqual(followTarget(1801, 1830, top, height), { top: 1601, far: true });
});

test("a line put a third of the way down holds still there", () => {
  const [a, b] = [1600 + 200, 1600 + 230];
  assert.equal(followTarget(a, b, 1600, height), null);
});
