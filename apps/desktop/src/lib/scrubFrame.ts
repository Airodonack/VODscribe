// Where the timeline's thumbnail (ScrubFrame.svelte) and its time tip go,
// and which moment they show: the arithmetic, apart, to test.

/** The thumbnail's height in CSS pixels: inside the timeline's 48. */
export const FRAME_HEIGHT = 44;
/** Between the hover line and the thumbnail, and between it and the tip. */
export const GAP = 4;
/** How far in from the timeline's ends the tip's middle stays, alone. */
export const TIP_INSET = 28;
/** A stored frame shows if it's within this many pixels of the pointer's time (or its storyboard's interval). */
export const NEAR_PX = 30;

/**
 * Where the thumbnail (`frame` pixels wide) and the tip (`tip` wide) go for
 * the hover line at `x` on a timeline `width` wide: their left edges. Beside
 * the line, the thumbnail nearest it, to its right; on its left where they'd
 * pass the timeline's right end. With no thumbnail, the tip is centred on
 * the line, kept `TIP_INSET` from the ends.
 */
export function place(x: number, width: number, tip: number, frame: number) {
  if (frame <= 0) {
    const middle = Math.min(Math.max(x, TIP_INSET), width - TIP_INSET);
    return { frame: null, tip: middle - tip / 2, flipped: false };
  }
  const group = frame + GAP + tip;
  if (x + GAP + group <= width) return { frame: x + GAP, tip: x + GAP + frame + GAP, flipped: false };
  // On the left of the line, but never past the timeline's left end.
  const left = Math.max(0, x - GAP - group);
  return { frame: left + tip + GAP, tip: left, flipped: true };
}

/** Seconds `NEAR_PX` pixels cover on a timeline showing `span` seconds `width` pixels wide. */
export function nearWithin(span: number, width: number): number {
  return width > 0 ? (span / width) * NEAR_PX : 0;
}
