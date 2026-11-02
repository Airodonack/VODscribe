// Short mode: a window too short for the player and the transcript together
// (a maximized window on a 1366 by 768 laptop, a 13" Mac with larger text)
// shows the player alone in the main column, with its timeline and bar, and
// the transcript, chat and pane not drawn. The player's header button swaps
// the two (Player.svelte): the transcript and chat then take the column, and
// stay there as other VODs open, for the rest of the session.

/** Under this many pixels for the transcript, it's too short to read. */
export const PANE_LEAST = 200;
/** The column's height besides the picture and the transcript: the toolbar
 *  (60 px), the player's header (40 px), and its timeline and bar (98 px),
 *  as measured on macOS and Windows. */
export const PLAYER_REST = 200;

export const shortMode = $state({
  /** The window is short (App.svelte, from the column's size). */
  on: false,
  /** In short mode, the transcript and chat shown in the player's place. */
  pane: false,
  /** Off for the end-to-end tests, written for a page with room for both:
   *  on a short screen (CI's hosted Mac) the harness turns it off, except
   *  where a scenario tests it (tests/e2e/lib/app.mjs). */
  off: false,
});

/** Whether a column `width` by `height` is short: with the picture at its
 *  usual size (16:9, at most `pictureMost` tall), the transcript under it
 *  would get less than PANE_LEAST. */
export function isShort(width: number, height: number, pictureMost: number): boolean {
  if (shortMode.off || height <= 0) return false;
  const picture = Math.min((width * 9) / 16, pictureMost);
  return height - PLAYER_REST - picture < PANE_LEAST;
}

// The harness's word, whether this module loads before it or after.
if (typeof window !== "undefined") {
  const e2e = (window as { __e2e?: { shortMode?: boolean } }).__e2e;
  if (e2e?.shortMode === false) shortMode.off = true;
  window.addEventListener("vodscribe-e2e-short", (e) => {
    shortMode.off = !(e as CustomEvent<boolean>).detail;
  });
}
