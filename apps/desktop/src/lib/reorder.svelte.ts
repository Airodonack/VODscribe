// Drag-to-reorder for a vertical list of rows keyed by id, with Alt+↑/↓ as the
// keyboard equivalent.
//
// The list reorders live as the dragged row passes its neighbours' midpoints,
// and the dragged row follows the pointer. Layout positions come from
// offsetTop, which ignores the transforms of rows that are still animating
// (the list uses animate:flip). Items can be added or removed mid-drag: new
// ones go on top, as they do in the library.

import { tick } from "svelte";

interface Drag {
  id: number;
  order: number[];
  moved: boolean;
  /** How far the dragged row is drawn from its slot, in px. */
  dy: number;
}

/** Pointer travel before a press counts as a drag rather than a click. */
const THRESHOLD = 4;
/** Distance from the list's edge at which dragging scrolls it. */
const EDGE = 36;

export class Reorder {
  /** The scrolling list element (`bind:this`). */
  list: HTMLElement | undefined = undefined;
  /** Each row's element by id (`bind:this`). */
  rows: Record<number, HTMLElement | null> = {};
  #drag = $state<Drag | null>(null);
  #suppressClick = false;

  /**
   * @param ids the items' ids in their current order
   * @param commit called with the new order when a drag or key press changes it
   */
  constructor(
    private ids: () => number[],
    private commit: (ids: number[]) => void,
  ) {}

  /** The ids in display order, which is the live order during a drag. */
  get order(): number[] {
    return this.#drag?.moved ? this.#drag.order : this.ids();
  }

  /** Whether row `id` is the one being dragged. */
  lifted(id: number) {
    return !!this.#drag?.moved && this.#drag.id === id;
  }

  /** The transform that keeps the dragged row under the pointer. */
  transform(id: number) {
    return this.lifted(id) ? `translateY(${this.#drag!.dy}px)` : null;
  }

  /** animate:flip duration: the dragged row follows the pointer, not an animation. */
  flipDuration(id: number) {
    return this.#drag?.id === id ? 0 : 150;
  }

  /** Swallows the click that ends a drag, so it doesn't also select the row (use as onclickcapture). */
  clickCapture = (e: MouseEvent) => {
    if (this.#suppressClick) {
      e.stopPropagation();
      e.preventDefault();
    }
  };

  /** `order` minus ids removed since it was taken, plus any added (on top). */
  #reconcile(order: number[]) {
    const ids = this.ids();
    const kept = order.filter((id) => ids.includes(id));
    return [...ids.filter((id) => !kept.includes(id)), ...kept];
  }

  /** Starts tracking a press on row `id`; it becomes a drag once the pointer moves. */
  start(e: PointerEvent, id: number) {
    const list = this.list;
    const row = this.rows[id];
    if (e.button !== 0 || this.ids().length < 2 || !list || !row) return;
    const startY = e.clientY;
    const grab = e.clientY - row.getBoundingClientRect().top;
    let pointerY = startY;
    let frame = 0;
    this.#drag = { id, order: this.ids(), moved: false, dy: 0 };

    const step = () => {
      const drag = this.#drag;
      if (!drag) return;
      const order = this.#reconcile(drag.order);
      if (order.length !== drag.order.length || order.some((x, i) => x !== drag.order[i])) drag.order = order;
      const self = this.rows[drag.id];
      if (!order.includes(drag.id) || !self) return end();

      const box = list.getBoundingClientRect();
      if (pointerY < box.top + EDGE) list.scrollTop -= Math.ceil((box.top + EDGE - pointerY) / 4);
      else if (pointerY > box.bottom - EDGE) list.scrollTop += Math.ceil((pointerY - box.bottom + EDGE) / 4);

      // The pointer in the list's content coordinates.
      const y = pointerY - box.top + list.scrollTop;
      // Rows that haven't rendered yet have no midpoint, so they're never swapped with.
      const mid = (x: number) => {
        const el = this.rows[x];
        return el ? el.offsetTop + el.offsetHeight / 2 : NaN;
      };
      const top = y - grab;
      const bottom = top + self.offsetHeight;
      const o = drag.order;
      const i = o.indexOf(drag.id);
      if (i > 0 && top < mid(o[i - 1])) {
        [o[i - 1], o[i]] = [o[i], o[i - 1]];
      } else if (i < o.length - 1 && bottom > mid(o[i + 1])) {
        [o[i + 1], o[i]] = [o[i], o[i + 1]];
      }
      tick().then(() => {
        const el = this.#drag && this.rows[this.#drag.id];
        if (this.#drag && el) this.#drag.dy = y - grab - el.offsetTop;
      });
      frame = requestAnimationFrame(step);
    };

    const move = (e: PointerEvent) => {
      pointerY = e.clientY;
      const drag = this.#drag;
      if (drag && !drag.moved && Math.abs(pointerY - startY) > THRESHOLD) {
        drag.moved = true;
        document.body.classList.add("dragging");
        frame = requestAnimationFrame(step);
      }
    };

    const end = () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      document.body.classList.remove("dragging");
      const drag = this.#drag;
      if (drag?.moved) {
        this.#suppressClick = true;
        setTimeout(() => (this.#suppressClick = false));
        const order = this.#reconcile(drag.order);
        const ids = this.ids();
        if (order.some((x, i) => x !== ids[i])) this.commit(order);
      }
      this.#drag = null;
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  /** Alt+↑/↓ moves row `id` one place. Returns whether it moved. */
  key(e: KeyboardEvent, id: number): boolean {
    if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return false;
    e.preventDefault();
    const order = this.ids();
    const i = order.indexOf(id);
    const j = e.key === "ArrowUp" ? i - 1 : i + 1;
    if (j < 0 || j >= order.length) return false;
    [order[i], order[j]] = [order[j], order[i]];
    this.commit(order);
    return true;
  }
}
