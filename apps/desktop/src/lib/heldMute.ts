// A mute set on the bar before a site's player takes one (Twitch's, before
// it says ready: embed.ts): sent again once it's ready, and shown on the bar
// in place of the player's own word till the player reports it, or for a
// few seconds at most: a player that never takes it would otherwise keep
// the bar muted while it plays with sound, and its own controls unheard.

/** How long after it's sent at ready (or set again since) a held mute stands. */
const WAIT_MS = 3000;

export class HeldMute {
  /** Whether the bar shows the sound muted. */
  muted = false;
  #held: boolean | null = null;
  #ready = false;
  #until = 0;
  #now: () => number;

  constructor(now: () => number = () => performance.now()) {
    this.#now = now;
  }

  /** Muted (or not) on the bar. */
  set(muted: boolean) {
    this.muted = muted;
    if (this.#ready && this.#held === null) return;
    this.#held = muted;
    this.#until = this.#now() + WAIT_MS;
  }

  /** The player is ready: the mute to send it again, if one is held. */
  ready(): boolean | null {
    this.#ready = true;
    this.#until = this.#now() + WAIT_MS;
    return this.#held;
  }

  /** The player reported its mute. */
  heard(muted: boolean) {
    if (this.#held !== null && this.#ready && (muted === this.#held || this.#now() > this.#until)) this.#held = null;
    if (this.#held === null) this.muted = muted;
  }
}
