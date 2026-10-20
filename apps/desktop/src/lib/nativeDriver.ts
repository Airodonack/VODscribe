// The page's side of the app's own player (player.rs, crates/player), for
// whichever view plays in it: a file's player (NativePlayer.svelte) and an
// edit's (editPlayer.ts). It keeps what the player last told (its
// `player-time` events and its commands' replies, the newest by `seq`), runs
// the time on by the page's clock while the player's runs (after play it
// stands until the sound starts), and sends what's asked; until the player is
// open, a seek, play or speed asked is kept for then.

import { api, events, type PlayerStatus, type UnlistenFn } from "./api";
import { Clock } from "./playerClock";

export class NativeDriver {
  private clock: Clock;
  private isPlaying = false;
  /** Where the frame on screen is, as the player last said (null: none shows). */
  private shownAt: number | null = null;
  /** The time stands for footage still to come (a VOD from a site). */
  private waitingFor = false;
  /** A frame has shown since it opened. */
  private everShown = false;
  private seq = 0;
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
  fps: number | null = null;

  constructor(start = 0) {
    this.clock = new Clock(start, false);
    events.playerTime((s) => !this.gone && this.tell(s)).then((u) => (this.gone ? u() : (this.unlisten = u)));
  }

  private tell(status: PlayerStatus) {
    // A reply that arrives after a newer event is stale.
    if (status.seq !== undefined) {
      if (status.seq <= this.seq) return;
      this.seq = status.seq;
    }
    this.clock.set(status.time);
    this.clock.playing = status.running;
    this.clock.speed = status.speed;
    this.isPlaying = status.playing;
    this.shownAt = status.shown;
    this.waitingFor = status.waiting ?? false;
    if (status.shown !== null) this.everShown = true;
  }

  /** Whether the time stands for footage still to come (a VOD from a site, fetched as it plays). */
  waiting() {
    return this.waitingFor;
  }

  /** Whether a frame has shown since it opened. */
  showed() {
    return this.everShown;
  }

  /** What the page guesses until the player says: at `time`, not running. */
  private guess(time: number, playing: boolean, speed = this.clock.speed) {
    this.tell({ time, playing, running: false, speed, shown: this.shownAt });
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
    this.fps = fps;
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
    return this.shownAt;
  }

  togglePlay() {
    const playing = !this.isPlaying;
    this.guess(this.time(), playing);
    if (!this.open) {
      this.wanted = { time: this.time(), play: playing };
      return;
    }
    (playing ? api.playerPlay() : api.playerPause()).then(
      (s) => this.tell(s),
      () => {},
    );
  }

  seek(seconds: number, play: boolean) {
    // The picture shows what it did until the player has the new frame.
    this.guess(seconds, play);
    if (!this.open) {
      this.wanted = { time: seconds, play };
      return;
    }
    api.playerSeek(seconds, play).then(
      (s) => this.tell(s),
      () => {},
    );
  }

  /** Steps `frames` frames on (back, if negative), paused. */
  step(frames: number) {
    if (!this.open) return;
    api.playerStep(frames).then(
      (s) => this.tell(s),
      () => {},
    );
  }

  speed = {
    get: () => this.clock.speed,
    set: (speed: number) => {
      this.guess(this.time(), this.isPlaying, speed);
      if (!this.open) {
        this.wantedSpeed = speed;
        return;
      }
      api.playerSetSpeed(speed).then(
        (s) => this.tell(s),
        () => {},
      );
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
