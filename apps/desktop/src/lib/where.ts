// Where Ctrl+A (⌘A) selects and Ctrl+V (⌘V) pastes, by one rule (bulk.ts
// `whereKeys` asks it with the page as it is): over the clip bar, its
// clips; over the library's pane, its list; anywhere else on the main side
// (the player, the timeline, the transcript), the timeline of what's open.
// "Over" is where the pointer is; before the pointer has moved, where the
// focus is. Pure: node --test apps/desktop/src/lib/where.test.mjs.

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
