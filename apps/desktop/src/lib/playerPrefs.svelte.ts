// What every player remembers, the same in all of them: the volume, and
// whether captions show. A VOD's and an edit's player read and change one
// copy, kept on this computer (stored.ts).

import { load, save } from "./stored";

const VOLUME = "vodscribe.volume";
const CAPTIONS = "vodscribe.captions";

class PlayerPrefs {
  captions = $state(load(CAPTIONS) !== "false");

  /** The volume last set (0 to 1), or null if none was. */
  volume(): number | null {
    const saved = Number(load(VOLUME));
    return load(VOLUME) !== null && saved >= 0 && saved <= 1 ? saved : null;
  }

  setVolume(volume: number) {
    save(VOLUME, String(volume));
  }

  toggleCaptions() {
    this.captions = !this.captions;
    save(CAPTIONS, String(this.captions));
  }
}

export const prefs = new PlayerPrefs();
