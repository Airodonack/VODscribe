// Whose marks are shown (markSources.ts): node --test apps/desktop/src/lib/markSources.test.mjs
// Sources: markSources.ts's doc comments; CHANGELOG.md 0.7.0 ("Your and imported marks can be hidden.", "Newer
// annotations keep your own changes.").
import assert from "node:assert/strict";
import { test } from "node:test";
import { BOTH, changed, isImported, shownMarks, sourceOf } from "./markSources.ts";

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

/** markSources.ts `BOTH` ("Both kinds shown: how it starts"); CHANGELOG 0.7.0 "Your and imported marks can be hidden." ("both start on"). */
test("both kinds are shown to begin with, every mark in its order", () => {
  assert.deepEqual(BOTH, { user: true, imported: true });
  assert.deepEqual(ids(shownMarks(marks, BOTH)), [1, 2, 3, 4, 5]);
});

/** CHANGELOG 0.7.0 "Your and imported marks can be hidden." ("the chapters and highlights you made and those from annotations"). */
test("with imported marks hidden, only the user's chapters and highlights show", () => {
  assert.deepEqual(ids(shownMarks(marks, { user: true, imported: false })), [2, 3, 5]);
});

/** CHANGELOG 0.7.0 "Your and imported marks can be hidden.". */
test("with the user's marks hidden, only the imported chapters and highlights show", () => {
  assert.deepEqual(ids(shownMarks(marks, { user: false, imported: true })), [1, 2, 4]);
});

/** markSources.ts header ("Cuts are always shown: they make the clips, whoever's the marks around them"). */
test("cuts are always shown, even with both kinds hidden", () => {
  assert.deepEqual(ids(shownMarks(marks, { user: false, imported: false })), [2]);
});

/** markSources.ts `isImported` ("Whether `mark` came with an annotations import (the demo's own have no source: the visitor's)"), `sourceOf`. */
test("a mark with the annotations' source is imported; one with no source is the visitor's own", () => {
  assert.ok(isImported(marks[0]));
  assert.ok(!isImported(marks[1]));
  assert.ok(!isImported(marks[4]));
  assert.deepEqual(marks.map(sourceOf), ["imported", "user", "user", "imported", "user"]);
});

/** markSources.ts `changed` ("one an import brought is theirs from then on"); CHANGELOG 0.7.0 "Newer annotations keep your own changes." ("an imported chapter or highlight you renamed or moved"). */
test("an imported mark the user renames or moves is theirs: shown by Your marks from then on", () => {
  const renamed = changed(marks[0], { text: "Mine now" });
  assert.deepEqual(renamed, { id: 1, kind: "chapter", source: "user", text: "Mine now" });
  assert.equal(sourceOf(renamed), "user");
  assert.deepEqual(ids(shownMarks([renamed], { user: true, imported: false })), [1]);
  assert.deepEqual(ids(shownMarks([changed(marks[3], { start: 9 })], { user: false, imported: true })), []);
});

/** markSources.ts `changed` (only "one an import brought" changes hands). */
test("a mark of the user's own, or the visitor's, stays as it is when changed", () => {
  assert.deepEqual(changed(marks[2], { start: 9 }), { id: 3, kind: "highlight", source: "user", start: 9 });
  assert.deepEqual(changed(marks[4], { text: "x" }), { id: 5, kind: "chapter", text: "x" });
});

/** markSources.ts `changed` (the mark is returned changed, the one given left as it was). */
test("changing a mark leaves the one given as it was", () => {
  const was = { ...marks[0] };
  changed(marks[0], { text: "x" });
  assert.deepEqual(marks[0], was);
});
