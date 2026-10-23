// The transcript view's rows (Transcript.svelte): each transcript line with the
// chat sent while it was being said, from its start to the next line's. A part
// of the VOD not transcribed (stretches.ts) has a row of its own where it
// starts, which says so, with the chat from there like a line's. Where
// there's no transcript (yet), chat goes in rows of a minute each: all of it
// before the first line arrives, before a first line more than a minute in
// (otherwise that's a row of its own), after the last line while the rest is
// transcribed, and in a stretch of more than a minute without speech, so the
// minimap can scroll through it minute by minute rather than past one row of it.

import type { Line } from "./api";
import type { Gap } from "./stretches";

/** Where a row with nothing after it ends. */
export const FAR = 1e12;

export interface Row {
  /** Unique among the rows, and kept as lines are added or replaced: a
   *  line's start (lines can share one: then which of them it is), and where
   *  its row ends, so a row whose next line arrives is fetched again. By time,
   *  not place, so lines arriving in the middle don't change the keys below. */
  key: string;
  line: Line | null;
  /** A part not transcribed, starting here. */
  gap: Gap | null;
  from: number;
  to: number;
}

/** Rows of a minute each from minute `first` to the end of a VOD `duration` seconds long, the last one open-ended. */
function minuteRows(first: number, duration: number): Row[] {
  // A length the platform got wrong (missing, negative, endless) gives none.
  const minutes = Number.isFinite(duration) && duration > 0 ? Math.ceil(duration / 60) : 0;
  return Array.from({ length: Math.max(0, minutes - first) }, (_, k) => {
    const m = first + k;
    const to = m === minutes - 1 ? FAR : (m + 1) * 60;
    return { key: `minute ${m} to ${to}`, line: null, gap: null, from: m * 60, to };
  });
}

/** What has a row of its own: a line, or a gap where it starts. */
interface Item {
  start: number;
  line: Line | null;
  gap: Gap | null;
  key: string;
}

/** `lines` and `gaps` (each in time order) in one, by start; a line before a gap starting with it. */
function items(lines: Line[], gaps: Gap[]): Item[] {
  const out: Item[] = [];
  const seen = new Map<number, number>();
  let [i, j] = [0, 0];
  while (i < lines.length || j < gaps.length) {
    if (j >= gaps.length || (i < lines.length && lines[i].start <= gaps[j].start)) {
      const line = lines[i++];
      const n = seen.get(line.start) ?? 0;
      seen.set(line.start, n + 1);
      out.push({ start: line.start, line, gap: null, key: n ? `line ${line.start} #${n}` : `line ${line.start}` });
    } else {
      const gap = gaps[j++];
      out.push({ start: gap.start, line: null, gap, key: `gap ${gap.start}` });
    }
  }
  return out;
}

/** The rows for `lines` and `gaps` of a VOD `duration` seconds long (0 if
 *  unknown); without chat, just the lines and gaps. */
export function buildRows(lines: Line[], duration: number, withChat: boolean, gaps: Gap[] = []): Row[] {
  const all = items(lines, gaps);
  const row = (item: Item, to: number): Row => ({
    key: `${item.key} to ${to}`,
    line: item.line,
    gap: item.gap,
    from: item.start,
    to,
  });
  if (!withChat) return all.map((item, i) => row(item, all[i + 1]?.start ?? FAR));
  if (all.length === 0) return minuteRows(0, duration);
  // The minute after the last line's starts the rows of minutes, if the VOD goes on that long.
  const tail = minuteRows(Math.floor(all[all.length - 1].start / 60) + 1, duration);
  // Chat before the first line: one row, or rows of a minute if it starts more than a minute in.
  const first = all[0].start;
  const out: Row[] = [];
  if (first <= 60) out.push({ key: `before to ${first}`, line: null, gap: null, from: -FAR, to: first });
  else
    for (let m = 0; m * 60 < first; m++) {
      const to = Math.min((m + 1) * 60, first);
      out.push({ key: `minute ${m} to ${to}`, line: null, gap: null, from: m === 0 ? -FAR : m * 60, to });
    }
  all.forEach((item, i) => {
    const next = all[i + 1]?.start;
    if (next === undefined) {
      out.push(row(item, tail[0]?.from ?? FAR));
      return;
    }
    // A minute or less to the next line: one row. More: the line's row ends at
    // the next whole minute, and minutes follow until the next line.
    const minute = (Math.floor(item.start / 60) + 1) * 60;
    if (next - item.start <= 60 || minute >= next) {
      out.push(row(item, next));
      return;
    }
    out.push(row(item, minute));
    for (let m = minute; m < next; m += 60) {
      const to = Math.min(m + 60, next);
      out.push({ key: `minute ${m / 60} to ${to}`, line: null, gap: null, from: m, to });
    }
  });
  return [...out, ...tail];
}

/**
 * The time `lines[first]` to `lines[last]` (either way round) take: from the
 * first one's start to the last one's end, or where the next line starts if
 * its end isn't known (the website's demo), or 5 s on for the very last; not
 * past the VOD's `duration`, if known.
 */
export function linesSpan(
  lines: Line[],
  first: number,
  last: number,
  duration?: number | null,
): { start: number; end: number } {
  const [a, b] = first <= last ? [first, last] : [last, first];
  const end = lines[b].end ?? lines[b + 1]?.start ?? lines[b].start + 5;
  // (A line starting past the stated length, as a VOD's can: to its own end.)
  const start = lines[a].start;
  return { start, end: duration && start < duration ? Math.min(end, duration) : end };
}

/** About how tall a chapter's heading over a line is, in pixels, for rows not yet measured. */
export const HEAD_HEIGHT = 44;

/** The index of the line in `lines` (in time order, at least one) that starts
 *  nearest `time`; of anything else with a start, likewise (rows with a line or a gap). */
export function nearestLine(lines: { start: number }[], time: number): number {
  let lo = 0;
  let hi = lines.length - 1;
  // The last line starting at or before `time` (or the first, if none does)...
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (lines[mid].start <= time) lo = mid;
    else hi = mid - 1;
  }
  // ...or the one after it, if that starts nearer.
  const next = lines[lo + 1];
  return next && next.start - time < Math.abs(time - lines[lo].start) ? lo + 1 : lo;
}
