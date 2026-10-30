// The exact frame of the moment the timeline's pointer rests on, from the
// app's own player (player_preview), for the thumbnail (ScrubFrame.svelte's
// `exact`) to show over the storyboard's keyframe. Asked for once the
// pointer has stood still a moment, one request at a time, the newest
// moment winning; the moment moving on lets the one shown go.

import { api, type Doc } from "./api";
import { latest } from "./latest";
import { FRAME_HEIGHT } from "./scrubFrame";

/** How long the pointer stands still before the exact frame is asked for, in ms. */
export const REST_MS = 120;

/** The exact frame of a moment: its time, and its picture. */
type Frame = { time: number; image: ImageBitmap } | null;

export class ExactFrame {
  frame = $state.raw<Frame>(null);
  #wanted: { doc: Doc; time: number } | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #asking = latest<{ doc: Doc; time: number }, Frame>({
    async fetch(want) {
      const ratio = typeof devicePixelRatio === "number" ? devicePixelRatio : 1;
      const tall = Math.ceil(FRAME_HEIGHT * ratio);
      const got = await api.playerPreview(want.doc, want.time, tall * 4, tall);
      return got?.exact ? { time: got.time, image: await createImageBitmap(got.pixels) } : null;
    },
    got: (frame) => (this.frame = frame),
    free: (frame) => frame?.image.close(),
    // Not in the app (the site's demo), or the player closed meanwhile: the storyboard's frame stays.
    failed() {},
  });

  /** The moment shown is `time` of `doc` (none: nothing shows). */
  want(doc: Doc | undefined, time: number | null) {
    if (!doc || time === null) return this.clear();
    const w = this.#wanted;
    if (w && w.doc.kind === doc.kind && w.doc.id === doc.id && w.time === time) return;
    const want = { doc, time };
    // Moved on: the frame shown, or coming, goes.
    this.clear();
    this.#wanted = want;
    this.#timer = setTimeout(() => this.#asking.want(want), REST_MS);
  }

  clear() {
    clearTimeout(this.#timer);
    this.#wanted = null;
    this.frame = null;
    this.#asking.clear();
  }
}
