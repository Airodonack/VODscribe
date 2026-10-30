// The one loop that reads a player every frame (Player.svelte for a VOD,
// EditView.svelte for an edit): what its controls show (`reading`), the
// frame rate marks and the selection's ends go on, where its playhead is
// remembered (nav.svelte.ts), and the playhead the transcript follows.

import type { Doc } from "./api";
import { cuts } from "./cuts.svelte";
import { nav } from "./nav.svelte";
import type { PlayerReading } from "./playerReading.svelte";

/** What the reading reads. */
type Readable = Parameters<PlayerReading["read"]>[0];

/**
 * Reads `player()` into `reading` every frame while the component calling
 * it is there, for document `doc()`: `idle` each frame there's no player,
 * `each` after it's read (what's the caller's own); the time is remembered
 * for the document while `noted()` (once it's that document's playhead).
 */
export function drivePlayer<P extends Readable>(
  reading: PlayerReading,
  doc: () => Doc | undefined,
  o: {
    player: () => P | null | undefined;
    idle?: () => void;
    each?: (p: P) => void;
    noted?: () => boolean;
  },
) {
  $effect(() => {
    let poll = requestAnimationFrame(function read() {
      poll = requestAnimationFrame(read);
      const p = o.player();
      if (!p) return o.idle?.();
      reading.read(p);
      o.each?.(p);
      const d = doc();
      if (!d) return;
      // Marks and the selection's ends placed elsewhere (the transcript) go on its frames.
      if (cuts.isOf(d)) cuts.fps = reading.fps;
      if (o.noted?.() ?? true) nav.note(d, { time: reading.now });
    });
    return () => cancelAnimationFrame(poll);
  });
  // The transcript follows the playhead (cuts.svelte.ts), while its marks are this document's.
  $effect(() => {
    const d = doc();
    if (!d || !cuts.isOf(d)) return;
    cuts.now = reading.actual;
    cuts.playing = reading.playing;
  });
}
