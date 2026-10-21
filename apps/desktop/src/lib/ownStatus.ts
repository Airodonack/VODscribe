// What the app's own player (player.rs) last said of itself, for every view
// it draws the picture of: a file's or an edit's (nativeDriver.ts), and a
// site's VOD under their frame (embed.ts). Where the frame on screen is and
// the frame rate there come from here alone, so the playhead and the time
// shown are the picture's, whichever plays.

import type { PlayerStatus } from "./api";

export class OwnStatus {
  private newest = 0;
  last: PlayerStatus | null = null;

  /** Takes `status` unless an older one than the last (a command's reply and
   *  the events travel apart); statuses without `seq` (the page's own
   *  guesses) always. Whether it was taken. */
  take(status: PlayerStatus): boolean {
    if (status.seq !== undefined) {
      if (status.seq <= this.newest) return false;
      this.newest = status.seq;
    }
    this.last = status;
    return true;
  }

  /** Where the frame on screen is (null: none shows, or nothing said yet). */
  shown(): number | null {
    return this.last?.shown ?? null;
  }

  /** The frame rate at the frame shown, once said. */
  fps(): number | null {
    return this.last?.fps ?? null;
  }
}
