// The open VOD's cast and activity, from an annotations import
// (crates/core annotations.rs), read through `vod_annotations` once per VOD
// and again after an import; and the import itself, from the file picker.
// The timeline (Scrubber.svelte) draws the activity, the transcript
// (Transcript.svelte) colours its lines by speaker and heads them with the
// cast member's picture, and its header lists the cast.

import { open } from "@tauri-apps/plugin-dialog";
import { SvelteMap } from "svelte/reactivity";
import { AVATAR_BASE } from "./accounts";
import { api, type Vod, type VodAnnotations } from "./api";
import { cuts } from "./cuts.svelte";

class Annotated {
  /** Each VOD's, once read: null when it has none (or they couldn't be read). */
  #of = new SvelteMap<number, VodAnnotations | null>();
  /** How many imports each VOD has had this run: what's read of it is read again at each. */
  #imports = new SvelteMap<number, number>();

  #avatarBase = $state<string | null>(null);
  #askedBase = false;

  /**
   * The image server the cast's pictures are asked of; null until the app
   * has said which (it may name its own, for tests), when none is asked.
   */
  avatarBase(): string | null {
    if (!this.#askedBase) {
      this.#askedBase = true;
      // Read from a microtask, not while drawing (it changes state).
      void Promise.resolve()
        .then(() => api.avatarBase())
        .then(
          (base) => (this.#avatarBase = base ?? AVATAR_BASE),
          () => (this.#avatarBase = AVATAR_BASE),
        );
    }
    return this.#avatarBase;
  }

  /** VOD `id`'s cast and activity, once read (`load`). */
  of(id: number | null | undefined): VodAnnotations | null {
    return id == null ? null : (this.#of.get(id) ?? null);
  }

  /** Reads VOD `id`'s, the first time: from an effect, not while drawing (it changes state). */
  load(id: number | null | undefined) {
    if (id == null || this.#of.has(id)) return;
    // Asked once: until it's read, it has none.
    this.#of.set(id, null);
    void this.#read(id);
  }

  /** A number that changes when VOD `id` has annotations imported: a view reading its transcript reads it again. */
  imports(id: number | null | undefined): number {
    return id == null ? 0 : (this.#imports.get(id) ?? 0);
  }

  async #read(id: number) {
    const got = await api.vodAnnotations(id).catch(() => null);
    this.#of.set(id, got);
  }

  /**
   * Asks for an annotations file with the file picker and imports it: the
   * VOD it's for is added if the library doesn't have it. Returns the VOD,
   * or null if nothing was picked; throws the import's error.
   */
  async pickAndImport(): Promise<Vod | null> {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Annotations", extensions: ["json"] }],
    }).catch(() => null);
    const path = Array.isArray(picked) ? picked[0] : picked;
    if (!path) return null;
    const { vod } = await api.importAnnotationsFile(path);
    this.imported(vod.id);
    return vod;
  }

  /** VOD `id` has had annotations imported: what's shown of it is read again. */
  imported(id: number) {
    this.#imports.set(id, this.imports(id) + 1);
    void this.#read(id);
    cuts.reload({ kind: "vod", id });
  }
}

export const annotated = new Annotated();
