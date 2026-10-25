// The transcript's groups of lines by one speaker (transcriptGroups.ts): node --test apps/desktop/src/lib/transcriptGroups.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { GROUP_GAP, groupHeads, initial } from "./transcriptGroups.ts";

/** A line from `start` to `end`, said by `speaker`, as a row with no divider over it.
 *  @type {(start: number, end?: number, speaker?: string) => import("./transcriptGroups").GroupItem} */
const said = (start, end, speaker) => ({ line: { start, end, speaker }, divided: false });
/** Where each group starts: the heads' names and times, null for the other rows.
 *  @type {(items: import("./transcriptGroups").GroupItem[], canonical?: (name: string) => string) => (string | null)[]} */
const heads = (items, canonical) => groupHeads(items, canonical).map((h) => h && `${h.name} ${h.start}`);

test("one speaker's lines one after another: one head, over the first", () => {
  assert.deepEqual(heads([said(0, 4, "Ada"), said(5, 9, "Ada"), said(10, 12, "Ada")]), ["Ada 0", null, null]);
});

test("someone else speaking starts a group, and so does the first speaker again", () => {
  assert.deepEqual(heads([said(0, 4, "Ada"), said(5, 9, "Marlowe"), said(10, 12, "Ada")]), [
    "Ada 0",
    "Marlowe 5",
    "Ada 10",
  ]);
});

test("a line no one is named for has no head, and the next named line starts afresh", () => {
  assert.deepEqual(heads([said(0, 4, "Ada"), said(5, 9), said(10, 12, "Ada"), said(13, 14, "  ")]), [
    "Ada 0",
    null,
    "Ada 10",
    null,
  ]);
});

test("a chapter's divider starts a group, though the speaker goes on", () => {
  const items = [said(0, 4, "Ada"), { ...said(5, 9, "Ada"), divided: true }, said(10, 12, "Ada")];
  assert.deepEqual(heads(items), ["Ada 0", "Ada 5", null]);
});

test("a row that isn't a line (a part not transcribed) starts a group after it", () => {
  const items = [said(0, 4, "Ada"), { line: null, divided: false }, said(5, 9, "Ada")];
  assert.deepEqual(heads(items), ["Ada 0", null, "Ada 5"]);
});

test(`a pause of more than ${GROUP_GAP} s after the last line ends starts a group; up to it, not`, () => {
  assert.deepEqual(heads([said(0, 10, "Ada"), said(10 + GROUP_GAP, 35, "Ada")]), ["Ada 0", null]);
  assert.deepEqual(heads([said(0, 10, "Ada"), said(10.5 + GROUP_GAP, 35, "Ada")]), [
    "Ada 0",
    `Ada ${10.5 + GROUP_GAP}`,
  ]);
});

test("a long line counts its pause from its end; a line with no end, from its start", () => {
  // Thirty seconds of talk, then the next line two seconds after it ends.
  assert.deepEqual(heads([said(0, 30, "Ada"), said(32, 34, "Ada")]), ["Ada 0", null]);
  const noEnds = [
    { line: { start: 0, speaker: "Ada" }, divided: false },
    { line: { start: 25, speaker: "Ada" }, divided: false },
  ];
  assert.deepEqual(heads(noEnds), ["Ada 0", "Ada 25"]);
});

test("an alias is its cast member: their group, headed by their name; a name not in the cast as the line has it", () => {
  /** @param {string} name */
  const canonical = (name) => (name.toLowerCase() === "mar" ? "Marlowe" : name);
  assert.deepEqual(heads([said(0, 4, "Mar"), said(5, 9, "Marlowe"), said(10, 12, "Ada")], canonical), [
    "Marlowe 0",
    null,
    "Ada 10",
  ]);
  assert.deepEqual(heads([said(0, 4, "Ada"), said(5, 9, "Mar")], canonical), ["Ada 0", "Marlowe 5"]);
  // By default, a different case or spaces around are the same name.
  assert.deepEqual(heads([said(0, 4, "Ada"), said(5, 9, " ada ")]), ["Ada 0", null]);
});

test("a name's initial: its first letter, a capital; a character outside the alphabet whole", () => {
  assert.equal(initial("ada"), "A");
  assert.equal(initial("  Émile"), "É");
  assert.equal(initial("🦊fox"), "🦊");
  assert.equal(initial(""), "?");
});
