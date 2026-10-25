// The rows of the annotations pane (AnnotationsPane.svelte), worked out from
// the VOD's marks (list_marks): its chapters, each running to the next one's
// start, and its highlights, in time order, each under the chapter it starts
// in; which chapter the playhead is in; what a highlight's length says; and
// how the keyboard goes from row to row.

import type { Mark } from "./api";

export interface ChapterRow {
  kind: "chapter";
  id: number;
  start: number;
  /** The next chapter's start, or the VOD's end (null when it isn't known). */
  end: number | null;
  title: string;
}

export interface HighlightRow {
  kind: "highlight";
  id: number;
  start: number;
  end: number | null;
  title: string;
  /** Its kind: funny, hype, drama… (annotated.ts colours it). */
  category: string | null;
  /** Why it's one. */
  why: string | null;
  /** Who is in it: cast members' names or aliases. */
  cast: string[];
}

export type PaneRow = ChapterRow | HighlightRow;

const byStart = (a: Mark, b: Mark) => a.start - b.start || (a.end ?? a.start) - (b.end ?? b.start) || a.id - b.id;

/** The chapters among `marks`, in time order, each to the next one's start
 *  (the last to `duration`, if it's known). */
export function chapterRows(marks: Mark[], duration?: number | null): ChapterRow[] {
  const chapters = marks.filter((m) => m.kind === "chapter").sort(byStart);
  return chapters.map((m, i) => ({
    kind: "chapter",
    id: m.id,
    start: m.start,
    end: chapters[i + 1]?.start ?? (duration ? duration : null),
    title: m.text?.trim() || "Untitled chapter",
  }));
}

/** The highlights among `marks`, in time order (the shorter first at one start). */
export function highlightRows(marks: Mark[]): HighlightRow[] {
  return marks
    .filter((m) => m.kind === "highlight")
    .sort(byStart)
    .map((m) => ({
      kind: "highlight",
      id: m.id,
      start: m.start,
      end: m.end ?? null,
      title: m.text?.trim() || "Highlight",
      category: m.category ?? null,
      why: m.note?.trim() || null,
      cast: m.cast ?? [],
    }));
}

/** A chapter and the highlights that start in it; `chapter` is null for
 *  those before the first chapter (or all of them, when there are none). */
export interface ChapterGroup {
  chapter: ChapterRow | null;
  highlights: HighlightRow[];
}

/** The key of a group whose chapter is null, where a chapter's is its id (mark ids are positive). */
export const BEFORE_FIRST = -1;

export const groupKey = (group: ChapterGroup): number => group.chapter?.id ?? BEFORE_FIRST;

/** `highlights` (in time order) each under the chapter (of `chapters`, in time
 *  order) whose span holds its start: every chapter, in order, with its own;
 *  first, a group with no chapter for those that start before any. */
export function nestRows(chapters: ChapterRow[], highlights: HighlightRow[]): ChapterGroup[] {
  const groups: ChapterGroup[] = chapters.map((chapter) => ({ chapter, highlights: [] }));
  const before: HighlightRow[] = [];
  for (const h of highlights) {
    const i = currentChapter(chapters, h.start);
    (i < 0 ? before : groups[i].highlights).push(h);
  }
  return before.length ? [{ chapter: null, highlights: before }, ...groups] : groups;
}

/** The rows shown, top to bottom, as the keyboard goes through them: each
 *  chapter, then its highlights unless it's collapsed (by `groupKey`); a
 *  group with no chapter has no row of its own. */
export function shownRows(groups: ChapterGroup[], collapsed: ReadonlySet<number>): PaneRow[] {
  return groups.flatMap((g) => [
    ...(g.chapter ? [g.chapter] : []),
    ...(collapsed.has(groupKey(g)) ? [] : g.highlights),
  ]);
}

/** Whether a highlight has anything to show when it's expanded. */
export const hasDetail = (row: HighlightRow): boolean => !!(row.why || row.cast.length || row.category);

/** What ← or → does on row `i` of `rows` (shownRows), tree-style: → opens a
 *  row that's closed and has something under it, ← closes one that's open,
 *  and ← on a closed highlight goes to its chapter. `open` says whether a row
 *  is open, `opens` whether it has anything to open. Null for anything else. */
export function sideStep(
  rows: PaneRow[],
  i: number,
  key: string,
  open: (row: PaneRow) => boolean,
  opens: (row: PaneRow) => boolean,
): { toggle: PaneRow } | { focus: number } | null {
  const row = rows[i];
  if (!row) return null;
  if (key === "ArrowRight") return opens(row) && !open(row) ? { toggle: row } : null;
  if (key !== "ArrowLeft") return null;
  if (opens(row) && open(row)) return { toggle: row };
  if (row.kind === "chapter") return null;
  for (let j = i - 1; j >= 0; j--) if (rows[j].kind === "chapter") return { focus: j };
  return null;
}

/** The place in `chapters` (in time order) of the one `time` is in: the last
 *  to start by then; -1 before the first. */
export function currentChapter(chapters: { start: number }[], time: number): number {
  let found = -1;
  for (let i = 0; i < chapters.length && chapters[i].start <= time + 1e-6; i++) found = i;
  return found;
}

/** How long a highlight lasts, as the pane says it: "7 s" under a minute,
 *  "1:05" or "1:02:05" over; "" with no end (or none after its start). */
export function lengthLabel(start: number, end: number | null): string {
  if (end === null || !(end > start)) return "";
  const s = Math.round(end - start);
  if (s < 60) return `${Math.max(1, s)} s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** Where focus goes from row `i` of `count` on `key` (ArrowUp, ArrowDown,
 *  Home, End), or null for another key. Up from the first and down from the
 *  last stay put. */
export function stepRow(i: number, count: number, key: string): number | null {
  if (count === 0) return null;
  if (key === "ArrowUp") return Math.max(0, i - 1);
  if (key === "ArrowDown") return Math.min(count - 1, i + 1);
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  return null;
}
