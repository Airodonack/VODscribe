// The annotations pane's rows (annotationRows.ts): node --test apps/desktop/src/lib/annotationRows.test.mjs
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

test("chapters come in time order, each to the next one's start, the last to the VOD's end, with their icon", () => {
  const rows = chapterRows(marks, 400);
  assert.deepEqual(
    rows.map((r) => [r.id, r.start, r.end, r.title, r.icon]),
    [
      [4, 0, 120, "The start", null],
      [1, 120, 300, "The door", "🚪"],
      [7, 300, 400, "Untitled chapter", null],
    ],
  );
  assert.equal(chapterRows(marks).at(-1)?.end, null);
  assert.ok(rows.every((r) => r.kind === "chapter"));
});

test("highlights come in time order, the shorter first at one start, with their kind and why", () => {
  const rows = highlightRows(marks);
  assert.deepEqual(
    rows.map((r) => [r.id, r.title, r.end, r.category, r.why, r.cast]),
    [
      [6, "Short", 14, "hype", null, []],
      [5, "Long", 30, null, null, []],
      [2, "Later", 60, "funny", "Why.", ["Ada", "Mar"]],
    ],
  );
});

test("the current chapter is the last to start by the playhead, none before the first", () => {
  const rows = [{ start: 5 }, { start: 120 }, { start: 300 }];
  assert.equal(currentChapter(rows, 0), -1);
  assert.equal(currentChapter(rows, 5), 0);
  assert.equal(currentChapter(rows, 119.9), 0);
  assert.equal(currentChapter(rows, 120), 1);
  assert.equal(currentChapter(rows, 4000), 2);
  assert.equal(currentChapter([], 10), -1);
});

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

test("a highlight's length reads in seconds under a minute, as a time over", () => {
  assert.equal(lengthLabel(7, 14), "7 s");
  assert.equal(lengthLabel(7, 7.2), "1 s");
  assert.equal(lengthLabel(0, 65), "1:05");
  assert.equal(lengthLabel(0, 3725), "1:02:05");
  assert.equal(lengthLabel(7, null), "");
  assert.equal(lengthLabel(7, 5), "");
});

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

test("the rows shown are each chapter and, unless it's collapsed, its highlights", () => {
  const groups = groupsOf(nested);
  assert.deepEqual(ids(shownRows(groups, new Set())), ["c1", "h4", "h5", "h6", "c2", "h7", "c3", "h8"]);
  assert.deepEqual(ids(shownRows(groups, new Set([1, 3]))), ["c1", "c2", "h7", "c3"]);
  const before = groupsOf(nested.map((m) => (m.id === 1 ? { ...m, start: 30 } : m)));
  assert.deepEqual(ids(shownRows(before, new Set())).slice(0, 3), ["h4", "h5", "c1"]);
  assert.deepEqual(ids(shownRows(before, new Set([BEFORE_FIRST]))).slice(0, 2), ["c1", "h6"]);
});

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
