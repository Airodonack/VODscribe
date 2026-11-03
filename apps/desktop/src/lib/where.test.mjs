// Where Ctrl+A and Ctrl+V act, and where in it a paste goes (where.ts):
// node --test apps/desktop/src/lib/where.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";

const { insertionPoint, where } = await import("./where.ts");

const pane = { left: 0, right: 300, top: 0, bottom: 900 };
const bar = { left: 300, right: 1400, top: 600, bottom: 660 };
/** @param {Partial<import("./where.ts").Page>} [over] @returns {import("./where.ts").Page} */
const page = (over) => ({ pointer: null, pane, bar, focus: "other", timeline: true, ...over });

test("over the clip bar, its clips; over the library's pane, its list; elsewhere, the timeline", () => {
  assert.equal(where(page({ pointer: { x: 800, y: 630 } })), "clips");
  assert.equal(where(page({ pointer: { x: 100, y: 630 } })), "library");
  // The player, the timeline, the transcript: the main side, not the bar.
  assert.equal(where(page({ pointer: { x: 800, y: 300 } })), "timeline");
  assert.equal(where(page({ pointer: { x: 800, y: 700 } })), "timeline");
  // The pointer goes by, whatever has the focus.
  assert.equal(where(page({ pointer: { x: 800, y: 300 }, focus: "clips" })), "timeline");
  assert.equal(where(page({ pointer: { x: 800, y: 630 }, focus: "library" })), "clips");
});

test("before the pointer has moved, where the focus is", () => {
  assert.equal(where(page({ focus: "library" })), "library");
  assert.equal(where(page({ focus: "clips" })), "clips");
  assert.equal(where(page({ focus: "other" })), "timeline");
});

test("nothing open on the main side: nothing there to select or paste into", () => {
  const library = { bar: null, timeline: false };
  assert.equal(where(page({ ...library, pointer: { x: 800, y: 300 } })), null);
  assert.equal(where(page({ ...library })), null);
  assert.equal(where(page({ ...library, pointer: { x: 100, y: 300 } })), "library");
  // No clip bar (a clip open in the editor has none): its timeline.
  assert.equal(where(page({ bar: null, focus: "clips" })), "timeline");
  // The library hidden: the main side is all of the window.
  assert.equal(where(page({ pane: null, pointer: { x: 100, y: 300 } })), "timeline");
});

/** Nothing selected, the playhead at 10 s. @param {Partial<import("./where.ts").Selected>} [over] */
const selected = (over) => ({ stretch: null, picked: [], mark: null, playhead: 10, ...over });
/** @param {number} start @param {number} end */
const segment = (start, end) => /** @type {const} */ ({ kind: "segment", start, end });
/** @param {number} id */
const clip = (id) => /** @type {const} */ ({ kind: "clip", id });

test("on a timeline a paste goes after what's selected, at the playhead only when nothing is", () => {
  assert.deepEqual(insertionPoint("timeline", selected()), { at: 10 });
  assert.deepEqual(insertionPoint("timeline", selected({ stretch: { start: 2, end: 4 } })), { at: 4 });
  // A stretch picked (Alt-click): after it, not at the playhead.
  assert.deepEqual(insertionPoint("timeline", selected({ picked: [segment(1, 2)] })), { at: 2 });
  // Several, picked out of the timeline's order: after the last on the timeline.
  assert.deepEqual(insertionPoint("timeline", selected({ picked: [segment(5, 6), segment(1, 2)] })), { at: 6 });
  // A mark selected: at its moment.
  assert.deepEqual(insertionPoint("timeline", selected({ mark: 3 })), { at: 3 });
  // Clips picked on the bar aren't on the timeline: the playhead.
  assert.deepEqual(insertionPoint("timeline", selected({ picked: [clip(7)] })), { at: 10 });
});

test("on a clip bar a paste goes after the clip picked, else after them all", () => {
  const bar = [7, 8, 9];
  assert.deepEqual(insertionPoint("bar", selected(), bar), { after: null });
  assert.deepEqual(insertionPoint("bar", selected({ picked: [clip(8)] }), bar), { after: 8 });
  // Several, picked out of the bar's order: after the last on the bar.
  assert.deepEqual(insertionPoint("bar", selected({ picked: [clip(9), clip(7)] }), bar), { after: 9 });
  // A clip picked that isn't on the bar shown, a stretch, a mark: after them all.
  assert.deepEqual(insertionPoint("bar", selected({ picked: [clip(4)] }), bar), { after: null });
  assert.deepEqual(insertionPoint("bar", selected({ stretch: { start: 2, end: 4 }, mark: 3 }), bar), { after: null });
  assert.deepEqual(insertionPoint("bar", selected({ picked: [segment(1, 2)] }), bar), { after: null });
});
