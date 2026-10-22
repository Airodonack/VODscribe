// Whether the app's own player draws here (asked once, at start): files
// play in it (NativePlayer.svelte), and, in a build with the embed-takeover
// feature, a site's VOD has its picture drawn by it under the site's player
// (embed.ts); where it can't, files play in the web view. A site's VOD
// otherwise plays in the site's player alone.

import { api } from "./api";

/** When a no is asked again, in milliseconds after each. */
const RETRIES = [500, 1500, 3000, 10000];

class OwnPlayer {
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
