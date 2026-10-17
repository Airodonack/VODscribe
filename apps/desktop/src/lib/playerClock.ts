// A player's position on the page: where it was last told to be or said it
// was, run on by the page's clock while it plays (`now`), and, apart from
// that, where it really is as near as its own reports say (`shown`), which
// the timeline's solid playhead follows (Scrubber.svelte). For the sites'
// embeds (embed.ts), which report a few times a second, and the edit
// player's page side (editPlayer.ts).

/** The players' position as last reported, advanced by the clock while playing. */
export class Clock {
  private at = performance.now();
  /** Seconds of the media a second of play is. */
  speed = 1;
  /** What the player itself last said: where it was, when, and whether it
   *  was moving (playing, not buffering). */
  private heard: { seconds: number; at: number; moving: boolean } | null = null;
  private seconds: number;
  playing: boolean;
  constructor(seconds: number, playing: boolean) {
    this.seconds = seconds;
    this.playing = playing;
  }
  set(seconds: number) {
    this.seconds = seconds;
    this.at = performance.now();
  }
  now() {
    return this.seconds + (this.playing ? ((performance.now() - this.at) / 1000) * this.speed : 0);
  }
  /** The player said it's at `seconds`, moving or not (unknown: as it last said). */
  heardAt(seconds: number, moving?: boolean) {
    this.set(seconds);
    this.heard = { seconds, at: performance.now(), moving: moving ?? this.heard?.moving ?? false };
  }
  /** The player said whether it's moving, without a position. */
  heardMoving(moving: boolean) {
    if (this.heard) this.heard = { seconds: this.shown()!, at: performance.now(), moving };
  }
  /** Where the player really is, as near as its reports say: run on from the
   *  last only while it said it was moving, and at most `STALE` seconds past
   *  it, so a player that stalls or pauses without saying stops showing as
   *  moving. Null before it has said anything. */
  shown(): number | null {
    if (!this.heard) return null;
    const run = this.heard.moving ? Math.min((performance.now() - this.heard.at) / 1000, STALE) : 0;
    return this.heard.seconds + run * this.speed;
  }
}

/** How far past an embed's last report its position is taken to run (it
 *  reports a few times a second while playing). */
const STALE = 0.5;
