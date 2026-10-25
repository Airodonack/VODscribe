// The rows of the annotations pane (AnnotationsPane.svelte), worked out from
// the VOD's marks (list_marks): its chapters, each running to the next one's
// start, and its highlights, in time order; which chapter the playhead is in;
// and what a highlight's length says.

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
    }));
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
