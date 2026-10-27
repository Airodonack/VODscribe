// What a click or a key does to a set of things picked, as in a file manager
// (selection.svelte.ts keeps the set; this is the rule, with no state of its
// own, so it's tested by itself: pick.test.mjs).
//
// A set is what's picked, in the order it was, and the anchor, where a
// Shift-click's run starts. Where a list has an open (current) item, the
// library's open row, it's picked by itself when nothing else is: a plain
// click opens a row and picks it alone, so a Ctrl-click then adds another to
// it, and letting go of the set (Escape, a click on empty space) leaves the
// open row picked, as a file manager leaves the item clicked selected.
//
// - A plain click: that item alone, and the anchor.
// - A Ctrl-click (⌘ on a Mac): that item in, or out again; the anchor.
// - A Shift-click: the run from the anchor to here, in the order on screen,
//   in place of what was picked; the anchor stays. With Ctrl too, the run is
//   added to what was picked.
// - The arrow keys: the next item (or the one before) alone, the anchor; with
//   Shift, the run from the anchor to it, the anchor staying.

export type Same<T> = (a: T, b: T) => boolean;

/** What's picked, in the order it was, and where a Shift-click's run starts. */
export interface Picked<T> {
  items: T[];
  anchor: T | null;
}

/** How a click picks: plain, Ctrl (or ⌘), Shift, or Ctrl+Shift. */
export type How = "plain" | "toggle" | "range" | "addRange";

/** The way a click with these keys held picks. */
export function howOf(toggles: boolean, shift: boolean): How {
  if (shift) return toggles ? "addRange" : "range";
  return toggles ? "toggle" : "plain";
}

/** The set at rest: the open item alone, if there's one. */
export function resting<T>(current: T | null): Picked<T> {
  return { items: current === null ? [] : [current], anchor: current };
}

/** Whether `set` holds more than its rest (the open item alone, or nothing):
 *  then it's several things picked to act on, not just the one open. */
export function picking<T>(set: Picked<T>, current: T | null, same: Same<T>): boolean {
  if (!set.items.length) return false;
  return !(set.items.length === 1 && current !== null && same(set.items[0], current));
}

/** The run of `order` from `from` to `to` (either way), in `order`'s order; just `to` if `from` isn't in it. */
function run<T>(order: T[], from: T | null, to: number, same: Same<T>): T[] {
  const at = from === null ? -1 : order.findIndex((o) => same(o, from));
  const start = at < 0 ? to : at;
  return order.slice(Math.min(start, to), Math.max(start, to) + 1);
}

/** `set` after a click on `item` (`order`: what's on screen, in its order). */
export function click<T>(set: Picked<T>, item: T, how: How, order: T[], same: Same<T>): Picked<T> {
  const has = set.items.some((i) => same(i, item));
  switch (how) {
    case "plain":
      return { items: [item], anchor: item };
    case "toggle":
      return { items: has ? set.items.filter((i) => !same(i, item)) : [...set.items, item], anchor: item };
    case "range":
    case "addRange": {
      const to = order.findIndex((o) => same(o, item));
      if (to < 0) return set;
      const anchor = set.anchor ?? item;
      const span = run(order, anchor, to, same);
      if (how === "range") return { items: span, anchor };
      return { items: [...set.items, ...span.filter((s) => !set.items.some((i) => same(i, s)))], anchor };
    }
  }
}

/** `set` after an arrow key from `from` (the item with the focus, or the open
 *  one) to the next (`by` 1) or the one before (-1); null at an end. */
export function step<T>(
  set: Picked<T>,
  from: T,
  by: 1 | -1,
  extend: boolean,
  order: T[],
  same: Same<T>,
): { set: Picked<T>; to: T } | null {
  const at = order.findIndex((o) => same(o, from));
  const to = at < 0 ? undefined : order[at + by];
  if (to === undefined) return null;
  if (!extend) return { set: { items: [to], anchor: to }, to };
  const anchor = set.anchor ?? from;
  return { set: { items: run(order, anchor, at + by, same), anchor }, to };
}
