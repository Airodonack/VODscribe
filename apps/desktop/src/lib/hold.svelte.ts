// A button of the player bar pressed and held opens a menu above it (the
// speeds, PlayerBar.svelte; how much to transcribe, Player.svelte): let go
// over one of its items, that one's picked, and elsewhere the menu just
// closes. A plain click still does the button's own thing. A press that
// moves first is a drag, not a hold.

import { DRAG_SLOP, holds } from "./pressToDrag";

const HOLD_MS = 400;

export class Hold {
  /** The menu is open (bound to its Popover). */
  open = $state(false);
  #press: { x: number; y: number; timer: ReturnType<typeof setTimeout> } | null = null;
  // The menu was opened by this press: its let-go isn't a click on the button.
  #held = false;
  #item: string;
  #pick: (item: HTMLElement) => void;
  #opening: () => void;

  /** `item` finds the menu's items (a selector); `pick` is one let go over; `opening` runs as the hold opens the menu. */
  constructor(item: string, pick: (item: HTMLElement) => void, opening: () => void = () => {}) {
    this.#item = item;
    this.#pick = pick;
    this.#opening = opening;
  }

  /** Whether the click on the button now is a hold's let-go, not a click. */
  get held(): boolean {
    return this.#held;
  }

  /** The button's pointerdown. */
  down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    this.#held = false;
    const timer = setTimeout(() => {
      // Let go (or moved away) meanwhile: #press is gone, no menu.
      if (!this.#press) return;
      this.#press = null;
      this.#held = true;
      this.#opening();
      this.open = true;
    }, HOLD_MS);
    this.#press = { x: e.clientX, y: e.clientY, timer };
    window.addEventListener("pointermove", this.#move);
    window.addEventListener("pointerup", this.#up);
    window.addEventListener("pointercancel", this.#letGo);
  };

  #letGo = () => {
    if (this.#press) clearTimeout(this.#press.timer);
    this.#press = null;
  };

  #stop() {
    window.removeEventListener("pointermove", this.#move);
    window.removeEventListener("pointerup", this.#up);
    window.removeEventListener("pointercancel", this.#letGo);
  }

  #move = (e: PointerEvent) => {
    const press = this.#press;
    if (press && (!holds(e) || Math.hypot(e.clientX - press.x, e.clientY - press.y) >= DRAG_SLOP)) this.#letGo();
    // Its let-go missed (it happened outside the window): closed.
    else if (this.open && this.#held && !holds(e)) this.open = false;
    if (!this.#press && !(this.open && this.#held)) this.#stop();
  };

  #up = (e: PointerEvent) => {
    this.#letGo();
    this.#stop();
    if (!this.open || !this.#held) return;
    this.open = false;
    // By where it's let go: the press began on the button, so the button may be the event's target.
    const item = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>(this.#item);
    if (item) this.#pick(item);
    // The click that may follow (let go on the button) comes before this.
    setTimeout(() => (this.#held = false));
  };
}
