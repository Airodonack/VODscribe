// The transcript's groups of lines by one speaker (transcriptGroups.ts): node --test apps/desktop/src/lib/transcriptGroups.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { GROUP_GAP, UNKNOWN_SPEAKER, anyoneNamed, groupHeads, initial } from "./transcriptGroups.ts";

/** A line from `start` to `end`, said by `speaker`, as a row with no divider over it.
 *  @type {(start: number, end?: number, speaker?: string) => import("./transcriptGroups").GroupItem} */
const said = (start, end, speaker) => ({ line: { start, end, speaker }, divided: false });
/** Where each group starts: the heads' names and times, null for the other rows.
 *  @type {(items: import("./transcriptGroups").GroupItem[], canonical?: (name: string) => string) => (string | null)[]} */
const heads = (items, canonical) => groupHeads(items, canonical).map((h) => h && `${h.name} ${h.start}`);

// Source: transcriptGroups.ts header ("a speaker's lines one after another make a group, under one head").
test("one speaker's lines one after another: one head, over the first", () => {
  assert.deepEqual(heads([said(0, 4, "Ada"), said(5, 9, "Ada"), said(10, 12, "Ada")]), ["Ada 0", null, null]);
});

// Source: transcriptGroups.ts header ("A group ends where someone else speaks").
test("someone else speaking starts a group, and so does the first speaker again", () => {
  assert.deepEqual(heads([said(0, 4, "Ada"), said(5, 9, "Marlowe"), said(10, 12, "Ada")]), [
    "Ada 0",
    "Marlowe 5",
    "Ada 10",
  ]);
});

// Source: transcriptGroups.ts header ("in one where no one is, there are no heads").
test("a line no one is named for has no head, and the next named line starts afresh", () => {
  assert.deepEqual(heads([said(0, 4, "Ada"), said(5, 9), said(10, 12, "Ada"), said(13, 14, "  ")]), [
    "Ada 0",
    null,
    "Ada 10",
    null,
  ]);
});

// Source: transcriptGroups.ts header ("they make groups of their own under an \"Unknown\" head").
test("where anyone is named, lines no one is named for are an unknown speaker's, in groups of their own", () => {
  const items = [
    said(0, 4, "Ada"),
    said(5, 9),
    said(10, 12, " "),
    said(13, 14, "Unknown"),
    said(15, 16),
    { line: null, divided: false },
    said(17, 18),
    { ...said(19, 20), divided: true },
    said(50, 51),
  ];
  assert.ok(anyoneNamed(items.map((i) => i.line)));
  assert.ok(!anyoneNamed([said(0, 1).line, said(2, 3, " ").line, null]));
  const got = groupHeads(items, undefined, true);
  assert.deepEqual(
    got.map((h) => h && `${h.name} ${h.start}${h.unknown ? " ?" : ""}`),
    [
      "Ada 0",
      `${UNKNOWN_SPEAKER} 5 ?`,
      null,
      // Someone named "Unknown" is someone: not the unknown speaker's group.
      "Unknown 13",
      `${UNKNOWN_SPEAKER} 15 ?`,
      null,
      `${UNKNOWN_SPEAKER} 17 ?`,
      `${UNKNOWN_SPEAKER} 19 ?`,
      `${UNKNOWN_SPEAKER} 50 ?`,
    ],
    "after a row that isn't a line, a divider and a long pause, a new group as for anyone",
  );
});

// Source: transcriptGroups.ts header ("at a chapter's divider").
test("a chapter's divider starts a group, though the speaker goes on", () => {
  const items = [said(0, 4, "Ada"), { ...said(5, 9, "Ada"), divided: true }, said(10, 12, "Ada")];
  assert.deepEqual(heads(items), ["Ada 0", "Ada 5", null]);
});

// Source: transcriptGroups.ts header ("at a row that isn't a line").
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

// Source: transcriptGroups.ts `groupHeads` (`canonical`).
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

// Source: transcriptGroups.ts `initial`.
test("a name's initial: its first letter, a capital; a character outside the alphabet whole", () => {
  assert.equal(initial("ada"), "A");
  assert.equal(initial("  Émile"), "É");
  assert.equal(initial("🦊fox"), "🦊");
  assert.equal(initial(""), "?");
});

// Source for the tests below: transcriptGroups.ts's header ("a group ends
// where someone else speaks, at a chapter's divider, at a row that isn't a
// line ... and after a pause of more than `GROUP_GAP` seconds") and its
// `groupHeads` doc comment.

test("a line starting before the last one ended goes on its speaker's group", () => {
  assert.deepEqual(heads([said(0, 10, "Ada"), said(4, 6, "Ada"), said(5, 9, "Marlowe")]), ["Ada 0", null, "Marlowe 5"]);
});

test("a name the cast maps to nothing is still a speaker, apart from the unknown one", () => {
  const items = [said(0, 4, "Ada"), said(5, 9, "ghost"), said(10, 12)];
  const got = groupHeads(items, (name) => (name === "ghost" ? "" : name), true);
  assert.deepEqual(
    got.map((h) => h && `${h.name}|${h.unknown ? "?" : ""}`),
    ["Ada|", "|", `${UNKNOWN_SPEAKER}|?`],
  );
});

test("the head's time is the first line's start, of the group it heads", () => {
  assert.deepEqual(groupHeads([said(7.5, 9, "Ada"), said(10, 11, "Ada")])[0], { name: "Ada", start: 7.5 });
});

test("a transcript no one is named in has no heads, whatever the lines' places", () => {
  assert.deepEqual(groupHeads([said(0, 4), said(5, 9), { line: null, divided: true }]), [null, null, null]);
  assert.ok(!anyoneNamed([]));
});

test("lines no one is named for are headed Unknown", () => {
  assert.equal(UNKNOWN_SPEAKER, "Unknown");
  assert.equal(groupHeads([said(0, 1, "Ada"), said(2, 3)], undefined, true)[1]?.name, "Unknown");
});

test("a name the cast gives with spaces around is headed without them", () => {
  assert.equal(groupHeads([said(0, 1, "mar")], () => "  Marlowe ")[0]?.name, "Marlowe");
});
