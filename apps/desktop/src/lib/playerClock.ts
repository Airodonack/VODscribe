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
  /** A seek asked of the player and not yet seen through: to `target`,
   *  playing after it or not, given up at `until` (ms). */
  private seeking: { target: number; play: boolean; until: number } | null = null;
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
  /** Asks the player to go to `seconds` (`play`: playing there): the page's
   *  clock goes there at once, and until the player is seen there, what it
   *  says of anywhere else is set aside (Twitch, reloading a long VOD for a
   *  seek, reports 0 or the old place meanwhile). */
  asked(seconds: number, play: boolean) {
    this.set(seconds);
    this.seeking = { target: seconds, play, until: performance.now() + SEEK_GIVE_UP * 1000 };
  }
  /** The player said it's at `seconds`, moving or not (unknown: as it last said). */
  heardAt(seconds: number, moving?: boolean) {
    const now = performance.now();
    if (this.seeking && now > this.seeking.until) this.seeking = null;
    if (this.seeking) {
      if (Math.abs(seconds - this.seeking.target) > NEAR) return;
      // There: the seek is through once it plays there, or at once if it stays paused.
      if (!this.seeking.play || moving) this.seeking = null;
    }
    this.set(seconds);
    this.heard = { seconds, at: now, moving: moving ?? this.heard?.moving ?? false };
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

/** How far past an embed's last report its position is taken to run:
 *  Twitch reports once a second while playing. */
const STALE = 1.5;
/** How near a seek's target a report must be to count as there, and how
 *  long reports from elsewhere are set aside before the seek is given up on. */
const NEAR = 2;
const SEEK_GIVE_UP = 15;
