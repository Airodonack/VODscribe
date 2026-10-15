// Small things remembered on this computer (localStorage): a choice, a
// setting of the player's. Storage can be missing or refuse (a private
// window, a full disk), and then nothing is remembered, which is fine.

/** The value kept under `key`, or null. */
export function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Keeps `value` under `key`, if it can. */
export function save(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Not remembered this time.
  }
}
