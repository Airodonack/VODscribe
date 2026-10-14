// The transcript view's rows (Transcript.svelte): each transcript line with the
// chat sent while it was being said, from its start to the next line's. Chat
// before the first line gets a row of its own. Where there's no transcript
// (yet), chat goes in rows of a minute each: all of it before the first line
// arrives, after the last line while the rest is transcribed, and in a stretch
// of more than a minute without speech, so the minimap can scroll through it
// minute by minute rather than past one row of it.

import type { Line } from "./api";

/** Where a row with nothing after it ends. */
export const FAR = 1e12;

export interface Row {
  /** Unique among the rows, and kept as lines are added: a line's place in
   *  the transcript (lines can share a start), and where its row ends, so a
   *  row whose next line arrives is fetched again. */
  key: string;
  line: Line | null;
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
    return { key: `minute ${m} to ${to}`, line: null, from: m * 60, to };
  });
}

/** The rows for `lines` of a VOD `duration` seconds long (0 if unknown); without chat, just the lines. */
export function buildRows(lines: Line[], duration: number, withChat: boolean): Row[] {
  if (!withChat) {
    return lines.map((line, i) => {
      const to = lines[i + 1]?.start ?? FAR;
      return { key: `line ${i} to ${to}`, line, from: line.start, to };
    });
  }
  if (lines.length === 0) return minuteRows(0, duration);
  // The minute after the last line's starts the rows of minutes, if the VOD goes on that long.
  const tail = minuteRows(Math.floor(lines[lines.length - 1].start / 60) + 1, duration);
  const out: Row[] = [{ key: `before to ${lines[0].start}`, line: null, from: -FAR, to: lines[0].start }];
  lines.forEach((line, i) => {
    const next = lines[i + 1]?.start;
    if (next === undefined) {
      const to = tail[0]?.from ?? FAR;
      out.push({ key: `line ${i} to ${to}`, line, from: line.start, to });
      return;
    }
    // A minute or less to the next line: one row. More: the line's row ends at
    // the next whole minute, and minutes follow until the next line.
    const minute = (Math.floor(line.start / 60) + 1) * 60;
    if (next - line.start <= 60 || minute >= next) {
      out.push({ key: `line ${i} to ${next}`, line, from: line.start, to: next });
      return;
    }
    out.push({ key: `line ${i} to ${minute}`, line, from: line.start, to: minute });
    for (let m = minute; m < next; m += 60) {
      const to = Math.min(m + 60, next);
      out.push({ key: `minute ${m / 60} to ${to}`, line: null, from: m, to });
    }
  });
  return [...out, ...tail];
}
