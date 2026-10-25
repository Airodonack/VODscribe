// The chapter or highlight the pointer is over, in the chapters and
// highlights pane (AnnotationsPane.svelte) or on the timeline (Scrubber.svelte),
// by its mark's id, so the other lights it too: a row lights its slice or
// glyph, a slice or glyph its row. Each place that points says who it is
// (`by`), so one the pointer leaves only lets go of what it set itself:
// every timeline hears the window's pointer moves, and only the one under
// the pointer has a mark to point at (annotationRows.ts `pointAt`). A
// finger has no hover, so touch sets nothing.

import { pointAt, type Pointed } from "./annotationRows";

class MarkHover {
  #at = $state.raw<Pointed>(null);

  /** The mark pointed at, wherever it was. */
  get id(): number | null {
    return this.#at?.id ?? null;
  }

  /** `by` points at mark `id`, or at none (null). */
  point(id: number | null, by: object) {
    const next = pointAt(this.#at, id, by);
    if (next !== this.#at) this.#at = next;
  }
}

export const markHover = new MarkHover();
