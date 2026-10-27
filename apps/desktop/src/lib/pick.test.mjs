// What a click or a key picks, as in a file manager (pick.ts): node --test apps/desktop/src/lib/pick.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { click, howOf, picking, resting, step } from "./pick.ts";

/** @param {string} a @param {string} b */
const same = (a, b) => a === b;
const order = ["a", "b", "c", "d", "e"];
/**
 * @param {import("./pick").Picked<string>} set @param {string} item
 * @param {import("./pick").How} how
 */
const at = (set, item, how) => click(set, item, how, order, same);
/** Nothing open, nothing picked. @type {import("./pick").Picked<string>} */
const nothing = resting(/** @type {string | null} */ (null));

test("the keys held say how a click picks", () => {
  assert.equal(howOf(false, false), "plain");
  assert.equal(howOf(true, false), "toggle");
  assert.equal(howOf(false, true), "range");
  assert.equal(howOf(true, true), "addRange");
});

test("a plain click picks one alone and is the anchor, letting go of the rest", () => {
  const set = { items: ["a", "c", "d"], anchor: "d" };
  assert.deepEqual(at(set, "b", "plain"), { items: ["b"], anchor: "b" });
});

test("a plain click then a Ctrl-click picks both, in that order", () => {
  const one = at(nothing, "b", "plain");
  assert.deepEqual(at(one, "d", "toggle"), { items: ["b", "d"], anchor: "d" });
});

test("a Ctrl-click on the open item at rest takes it out", () => {
  assert.deepEqual(at(resting("b"), "b", "toggle"), { items: [], anchor: "b" });
});

test("a Ctrl-click on one picked takes it out and moves the anchor there", () => {
  assert.deepEqual(at({ items: ["a", "c"], anchor: "c" }, "a", "toggle"), { items: ["c"], anchor: "a" });
});

test("a Shift-click picks the run from the anchor, either way, in place of the rest; the anchor stays", () => {
  const set = { items: ["e"], anchor: "b" };
  assert.deepEqual(at(set, "d", "range"), { items: ["b", "c", "d"], anchor: "b" });
  assert.deepEqual(at(at(set, "d", "range"), "a", "range"), { items: ["a", "b"], anchor: "b" });
});

test("a Shift-click from the open item at rest runs from it", () => {
  assert.deepEqual(at(resting("b"), "d", "range"), { items: ["b", "c", "d"], anchor: "b" });
});

test("a Shift-click with no anchor picks just that one, and it's the anchor", () => {
  assert.deepEqual(at(nothing, "c", "range"), { items: ["c"], anchor: "c" });
});

test("a Ctrl+Shift-click adds the run to what's picked", () => {
  const set = { items: ["a", "e"], anchor: "c" };
  assert.deepEqual(at(set, "d", "addRange"), { items: ["a", "e", "c", "d"], anchor: "c" });
});

test("a click on something not on screen changes nothing for a run", () => {
  const set = { items: ["a"], anchor: "a" };
  assert.equal(at(set, "z", "range"), set);
});

test("the open item alone, or nothing, isn't several picked", () => {
  assert.equal(picking(resting("b"), "b", same), false);
  assert.equal(picking(nothing, null, same), false);
  assert.equal(picking({ items: ["c"], anchor: "c" }, "b", same), true);
  assert.equal(picking({ items: ["b", "c"], anchor: "c" }, "b", same), true);
  assert.equal(picking({ items: ["b"], anchor: "b" }, null, same), true);
});

test("an arrow key goes to the next or the one before, alone; at an end, nowhere", () => {
  assert.deepEqual(step(resting("b"), "b", 1, false, order, same), { set: { items: ["c"], anchor: "c" }, to: "c" });
  assert.deepEqual(step(resting("b"), "b", -1, false, order, same), { set: { items: ["a"], anchor: "a" }, to: "a" });
  assert.equal(step(resting("e"), "e", 1, false, order, same), null);
});

test("Shift with an arrow key picks the run from the anchor, which stays", () => {
  const once = step(resting("b"), "b", 1, true, order, same);
  assert.deepEqual(once, { set: { items: ["b", "c"], anchor: "b" }, to: "c" });
  const twice = step(once.set, "c", 1, true, order, same);
  assert.deepEqual(twice, { set: { items: ["b", "c", "d"], anchor: "b" }, to: "d" });
  const back = step(twice.set, "d", -1, true, order, same);
  assert.deepEqual(back, { set: { items: ["b", "c"], anchor: "b" }, to: "c" });
});
