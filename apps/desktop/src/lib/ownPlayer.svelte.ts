// Whether a VOD from a site (YouTube, Twitch, Kick) plays in the app's own
// player (NativePlayer.svelte; player.rs plays it from footage fetched as it
// plays) or in the site's (embed.ts). The app's own wherever it can draw
// (asked once, at start), frame-exact as for a file; the site's where it
// can't, for a VOD that's only sound (the app's own needs a picture), and
// for one whose footage couldn't be had at all (a bot check, a VOD only
// signed-in viewers see) for the rest of the session.

import { SvelteSet } from "svelte/reactivity";
import { api } from "./api";

class OwnPlayer {
  /** Whether the app's own player draws here; null until known. */
  here = $state<boolean | null>(null);
  /** VODs from sites whose footage couldn't be had: they play in the site's player. */
  fellBack = new SvelteSet<number>();
  #asked: Promise<boolean> | null = null;

  /** Whether the app's own player draws here, asked once. */
  ask(): Promise<boolean> {
    this.#asked ??= api
      .playerAvailable()
      .catch(() => false)
      .then((here) => (this.here = here));
    return this.#asked;
  }
}

export const ownPlayer = new OwnPlayer();
