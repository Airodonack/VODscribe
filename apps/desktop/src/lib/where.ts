// Where Ctrl+A (⌘A) selects and Ctrl+V (⌘V) pastes, by one rule (bulk.ts
// `whereKeys` asks it with the page as it is): over the clip bar, its
// clips; over the library's pane, its list; anywhere else on the main side
// (the player, the timeline, the transcript), the timeline of what's open.
// "Over" is where the pointer is; before the pointer has moved, where the
// focus is. And where in it a paste puts what it brings (`insertionPoint`):
// after what's selected, the playhead only when nothing is. Pure:
// node --test apps/desktop/src/lib/where.test.mjs.

/** What a key acts on: the library's list, the clip bar, or the timeline. */
export type Where = "library" | "clips" | "timeline";

/** A box on screen, as `getBoundingClientRect` gives it. */
export type Box = { left: number; right: number; top: number; bottom: number };

export type Page = {
  /** Where the pointer last was in the window; null before it has moved. */
  pointer: { x: number; y: number } | null;
  /** The library's pane (on the left of the window), if it's there. */
  pane: Box | null;
  /** The clip bar, if one shows. */
  bar: Box | null;
  /** What has the focus: in the library's list, in the clip bar, or elsewhere. */
  focus: "library" | "clips" | "other";
  /** Whether a VOD or an edit is open with its timeline. */
  timeline: boolean;
};

const inside = (box: Box, x: number, y: number) => x >= box.left && x < box.right && y >= box.top && y < box.bottom;

/** What Ctrl+A or Ctrl+V acts on in `page`; null if nothing's there to (the
 *  main side with nothing open: the library's page). */
export function where(page: Page): Where | null {
  const { pointer, pane, bar } = page;
  const side: Where = pointer
    ? pane && pointer.x < pane.right
      ? "library"
      : bar && inside(bar, pointer.x, pointer.y)
        ? "clips"
        : "timeline"
    : page.focus === "library"
      ? "library"
      : page.focus === "clips"
        ? "clips"
        : "timeline";
  if (side === "clips" && !bar) return page.timeline ? "timeline" : null;
  if (side === "timeline" && !page.timeline) return null;
  return side;
}

/** What's selected, as `insertionPoint` reads it: one kind at a time
 *  (selection.svelte.ts), all of the timeline and clip bar open. */
export type Selected = {
  /** The stretch selected on the timeline (I and O, lines, Ctrl+A). */
  stretch: { start: number; end: number } | null;
  /** What's picked, in the order picked: stretches between cuts of the
   *  timeline, and clips of the clip bar. */
  picked: readonly ({ kind: "segment"; start: number; end: number } | { kind: "clip"; id: number })[];
  /** The moment of the mark or cut selected. */
  mark: number | null;
  /** Where the timeline's player is. */
  playhead: number;
};

/** Where a paste goes: a moment of the timeline, or on the clip bar after
 *  one of its clips (null: after them all). */
export type Insertion = { at: number } | { after: number | null };

/**
 * Where a paste puts what it brings, on the timeline or on the clip bar
 * (`bar`: its clips' ids, in their order): after what's selected, never in
 * its place. On a timeline, after the stretch selected, after the last
 * stretch picked (in the timeline's order, whatever order they were picked
 * in), at the mark selected, else at the playhead: the playhead is where the
 * video plays, a target only when nothing is selected. On a clip bar, after
 * the last clip picked (in the bar's order), else after them all.
 */
export function insertionPoint(on: "timeline" | "bar", selected: Selected, bar: readonly number[] = []): Insertion {
  if (on === "bar") {
    const last = Math.max(-1, ...selected.picked.map((p) => (p.kind === "clip" ? bar.indexOf(p.id) : -1)));
    return { after: last < 0 ? null : bar[last] };
  }
  if (selected.stretch) return { at: selected.stretch.end };
  const ends = selected.picked.flatMap((p) => (p.kind === "segment" ? [p.end] : []));
  if (ends.length) return { at: Math.max(...ends) };
  return { at: selected.mark ?? selected.playhead };
}
