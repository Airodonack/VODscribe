// Whether a VOD from a site (YouTube, Twitch, Kick) plays in the app's own
// player (NativePlayer.svelte; player.rs plays it from footage fetched as it
// plays) or in the site's (embed.ts). The app's own wherever it can draw
// (asked once, at start), frame-exact as for a file; the site's where it
// can't, for a VOD that's only sound (the app's own needs a picture), and
// for one whose footage couldn't be had at all (a bot check, a VOD only
// signed-in viewers see) for the rest of the session.

import { SvelteSet } from "svelte/reactivity";
import { api } from "./api";

/** When a no is asked again, in milliseconds after each. */
const RETRIES = [500, 1500, 3000, 10000];

class OwnPlayer {
  /** Whether the app's own player draws here; null until known. */
  here = $state<boolean | null>(null);
  /** VODs from sites whose footage couldn't be had: they play in the site's player. */
  fellBack = new SvelteSet<number>();
  #asked: Promise<boolean> | null = null;

  /** Whether the app's own player draws here. Asked once; a no is asked
   *  again a few times: on macOS the player's screen is put in the window
   *  after the page has started, and may not be there yet when it asks. */
  ask(): Promise<boolean> {
    this.#asked ??= this.#askUntil(RETRIES);
    return this.#asked;
  }

  async #askUntil(retries: number[]): Promise<boolean> {
    const here = await api.playerAvailable().catch(() => false);
    this.here = here;
    if (!here && retries.length) {
      setTimeout(
        () => void this.#askUntil(retries.slice(1)).then((h) => h && (this.#asked = Promise.resolve(true))),
        retries[0],
      );
    }
    return here;
  }
}

export const ownPlayer = new OwnPlayer();
