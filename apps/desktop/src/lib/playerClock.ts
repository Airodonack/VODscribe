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
  /** The position the player last reported, as it said it. */
  private said: number | null = null;
  /** A seek asked of the player and not yet seen through: to `target`,
   *  playing after it or not, given up at `until` (ms); `seen`: reported
   *  near the target since. */
  private seeking: { target: number; play: boolean; until: number; seen: boolean } | null = null;
  private seconds: number;
  private isPlaying: boolean;
  /** Its time runs only once the player has said it's moving: a site's
   *  player says it plays as soon as it's asked to, then buffers before its
   *  picture moves; run on meanwhile, the time went ahead, then back. */
  private waits: boolean;
  constructor(seconds: number, playing: boolean, { waitsForPlayer = false } = {}) {
    this.seconds = seconds;
    this.isPlaying = playing;
    this.waits = waitsForPlayer;
  }
  /** Whether it plays (or is about to: buffering). The time goes on from
   *  where it is as this changes. */
  get playing() {
    return this.isPlaying;
  }
  set playing(playing: boolean) {
    this.keeping(() => (this.isPlaying = playing));
  }
  set(seconds: number) {
    this.seconds = seconds;
    this.at = performance.now();
  }
  /** Whether its time runs now. */
  private running() {
    return this.isPlaying && (!this.waits || this.heard?.moving === true);
  }
  now() {
    return this.seconds + (this.running() ? ((performance.now() - this.at) / 1000) * this.speed : 0);
  }
  /** Makes `change`, the time going on from where it was, if it starts or
   *  stops running: not leaping by the time it stood, nor back by the time
   *  it ran. */
  private keeping(change: () => void) {
    const [time, was] = [this.now(), this.running()];
    change();
    if (this.running() !== was) this.set(time);
  }
  private hear(heard: { seconds: number; at: number; moving: boolean }) {
    this.keeping(() => (this.heard = heard));
  }
  /** Plays `speed` times as fast from now: both positions run on from
   *  where they are, rather than jumping. */
  setSpeed(speed: number) {
    if (speed === this.speed) return;
    this.set(this.now());
    const shown = this.shown();
    if (this.heard && shown !== null) this.heard = { ...this.heard, seconds: shown, at: performance.now() };
    this.speed = speed;
  }
  /** Asks the player to go to `seconds` (`play`: playing there): the page's
   *  clock goes there at once, and until the player is seen there, the
   *  positions it reports from anywhere else are taken as the target, and
   *  only its state believed. Twitch, after a seek, reports the old place
   *  while buffering there, and still as it starts playing there; paused,
   *  it reports the old place once, Idle, and then nothing until played. */
  asked(seconds: number, play: boolean) {
    this.set(seconds);
    // The picture stands where it was while the player seeks.
    const shown = this.shown();
    if (this.heard && shown !== null) this.heard = { seconds: shown, at: performance.now(), moving: false };
    this.seeking = { target: seconds, play, until: performance.now() + SEEK_GIVE_UP * 1000, seen: false };
  }
  /** The player said it's at `seconds`, moving or not (unknown: as it last said). */
  heardAt(seconds: number, moving?: boolean) {
    const now = performance.now();
    // Twitch says its last position again: a second time each second while
    // playing, and once after a seek (the place before it). Said again, it's
    // where the player was, not where it is.
    const again = seconds === this.said;
    this.said = seconds;
    if (this.seeking && now > this.seeking.until) this.seeking = null;
    if (this.seeking) {
      const target = this.seeking.target;
      if (again || Math.abs(seconds - target) > NEAR) {
        // At the target, or where it's been seen near it since; moving or not as it says.
        const shown = this.shown();
        const there = this.seeking.seen && shown !== null ? shown : target;
        this.hear({ seconds: there, at: now, moving: moving ?? this.heard?.moving ?? false });
        // Not moving there yet: the time stands at the target, not where it had run on to.
        if (!this.heard!.moving) this.set(this.seeking.target);
        return;
      }
      // There: the seek is through once it plays there, or at once if it stays paused.
      this.seeking.seen = true;
      if (!this.seeking.play || moving) this.seeking = null;
    }
    // Playing still, it's run on from where it first said.
    if (again && this.heard?.moving && (moving ?? true)) return;
    // Stopped, said with the place it last said while moving (Twitch, as it
    // pauses): it stopped where it had run on to, as near as that says,
    // not back there; its true place, if said after, counts.
    if (again && this.heard?.moving && moving === false) {
      const ran = this.shown() ?? seconds;
      this.set(ran);
      this.heard = { seconds: ran, at: now, moving: false };
      return;
    }
    this.set(seconds);
    this.heard = { seconds, at: now, moving: moving ?? this.heard?.moving ?? false };
  }
  /** The player said whether it's moving, without a position. */
  heardMoving(moving: boolean) {
    if (this.heard) this.hear({ seconds: this.shown()!, at: performance.now(), moving });
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
