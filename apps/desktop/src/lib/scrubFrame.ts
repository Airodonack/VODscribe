// Where the timeline's thumbnail (ScrubFrame.svelte) and its time tip go,
// and which moment they show: the arithmetic, apart, to test.

/** The thumbnail's height in CSS pixels: inside the timeline's 48 px bar. */
export const FRAME_HEIGHT = 44;
/** How far in from the timeline's ends the tip's middle stays, alone. */
export const TIP_INSET = 28;
/** While a storyboard is still being made, a stored frame stands in for the one at or before the pointer if it's
 *  within this many pixels of the pointer's time (or the storyboard's interval). */
export const NEAR_PX = 30;

/** The gap between the hover line and the thumbnail beside it. */
export const FRAME_GAP = 8;

/**
 * Where the thumbnail (`frame` pixels wide) and the tip (`tip` wide) go for
 * the hover line at `x` on a timeline `width` wide: their left edges. The
 * tip is centred on the line, kept `TIP_INSET` from the ends. The thumbnail
 * sits to the left of the line, `FRAME_GAP` off it, so it never stands
 * under the pointer; where the timeline's left end leaves it no room it
 * sits to the right instead, and it never passes the right end.
 */
export function place(x: number, width: number, tip: number, frame: number) {
  const middle = Math.min(Math.max(x, TIP_INSET), width - TIP_INSET);
  if (frame <= 0) return { frame: null, tip: middle - tip / 2 };
  const left = x - FRAME_GAP - frame;
  const at = left >= 0 ? left : Math.max(0, Math.min(x + FRAME_GAP, width - frame));
  return { frame: at, tip: middle - tip / 2 };
}

/** Seconds `NEAR_PX` pixels cover on a timeline showing `span` seconds `width` pixels wide. */
export function nearWithin(span: number, width: number): number {
  return width > 0 ? (span / width) * NEAR_PX : 0;
}
