// A press that becomes a drag once the pointer moves far enough with the
// button held, and the click its release leaves behind not taken as one: a
// clip's pill, a library row, a highlight's row, the Clip button, a line of
// the transcript. One way of doing it, so they all notice a release they
// never saw (let go outside the window, or during the system's drag): the
// next move with the button up ends the press, and coming back over the
// window with no button held never starts a drag.

/** How far, in pixels, a press moves before it's a drag. */
export const DRAG_SLOP = 4;
/** How long after a drag's release its click is still the drag's, not one of its own: the
 *  click comes with the release, before anything waiting for a time to pass, however long
 *  the drag was; a click after that is one of its own. */
export const SWALLOW_MS = 0;

/** The bit in `PointerEvent.buttons` of the button `PointerEvent.button` numbers. */
const BIT = [1, 4, 2, 8, 16];
/** Whether the button `button` (as `PointerEvent.button` numbers it) is held in `e`. */
export const holds = (e: { buttons: number }, button = 0) => (e.buttons & (BIT[button] ?? 0)) !== 0;

/** Where a press began. */
export interface Pressed {
  x: number;
  y: number;
}

export interface PressToDrag {
  /** A press of the main button on the node: false if it's not one that can
   *  be dragged (the press is then left alone). */
  press?: (e: PointerEvent) => boolean | void;
  /** It moved DRAG_SLOP with the button held: the drag begins (once a press). */
  start: (e: PointerEvent, from: Pressed) => void;
  /** Each move after the drag began, while the button's held. */
  move?: (e: PointerEvent) => void;
  /** The press is over: let go, or found let go (a move with the button up),
   *  or cancelled; `dragged` if its drag began. */
  end?: (dragged: boolean) => void;
  /** A click on the node that isn't the one a drag's release leaves. */
  click?: (e: MouseEvent) => void;
}

/**
 * Makes `node` a press-to-drag (a Svelte action): see the top of this file.
 * The window's listeners are on only while a press is held, and a little
 * after a drag, for its click.
 */
export function pressToDrag(node: HTMLElement, options: PressToDrag) {
  let opts = options;
  let press: Pressed | null = null;
  let dragged = false;
  /** Whether a click on the node now is the drag's (from its start to just after its release). */
  let swallowing = false;
  let forget: ReturnType<typeof setTimeout> | undefined;

  const listen = (on: boolean) => {
    const how = on ? "addEventListener" : "removeEventListener";
    window[how]("pointermove", onMove as EventListener, true);
    window[how]("pointerup", onUp as EventListener, true);
    window[how]("pointercancel", onUp as EventListener, true);
  };
  const over = () => {
    if (!press) return;
    const was = dragged;
    press = null;
    dragged = false;
    listen(false);
    if (was) forget = setTimeout(stopSwallowing, SWALLOW_MS);
    opts.end?.(was);
  };
  const stopSwallowing = () => {
    swallowing = false;
    clearTimeout(forget);
    window.removeEventListener("click", onWindowClick, true);
  };
  function onDown(e: PointerEvent) {
    if (e.button !== 0 || press || opts.press?.(e) === false) return;
    press = { x: e.clientX, y: e.clientY };
    dragged = false;
    listen(true);
  }
  function onMove(e: PointerEvent) {
    if (!press) return;
    // Let go where we didn't see it: the press is over, and no drag begins.
    if (!holds(e)) return over();
    if (dragged) return opts.move?.(e);
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_SLOP) return;
    dragged = true;
    stopSwallowing();
    swallowing = true;
    window.addEventListener("click", onWindowClick, true);
    opts.start(e, press);
  }
  function onUp(e: PointerEvent) {
    if (e.type === "pointercancel" || e.button === 0) over();
  }
  // The drag's click: taken before anything under the window sees it.
  function onWindowClick(e: MouseEvent) {
    if (!swallowing || !node.contains(e.target as Node)) return;
    stopSwallowing();
    e.preventDefault();
    e.stopPropagation();
    e.stopImmediatePropagation();
  }
  function onClick(e: MouseEvent) {
    opts.click?.(e);
  }
  node.addEventListener("pointerdown", onDown as EventListener);
  node.addEventListener("click", onClick);
  return {
    update(next: PressToDrag) {
      opts = next;
    },
    destroy() {
      // A press held goes on to its release: its node can go while it's
      // dragged (a line scrolled out of the transcript as it's dragged on).
      node.removeEventListener("pointerdown", onDown as EventListener);
      node.removeEventListener("click", onClick);
    },
  };
}
