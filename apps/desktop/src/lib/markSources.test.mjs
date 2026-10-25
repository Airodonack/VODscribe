// Whose marks are shown (markSources.ts): node --test apps/desktop/src/lib/markSources.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { BOTH, changed, shownMarks, sourceOf } from "./markSources.ts";

/** @type {{ id: number; kind: "cut" | "chapter" | "highlight"; source?: string; text?: string; start?: number }[]} */
const marks = [
  { id: 1, kind: "chapter", source: "annotations" },
  { id: 2, kind: "cut", source: "user" },
  { id: 3, kind: "highlight", source: "user" },
  { id: 4, kind: "highlight", source: "annotations" },
  // The demo's visitor's own: no source.
  { id: 5, kind: "chapter" },
];
/** @param {{ id: number }[]} list */
const ids = (list) => list.map((m) => m.id);

test("both shown: every mark", () => {
  assert.deepEqual(ids(shownMarks(marks, BOTH)), [1, 2, 3, 4, 5]);
});

test("one kind hidden: its chapters and highlights go, the cuts stay", () => {
  assert.deepEqual(ids(shownMarks(marks, { user: false, imported: true })), [1, 2, 4]);
  assert.deepEqual(ids(shownMarks(marks, { user: true, imported: false })), [2, 3, 5]);
  assert.deepEqual(ids(shownMarks(marks, { user: false, imported: false })), [2]);
});

test("an imported mark the user renames or moves is theirs: shown by Your marks from then on", () => {
  const renamed = changed(marks[0], { text: "Mine now" });
  assert.deepEqual(renamed, { id: 1, kind: "chapter", source: "user", text: "Mine now" });
  assert.equal(sourceOf(renamed), "user");
  assert.deepEqual(ids(shownMarks([renamed], { user: true, imported: false })), [1]);
  assert.deepEqual(ids(shownMarks([changed(marks[3], { start: 9 })], { user: false, imported: true })), []);
  // The user's own, and the demo visitor's (no source), stay as they are.
  assert.deepEqual(changed(marks[2], { start: 9 }), { id: 3, kind: "highlight", source: "user", start: 9 });
  assert.deepEqual(changed(marks[4], { text: "x" }), { id: 5, kind: "chapter", text: "x" });
});
