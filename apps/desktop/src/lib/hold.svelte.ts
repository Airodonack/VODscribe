// A button of the player bar right-clicked, or pressed and held, opens a
// menu above it (the speeds, PlayerBar.svelte; how much to transcribe,
// Player.svelte). Right-clicked, the menu stays open for a click on one of
// its items. Held, let go over one of its items, that one's picked, and
// elsewhere the menu just closes. A plain click still does the button's own
// thing. A press that moves first is a drag, not a hold. While held, the
// item under the pointer is `pointed`, for the menu to shade it as hovered:
// while the press that began on the button lasts, the web view doesn't
// apply :hover to anything else.

import { DRAG_SLOP, holds } from "./pressToDrag";

const HOLD_MS = 400;

export class Hold {
  /** The menu is open (bound to its Popover). */
  open = $state(false);
  /** The menu's item under the pointer while it's held open, else null. */
  pointed = $state<HTMLElement | null>(null);
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

  /** The button's contextmenu (a right-click, or Control-click on a Mac):
   *  the menu, open till a click picks an item or closes it. */
  menu = (e: MouseEvent) => {
    e.preventDefault();
    this.#letGo();
    this.#held = false;
    this.pointed = null;
    if (this.open) return;
    this.#opening();
    this.open = true;
  };

  /** The button's pointerdown. */
  down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    this.#held = false;
    this.pointed = null;
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
    else if (this.open && this.#held && !holds(e)) {
      this.open = false;
      this.pointed = null;
    } else if (this.open && this.#held) this.pointed = this.#itemAt(e);
    if (!this.#press && !(this.open && this.#held)) this.#stop();
  };

  // By where the pointer is: the press began on the button, so the button may be the event's target.
  #itemAt(e: PointerEvent): HTMLElement | null {
    return document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>(this.#item) ?? null;
  }

  #up = (e: PointerEvent) => {
    this.#letGo();
    this.#stop();
    this.pointed = null;
    if (!this.open || !this.#held) return;
    this.open = false;
    const item = this.#itemAt(e);
    if (item) this.#pick(item);
    // The click that may follow (let go on the button) comes before this.
    setTimeout(() => (this.#held = false));
  };
}
