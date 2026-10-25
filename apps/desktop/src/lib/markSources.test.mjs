// Whose marks are shown (markSources.ts): node --test apps/desktop/src/lib/markSources.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { BOTH, shownMarks } from "./markSources.ts";

/** @type {{ id: number; kind: "cut" | "chapter" | "highlight"; source?: string }[]} */
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
