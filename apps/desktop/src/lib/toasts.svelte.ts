// A short message at the bottom of the window (Toast.svelte): what was just
// done, with something to do about it ("Piece removed · Undo"). One at a time;
// a new one replaces it, and it goes after a few seconds. What went wrong
// (`error`: the marks', an edit's, a clip bar's) shows the same way wherever
// the user is, and stays until it's dismissed, replaced, or put right.

const SHOWN_FOR = 6000;

export interface Shown {
  message: string;
  /** A button in it: its label, and what it does. */
  action?: { label: string; run: () => void };
  /** Counts what's shown, so an older one's timer leaves a newer one alone. */
  seq: number;
  /** Whether it's something that went wrong. */
  error?: boolean;
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

  /** Shows what went wrong, until it's dismissed or replaced (or `drop`ped);
   *  returns which it is, for `drop`. */
  error(message: string): number {
    clearTimeout(this.#timer);
    const seq = ++this.#seq;
    this.shown = { message, seq, error: true };
    return seq;
  }

  /** Hides error `seq` (`error`), if it's still the one shown: what went wrong was put right. */
  drop(seq: number) {
    if (this.shown?.seq === seq) this.hide();
  }

  hide() {
    clearTimeout(this.#timer);
    this.shown = null;
  }
}

export const toast = new Toasts();
