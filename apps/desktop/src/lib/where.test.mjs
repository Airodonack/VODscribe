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

// where.ts: "'Over' is where the pointer is"; the library's pane is on the left of the window.
test("the pane's right edge is the main side's; the bar's left and top edges are the bar's, its right and bottom the timeline's", () => {
  assert.equal(where(page({ pointer: { x: 299, y: 630 } })), "library");
  assert.equal(where(page({ pointer: { x: 300, y: 630 } })), "clips");
  assert.equal(where(page({ pointer: { x: 800, y: 600 } })), "clips");
  assert.equal(where(page({ pointer: { x: 800, y: 660 } })), "timeline");
  assert.equal(where(page({ pointer: { x: 1400, y: 630 } })), "timeline");
});

// where.ts: "over the library's pane, its list"; CHANGELOG 0.7.1: "over the library its list".
test("over the library's pane, whatever its height, its list; with no pane the pointer never says library", () => {
  assert.equal(where(page({ pointer: { x: 100, y: 620 }, focus: "clips" })), "library");
  assert.equal(where(page({ pane: null, pointer: { x: 100, y: 620 }, bar: { ...bar, left: 0 } })), "clips");
});

// where.ts: "before the pointer has moved, where the focus is".
test("with a pointer, the focus is not asked", () => {
  assert.equal(where(page({ pointer: { x: 100, y: 10 }, focus: "other" })), "library");
  assert.equal(where(page({ pointer: { x: 800, y: 10 }, focus: "library" })), "timeline");
});

// where.ts: "(a clip open in the editor has none): its timeline" and "(the main side with nothing open: the library's page)".
test("the focus in the clip bar, with no bar and nothing open, is nowhere", () => {
  assert.equal(where(page({ bar: null, timeline: false, focus: "clips" })), null);
  assert.equal(where(page({ bar: null, timeline: false, pointer: { x: 800, y: 630 } })), null);
});

// where.ts `insertionPoint`: "after the stretch selected, after the last stretch picked ..., at the mark selected, else at the playhead".
test("on a timeline the stretch selected beats what's picked, and what's picked beats the mark", () => {
  const picked = [segment(5, 6)];
  assert.deepEqual(insertionPoint("timeline", selected({ stretch: { start: 1, end: 2 }, picked, mark: 8 })), { at: 2 });
  assert.deepEqual(insertionPoint("timeline", selected({ picked, mark: 8 })), { at: 6 });
  // Clips picked beside the stretches don't count for the timeline.
  assert.deepEqual(insertionPoint("timeline", selected({ picked: [clip(3), segment(5, 6)] })), { at: 6 });
});

// where.ts `insertionPoint`: "a target only when nothing is selected" (the playhead).
test("a mark at the start of the video is a place, not nothing", () => {
  assert.deepEqual(insertionPoint("timeline", selected({ mark: 0, playhead: 10 })), { at: 0 });
  assert.deepEqual(insertionPoint("timeline", selected({ stretch: { start: 0, end: 0 }, playhead: 10 })), { at: 0 });
});

// where.ts `insertionPoint`: "On a clip bar, after the last clip picked (in the bar's order)".
test("on a clip bar a clip picked first on the bar counts as the first, not as none", () => {
  assert.deepEqual(insertionPoint("bar", selected({ picked: [clip(7)] }), [7, 8]), { after: 7 });
  assert.deepEqual(insertionPoint("bar", selected({ picked: [clip(7), clip(99)] }), [7, 8]), { after: 7 });
  assert.deepEqual(insertionPoint("bar", selected({ picked: [clip(7)] })), { after: null });
});

// where.ts `where`: "over the clip bar, its clips": the bar's own box, not what is left of it.
test("left of the clip bar, when the pane ends before it, is the timeline", () => {
  const apart = { ...bar, left: 400 };
  assert.equal(where(page({ bar: apart, pointer: { x: 350, y: 630 } })), "timeline");
  assert.equal(where(page({ bar: apart, pointer: { x: 400, y: 630 } })), "clips");
});
