// The press that closed a menu, and the rest of its click, kept from the
// page (swallowClick.ts), driven with synthetic events:
// node --test apps/desktop/src/lib/swallowClick.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";

import { swallowClick } from "./swallowClick.ts";

/** @param {string} type */
const event = (type) => new Event(type, { cancelable: true });

/** A page: what reaches it past the swallow (listened for after it), by type.
 *  Its listeners are counted, each with the capture flag it was added with, and
 *  one is taken off only with the same flag, as the DOM's are. */
function page() {
  // Node's EventTarget dispatches in one phase, so the capture flag is kept
  // here, not by it: a listener added with `true` and asked off with `false`
  // stays, as in a browser.
  const target = new EventTarget();
  /** @type {{ type: string, f: EventListener, capture: boolean }[]} */
  const listeners = [];
  const doc = {
    /** @param {string} type @param {EventListener} f @param {boolean} [capture] */
    addEventListener(type, f, capture = false) {
      listeners.push({ type, f, capture });
      target.addEventListener(type, f);
    },
    /** @param {string} type @param {EventListener} f @param {boolean} [capture] */
    removeEventListener(type, f, capture = false) {
      const at = listeners.findIndex((l) => l.type === type && l.f === f && l.capture === capture);
      if (at < 0) return;
      listeners.splice(at, 1);
      if (!listeners.some((l) => l.type === type && l.f === f)) target.removeEventListener(type, f);
    },
    /** @param {Event} e */
    dispatchEvent: (e) => target.dispatchEvent(e),
  };
  /** @type {string[]} */
  const reached = [];
  const listen = () => {
    for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "click", "contextmenu"])
      doc.addEventListener(type, () => reached.push(type));
  };
  /** @param {string[]} types */
  const send = (...types) => types.forEach((type) => doc.dispatchEvent(event(type)));
  return { doc, reached, listen, send, count: () => listeners.length };
}

/** Swallows a press on `doc`, as the menu does once it's closed by it.
 *  @param {ReturnType<typeof page>} p */
function pressed(p) {
  const down = event("pointerdown");
  swallowClick(down, p.doc);
  p.listen();
  return down;
}

// Source: swallowClick.ts header; 0.7.1 Fixes ("Clicking a row closes its menu.").
test("the press and its click are kept from the page; the next click reaches it", () => {
  const p = page();
  const down = pressed(p);
  assert.equal(down.defaultPrevented, true);
  p.send("mousedown", "pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, []);
  p.send("pointerdown", "mousedown", "pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]);
});

// Source: swallowClick.ts header ("once the release has come without one").
test("let go away from where it was pressed, no click: what comes next reaches the page", async () => {
  const p = page();
  pressed(p);
  p.send("mousedown", "pointerup", "mouseup");
  await sleep(0);
  p.send("click");
  assert.deepEqual(p.reached, ["click"]);
});

// Source: swallowClick.ts header ("the press cancelled").
test("a press whose release never comes (cancelled) leaves the next click alone", () => {
  const p = page();
  pressed(p);
  p.send("mousedown", "pointercancel");
  p.send("pointerdown", "mousedown", "pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]);
});

// Source: swallowClick.ts header ("a new press begun (on a Mac, a Control-click's release is lost").
test("a press whose release is lost (a Control-click on a Mac, its menu up) leaves the next click alone", () => {
  const p = page();
  pressed(p);
  p.send("mousedown", "contextmenu");
  p.send("pointerdown", "mousedown", "pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, ["contextmenu", "pointerdown", "mousedown", "pointerup", "mouseup", "click"]);
});

// Source for the tests below: swallowClick.ts's header ("A press that closed a
// menu ... does nothing else: it and the rest of its click are kept from the
// page ... It ends with the click, or once the release has come without one,
// or when the release won't come"); 0.7.1 Fixes ("Clicking a row closes its
// menu.").

test("the click's own default is prevented too, so a link or a button under it does nothing", () => {
  const p = page();
  pressed(p);
  const click = event("click");
  p.doc.dispatchEvent(click);
  assert.equal(click.defaultPrevented, true);
});

test("once the click has come the swallowing ends: the next press is the page's, with its default", () => {
  const p = page();
  pressed(p);
  p.send("mousedown", "pointerup", "mouseup", "click");
  const next = event("mousedown");
  p.doc.dispatchEvent(next);
  assert.equal(next.defaultPrevented, false);
  assert.deepEqual(p.reached, ["mousedown"]);
});

test("a press and its click that have come leave no listener on the page", () => {
  const p = page();
  pressed(p);
  const during = p.count();
  assert.ok(during > 0);
  p.send("mousedown", "pointerup", "mouseup", "click");
  // The page's own six, listened for after the swallow.
  assert.equal(p.count(), 6);
});

test("a press whose release never comes leaves no listener on the page", () => {
  const p = page();
  pressed(p);
  p.send("pointercancel");
  assert.equal(p.count(), 6);
});

test("a cancelled press, the next thing to come a click, leaves that click alone", () => {
  const p = page();
  pressed(p);
  p.send("pointercancel", "click");
  assert.deepEqual(p.reached, ["click"]);
});

test("a press held still for a moment is kept from the page until it's let go", async () => {
  const p = page();
  pressed(p);
  p.send("mousedown");
  await sleep(0);
  p.send("pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, []);
});

test("a release seen as a pointer's alone, or as a mouse's alone, lets the next event through", async () => {
  for (const release of ["pointerup", "mouseup"]) {
    const p = page();
    pressed(p);
    p.send("mousedown", release);
    await sleep(0);
    p.send("click");
    assert.deepEqual(p.reached, ["click"], release);
  }
});
