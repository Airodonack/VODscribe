// The transcript following the playhead (Transcript.svelte): where the view
// goes so the line at the playhead stays in view. It holds still while the
// line is well in view, so the text stays put to read, and otherwise puts the
// line a third of the way down, with what's coming below it. Playing, that's
// once a screenful; a jump of the playhead, anywhere, lands there too.

/** Where the view's top goes so the row from `a` to `b` is in view, the view
 *  showing `top` to `top + height` (all in the list's own coordinates): null
 *  if it stays (the row in view and clear of the bottom, or a tall row starting
 *  high enough), else the new top, and whether that's `far`, more than a
 *  screen away (a jump: there at once, where a short move glides). */
export function followTarget(a: number, b: number, top: number, height: number): { top: number; far: boolean } | null {
  if (height <= 0) return null;
  if (a >= top && (b <= top + height * 0.9 || a <= top + height / 3)) return null;
  const to = a - height / 3;
  return { top: to, far: Math.abs(to - top) > height };
}
