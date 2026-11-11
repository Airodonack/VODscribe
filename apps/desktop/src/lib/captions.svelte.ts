// Each VOD's captions: its transcript's lines as cues (vods.rs `captions`),
// for every player that shows them. Player.svelte's captions button and the
// strip under a site's player, NativePlayer's and LocalPlayer's over a file's
// picture and embed.ts's over a site's read them here, so a VOD is read once
// for all of them. Read while a player `watch`es the VOD, and again whenever
// the app says it changed ("vod": a stretch transcribed, a transcript
// imported), one read at a time and the newest winning (latest.ts): an
// answer read before a change never replaces one read after it.

import { SvelteMap } from "svelte/reactivity";
import { api, events, type Cue } from "./api";
import { latest } from "./latest";

const NONE: Cue[] = [];

type Reads = ReturnType<typeof latest<number, Cue[]>>;

class Captions {
  /** Each watched VOD's cues, once read. */
  #of = new SvelteMap<number, Cue[]>();
  /** The VODs watched: by how many players, and their reads. */
  #watched = new Map<number, { players: number; reads: Reads }>();

  constructor() {
    void events.vod((vod) => this.#watched.get(vod.id)?.reads.want(vod.id));
  }

  /** VOD `id`'s cues; none until they're read. */
  of(id: number): Cue[] {
    return this.#of.get(id) ?? NONE;
  }

  /** Whether VOD `id`'s cues have been read: until then, whether there are any isn't known. */
  read(id: number): boolean {
    return this.#of.has(id);
  }

  /** Reads VOD `id`'s cues, now and at each change, until every player that
   *  asked has let go (the function given back). */
  watch(id: number): () => void {
    const watched = this.#watched.get(id) ?? this.#start(id);
    watched.players++;
    let released = false;
    return () => {
      if (released) return;
      released = true;
      if (--watched.players > 0) return;
      // An answer still on its way is dropped (latest.ts `clear`).
      watched.reads.clear();
      this.#watched.delete(id);
      this.#of.delete(id);
    };
  }

  /** VOD `id` watched, by no player yet: its first read asked. */
  #start(id: number) {
    const reads: Reads = latest<number, Cue[]>({
      fetch: (id) => api.captions(id),
      got: (cues, id) => this.#of.set(id, cues),
      failed: (e) => console.error("captions", e),
    });
    const watched = { players: 0, reads };
    this.#watched.set(id, watched);
    reads.want(id);
    return watched;
  }
}

export const captions = new Captions();
