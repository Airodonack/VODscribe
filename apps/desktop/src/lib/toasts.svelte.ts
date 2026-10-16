// A short message at the bottom of the window (Toast.svelte): what was just
// done, with something to do about it ("Piece removed · Undo"). One at a time;
// a new one replaces it, and it goes after a few seconds.

const SHOWN_FOR = 6000;

export interface Shown {
  message: string;
  /** A button in it: its label, and what it does. */
  action?: { label: string; run: () => void };
  /** Counts what's shown, so an older one's timer leaves a newer one alone. */
  seq: number;
}

class Toasts {
  shown = $state<Shown | null>(null);
  #seq = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;

  show(message: string, action?: Shown["action"]) {
    clearTimeout(this.#timer);
    const seq = ++this.#seq;
    this.shown = { message, action, seq };
    this.#timer = setTimeout(() => this.shown?.seq === seq && (this.shown = null), SHOWN_FOR);
  }

  hide() {
    clearTimeout(this.#timer);
    this.shown = null;
  }
}

export const toast = new Toasts();
