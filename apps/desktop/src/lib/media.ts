// The arithmetic behind the player and its timeline: which caption is up,
// the waveform's columns and its gaps, where a frame step or a cut lands, the stretches
// cuts make, and the timeline's ticks.

import type { Cue, Stretch } from "./api";

/** Levels per second in a VOD's waveform (crates/core peaks.rs). */
export const PEAKS_RATE = 10;

/** The caption up at `time`, if any. `cues` are in order and don't overlap. */
export function cueAt(cues: Cue[], time: number): Cue | null {
  let lo = 0;
  let hi = cues.length;
  // The first cue starting after `time`; the one before it is the candidate.
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (cues[mid].start <= time) lo = mid + 1;
    else hi = mid;
  }
  const cue = cues[lo - 1];
  return cue && time < cue.end ? cue : null;
}

/**
 * The waveform squeezed into `columns` columns, each the loudest level (0 to 1)
 * in its stretch of `duration` seconds. Levels past the end of `levels` are 0.
 */
export function columns(levels: Uint8Array, duration: number, columns: number): Float32Array {
  const out = new Float32Array(Math.max(0, columns));
  if (!levels.length || !(duration > 0)) return out;
  const perColumn = (duration * PEAKS_RATE) / columns;
  for (let c = 0; c < columns; c++) {
    const from = Math.floor(c * perColumn);
    const to = Math.min(levels.length, Math.max(from + 1, Math.floor((c + 1) * perColumn)));
    let max = 0;
    for (let i = from; i < to; i++) if (levels[i] > max) max = levels[i];
    out[c] = max / 255;
  }
  return out;
}

/**
 * Where a VOD from a site has no waveform yet, in order: its levels are read
 * from the audio fetched to transcribe each stretch (crates/core peaks.rs
 * `merge`), so they're there for what's transcribed, and all of a request
 * whose audio is here (ready or transcribing); elsewhere they're 0, as
 * silence is, hence asked of its stretches rather than read from its levels.
 */
export function waveformGaps(stretches: Stretch[], duration: number): { start: number; end: number }[] {
  const heard = stretches
    .map((s) => ({
      start: s.start,
      end:
        s.status === "done" || s.status === "ready" || s.status === "transcribing"
          ? (s.end ?? duration)
          : (s.reached ?? s.start),
    }))
    .filter((s) => s.end > s.start)
    .sort((a, b) => a.start - b.start);
  const gaps = [];
  let at = 0;
  for (const s of heard) {
    // Less than a level apart: no gap.
    if (s.start - at >= 1 / PEAKS_RATE) gaps.push({ start: at, end: s.start });
    at = Math.max(at, s.end);
  }
  if (duration - at >= 1 / PEAKS_RATE) gaps.push({ start: at, end: duration });
  return gaps;
}

/** The number of the frame showing at `time`, at `fps`: its start counts
 *  as it, though a frame's start (k / fps) comes out a hair below. */
export function frameIndex(time: number, fps: number): number {
  return Math.floor(time * fps + 1e-3);
}

/** The number of the frame edge nearest `time`, at `fps`: where something
 *  dragged or moved between frames settles. */
export function edgeIndex(time: number, fps: number): number {
  return Math.round(time * fps);
}

/**
 * Where `frames` frames on from `time` lands, at `fps`. A quarter of a frame
 * into it, so the frame shown is that one whichever way the player rounds,
 * and stepping again counts from the same frame.
 */
export function stepTime(time: number, frames: number, fps: number): number {
  const frame = frameIndex(time, fps) + frames;
  return Math.max(0, (frame + 0.25) / fps);
}

/** The frame rate assumed until a player says (a site's player never does). */
export const DEFAULT_FPS = 30;

/** A quarter of a frame after a cut at `cut`, within 0..`duration`: where to
 *  seek to show the frame it starts, whichever way the player rounds. */
export function inFrame(cut: number, fps: number, duration: number): number {
  return Math.min(duration, cut + 0.25 / fps);
}

/** Seconds between labeled ticks: the first step that leaves room for a label. */
const STEPS = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 14400];

/** The tick step for `span` seconds drawn `width` pixels wide, labels at least 64 px apart. */
export function tickStep(span: number, width: number): number {
  const fits = (step: number) => (step / span) * width >= 64;
  return STEPS.find(fits) ?? STEPS[STEPS.length - 1];
}

/**
 * Where the frame showing at `time` starts, at `fps`: a mark goes there, a cut
 * between it and the frame before, so the stretches either side of it never
 * share a frame.
 */
export function frameStart(time: number, fps: number): number {
  return Math.max(0, frameIndex(time, fps) / fps);
}

/**
 * The moment a selection's `edge` at `at` shows: its start's frame, or for
 * its end the frame before (the last one in: the end is where the next
 * starts), at `fps`.
 */
export function edgeShows(edge: "start" | "end", at: number, fps: number): number {
  return edge === "end" ? frameStart(Math.max(0, at - 1 / fps), fps) : at;
}

/** A mark at `time` moved `frames` frames on (back, if negative), within 0..`duration`. */
export function nudge(time: number, frames: number, fps: number, duration: number): number {
  return Math.min(duration, Math.max(0, (edgeIndex(time, fps) + frames) / fps));
}

/** The stretches `cuts` (seconds, any order) split 0..`duration` into. */
export function stretches(cuts: number[], duration: number): { start: number; end: number }[] {
  const edges = [0, ...cuts.filter((c) => c > 0 && c < duration).sort((a, b) => a - b), duration];
  const out = [];
  for (let i = 0; i + 1 < edges.length; i++)
    if (edges[i + 1] > edges[i]) out.push({ start: edges[i], end: edges[i + 1] });
  return out;
}

/** The stretch between `cuts` that `time` is in, of a VOD `duration` long; at
 *  the very end (or past it, as a site's player can be), the last one. */
export function stretchIn(cuts: number[], duration: number, time: number): { start: number; end: number } | undefined {
  const all = stretches(cuts, duration);
  return all.find((s) => time >= s.start && time < s.end) ?? (time >= duration ? all.at(-1) : undefined);
}

/**
 * What plays of `start`..`end` with the `struck` stretches jumped over (as an
 * edit's piece plays; crates/core edits.rs `spans`): what's left, in order,
 * less slivers under 0.04 s.
 */
export function playedSpans(start: number, end: number, struck: [number, number][]): [number, number][] {
  const cuts = struck.filter(([a, b]) => b > a).sort((x, y) => x[0] - y[0]);
  const out: [number, number][] = [];
  let at = start;
  for (const [a, b] of cuts) {
    if (a > at) out.push([at, Math.min(a, end)]);
    at = Math.max(at, b);
    if (at >= end) break;
  }
  if (at < end) out.push([at, end]);
  return out.filter(([a, b]) => b - a >= 0.04);
}

/** What the web view's player says when a file won't play: that its format
 *  can't be (MEDIA_ERR_SRC_NOT_SUPPORTED), or that it couldn't be. */
export function unplayable(error: Pick<MediaError, "code"> | null | undefined): string {
  return error?.code === 4 ? "This video's format can't be played here." : "This video couldn't be played.";
}

/** Whether `message` is one `unplayable` gives (tests/e2e looks for them too). */
export const isUnplayable = (message: string) => /can't be played here\.|couldn't be played\./.test(message);

/**
 * A media element's volume control, muting by setting the volume to 0 and
 * back: WebKit lets an element be unmuted (`muted = false`) only from a
 * click, so a key (Shift+M) could mute but never unmute it.
 */
export function elementSound(element: () => HTMLMediaElement | undefined, saved?: (volume: number) => void) {
  let muted = false;
  let level = 1;
  return {
    get: () => (muted ? level : (element()?.volume ?? 1)),
    muted: () => muted,
    set(volume: number) {
      const e = element();
      if (!e) return;
      e.muted = false;
      e.volume = volume;
      [muted, level] = [volume === 0, volume];
      saved?.(volume);
    },
    setMuted(on: boolean) {
      const e = element();
      if (!e || on === muted) return;
      if (on) level = e.volume;
      e.muted = false;
      e.volume = on ? 0 : level;
      muted = on;
    },
  };
}
