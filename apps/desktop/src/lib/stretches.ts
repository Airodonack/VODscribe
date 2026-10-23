// What of a VOD is transcribed, and what is asked to be (crates/core
// stretches.rs, read with `transcript_stretches`): for the transcript's rows
// where there are no lines yet (Transcript.svelte), Transcribe the rest, and
// how much a row says is transcribed (VodRow.svelte).

import type { Status, Stretch, Vod, api } from "./api";

/** A request's status in words, as a VOD's row and the transcript's rows say it; none for done or nothing asked. */
export const statusLabel: Record<Status, string> = {
  queued: "Waiting to download",
  downloading: "Downloading",
  ready: "Waiting to transcribe",
  transcribing: "Transcribing",
  paused: "Paused",
  done: "",
  error: "Failed",
  none: "",
};

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

/** The gaps the transcript shows as rows of their own (`gaps`), for a VOD
 *  `transcribed` that much with `lines` lines so far: none before anything is
 *  transcribed or asked for (the empty state offers that instead), but once
 *  some is, even with no lines (silence, music), so the rest can be asked for. */
export function shownGaps(
  stretches: Stretch[],
  duration: number | null,
  transcribed: Vod["transcribed"],
  lines: number,
): Gap[] {
  const found = gaps(stretches, duration);
  return lines > 0 || transcribed !== "none" || found.some((g) => g.request) ? found : [];
}

/** What a VOD's row says after the state of the request it shows (`shown`,
 *  of a VOD `duration` long): the stretch's range, its times put as `format`
 *  says (platform.ts `formatTime`, passed in so node's tests load this module
 *  alone), unless it's all of it; and how many more requests wait. Empty with
 *  nothing to say. */
export function requestDetail(
  shown: Pick<Stretch, "start" | "end"> | null,
  duration: number | null,
  waiting: number,
  format: (seconds: number) => string,
): string {
  let out = "";
  const end = shown?.end ?? duration;
  if (shown && !(shown.start <= 0 && shown.end === null)) {
    out += end !== null ? ` · ${format(shown.start)}–${format(end)}` : ` · from ${format(shown.start)}`;
  }
  if (waiting > 0) out += ` · ${waiting} more waiting`;
  return out;
}

/** Asks for every part of `vod` not transcribed, nor asked for yet, to be
 *  transcribed (Transcribe the rest): one request a gap, in time order. The
 *  page's `api` is passed in, so node's tests can load this module (and give
 *  it one of their own) without the app's. */
export async function transcribeRest(
  vod: Vod,
  calls: Pick<typeof api, "transcriptStretches" | "transcribeStretch">,
): Promise<void> {
  const stretches = await calls.transcriptStretches(vod.id);
  for (const gap of gaps(stretches, vod.duration)) {
    if (!gap.request) await calls.transcribeStretch(vod.id, gap.start, gap.end);
  }
}

/** Whether request `stretch`'s progress, now `status`, is for a VOD's row
 *  that shows request `shown` at `shownStatus`: the one it shows, or a
 *  running one while that one isn't running (crates/core `summarize` shows
 *  the first one running). Two running at once (one downloading as another
 *  transcribes) would otherwise take the row in turns at each event, and the
 *  transcript would read all its lines again at each turn. */
export function showsProgress(shown: number | null, shownStatus: Status, stretch: number, status: Status): boolean {
  const running = (s: Status) => s === "downloading" || s === "transcribing";
  return shown === stretch || (running(status) && !running(shownStatus));
}
