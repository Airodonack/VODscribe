// A press that becomes a drag (pressToDrag.ts), driven with synthetic events:
// node --test apps/desktop/src/lib/pressToDrag.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { DRAG_SLOP, SWALLOW_MS, holds, pressToDrag } from "./pressToDrag.ts";

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
  move(50, 0, 1);
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

// pressToDrag.ts: "start: It moved DRAG_SLOP with the button held: the drag begins"; `Pressed`: "Where a press began."
test("the drag starts with the pointer's event and where the press began", () => {
  const { node } = page();
  /** @type {{ x: number, y: number }[]} */
  const from = [];
  /** @type {number[]} */
  const at = [];
  pressToDrag(node, {
    start: (e, f) => {
      from.push(f);
      at.push(e.clientX);
    },
  });
  down(node, 7, 9);
  move(30, 9);
  up();
  assert.deepEqual(from, [{ x: 7, y: 9 }]);
  assert.deepEqual(at, [30]);
});

// pressToDrag.ts: "past the slop" is a distance, not a distance along one axis.
test("a diagonal move counts by its length", () => {
  const { node } = page();
  const seen = counting(node);
  down(node, 0, 0);
  move(3, 3);
  assert.equal(seen.start, 1);
});

// pressToDrag.ts: "The window's listeners are on only while a press is held".
test("after a press is over, moves are no one's", () => {
  const { node } = page();
  const seen = counting(node);
  down(node);
  move(20);
  up();
  move(60);
  move(90);
  assert.equal(seen.move, 0);
  assert.deepEqual(seen.end, [true]);
});

// pressToDrag.ts `PressToDrag.end`: "or cancelled".
test("a cancelled press is over, dragged or not", () => {
  const { node } = page();
  const seen = counting(node);
  down(node);
  window.dispatchEvent(event("pointercancel", { button: 2, buttons: 0 }));
  down(node);
  move(20);
  window.dispatchEvent(event("pointercancel", { button: 2, buttons: 0 }));
  assert.deepEqual(seen.end, [false, true]);
});

// pressToDrag.ts: a release of another button isn't the press's.
test("another button let go doesn't end the press", () => {
  const { node } = page();
  const seen = counting(node);
  down(node);
  window.dispatchEvent(event("pointerup", { button: 2, buttons: 1 }));
  assert.deepEqual(seen.end, []);
  move(20);
  assert.equal(seen.start, 1);
});

// pressToDrag.ts `onDown`: one press at a time.
test("a second press while one is held starts nothing of its own", () => {
  const { node } = page();
  const seen = counting(node);
  down(node, 0, 0);
  // Pressed again, near where the pointer will be: not a new press, so the drag is of the first.
  down(node, 18, 0);
  move(20, 0);
  assert.equal(seen.start, 1);
  up();
  assert.deepEqual(seen.end, [true]);
});

// pressToDrag.ts `PressToDrag.press`: "false if it's not one that can be dragged (the press is then left alone)".
test("a press the node says nothing about is a press", () => {
  const { node } = page();
  const seen = counting(node, /** @type {() => boolean} */ (/** @type {unknown} */ (() => undefined)));
  down(node);
  move(20);
  assert.equal(seen.start, 1);
});

// pressToDrag.ts: "a click on the node that isn't the one a drag's release leaves".
test("a click on the node with no press is a click", () => {
  const { node } = page();
  const seen = counting(node);
  assert.equal(click(node), true);
  assert.equal(seen.click, 1);
});

// pressToDrag.ts: the drag's click is taken only if it's on the node.
test("a click elsewhere after a drag isn't the drag's, and leaves the node's next click alone", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { node } = page();
  const seen = counting(node);
  const other = new EventTarget();
  down(node);
  move(20);
  up();
  assert.equal(click(node, other), true);
  t.mock.timers.tick(SWALLOW_MS + 1);
  assert.equal(click(node), true);
  assert.equal(seen.click, 2);
});

// pressToDrag.ts: "a press ... the Svelte action": `update` hands over the new options.
test("update hands the press the new options", () => {
  const { node } = page();
  const first = { start: 0 };
  const second = { start: 0 };
  const action = pressToDrag(node, { start: () => first.start++ });
  action.update({ start: () => second.start++ });
  down(node);
  move(20);
  assert.deepEqual([first.start, second.start], [0, 1]);
});

// pressToDrag.ts `holds`: "The bit in PointerEvent.buttons of the button PointerEvent.button numbers."
test("a button is held when its own bit is", () => {
  assert.equal(holds({ buttons: 1 }), true);
  assert.equal(holds({ buttons: 2 }), false);
  assert.equal(holds({ buttons: 2 }, 2), true);
  assert.equal(holds({ buttons: 4 }, 1), true);
  assert.equal(holds({ buttons: 8 }, 3), true);
  assert.equal(holds({ buttons: 16 }, 4), true);
  assert.equal(holds({ buttons: 0 }, 0), false);
  assert.equal(holds({ buttons: 31 }, 9), false);
});

// pressToDrag.ts: "The window's listeners are on only while a press is held, and a little after a drag, for its click"; the click "taken before anything under the window sees it".
test("the window's listeners are taken on in the capture phase and taken off when the press is over", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { node } = page();
  /** @type {string[]} */
  const log = [];
  for (const how of /** @type {const} */ (["addEventListener", "removeEventListener"])) {
    const was = window[how].bind(window);
    window[how] = /** @type {any} */ (
      (/** @type {string} */ type, /** @type {any} */ fn, /** @type {any} */ capture) => {
        log.push(`${how === "addEventListener" ? "+" : "-"}${type}${capture === true ? " capture" : ""}`);
        was(type, fn, capture);
      }
    );
  }
  counting(node);
  down(node);
  assert.deepEqual(log, ["+pointermove capture", "+pointerup capture", "+pointercancel capture"]);
  move(20);
  assert.equal(log.at(-1), "+click capture");
  log.length = 0;
  up();
  assert.deepEqual(log, ["-pointermove capture", "-pointerup capture", "-pointercancel capture"]);
  log.length = 0;
  t.mock.timers.tick(SWALLOW_MS + 1);
  assert.deepEqual(log, ["-click capture"]);
});

// pressToDrag.ts: a press with no drag leaves no click to wait for; the wait ends with clearTimeout's call.
test("a press without a drag sets no timer for a click; a drag's is cleared once it's over", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { node } = page();
  counting(node);
  let timers = 0;
  const set = globalThis.setTimeout;
  globalThis.setTimeout = /** @type {any} */ (
    (/** @type {any[]} */ ...args) => (timers++, /** @type {any} */ (set)(...args))
  );
  const clear = globalThis.clearTimeout;
  let cleared = 0;
  globalThis.clearTimeout = /** @type {any} */ (
    (/** @type {any[]} */ ...args) => (cleared++, /** @type {any} */ (clear)(...args))
  );
  t.after(() => {
    globalThis.setTimeout = set;
    globalThis.clearTimeout = clear;
  });
  down(node);
  up();
  assert.equal(timers, 0);
  down(node);
  move(20);
  up();
  assert.equal(timers, 1);
  const before = cleared;
  click(node);
  assert.ok(cleared > before, "the wait is cleared once the click came");
});

// pressToDrag.ts: a drag's click is stopped from reaching anything under the window.
test("the drag's click is stopped, not only prevented", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { node, child } = page();
  counting(node);
  down(node);
  move(20);
  up();
  const e = event("click");
  Object.defineProperty(e, "target", { value: child });
  const stopped = { propagation: 0, immediate: 0 };
  e.stopPropagation = () => void stopped.propagation++;
  e.stopImmediatePropagation = () => void stopped.immediate++;
  window.dispatchEvent(e);
  assert.deepEqual(stopped, { propagation: 1, immediate: 1 });
});

// pressToDrag.ts: a release's timer from the drag before doesn't end the click of the drag now.
test("a drag begun before the last one's wait is over keeps its own click", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { node } = page();
  counting(node);
  down(node);
  move(20);
  up();
  down(node);
  move(20);
  t.mock.timers.tick(SWALLOW_MS + 1);
  up();
  assert.equal(click(node), false);
});

// pressToDrag.ts: every handler is optional but start.
test("a press with only a start handler drags, ends and clicks without one", () => {
  const { node } = page();
  let started = 0;
  pressToDrag(node, { start: () => started++ });
  down(node);
  move(20);
  move(30);
  up();
  assert.equal(started, 1);
  assert.equal(click(node), false);
  assert.equal(click(node), true);
});

// pressToDrag.ts `destroy`: the node is let go of.
test("a node destroyed takes no more presses or clicks", () => {
  const { node } = page();
  const seen = { start: 0, click: 0 };
  const action = pressToDrag(node, { start: () => seen.start++, click: () => seen.click++ });
  action.destroy();
  down(node);
  move(20);
  click(node);
  assert.deepEqual(seen, { start: 0, click: 0 });
});
