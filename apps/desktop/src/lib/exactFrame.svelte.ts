// The exact frame of the moment the timeline's pointer rests on, from the
// app's own player (player_preview), for the thumbnail (ScrubFrame.svelte's
// `exact`) to show over the storyboard's nearest frame. Asked for once the
// pointer has stood still a moment, one request at a time, the newest
// moment winning; the moment moving on lets the one shown go.

import { api, type Doc } from "./api";
import { FRAME_HEIGHT } from "./scrubFrame";

/** How long the pointer stands still before the exact frame is asked for, in ms. */
export const REST_MS = 120;

export class ExactFrame {
  frame = $state.raw<{ time: number; image: ImageBitmap } | null>(null);
  #wanted: { doc: Doc; time: number } | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #asking = false;

  /** The moment shown is `time` of `doc` (none: nothing shows). */
  want(doc: Doc | undefined, time: number | null) {
    if (!doc || time === null) return this.clear();
    const w = this.#wanted;
    if (w && w.doc.kind === doc.kind && w.doc.id === doc.id && w.time === time) return;
    this.#wanted = { doc, time };
    this.frame = null;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.#ask(), REST_MS);
  }

  clear() {
    clearTimeout(this.#timer);
    this.#wanted = null;
    this.frame = null;
  }

  async #ask() {
    if (this.#asking || !this.#wanted) return;
    this.#asking = true;
    const want = this.#wanted;
    try {
      const ratio = typeof devicePixelRatio === "number" ? devicePixelRatio : 1;
      const tall = Math.ceil(FRAME_HEIGHT * ratio);
      const got = await api.playerPreview(want.doc, want.time, tall * 4, tall);
      if (this.#wanted !== want || !got?.exact) return;
      const image = await createImageBitmap(got.pixels);
      if (this.#wanted === want) this.frame = { time: got.time, image };
      else image.close();
    } catch (e) {
      // Not in the app (the site's demo), or the player closed meanwhile: the storyboard's frame stays.
    } finally {
      this.#asking = false;
      // Moved on while asking, and stood still again since: the newest.
      if (this.#wanted && this.#wanted !== want && !this.frame) this.#ask();
    }
  }
}
