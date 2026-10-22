// A site's player played or paused (embed.ts bridge), passed on to the app's
// player only once it has settled: Twitch's, on a user's play, plays its
// element and pauses it at once (to have the gesture count for later plays),
// then plays it for real, which may come a while after (data to fetch); the
// app's player shouldn't pause meanwhile.

/** How long a play or pause must stand before it's passed on (ms). */
export const SETTLE = 30;

/** Passes on `forward(playing)` the last of what's wanted within `ms` of each
 *  other; always, even if the same as before (the app's player may have been
 *  paused otherwise, as for an ad). */
export function settling(forward: (playing: boolean) => void, ms = SETTLE) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  return {
    want(playing: boolean) {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        forward(playing);
      }, ms);
    },
    stop() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
}
