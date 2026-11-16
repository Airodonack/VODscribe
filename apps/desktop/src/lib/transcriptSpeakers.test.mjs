// Who the transcript's lines are said by (transcriptGroups.ts's speakers: `anyoneNamed`, the unknown speaker, the
// speaker's name): node --test apps/desktop/src/lib/transcriptSpeakers.test.mjs
// Sources: transcriptGroups.ts's doc comments; CHANGELOG.md 0.7.0 ("Annotated transcripts read like a chat.", "Unnamed
// lines sit under Unknown."). The layout of the groups (dividers, pauses, rows that aren't lines) and the cases of
// transcriptGroups.test.mjs that name speakers (one head over a speaker's lines, an alias is its cast member, a name's
// initial) are tested there.
import assert from "node:assert/strict";
import { test } from "node:test";
import { UNKNOWN_SPEAKER, anyoneNamed, groupHeads } from "./transcriptGroups.ts";

/** A line from `start` to `start + 1`, said by `speaker`, as a row with no divider over it.
 *  @type {(start: number, speaker?: string | null) => import("./transcriptGroups").GroupItem} */
const said = (start, speaker) => ({ line: { start, end: start + 1, speaker }, divided: false });

/** transcriptGroups.ts `anyoneNamed` ("Whether anyone is named for any of `lines`"). */
test("anyone is named when any line has a speaker", () => {
  assert.ok(anyoneNamed([said(0).line, said(2, "Ada").line]));
  assert.ok(anyoneNamed([null, said(2, " Ada ").line]));
});

/** transcriptGroups.ts `anyoneNamed` (no speaker, a blank one, or a row that isn't a line is no one). */
test("no one is named when no line has a speaker, or only blank ones", () => {
  assert.ok(!anyoneNamed([]));
  assert.ok(!anyoneNamed([said(0).line, said(2, " ").line, said(4, null).line, null]));
});

/** CHANGELOG 0.7.0 "Annotated transcripts read like a chat." / header ("in one where no one is, there are no heads"). */
test("in a transcript where no one is named there are no heads", () => {
  const items = [said(0), said(2, " "), said(4)];
  assert.ok(!anyoneNamed(items.map((i) => i.line)));
  assert.deepEqual(groupHeads(items), [null, null, null]);
});

/** CHANGELOG 0.7.0 "Unnamed lines sit under Unknown." ("lines nobody is named for get a heading of their own ... so they never read as said by the person before them"). */
test("where anyone is named, lines no one is named for have a heading of their own, never the speaker's before", () => {
  const items = [said(0, "Ada"), said(2), said(4, "Ada")];
  const heads = groupHeads(items, undefined, true);
  assert.deepEqual(heads, [
    { name: "Ada", start: 0 },
    { name: UNKNOWN_SPEAKER, start: 2, unknown: true },
    { name: "Ada", start: 4 },
  ]);
});

/** transcriptGroups.ts `groupHeads` ("lines no one is named for are grouped as one unknown speaker's"). */
test("lines no one is named for, one after another, are one unknown speaker's group", () => {
  const heads = groupHeads([said(0, "Ada"), said(2), said(4, " "), said(6, null)], undefined, true);
  assert.deepEqual(
    heads.map((h) => h && h.name),
    ["Ada", UNKNOWN_SPEAKER, null, null],
  );
  assert.equal(heads[1]?.start, 2, "its time is the first line's");
});

/** transcriptGroups.ts `GroupHead` (`unknown`: "`name` is `UNKNOWN_SPEAKER`, and it's no one in the cast"); `groupHeads` ("never as a cast member's who may be called \"Unknown\""). */
test("someone named Unknown is someone: not the unknown speaker's group", () => {
  const heads = groupHeads([said(0, "Unknown"), said(2), said(4, "Unknown")], undefined, true);
  assert.deepEqual(heads, [
    { name: "Unknown", start: 0 },
    { name: UNKNOWN_SPEAKER, start: 2, unknown: true },
    { name: "Unknown", start: 4 },
  ]);
  assert.ok(!("unknown" in /** @type {object} */ (heads[0])), "a named speaker's head isn't marked unknown");
});

/** transcriptGroups.ts `groupHeads` (a speaker's line with no name given: `unnamed` off leaves it with no head). */
test("without `unnamed`, a line no one is named for has no head though others are named", () => {
  assert.deepEqual(groupHeads([said(0, "Ada"), said(2), said(4, "Ada")]), [
    { name: "Ada", start: 0 },
    null,
    { name: "Ada", start: 4 },
  ]);
});

/** transcriptGroups.ts `groupHeads` ("`canonical` gives the name a line's speaker goes by ...: that's the name on the head"). */
test("the head names the speaker as `canonical` gives them, trimmed", () => {
  const heads = groupHeads([said(0, "Mar")], (name) => (name === "Mar" ? " Marlowe " : name));
  assert.deepEqual(heads, [{ name: "Marlowe", start: 0 }]);
});

/** transcriptGroups.ts `groupHeads` ("two lines whose names are the same but for case are one speaker's"). */
test("two lines whose speakers' names differ only in case are one speaker's", () => {
  assert.deepEqual(
    groupHeads([said(0, "Ada"), said(2, "ADA"), said(4, " ada ")]).map((h) => h && h.name),
    ["Ada", null, null],
  );
});

/** transcriptGroups.ts `groupHeads` ("No name's key is empty: the unknown speaker's is"): a speaker `canonical` names with nothing is not the unknown speaker. */
test("a speaker whose name comes out empty is not the unknown speaker", () => {
  const heads = groupHeads(
    [said(0, "Ghost"), said(2), said(4, "Ghost")],
    (name) => (name === "Ghost" ? "" : name),
    true,
  );
  assert.deepEqual(heads, [
    { name: "", start: 0 },
    { name: UNKNOWN_SPEAKER, start: 2, unknown: true },
    { name: "", start: 4 },
  ]);
});

/** CHANGELOG 0.7.0 "Unnamed lines sit under Unknown." (a heading of their own reading Unknown). */
test("the heading over lines no one is named for says Unknown", () => {
  assert.equal(UNKNOWN_SPEAKER, "Unknown");
});
