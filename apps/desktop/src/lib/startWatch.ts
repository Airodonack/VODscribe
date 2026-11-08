// Whether a site's player refused to start (Twitch's, embed.ts): asked to
// play, it says it's ready (or, paused before, idle) and stays so, never
// playing. Twitch does that for
// reasons of its own: a player too small (it says 400 by 300 at least), one it
// counts as covered, or rules it adds. Only a press on its own Play starts
// it then, which the page asks for (Player.svelte).

/** How long a player asked to play may stay ready before that's a refusal.
 *  Counted from when it said it was ready (or from the ask, if later), not
 *  from the ask alone: how long its frame takes to load is the network's
 *  and the machine's. The test VOD on Twitch (Linux, 2026-11-07, nine
 *  starts) played 3.0-4.3 s after Play, ready 0.1-0.9 s of that before it
 *  buffered; refusing, it stayed ready for good. Five times the longest. */
export const WAIT_MS = 5000;

export class StartWatch {
  /** When it was asked to play and hasn't since; null: not waiting. */
  #asked: number | null = null;
  /** What it last said it's doing ("Idle", "Ready", "Buffering", "Playing"…), and since when. */
  #state: string | null = null;
  #since = 0;
  /** Whether it has loaded (said it was ready, or played): its Idle then
   *  means paused, where before it means loading. */
  #loaded = false;
  #now: () => number;

  constructor(now: () => number = () => performance.now()) {
    this.#now = now;
  }

  /** Asked to play: waited on from the first ask, not the last (a second
   *  Play on the bar doesn't start the wait over). */
  play() {
    if (this.#state !== "Playing") this.#asked ??= this.#now();
  }

  /** Asked to pause: nothing to wait for. */
  pause() {
    this.#asked = null;
  }

  /** It said what it's doing (said again, the same, it's still so since before). */
  heard(state: string) {
    if (state !== this.#state) this.#since = this.#now();
    this.#state = state;
    if (state !== "Idle") this.#loaded = true;
    if (state === "Playing") this.#asked = null;
  }

  /** Its frame was loaded again (the bridge's plain reload): what the old
   *  one said no longer holds. */
  reloaded() {
    this.#state = null;
    this.#loaded = false;
  }

  /** Whether it was asked to play and has been only ready, or paused, for
   *  the wait. Loading or buffering isn't a refusal, however long it takes. */
  refused(): boolean {
    const waiting = this.#state === "Ready" || (this.#state === "Idle" && this.#loaded);
    if (this.#asked === null || !waiting) return false;
    return this.#now() - Math.max(this.#asked, this.#since) >= WAIT_MS;
  }
}
