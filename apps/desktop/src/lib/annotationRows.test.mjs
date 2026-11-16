// The annotations pane's rows (annotationRows.ts): node --test apps/desktop/src/lib/annotationRows.test.mjs
// Sources: annotationRows.ts's doc comments; CHANGELOG.md 0.7.0 ("Chapters and highlights get a column.", "The column
// and timeline light together.", "Highlight rows show who's in them.", "Chapters show above the timeline.").
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// annotationRows.ts imports "./platform" as the page's bundler finds it; here it is the
// .ts file it is.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(specifier, context, next) {
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith(".ts"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`),
);
const {
  BEFORE_FIRST,
  chapterRows,
  currentChapter,
  currentHighlights,
  groupKey,
  hasDetail,
  highlightRows,
  lengthLabel,
  nestRows,
  pointAt,
  shownRows,
  sideStep,
  stepRow,
} = await import("./annotationRows.ts");

/** @type {(id: number, kind: import("./api").MarkKind, start: number, extra?: object) => import("./api").Mark} */
const mark = (id, kind, start, extra = {}) => ({ id, vod_id: 1, kind, start, text: null, ...extra });

const marks = [
  mark(1, "chapter", 120, { text: "The door", icon: "🚪" }),
  mark(2, "highlight", 50, { text: "Later", end: 60, category: "funny", note: " Why. ", cast: ["Ada", "Mar"] }),
  mark(3, "cut", 10),
  mark(4, "chapter", 0, { text: "The start" }),
  mark(5, "highlight", 7, { text: "Long", end: 30 }),
  mark(6, "highlight", 7, { text: "Short", end: 14, category: "hype" }),
  mark(7, "chapter", 300, { text: "  " }),
];

/** annotationRows.ts `chapterRows` ("in time order, each to the next one's start (the last to `duration`, if it's known)"). */
test("chapters come in time order, each to the next one's start, the last to the VOD's end", () => {
  const rows = chapterRows(marks, 400);
  assert.deepEqual(
    rows.map((r) => [r.id, r.start, r.end]),
    [
      [4, 0, 120],
      [1, 120, 300],
      [7, 300, 400],
    ],
  );
  assert.ok(rows.every((r) => r.kind === "chapter"));
  assert.ok(chapterRows(marks, 400).length === 3, "no cut or highlight among them");
});

/** annotationRows.ts `ChapterRow` (`end`: "the VOD's end (null when it isn't known)"). */
test("the last chapter has no end when the VOD's length isn't known", () => {
  assert.equal(chapterRows(marks).at(-1)?.end, null);
  assert.equal(chapterRows(marks, null).at(-1)?.end, null);
  assert.equal(chapterRows(marks, 0).at(-1)?.end, null);
});

/** CHANGELOG 0.7.0 "Chapters show above the timeline." ("each with its emoji when the file gives one"); `ChapterRow` (`icon`). */
test("a chapter has its icon when an import gave it one", () => {
  const rows = chapterRows(marks, 400);
  assert.deepEqual(
    rows.map((r) => [r.id, r.icon]),
    [
      [4, null],
      [1, "🚪"],
      [7, null],
    ],
  );
});

/** annotationRows.ts `chapterRows` (a chapter with no title, or only spaces, is "Untitled chapter"). */
test("a chapter with no title reads as an untitled chapter", () => {
  const rows = chapterRows([
    mark(1, "chapter", 0),
    mark(2, "chapter", 5, { text: "  " }),
    mark(3, "chapter", 9, { text: " Hi " }),
  ]);
  assert.deepEqual(
    rows.map((r) => r.title),
    ["Untitled chapter", "Untitled chapter", "Hi"],
  );
});

/** annotationRows.ts `highlightRows` ("in time order (the shorter first at one start)"). */
test("highlights come in time order, the shorter first at one start", () => {
  const rows = highlightRows(marks);
  assert.deepEqual(
    rows.map((r) => [r.id, r.title, r.end]),
    [
      [6, "Short", 14],
      [5, "Long", 30],
      [2, "Later", 60],
    ],
  );
  assert.deepEqual(
    highlightRows([mark(9, "highlight", 5), mark(8, "highlight", 5)]).map((r) => r.id),
    [8, 9],
    "alike in start and length: by id",
  );
});

/** annotationRows.ts `HighlightRow` (`category`: "Its kind"; `why`: "Why it's one"; `cast`: "Who is in it"). */
test("a highlight has its kind, why it is one and who is in it, or none", () => {
  const rows = highlightRows(marks);
  assert.deepEqual(
    rows.map((r) => [r.id, r.category, r.why, r.cast]),
    [
      [6, "hype", null, []],
      [5, null, null, []],
      [2, "funny", "Why.", ["Ada", "Mar"]],
    ],
  );
});

/** annotationRows.ts `highlightRows` (a highlight with no title is "Highlight"; one with no end has none). */
test("a highlight with no title reads as a highlight, and one with no end has none", () => {
  const [bare, spaced] = highlightRows([mark(1, "highlight", 0), mark(2, "highlight", 5, { text: "  ", end: 9 })]);
  assert.deepEqual([bare.title, bare.end, spaced.title, spaced.end], ["Highlight", null, "Highlight", 9]);
});

/** annotationRows.ts `currentChapter` ("the last to start by then; -1 before the first"). */
test("the current chapter is the last to start by the playhead, none before the first", () => {
  const rows = [{ start: 5 }, { start: 120 }, { start: 300 }];
  assert.equal(currentChapter(rows, 0), -1);
  assert.equal(currentChapter(rows, 5), 0);
  assert.equal(currentChapter(rows, 119.9), 0);
  assert.equal(currentChapter(rows, 120), 1);
  assert.equal(currentChapter(rows, 4000), 2);
  assert.equal(currentChapter([], 10), -1);
});

/** annotationRows.ts `currentHighlights` ("from their start up to their end (one with no end isn't playing past its start)"); CHANGELOG 0.7.0 "Chapters and highlights get a column." ("keeps the chapter and highlight playing lit"). */
test("the highlights playing are those from their start up to their end; one with no end, none", () => {
  const rows = highlightRows(marks);
  // Long 7–30, Short 7–14, Later 50–60.
  assert.deepEqual(currentHighlights(rows, 6.9), []);
  assert.deepEqual(currentHighlights(rows, 7).sort(), [5, 6]);
  assert.deepEqual(currentHighlights(rows, 14), [5]);
  assert.deepEqual(currentHighlights(rows, 55), [2]);
  assert.deepEqual(currentHighlights(rows, 60), []);
  assert.deepEqual(currentHighlights([{ id: 9, start: 10, end: null }], 10), []);
});

/** annotationRows.ts `lengthLabel` ("7 s" under a minute, "1:05" or "1:02:05" over; the empty text with no end (or none after its start)). */
test("a highlight's length reads in seconds under a minute, as a time over", () => {
  assert.equal(lengthLabel(7, 14), "7 s");
  assert.equal(lengthLabel(7, 7.2), "1 s");
  assert.equal(lengthLabel(0, 65), "1:05");
  assert.equal(lengthLabel(0, 3725), "1:02:05");
  assert.equal(lengthLabel(7, null), "");
  assert.equal(lengthLabel(7, 5), "");
});

/** annotationRows.ts `stepRow` ("Where focus goes from row `i` of `count` on `key` (ArrowUp, ArrowDown, Home, End), or null for another key. Up from the first and down from the last stay put."). */
test("arrows, Home and End move between rows; other keys don't", () => {
  assert.equal(stepRow(0, 3, "ArrowUp"), 0);
  assert.equal(stepRow(0, 3, "ArrowDown"), 1);
  assert.equal(stepRow(2, 3, "ArrowDown"), 2);
  assert.equal(stepRow(1, 3, "Home"), 0);
  assert.equal(stepRow(1, 3, "End"), 2);
  assert.equal(stepRow(1, 3, "Enter"), null);
  assert.equal(stepRow(0, 0, "ArrowDown"), null);
});

// Chapters at 0, 120 and 300; highlights at 7 (two), 50, 120 (on the second's start) and 400.
const nested = [
  mark(1, "chapter", 0, { text: "One" }),
  mark(2, "chapter", 120, { text: "Two" }),
  mark(3, "chapter", 300, { text: "Three" }),
  mark(4, "highlight", 7, { end: 14, note: "why" }),
  mark(5, "highlight", 7, { end: 30 }),
  mark(6, "highlight", 50, { cast: ["Ada"] }),
  mark(7, "highlight", 120, { category: "lore" }),
  mark(8, "highlight", 400),
];
/** @typedef {import("./annotationRows.ts").PaneRow} Row */
/** @type {(marks: import("./api").Mark[]) => import("./annotationRows.ts").ChapterGroup[]} */
const groupsOf = (marks) => nestRows(chapterRows(marks, 500), highlightRows(marks));
/** A row as the tests name it: "c1" for chapter 1, "h4" for highlight 4. @type {(row: Row) => string} */
const key = (r) => `${r.kind[0]}${r.id}`;
/** @type {(rows: Row[]) => string[]} */
const ids = (rows) => rows.map(key);

/** annotationRows.ts `nestRows` ("each under the chapter (of `chapters`, in time order) whose span holds its start: every chapter, in order, with its own"). */
test("each highlight goes under the chapter its start is in; a chapter with none has none", () => {
  const groups = groupsOf(nested);
  assert.deepEqual(
    groups.map((g) => [g.chapter?.id, g.highlights.map((h) => h.id)]),
    [
      [1, [4, 5, 6]],
      [2, [7]],
      [3, [8]],
    ],
  );
  const empty = groupsOf(nested.filter((m) => m.id !== 8));
  assert.deepEqual(empty.at(-1)?.highlights, []);
});

/** annotationRows.ts `nestRows` ("first, a group with no chapter for those that start before any"). */
test("highlights before the first chapter, or with no chapters, go in a group of their own first", () => {
  const late = nested.map((m) => (m.id === 1 ? { ...m, start: 30 } : m));
  assert.deepEqual(
    groupsOf(late).map((g) => [g.chapter?.id ?? null, g.highlights.map((h) => h.id)]),
    [
      [null, [4, 5]],
      [1, [6]],
      [2, [7]],
      [3, [8]],
    ],
  );
  const none = groupsOf(nested.filter((m) => m.kind === "highlight"));
  assert.deepEqual(
    none.map((g) => [g.chapter, g.highlights.length]),
    [[null, 5]],
  );
  assert.deepEqual(nestRows([], []), []);
});

/** annotationRows.ts `shownRows` ("each chapter, then its highlights unless it's collapsed (by `groupKey`); a group with no chapter has no row of its own"). */
test("the rows shown are each chapter and, unless it's collapsed, its highlights", () => {
  const groups = groupsOf(nested);
  assert.deepEqual(ids(shownRows(groups, new Set())), ["c1", "h4", "h5", "h6", "c2", "h7", "c3", "h8"]);
  assert.deepEqual(ids(shownRows(groups, new Set([1, 3]))), ["c1", "c2", "h7", "c3"]);
  const before = groupsOf(nested.map((m) => (m.id === 1 ? { ...m, start: 30 } : m)));
  assert.deepEqual(ids(shownRows(before, new Set())).slice(0, 3), ["h4", "h5", "c1"]);
  assert.deepEqual(ids(shownRows(before, new Set([BEFORE_FIRST]))).slice(0, 2), ["c1", "h6"]);
});

/** annotationRows.ts `shownRows`, `stepRow` (the keyboard goes through the rows shown as one list). */
test("the arrows step across chapters and highlights as one list", () => {
  const rows = shownRows(groupsOf(nested), new Set([2]));
  // From the last highlight of the first chapter, down to the collapsed second, then the third.
  /** @type {(from: string, k: string) => string | undefined} */
  const step = (from, k) => ids(rows)[stepRow(ids(rows).indexOf(from), rows.length, k) ?? -1];
  assert.equal(step("h6", "ArrowDown"), "c2");
  assert.equal(step("c2", "ArrowDown"), "c3");
  assert.equal(step("c1", "ArrowDown"), "h4");
  assert.equal(step("c3", "ArrowUp"), "c2");
  assert.equal(step("h5", "End"), "h8");
});

/** annotationRows.ts `sideStep` ("→ opens a row that's closed and has something under it, ← closes one that's open, and ← on a closed highlight goes to its chapter"). */
test("→ opens, ← closes, and ← on a closed highlight goes to its chapter", () => {
  const groups = groupsOf(nested);
  const rows = shownRows(groups, new Set());
  const open = new Set(["c1", "h4"]);
  /** @type {(r: Row) => boolean} */
  const isOpen = (r) => open.has(key(r));
  /** @type {(r: Row) => boolean} */
  const opens = (r) =>
    r.kind === "chapter" ? !!groups.find((g) => g.chapter?.id === r.id)?.highlights.length : hasDetail(r);
  /** @type {(id: string, k: string) => string | null} */
  const step = (id, k) => {
    const got = sideStep(rows, ids(rows).indexOf(id), k, isOpen, opens);
    return got && ("toggle" in got ? `toggle ${key(got.toggle)}` : `focus ${ids(rows)[got.focus]}`);
  };
  assert.equal(step("c1", "ArrowLeft"), "toggle c1");
  assert.equal(step("c1", "ArrowRight"), null, "already open");
  assert.equal(step("h4", "ArrowLeft"), "toggle h4");
  assert.equal(step("h6", "ArrowRight"), "toggle h6", "a cast is something to show");
  assert.equal(step("h5", "ArrowRight"), null, "nothing to show");
  assert.equal(step("h5", "ArrowLeft"), "focus c1");
  assert.equal(step("h7", "ArrowLeft"), "focus c2");
  assert.equal(step("c2", "Enter"), null);
  open.delete("c1");
  assert.equal(step("c1", "ArrowRight"), "toggle c1");
  assert.equal(step("c1", "ArrowLeft"), null, "a closed chapter has nowhere to go");
});

/** annotationRows.ts `pointAt` ("it only lets go of what it pointed at itself; the same as before comes back as it was"); CHANGELOG 0.7.0 "The column and timeline light together.". */
test("a mark pointed at in one place is let go of only there", () => {
  const pane = {};
  const timeline = {};
  const other = {};
  let at = pointAt(null, 4, pane);
  assert.deepEqual(at, { id: 4, by: pane });
  assert.equal(pointAt(at, 4, pane), at, "the same again changes nothing");
  assert.equal(pointAt(at, null, timeline), at, "a timeline the pointer isn't over keeps the pane's");
  at = pointAt(at, 7, timeline);
  assert.deepEqual(at, { id: 7, by: timeline });
  assert.equal(pointAt(at, null, other), at, "another timeline keeps this one's");
  assert.equal(pointAt(at, null, pane), at);
  assert.equal(pointAt(at, null, timeline), null);
  assert.equal(pointAt(null, null, pane), null);
});

/** annotationRows.ts `groupKey` ("The key of a group whose chapter is null, where a chapter's is its id (mark ids are positive)"). */
test("a group's key is its chapter's id, and the group with no chapter has a key no id can be", () => {
  const [before, first] = groupsOf([mark(1, "chapter", 30, { text: "One" }), mark(2, "highlight", 5)]);
  assert.equal(groupKey(first), 1);
  assert.equal(groupKey(before), BEFORE_FIRST);
  assert.ok(BEFORE_FIRST < 0);
});

/** annotationRows.ts `hasDetail` ("Whether a highlight has anything to show when it's expanded"); CHANGELOG 0.7.0 "Highlight rows show who's in them.". */
test("a highlight has detail to open when it has a why, a cast or a kind", () => {
  const [bare, why, cast, kind] = highlightRows([
    mark(1, "highlight", 1),
    mark(2, "highlight", 2, { note: "Because" }),
    mark(3, "highlight", 3, { cast: ["Ada"] }),
    mark(4, "highlight", 4, { category: "funny" }),
  ]);
  assert.deepEqual([bare, why, cast, kind].map(hasDetail), [false, true, true, true]);
  assert.equal(hasDetail(highlightRows([mark(5, "highlight", 1, { note: "  " })])[0]), false, "a blank why is none");
});

/** annotationRows.ts `sideStep` ("Null for anything else"): a row that isn't there, another key, and a closed chapter. */
test("a side step is none for a row that isn't there, for another key, and from a closed chapter", () => {
  const rows = shownRows(groupsOf(nested), new Set());
  const closed = () => false;
  const opens = () => true;
  assert.equal(sideStep(rows, 99, "ArrowRight", closed, opens), null);
  assert.equal(sideStep(rows, -1, "ArrowLeft", closed, opens), null);
  const h5 = ids(rows).indexOf("h5");
  assert.equal(sideStep(rows, h5, "Enter", closed, opens), null, "another key on a closed highlight");
  assert.equal(sideStep(rows, h5, "ArrowDown", closed, opens), null);
  const c2 = ids(rows).indexOf("c2");
  assert.equal(sideStep(rows, c2, "ArrowLeft", closed, opens), null, "a closed chapter has no earlier one to go to");
});

/** annotationRows.ts `lengthLabel` ("7 s" under a minute, "1:05" or "1:02:05" over; the empty text when none is after its start). */
test("a highlight's length is a time from a minute on, and none when it ends where it starts", () => {
  assert.equal(lengthLabel(0, 59), "59 s");
  assert.equal(lengthLabel(0, 60), "1:00");
  assert.equal(lengthLabel(5, 5), "");
});

/** annotationRows.ts `pointAt` ("it only lets go of what it pointed at itself"): the same mark pointed at from another place is that place's. */
test("the same mark pointed at from another place is pointed at by that place", () => {
  const pane = {};
  const timeline = {};
  const got = pointAt({ id: 4, by: pane }, 4, timeline);
  assert.equal(got?.id, 4);
  assert.equal(got?.by, timeline);
});
