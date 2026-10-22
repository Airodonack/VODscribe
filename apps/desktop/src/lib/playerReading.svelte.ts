// What a player's controls show, read from it every frame (Player.svelte,
// EditView.svelte): a site's player may be in an iframe that only reports by
// message, and the app's own reports by events, so there's nothing to bind.

import { DEFAULT_FPS } from "./media";

/** What's read: any player's, a VOD's (embed.ts) or an edit's (editPlayer.ts). */
interface Readable {
  time(): number;
  shown?(): number | null;
  playing(): boolean;
  fps?(): number | null;
  volume?: { get(): number; muted(): boolean };
  rate?: { get(): number };
}

export class PlayerReading {
  now = $state(0);
  /** Where the frame on screen is, where the player knows (the app's own). */
  shown = $state<number | null>(null);
  /** Where the picture is: the frame on screen where known, else the time;
   *  after the page moves the playhead, the time, until a frame is drawn
   *  there. The time shown is this. */
  actual = $state(0);
  playing = $state(false);
  volume = $state(1);
  muted = $state(false);
  rate = $state(1);
  /** Exact for the app's own player; others don't say, so marks there are as near as 30 fps gets. */
  fps = $state(DEFAULT_FPS);

  /** The frame on screen when the page last moved the playhead (`at`), while
   *  the player hasn't drawn one where it went: until then the picture is
   *  taken to be there already, so the time shown doesn't go back to the
   *  old frame after showing the new time. */
  private left: number | null = null;

  constructor(now = 0) {
    this.now = now;
    this.actual = now;
  }

  /** Put at `time` by the page itself (no player to read yet, or the
   *  playhead moved ahead of it): the time and where the picture is both. */
  at(time: number) {
    this.left = this.shown;
    this.now = time;
    this.actual = time;
  }

  read(p: Readable) {
    this.now = p.time();
    this.shown = p.shown?.() ?? null;
    const fps = p.fps?.() ?? DEFAULT_FPS;
    // Moved by the page: what shows is still the frame it left, until a new
    // one is drawn near the time (or the time is on the frame it left).
    if (
      this.left !== null &&
      this.shown !== null &&
      (this.shown === this.left || Math.abs(this.shown - this.now) > 1 / fps + 1e-3) &&
      Math.abs(this.shown - this.now) >= 0.5 / fps
    ) {
      this.actual = this.now;
    } else {
      this.left = null;
      this.actual = this.shown ?? this.now;
    }
    this.playing = p.playing();
    this.fps = p.fps?.() ?? DEFAULT_FPS;
    if (p.volume) [this.volume, this.muted] = [p.volume.get(), p.volume.muted()];
    this.rate = p.rate?.get() ?? 1;
  }
}
