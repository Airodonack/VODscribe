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
  playing = $state(false);
  volume = $state(1);
  muted = $state(false);
  rate = $state(1);
  /** Exact for the app's own player; others don't say, so marks there are as near as 30 fps gets. */
  fps = $state(DEFAULT_FPS);

  constructor(now = 0) {
    this.now = now;
  }

  read(p: Readable) {
    this.now = p.time();
    this.shown = p.shown?.() ?? null;
    this.playing = p.playing();
    this.fps = p.fps?.() ?? DEFAULT_FPS;
    if (p.volume) [this.volume, this.muted] = [p.volume.get(), p.volume.muted()];
    this.rate = p.rate?.get() ?? 1;
  }
}
