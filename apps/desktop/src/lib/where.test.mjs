// Where Ctrl+A and Ctrl+V act (where.ts): node --test apps/desktop/src/lib/where.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";

const { where } = await import("./where.ts");

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
