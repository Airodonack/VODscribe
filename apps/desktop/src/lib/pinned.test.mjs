// The transcript's pinned headings (pinned.ts): node --test apps/desktop/src/lib/pinned.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { groupSpans, pinnedAt, reserveAt } from "./pinned.ts";
import { anyoneNamed, groupHeads } from "./transcriptGroups.ts";

// A transcript of ten rows, 40 px each: a chapter's divider (30 px) over row
// 0 and one over row 6; one speaker from row 0 to 3, another from row 3 to 5,
// row 5 a line no one is named for, a third speaker from row 6 to the end.
// Where a row has a divider or a head, the row's 40 px include them.
const offsets = Float64Array.from({ length: 11 }, (_, i) => i * 40);
const dividers = [
  { row: 0, height: 30 },
  { row: 6, height: 30 },
];
const SPEAKER = 20;
const groups = [
  { row: 0, end: 3, height: SPEAKER },
  { row: 3, end: 5, height: SPEAKER },
  { row: 6, end: 10, height: SPEAKER },
];
/** What's pinned with the view's top at `y`. @type {(y: number) => import("./pinned").Pinned} */
const at = (y) => pinnedAt(offsets, dividers, groups, y);

test("a group goes on over its named lines, and ends at a head, a line no one is named for, or a row that isn't a line", () => {
  const head = { name: "A", start: 0 };
  assert.deepEqual(
    groupSpans([head, null, null, head, null, null, head, null], [true, true, true, true, true, false, true, true]),
    [
      { row: 0, end: 3 },
      { row: 3, end: 5 },
      { row: 6, end: 8 },
    ],
  );
  assert.deepEqual(groupSpans([null, null], [false, true]), []);
});

test("at the very top, nothing is pinned: the headings are where they are", () => {
  assert.deepEqual(at(0), { chapter: -1, group: -1, chapterTop: 0, speakerTop: 0, height: 0 });
});

test("once a heading is off the top, its chapter and speaker are pinned, the speaker under the divider", () => {
  // Row 1's middle at the top: the divider and the head over row 0 are above.
  assert.deepEqual(at(60), { chapter: 0, group: 0, chapterTop: 0, speakerTop: 30, height: 50 });
  // Just past the divider's top, both are pinned together, where they were: the head is on the divider's row.
  assert.deepEqual(at(1), { chapter: 0, group: 0, chapterTop: 0, speakerTop: 30, height: 50 });
  // A head on a row of its own is pinned once its top is past the divider's bottom.
  assert.equal(at(89).group, 0);
  assert.equal(at(91).group, 1);
});

test("the next speaker's head pushes the pinned one up by as much as they'd overlap, then takes its place", () => {
  // Row 3 (the next head) starts at 120; the pinned head's bottom is at y + 50.
  assert.equal(at(70).speakerTop, 30);
  // 10 px into it: the pinned head is 10 px higher, its bottom on the next head's top.
  const mid = at(80);
  assert.equal(mid.group, 0);
  assert.equal(mid.speakerTop, 20);
  assert.equal(mid.chapterTop, 0);
  assert.equal(80 + mid.speakerTop + SPEAKER, 120);
  // The next head at the divider's bottom: it's the one pinned, where it is.
  const next = at(91);
  assert.equal(next.group, 1);
  assert.equal(next.speakerTop, 30);
});

test("a line no one is named for pushes the speaker out, and none is pinned over it", () => {
  // Row 5 (unnamed) starts at 200: the second group ends there.
  assert.equal(at(150).speakerTop, 30);
  assert.equal(at(165).speakerTop, 200 - 165 - SPEAKER);
  const over = at(175);
  assert.equal(over.group, -1);
  assert.equal(over.chapter, 0);
  assert.equal(over.height, 30);
});

test("where anyone is named, a stretch no one is named for is Unknown's: its own group, pinned over it", () => {
  // The same rows, as lines: row 5 no one's, so in a transcript naming
  // others it heads a group of its own (transcriptGroups.ts), pinned as theirs are.
  const who = ["A", "A", "A", "B", "B", null, "C", "C", "C", "C"];
  const lines = who.map((speaker, i) => ({ start: i * 4, end: i * 4 + 3, speaker }));
  const items = lines.map((line, i) => ({ line, divided: i === 0 || i === 6 }));
  const heads = groupHeads(items, undefined, anyoneNamed(lines));
  const spans = groupSpans(
    heads,
    lines.map(() => true),
  );
  assert.deepEqual(spans, [
    { row: 0, end: 3 },
    { row: 3, end: 5 },
    { row: 5, end: 6 },
    { row: 6, end: 10 },
  ]);
  const withUnknown = spans.map((span) => ({ ...span, height: SPEAKER }));
  const over = pinnedAt(offsets, dividers, withUnknown, 215);
  assert.equal(over.group, 2);
  assert.ok(heads[withUnknown[over.group].row]?.unknown);
  // Its lines end at row 6's divider (at 240), which pushes it up as it comes.
  assert.equal(over.speakerTop, 240 - 215 - SPEAKER);
});

test("a new chapter pushes out both the pinned divider and the speaker under it", () => {
  // The first chapter's rows with a group to its end, here: row 6's divider starts at 240.
  const spans = [
    { row: 0, end: 6, height: SPEAKER },
    { row: 6, end: 10, height: SPEAKER },
  ];
  const mid = pinnedAt(offsets, dividers, spans, 200);
  assert.equal(mid.chapter, 0);
  assert.equal(mid.group, 0);
  // The stack (50 px) 10 px into the next divider: both 10 px up, its bottom on the divider's top.
  assert.equal(mid.chapterTop, -10);
  assert.equal(mid.speakerTop, 20);
  assert.equal(mid.height, 40);
  assert.equal(200 + mid.height, 240);
  // Pushed further than the divider is tall, the speaker under it is still pushed out with it, not dropped.
  const far = pinnedAt(offsets, dividers, spans, 225);
  assert.deepEqual(far, { chapter: 0, group: 0, chapterTop: -35, speakerTop: -5, height: 15 });
  // The divider's top at the view's: the old chapter is pushed out whole.
  assert.equal(pinnedAt(offsets, dividers, spans, 240).height, 0);
  // Just past it: it and the head on its row are pinned, at rest.
  assert.deepEqual(pinnedAt(offsets, dividers, spans, 241), {
    chapter: 1,
    group: 1,
    chapterTop: 0,
    speakerTop: 30,
    height: 50,
  });
});

test("an ungrouped stretch's chapter is pinned alone; without chapters, a speaker alone", () => {
  assert.deepEqual(at(210), { chapter: 0, group: -1, chapterTop: 0, speakerTop: 0, height: 30 });
  // Pushed out by the next chapter, alone: 10 px into row 6's divider.
  assert.equal(at(220).chapterTop, -10);
  const plain = pinnedAt(offsets, [], groups, 60);
  assert.deepEqual(plain, { chapter: -1, group: 0, chapterTop: 0, speakerTop: 0, height: 20 });
  // Its next head pushes it off the top, there being nothing above it.
  assert.equal(pinnedAt(offsets, [], groups, 110).speakerTop, -10);
  assert.equal(pinnedAt(offsets, [], groups, 110).height, 10);
  assert.equal(pinnedAt(offsets, [], [], 110).height, 0);
});

test("a row brought into view goes below what's pinned with it at the top", () => {
  // Inside a group: the divider and the head.
  assert.equal(reserveAt(1, dividers, groups), 50);
  // Starting a group: its head is in view, under the divider.
  assert.equal(reserveAt(3, dividers, groups), 30);
  // A line no one is named for: the divider.
  assert.equal(reserveAt(5, dividers, groups), 30);
  // A row with its own divider pushes out the rest.
  assert.equal(reserveAt(6, dividers, groups), 0);
  assert.equal(reserveAt(0, dividers, groups), 0);
  assert.equal(reserveAt(2, [], []), 0);
});
