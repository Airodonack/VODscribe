// The page's side of the app's own player (player.rs, crates/player), for
// whichever view plays in it: a file's player (NativePlayer.svelte) and an
// edit's (editPlayer.ts). It keeps what the player last told (its
// `player-time` events and its commands' replies, the newest by `seq`; while
// a command is out, its reply alone), runs
// the time on by the page's clock while the player's runs (after play it
// stands until the sound starts), and sends what's asked; until the player is
// open, a seek, play or speed asked is kept for then.

import { api, events, type PlayerStatus, type UnlistenFn } from "./api";
import { OwnStatus } from "./ownStatus";
import { Clock } from "./playerClock";

/** A status the page took (`seq` null: its own guess), as the tests see it. */
type Taken = { seq: number | null; time: number; playing: boolean };

export class NativeDriver {
  private clock: Clock;
  private isPlaying = false;
  /** What the player last said: where the frame on screen is, the frame rate there. */
  private status = new OwnStatus();
  private unlisten: UnlistenFn | null = null;
  private gone = false;
  private open = false;
  private wanted: { time: number; play: boolean } | null = null;
  private wantedSpeed: number | null = null;
  private loud = 1;
  private quiet = false;
  /** How long what's open is, once known (the time stays within it). */
  length = 0;
  /** Its frame rate at the start, once known. */
  private startFps: number | null = null;
  /** Seeks sent; when the last one's reply came (null while it's out). */
  private seeks = 0;
  private seekSettled: number | null = -Infinity;
  /** Commands out whose reply says where the player is. Meanwhile events
   *  aren't taken: one made before the command, arriving after the page's
   *  guess, would put the time back where it was (seen in CI); the reply,
   *  made after, is what's so. Those after the reply are newer by `seq`. */
  private pending = 0;

  constructor(start = 0) {
    this.clock = new Clock(start, false);
    events
      .playerTime((s) => !this.gone && !this.pending && this.tell(s))
      .then((u) => (this.gone ? u() : (this.unlisten = u)));
  }

  private tell(status: PlayerStatus) {
    // A reply that arrives after a newer event is stale.
    if (!this.status.take(status)) return;
    // For the tests (tests/e2e/scenarios/player-seek.mjs): each status
    // taken, in order, where the test harness made its window.__e2e.
    const e2e = (globalThis as { __e2e?: { taken?: Taken[] } }).__e2e;
    if (e2e) {
      (e2e.taken ??= []).push({ seq: status.seq ?? null, time: status.time, playing: status.playing });
      if (e2e.taken.length > 500) e2e.taken.shift();
    }
    this.clock.set(status.time);
    this.clock.playing = status.running;
    this.clock.speed = status.speed;
    this.isPlaying = status.playing;
  }

  /** Sends a command whose reply says where the player is; resolves true
   *  once that's taken, false if it was refused. */
  private command(sent: Promise<PlayerStatus>): Promise<boolean> {
    this.pending++;
    return sent.then(
      (s) => {
        this.pending--;
        this.tell(s);
        return true;
      },
      () => {
        this.pending--;
        return false;
      },
    );
  }

  /** What the page guesses until the player says: at `time`, not running. */
  private guess(time: number, playing: boolean, speed = this.clock.speed) {
    this.tell({ time, playing, running: false, speed, shown: this.status.shown(), fps: this.status.fps() });
  }

  /** Something else is being opened, at `time`: nothing's sent until it is. */
  opening(time: number) {
    this.open = false;
    this.wanted = null;
    this.guess(time, false);
  }

  /** It opened: what was asked meanwhile is done now. */
  opened({ fps, duration }: { fps: number | null; duration: number }) {
    this.open = true;
    this.startFps = fps;
    this.length = duration;
    api.playerSetVolume(this.quiet ? 0 : this.loud).catch(() => {});
    if (this.wantedSpeed !== null) this.speed.set(this.wantedSpeed);
    if (this.wanted) this.seek(this.wanted.time, this.wanted.play);
    this.wanted = null;
    this.wantedSpeed = null;
  }

  time() {
    const t = this.clock.now();
    return this.length ? Math.min(t, this.length) : t;
  }

  /** Whether its time is moving (playing, and the sound started). */
  running() {
    return this.clock.playing;
  }

  playing() {
    return this.isPlaying;
  }

  shown() {
    return this.status.shown();
  }

  /** The frame rate at the frame shown, else at the start, once known. */
  get fps(): number | null {
    return this.status.fps() ?? this.startFps;
  }

  togglePlay() {
    const playing = !this.isPlaying;
    this.guess(this.time(), playing);
    if (!this.open) {
      this.wanted = { time: this.time(), play: playing };
      return;
    }
    this.command(playing ? api.playerPlay() : api.playerPause());
  }

  /** Goes to `seconds`, and plays if `play`; one that plays goes on playing
   *  (the player doesn't pause for a seek: player.rs). */
  seek(seconds: number, play: boolean) {
    // The picture shows what it did until the player has the new frame.
    if (!this.open) {
      // Not open, what it says of playing may be the last file's (a late
      // event): only what's been asked of this one counts.
      const asked = play || (this.wanted?.play ?? false);
      this.guess(seconds, asked);
      this.wanted = { time: seconds, play: asked };
      return;
    }
    this.guess(seconds, play || this.isPlaying);
    const mine = ++this.seeks;
    this.seekSettled = null;
    this.command(api.playerSeek(seconds, play)).then((took) => {
      if (mine === this.seeks) this.seekSettled = took ? performance.now() : -Infinity;
    });
  }

  /** When the last seek's reply came (performance.now()), null while it's
   *  out, -Infinity if it was refused or none was sent. */
  settled() {
    return this.seekSettled;
  }

  /** Steps `frames` frames on (back, if negative), paused. */
  step(frames: number) {
    if (!this.open) return;
    this.command(api.playerStep(frames));
  }

  speed = {
    get: () => this.clock.speed,
    set: (speed: number) => {
      this.guess(this.time(), this.isPlaying, speed);
      if (!this.open) {
        this.wantedSpeed = speed;
        return;
      }
      this.command(api.playerSetSpeed(speed));
    },
  };

  volume = {
    get: () => this.loud,
    muted: () => this.quiet,
    set: (volume: number) => {
      this.loud = volume;
      this.quiet = volume === 0;
      if (this.open) api.playerSetVolume(volume).catch(() => {});
    },
    setMuted: (muted: boolean) => {
      this.quiet = muted;
      if (this.open) api.playerSetVolume(muted ? 0 : this.loud).catch(() => {});
    },
  };

  /** Stops listening and closes the player. */
  destroy() {
    this.gone = true;
    this.unlisten?.();
    api.playerClose().catch(() => {});
  }
}
