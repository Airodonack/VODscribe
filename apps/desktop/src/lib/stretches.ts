// What of a VOD is transcribed, and what is asked to be (crates/core
// stretches.rs, read with `transcript_stretches`): for the transcript's rows
// where there are no lines yet (Transcript.svelte), and Transcribe the rest.

import type { Stretch } from "./api";

/** Ranges this close count as touching (crates/core stretches.rs `EDGE`):
 *  a VOD's length from its site and the end of its audio differ by a little. */
const EDGE = 0.5;

/** A part of a VOD with no lines yet: one a request will transcribe, or one nobody asked for. */
export interface Gap {
  start: number;
  /** null: to the VOD's end. */
  end: number | null;
  /** The request that will transcribe it, if one does. */
  request: Stretch | null;
}

/** The ranges `stretches` transcribed, in order and apart: the done ones, and
 *  what requests have reached (crates/core `covered`). An end of Infinity is the VOD's. */
export function covered(stretches: Stretch[]): [number, number][] {
  const ranges = stretches
    .map((s): [number, number] | null =>
      s.status === "done" ? [s.start, s.end ?? Infinity] : s.reached !== null ? [s.start, s.reached] : null,
    )
    .filter((r): r is [number, number] => r !== null && r[1] > r[0])
    .sort((a, b) => a[0] - b[0]);
  const out: [number, number][] = [];
  for (const [a, b] of ranges) {
    const last = out.at(-1);
    if (last && a <= last[1] + EDGE) last[1] = Math.max(last[1], b);
    else out.push([a, b]);
  }
  return out;
}

/** The parts of a VOD `duration` long (null: unknown) that `stretches` don't
 *  cover, split where requests will transcribe them, in time order. */
export function gaps(stretches: Stretch[], duration: number | null): Gap[] {
  const end = duration !== null && duration > 0 ? duration : Infinity;
  const open: [number, number][] = [];
  let at = 0;
  for (const [a, b] of covered(stretches)) {
    if (a > at + EDGE) open.push([at, Math.min(a, end)]);
    at = Math.max(at, b);
  }
  if (at + EDGE < end) open.push([at, end]);
  // What each request has still to do: from where it reached to its end.
  const asked = stretches
    .filter((s) => s.status !== "done")
    .map((s) => ({ request: s, from: Math.max(s.start, s.reached ?? s.start), to: s.end ?? Infinity }))
    .sort((a, b) => a.from - b.from);
  const out: Gap[] = [];
  // One that runs to the VOD's end ends there whatever its length turns out to be.
  const push = (start: number, to: number, request: Stretch | null) =>
    out.push({ start, end: to >= end ? null : to, request });
  for (const [a, b] of open) {
    let from = a;
    for (const { request, from: rFrom, to: rTo } of asked) {
      const [s, e] = [Math.max(rFrom, from), Math.min(rTo, b)];
      if (e - s <= EDGE) continue;
      if (s > from + EDGE) push(from, s, null);
      push(s > from + EDGE ? s : from, e, request);
      from = e;
    }
    if (b - from > EDGE) push(from, b, null);
  }
  return out;
}
