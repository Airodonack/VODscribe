// Where the timeline's thumbnail (ScrubFrame.svelte) and its time tip go,
// and which moment they show: the arithmetic, apart, to test.

/** The thumbnail's height in CSS pixels: inside the timeline's 48 px bar. */
export const FRAME_HEIGHT = 44;
/** How far in from the timeline's ends the tip's middle stays, alone. */
export const TIP_INSET = 28;
/** While a storyboard is still being made, a stored frame stands in for the one at or before the pointer if it's
 *  within this many pixels of the pointer's time (or the storyboard's interval). */
export const NEAR_PX = 30;

/**
 * Where the thumbnail (`frame` pixels wide) and the tip (`tip` wide) go for
 * the hover line at `x` on a timeline `width` wide: their left edges. Both
 * centred on the line, the tip over the thumbnail's lower edge (as a video
 * site lays the time on its preview), shifted in whole rather than cut where
 * the wider of the two would pass the timeline's ends (its left end first,
 * on a timeline narrower than it). With no thumbnail, the tip is centred on
 * the line, kept `TIP_INSET` from the ends.
 */
export function place(x: number, width: number, tip: number, frame: number) {
  if (frame <= 0) {
    const middle = Math.min(Math.max(x, TIP_INSET), width - TIP_INSET);
    return { frame: null, tip: middle - tip / 2 };
  }
  const half = Math.max(frame, tip) / 2;
  const middle = Math.max(half, Math.min(x, width - half));
  return { frame: middle - frame / 2, tip: middle - tip / 2 };
}

/** Seconds `NEAR_PX` pixels cover on a timeline showing `span` seconds `width` pixels wide. */
export function nearWithin(span: number, width: number): number {
  return width > 0 ? (span / width) * NEAR_PX : 0;
}
