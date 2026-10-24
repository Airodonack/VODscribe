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
  /** When its last seek's reply came (performance.now()), null while one
   *  is out, -Infinity if it failed (the app's own player). */
  settled?(): number | null;
  volume?: { get(): number; muted(): boolean };
  rate?: { get(): number };
}

/** How long a paused seek's frame may take after its reply (ms) before the
 *  time shown goes back to the frame on screen (playing, four times this). */
const SETTLING = 250;

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

  /** Whether what shows is still from before the page moved the playhead
   *  (`at`), the time not on it. Playing, the old frames may go on being
   *  drawn a moment after the seek: held until one is drawn near the time.
   *  Paused, until any other frame is drawn, and not for long once the
   *  seek's reply has come: a seek may draw no new frame at all (past the
   *  last one, or refused), and the reply comes before the frame does, so it
   *  alone would show the old frame again. */
  private holds(p: Readable): boolean {
    if (this.left === null || this.shown === null) return false;
    const off = Math.abs(this.shown - this.now);
    if (off < 0.5 / this.fps) return false;
    // How long since the seek's reply came: 0 while it's out, or where the
    // player doesn't say (a site's).
    const settled = p.settled?.() ?? null;
    const since = settled === null ? 0 : performance.now() - settled;
    if (p.playing()) return (this.shown === this.left || off > 1 / this.fps + 1e-3) && since < SETTLING * 4;
    return this.shown === this.left && p.settled !== undefined && since < SETTLING;
  }

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
    this.fps = p.fps?.() ?? DEFAULT_FPS;
    if (this.holds(p)) {
      this.actual = this.now;
    } else {
      this.left = null;
      this.actual = this.shown ?? this.now;
    }
    this.playing = p.playing();
    this.readSound(p);
  }

  /** The sound and speed alone: a player's, or what one will start with
   *  before it's made (Player.svelte). */
  readSound(p: Pick<Readable, "volume" | "rate">) {
    if (p.volume) [this.volume, this.muted] = [p.volume.get(), p.volume.muted()];
    this.rate = p.rate?.get() ?? 1;
  }
}
