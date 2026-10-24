// Several things picked to act on together, the same way everywhere: a
// Ctrl-click (⌘ on a Mac) adds one or takes it out again and is where a
// Shift-click's run starts; a Shift-click picks the run from there to here,
// in the order they're on screen, either way; a plain click does what it
// always has (a row opens, the timeline scrubs) and leaves the set as it is,
// but on a clip bar's pill, as in a file manager, it picks that pill alone
// (`only`; a double-click opens it); Escape, or a click on empty space, lets
// it go. A box dragged over empty space (`dragBox` below) picks what it touches.
//
// Two sets, never both at once: the snippet set (stretches of the open VOD's
// or edit's timeline between its cuts, and clips of its clip bar, together)
// and the library set (VODs and edits in the library's list). Neither is
// there with a stretch selection (I and O, lines picked in the transcript,
// cuts.svelte.ts and edits.svelte.ts `selection`), nor with a mark or cut
// selected (cuts.svelte.ts `selected`; marks are picked one at a time):
// starting a set lets the stretch or the mark go, and making a stretch or
// selecting a mark lets the sets go. One kind of thing is selected at a
// time, so Delete and Escape are never in doubt. That rule is here, and
// only here: the stretches' and marks' owners hand over how they're let go
// (`clearsStretch`) and say when one is made (`stretchMade`).
//
// The order things were picked in is kept (a run is in on-screen order): it's
// the order a bulk action takes them in, and with two or more picked each
// shows its number in it. Picking never moves the playhead.

import { listed, os } from "./platform";

/** In the snippet set: a stretch between cuts of the open timeline (its time), or a clip of its clip bar. */
export type Snippet = { kind: "segment"; start: number; end: number } | { kind: "clip"; id: number };
/** In the library set: a VOD's row or an edit's. */
export type Entry = { kind: "vod" | "edit"; id: number };

export class Selection<T> {
  /** What's picked, in the order it was. */
  items = $state.raw<T[]>([]);
  /** Where a Shift-click's run starts: the last picked, or clicked plainly. */
  anchor = $state.raw<T | null>(null);
  #same: (a: T, b: T) => boolean;
  #starting: () => void;
  /** What was picked when a box began: kept beside what it holds, if Shift was held. */
  #base: T[] = [];

  /** `same` says whether two items are one; `starting` lets go of what can't be picked beside this set. */
  constructor(same: (a: T, b: T) => boolean, starting: () => void) {
    this.#same = same;
    this.#starting = starting;
  }

  get size(): number {
    return this.items.length;
  }

  has(item: T): boolean {
    return this.items.some((i) => this.#same(i, item));
  }

  /** `item`'s place in the order, from 1, while two or more are picked; else null. */
  number(item: T): number | null {
    if (this.items.length < 2) return null;
    const at = this.items.findIndex((i) => this.#same(i, item));
    return at < 0 ? null : at + 1;
  }

  /** A Ctrl-click: `item` in (last) or out, and the run's start. */
  toggle(item: T) {
    this.#starting();
    this.items = this.has(item) ? this.items.filter((i) => !this.#same(i, item)) : [...this.items, item];
    this.anchor = item;
  }

  /** A Shift-click: the run of `order` (what's on screen, in its order) from the anchor to `item`. */
  rangeTo(item: T, order: T[]) {
    const to = order.findIndex((o) => this.#same(o, item));
    if (to < 0) return;
    const anchor = this.anchor;
    const from = anchor === null ? -1 : order.findIndex((o) => this.#same(o, anchor));
    const start = from < 0 ? to : from;
    this.#starting();
    this.items = order.slice(Math.min(start, to), Math.max(start, to) + 1);
    if (from < 0) this.anchor = item;
  }

  /** Picks `list` (in its order), and only it. */
  set(list: T[]) {
    if (list.length) this.#starting();
    this.items = list;
    this.anchor = list.at(-1) ?? null;
  }

  /** Picks `list` too (a box drawn with Shift), after what's picked already. */
  add(list: T[]) {
    const more = list.filter((i) => !this.has(i));
    if (more.length) this.set([...this.items, ...more]);
  }

  /** A box (`dragBox`) begins. */
  beginBox() {
    this.#base = this.items;
  }

  /** What the box holds now, `found` (in on-screen order), picked: after what was before it, if `adding`. */
  boxed(found: T[], adding: boolean) {
    const base = adding ? this.#base : [];
    this.set([...base, ...found.filter((f) => !base.some((b) => this.#same(b, f)))]);
  }

  /** A plain click on `item` (it opens or plays, as ever): the set stays,
   *  and with none, a Shift-click after runs from it. */
  plain(item: T) {
    if (!this.items.length) this.anchor = item;
  }

  /** A plain click where it picks (a clip bar's pill, as in a file manager):
   *  `item` and only it, or, if it's picked alone already, nothing. */
  only(item: T) {
    if (this.items.length === 1 && this.has(item)) this.clear();
    else this.set([item]);
  }

  /** Keeps only what `still` says is there yet (a cut moved, a clip deleted another way). */
  keep(still: (item: T) => boolean) {
    if (this.items.every(still)) return;
    this.items = this.items.filter(still);
    if (this.anchor !== null && !still(this.anchor)) this.anchor = null;
  }

  clear() {
    this.items = [];
    this.anchor = null;
  }
}

const near = (a: number, b: number) => Math.abs(a - b) < 1e-6;
const sameSnippet = (a: Snippet, b: Snippet) =>
  a.kind === "segment" && b.kind === "segment"
    ? near(a.start, b.start) && near(a.end, b.end)
    : a.kind === "clip" && b.kind === "clip" && a.id === b.id;
const sameEntry = (a: Entry, b: Entry) => a.kind === b.kind && a.id === b.id;

/** How each stretch selection is let go, handed over by its owner. */
const stretches: (() => void)[] = [];

export const snippets: Selection<Snippet> = new Selection(sameSnippet, () => {
  library.clear();
  stretches.forEach((letGo) => letGo());
});
export const library: Selection<Entry> = new Selection(sameEntry, () => {
  snippets.clear();
  stretches.forEach((letGo) => letGo());
});

/** `letGo` lets go of a stretch selection: called whenever a set starts. */
export function clearsStretch(letGo: () => void) {
  stretches.push(letGo);
}

/** A stretch selection was made, or a mark selected: the sets go. */
export function stretchMade() {
  snippets.clear();
  library.clear();
}

/** Whether a click adds or takes out one (Ctrl, or ⌘ on a Mac, as the key map's defaults). */
export const toggles = (e: MouseEvent | PointerEvent) => (os === "mac" ? e.metaKey : e.ctrlKey);
/** Whether a click picks a set at all: a Ctrl (⌘) or a Shift one. */
export const picks = (e: MouseEvent | PointerEvent) => toggles(e) || e.shiftKey;

/** What to say a set holds: "3 clips", "2 VODs and 1 edit". */
export function count(parts: [number, string, string][]): string {
  return listed(parts.filter(([n]) => n > 0).map(([n, one, many]) => `${n} ${n === 1 ? one : many}`));
}

/**
 * A box dragged out from empty space in `node` (a scrolling list), drawn as
 * it goes: what `hits` finds in it (by where things are on screen, their
 * `getBoundingClientRect`) is picked into `set`, after what it held if Shift
 * was held as it started. A press on anything but `node` itself (a pill, a row, a
 * button) is left to its own handling: that's how carrying a pill and
 * dragging a row to reorder stay as they are. A click there, not dragged,
 * is `onempty` (it lets the set go).
 */
export function dragBox<T>(
  node: HTMLElement,
  options: { set: Selection<T>; hits: (box: DOMRect) => T[]; onempty: () => void },
) {
  let opts = options;
  let press: { x: number; y: number; adding: boolean } | null = null;
  let drawn: HTMLDivElement | null = null;

  const boxOf = (e: PointerEvent) => {
    const p = press!;
    return new DOMRect(
      Math.min(p.x, e.clientX),
      Math.min(p.y, e.clientY),
      Math.abs(e.clientX - p.x),
      Math.abs(e.clientY - p.y),
    );
  };
  function down(e: PointerEvent) {
    // On the list itself, not on its scrollbar.
    if (e.button !== 0 || e.target !== node || e.offsetX >= node.clientWidth || e.offsetY >= node.clientHeight) return;
    press = { x: e.clientX, y: e.clientY, adding: e.shiftKey };
    e.preventDefault();
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", done);
  }
  function move(e: PointerEvent) {
    if (!press) return;
    // Its release missed (it happened elsewhere): over.
    if (!(e.buttons & 1)) return done();
    const box = boxOf(e);
    if (!drawn && Math.hypot(box.width, box.height) < 4) return;
    if (!drawn) {
      drawn = document.createElement("div");
      drawn.className = "select-box";
      document.body.append(drawn);
      opts.set.beginBox();
    }
    Object.assign(drawn.style, {
      left: `${box.left}px`,
      top: `${box.top}px`,
      width: `${box.width}px`,
      height: `${box.height}px`,
    });
    opts.set.boxed(opts.hits(box), press.adding);
  }
  function up() {
    // Picked as the box last was; a press not dragged is a click on empty space.
    if (press && !drawn) opts.onempty();
    done();
  }
  function done() {
    press = null;
    drawn?.remove();
    drawn = null;
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", done);
  }
  node.addEventListener("pointerdown", down);
  return {
    update(next: typeof options) {
      opts = next;
    },
    destroy() {
      done();
      node.removeEventListener("pointerdown", down);
    },
  };
}

/** Whether `a` and `b` (on screen) overlap. */
export const overlaps = (a: DOMRect, b: DOMRect) =>
  a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
