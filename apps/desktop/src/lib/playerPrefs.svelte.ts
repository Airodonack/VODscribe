// What every player remembers, the same in all of them: the volume, the
// speed, and whether captions show. A VOD's and an edit's player read and
// change one copy, kept on this computer (stored.ts).

import { load, save } from "./stored";

const VOLUME = "vodscribe.volume";
const SPEED = "vodscribe.speed";
const CAPTIONS = "vodscribe.captions";

/** The number kept under `key`, if it's one between `least` and `most`. */
function kept(key: string, least: number, most: number): number | null {
  const saved = load(key);
  const n = Number(saved);
  return saved !== null && n >= least && n <= most ? n : null;
}

class PlayerPrefs {
  captions = $state(load(CAPTIONS) !== "false");
  #volume = $state(kept(VOLUME, 0, 1));
  #speed = $state(kept(SPEED, 0.25, 4));

  /** The volume last set (0 to 1), or null if none was. */
  volume(): number | null {
    return this.#volume;
  }

  setVolume(volume: number) {
    this.#volume = volume;
    save(VOLUME, String(volume));
  }

  /** The speed last set, or null if none was. */
  speed(): number | null {
    return this.#speed;
  }

  setSpeed(speed: number) {
    this.#speed = speed;
    save(SPEED, String(speed));
  }

  toggleCaptions() {
    this.captions = !this.captions;
    save(CAPTIONS, String(this.captions));
  }
}

export const prefs = new PlayerPrefs();
