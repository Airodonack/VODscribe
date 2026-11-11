// The transcript panel's reads (Transcript.svelte): everything read again
// when the VOD or what's asked of it changes (`whole`), and, while a stretch
// is transcribed, the lines in its range read again as they come (`range`),
// put in place of what was there.
//
// One read at a time, in the order asked, each made once the one before has
// landed: a read asked later then always sees the library as it was after
// the earlier one's, and lands after it. Made side by side, a whole read
// begun before the worker wrote a line could land after a range read that
// has it, and take it away again until the next range read.

import type { Line, Stretch } from "./api";

/** What a read gives: the lines it read, and (a VOD's) the stretches. */
export type Read = { lines: Line[]; stretches?: Stretch[] };

export interface Shown {
  /** The lines shown now, which a range's answer goes into. */
  lines: () => Line[];
  /** Shows what a read gave: all of it (`whole`), or the lines shown with a range's put in. */
  show: (read: Read, whole: boolean) => void;
  /** A read still wanted failed. */
  failed: (error: unknown, whole: boolean) => void;
}

/** See the top of this file. */
export function transcriptReads(shown: Shown) {
  // Only the latest request's answer counts, and one no longer wanted when
  // its turn comes isn't made; reading everything again makes any stretch's
  // read asked before it stale too.
  let wholes = 0;
  let ranges = 0;
  let last: Promise<void> = Promise.resolve();
  /** Makes `read` once those asked before it have landed, if it's still `wanted` then. */
  function inTurn(wanted: () => boolean, read: () => Promise<Read>, got: (r: Read) => void, whole: boolean) {
    last = last.then(async () => {
      if (!wanted()) return;
      try {
        const r = await read();
        if (wanted()) got(r);
      } catch (e) {
        if (wanted()) shown.failed(e, whole);
      }
    });
  }
  return {
    whole(read: () => Promise<Read>) {
      const mine = ++wholes;
      ranges++;
      inTurn(
        () => mine === wholes,
        read,
        (r) => shown.show(r, true),
        true,
      );
    },
    /** Reads `start` to `end` (null: the end) again. */
    range(start: number, end: number | null, read: () => Promise<Read>) {
      const mine = ++ranges;
      const got = ({ lines, stretches }: Read) => {
        const now = shown.lines();
        const before = now.filter((line) => line.start < start);
        const after = end === null ? [] : now.filter((line) => line.start >= end);
        shown.show({ lines: [...before, ...lines, ...after], stretches }, false);
      };
      inTurn(() => mine === ranges, read, got, false);
    },
  };
}
