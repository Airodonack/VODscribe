// The arithmetic behind the player for files played here (LocalPlayer.svelte):
// which caption is up, the waveform's columns, and where a frame step lands.

import type { Cue } from "./api";

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
 * Where `frames` frames on from `time` lands, at `fps`. A quarter of a frame
 * into it, so the frame shown is that one whichever way the player rounds,
 * and stepping again counts from the same frame.
 */
export function stepTime(time: number, frames: number, fps: number): number {
  const frame = Math.floor(time * fps + 1e-3) + frames;
  return Math.max(0, (frame + 0.25) / fps);
}

/** Seconds between labeled ticks: the first step that leaves room for a label. */
const STEPS = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 14400];

/** The tick step for `span` seconds drawn `width` pixels wide, labels at least 64 px apart. */
export function tickStep(span: number, width: number): number {
  const fits = (step: number) => (step / span) * width >= 64;
  return STEPS.find(fits) ?? STEPS[STEPS.length - 1];
}
