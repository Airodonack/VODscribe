// A press that becomes a drag (pressToDrag.ts), driven with synthetic events:
// node --test apps/desktop/src/lib/pressToDrag.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { DRAG_SLOP, SWALLOW_MS, pressToDrag } from "./pressToDrag.ts";

/** The window and a node in it, as far as the action uses them. */
function page() {
  globalThis.window = /** @type {any} */ (new EventTarget());
  const node = /** @type {HTMLElement} */ (/** @type {unknown} */ (new EventTarget()));
  const child = new EventTarget();
  node.contains = (t) => t === node || t === child;
  return { node, child };
}

/** @param {string} type @param {object} props */
const event = (type, props = {}) => Object.assign(new Event(type, { cancelable: true }), props);
/** @param {EventTarget} node */
const down = (node, x = 0, y = 0) => node.dispatchEvent(event("pointerdown", { button: 0, clientX: x, clientY: y }));
/** @param {number} x */
const move = (x, y = 0, buttons = 1) => window.dispatchEvent(event("pointermove", { clientX: x, clientY: y, buttons }));
const up = () => window.dispatchEvent(event("pointerup", { button: 0, buttons: 0 }));
/** A click on `target`: through the window first, as the page sends it; true if it got past.
 *  @param {EventTarget} node @param {EventTarget} target */
function click(node, target = node) {
  const e = event("click");
  Object.defineProperty(e, "target", { value: target });
  window.dispatchEvent(e);
  if (e.defaultPrevented) return false;
  node.dispatchEvent(event("click"));
  return true;
}

/** `node` made a press-to-drag that counts what it's told.
 *  @param {HTMLElement} node @param {() => boolean} [press] */
function counting(node, press) {
  const seen = { start: 0, move: 0, end: /** @type {boolean[]} */ ([]), click: 0 };
  pressToDrag(node, {
    press,
    start: () => seen.start++,
    move: () => seen.move++,
    end: (dragged) => seen.end.push(dragged),
    click: () => seen.click++,
  });
  return seen;
}

test("a press moved less than the slop is no drag; past it, the drag starts once", () => {
  const { node } = page();
  const seen = counting(node);
  down(node, 10, 10);
  move(10 + DRAG_SLOP - 1, 10);
  assert.equal(seen.start, 0);
  move(10 + DRAG_SLOP, 10);
  move(40, 10);
  move(80, 30);
  assert.equal(seen.start, 1);
  assert.equal(seen.move, 2);
  up();
  assert.deepEqual(seen.end, [true]);
});

test("let go outside the window, then moved back over it with no button held: no drag", () => {
  const { node } = page();
  const seen = counting(node);
  down(node, 10, 10);
  // The release went elsewhere; the next move says no button is down.
  move(200, 10, 0);
  move(300, 10, 0);
  assert.equal(seen.start, 0);
  assert.deepEqual(seen.end, [false]);
  // A press anew still works.
  down(node, 10, 10);
  move(30, 10);
  assert.equal(seen.start, 1);
});

test("a drag let go outside the window ends at the first move after it", () => {
  const { node } = page();
  const seen = counting(node);
  down(node);
  move(20);
  move(400, 0, 0);
  move(420, 0, 0);
  assert.deepEqual(seen.end, [true]);
  assert.equal(seen.move, 0);
});

test("the click a drag leaves is swallowed once; the next is a click", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { node, child } = page();
  const seen = counting(node);
  down(node);
  move(20);
  up();
  assert.equal(click(node, child), false);
  assert.equal(click(node), true);
  assert.equal(seen.click, 1);
});

test("a long drag's click is still the drag's; one after the release's is a click", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { node } = page();
  counting(node);
  down(node);
  move(20);
  // Carried for two seconds before it's let go.
  t.mock.timers.tick(2000);
  up();
  assert.equal(click(node), false);
  down(node);
  move(20);
  up();
  // No click came with that release (it was on something else).
  t.mock.timers.tick(SWALLOW_MS + 1);
  assert.equal(click(node), true);
});

test("a press without a drag leaves its click alone", () => {
  const { node } = page();
  const seen = counting(node);
  down(node);
  move(1);
  up();
  assert.deepEqual(seen.end, [false]);
  assert.equal(click(node), true);
  assert.equal(seen.click, 1);
});

test("a press the node refuses, or of another button, is left alone", () => {
  const { node } = page();
  const seen = counting(node, () => false);
  down(node);
  move(50);
  assert.equal(seen.start, 0);
  const other = page();
  const seen2 = counting(other.node);
  other.node.dispatchEvent(event("pointerdown", { button: 2, clientX: 0, clientY: 0 }));
  move(50, 0, 2);
  assert.equal(seen2.start, 0);
});

test("a press goes on to its release when its node goes", () => {
  const { node } = page();
  const seen = { end: /** @type {boolean[]} */ ([]) };
  const action = pressToDrag(node, { start() {}, end: (d) => seen.end.push(d) });
  down(node);
  move(20);
  action.destroy();
  up();
  assert.deepEqual(seen.end, [true]);
});
