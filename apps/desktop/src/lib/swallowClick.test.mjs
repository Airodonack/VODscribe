// The press that closed a menu, and the rest of its click, kept from the
// page (swallowClick.ts), driven with synthetic events:
// node --test apps/desktop/src/lib/swallowClick.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";

import { swallowClick } from "./swallowClick.ts";

/** @param {string} type */
const event = (type) => new Event(type, { cancelable: true });

/** A page: what reaches it past the swallow (listened for after it), by type. */
function page() {
  // Node's EventTarget doesn't take a listener added with `true` (capture)
  // off again when asked with `true`; a page's has one phase here anyway.
  const target = new EventTarget();
  const doc = {
    /** @param {string} type @param {EventListener} f */
    addEventListener: (type, f) => target.addEventListener(type, f),
    /** @param {string} type @param {EventListener} f */
    removeEventListener: (type, f) => target.removeEventListener(type, f),
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
  return { doc, reached, listen, send };
}

/** Swallows a press on `doc`, as the menu does once it's closed by it.
 *  @param {ReturnType<typeof page>} p */
function pressed(p) {
  const down = event("pointerdown");
  swallowClick(down, p.doc);
  p.listen();
  return down;
}

test("the press and its click are kept from the page; the next click reaches it", () => {
  const p = page();
  const down = pressed(p);
  assert.equal(down.defaultPrevented, true);
  p.send("mousedown", "pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, []);
  p.send("pointerdown", "mousedown", "pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]);
});

test("let go away from where it was pressed, no click: what comes next reaches the page", async () => {
  const p = page();
  pressed(p);
  p.send("mousedown", "pointerup", "mouseup");
  await sleep(0);
  p.send("click");
  assert.deepEqual(p.reached, ["click"]);
});

test("a press whose release never comes (cancelled) leaves the next click alone", () => {
  const p = page();
  pressed(p);
  p.send("mousedown", "pointercancel");
  p.send("pointerdown", "mousedown", "pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]);
});

test("a press whose release is lost (a Control-click on a Mac, its menu up) leaves the next click alone", () => {
  const p = page();
  pressed(p);
  p.send("mousedown", "contextmenu");
  p.send("pointerdown", "mousedown", "pointerup", "mouseup", "click");
  assert.deepEqual(p.reached, ["contextmenu", "pointerdown", "mousedown", "pointerup", "mouseup", "click"]);
});
