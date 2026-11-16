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

// pick.ts header: "With Ctrl too, the run is added to what was picked"; a Shift-click run starts at the anchor, here the item itself.
test("a Ctrl+Shift-click with nothing picked picks just that one, and it's the anchor", () => {
  assert.deepEqual(at(nothing, "c", "addRange"), { items: ["c"], anchor: "c" });
});

// pick.ts header: "With Ctrl too, the run is added to what was picked" (`click`, how "addRange"); CHANGELOG 0.7.0: "Ctrl+Shift-click adds that run".
test("a Ctrl+Shift-click adds only what isn't picked already, once, in the order picked then on screen", () => {
  const set = { items: ["d", "b"], anchor: "b" };
  assert.deepEqual(at(set, "d", "addRange"), { items: ["d", "b", "c"], anchor: "b" });
});

// pick.ts header: "A Shift-click: the run from the anchor to here"; the run from the anchor to the anchor is that one.
test("a Shift-click on the anchor itself picks it alone", () => {
  assert.deepEqual(at({ items: ["a", "c"], anchor: "b" }, "b", "range"), { items: ["b"], anchor: "b" });
});

// pick.ts header: "with Shift, the run from the anchor to it, the anchor staying"; CHANGELOG 0.7.0: "with Shift held to pick the run to it".
test("Shift with an arrow key starts its run at the item the arrow left when nothing is the anchor", () => {
  const got = step({ items: [], anchor: null }, "b", 1, true, order, same);
  assert.deepEqual(got, { set: { items: ["b", "c"], anchor: "b" }, to: "c" });
});

// pick.ts `step`: "null at an end".
test("an arrow key past an end with Shift goes nowhere", () => {
  assert.equal(step(resting("a"), "a", -1, true, order, same), null);
  assert.equal(step(resting("e"), "e", 1, true, order, same), null);
});

// pick.ts header: "A Shift-click: the run from the anchor to here, ... either way" and the arrow keys' run likewise (CHANGELOG 0.7.0: "Shift-click picks the run ... in the library, the clip bar and on the timeline alike").
test("an arrow key's run goes back past the anchor, the other way", () => {
  const got = step({ items: ["c", "d"], anchor: "c" }, "d", -1, true, order, same);
  assert.deepEqual(got?.set, { items: ["c"], anchor: "c" });
  const past = step(/** @type {NonNullable<typeof got>} */ (got).set, "c", -1, true, order, same);
  assert.deepEqual(past?.set, { items: ["b", "c"], anchor: "c" });
});

// pick.ts `resting`: "The set at rest: the open item alone, if there's one."
test("the open item at rest is the anchor, and nothing open is no anchor", () => {
  assert.deepEqual(resting("b"), { items: ["b"], anchor: "b" });
  assert.deepEqual(nothing, { items: [], anchor: null });
});

// pick.ts `Same`: items are compared by the function given (selection.svelte.ts compares stretches by their times, clips by id).
test("things are told apart by the comparison given, not by identity", () => {
  const byId = (/** @type {{ id: number }} */ a, /** @type {{ id: number }} */ b) => a.id === b.id;
  const rows = [{ id: 1 }, { id: 2 }, { id: 3 }];
  const got = click({ items: [{ id: 1 }], anchor: { id: 1 } }, { id: 3 }, "range", rows, byId);
  assert.deepEqual(got.items, rows);
  assert.deepEqual(click(got, { id: 2 }, "toggle", rows, byId).items, [{ id: 1 }, { id: 3 }]);
});

// pick.ts `step`: "the next item (or the one before)", from the first item too.
test("an arrow key from the first item goes to the second, and from the last to the one before", () => {
  assert.deepEqual(step(resting("a"), "a", 1, false, order, same)?.to, "b");
  assert.deepEqual(step(resting("e"), "e", -1, false, order, same)?.to, "d");
  assert.deepEqual(step(resting("a"), "a", 1, true, order, same)?.set, { items: ["a", "b"], anchor: "a" });
});
