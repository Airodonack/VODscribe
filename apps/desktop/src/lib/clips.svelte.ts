// The open VOD's clips (crates/core clips.rs): stretches between marks
// (cuts.svelte.ts) exported with the Export button, an Alt-drag on the
// timeline or a press held on the picture, shown as pills in the clip bar
// under the player (ClipStrip.svelte).

import { SvelteMap } from "svelte/reactivity";
import { api, events, type Clip } from "./api";
import { hideNote, noteHidden } from "./permission";

class Clips {
  vodId = $state<number | null>(null);
  list = $state.raw<Clip[]>([]);
  /** How far each clip being cut is, 0..1. */
  progress = new SvelteMap<number, number>();
  error = $state<string | null>(null);
  /** The stretch of the clip whose pill is hovered, lit up on the timeline. */
  hovered = $state<{ start: number; end: number } | null>(null);
  /** A stretch being carried (dragout.ts), and whether it's over the clip bar. */
  carrying = $state<{ start: number; end: number } | null>(null);
  overBar = $state(false);
  /** While the note before downloading a clip is up: how it's answered. */
  asking = $state<((download: boolean) => void) | null>(null);
  /** Whether the VOD shown is from a site, so its clips are downloaded. */
  #fromSite = false;
  get fromSite() {
    return this.#fromSite;
  }
  #making = new Map<string, Promise<Clip | null>>();
  #listening = false;
  #seq = 0;

  /** Shows VOD `vodId`'s clips (dropping a range picked on another); `fromSite`
   *  if they're downloaded from the site it's on rather than cut from a file here. */
  open(vodId: number, fromSite: boolean) {
    this.#fromSite = fromSite;
    if (this.vodId === vodId) return;
    this.answer(false);
    this.hovered = null;
    this.vodId = vodId;
    this.list = [];
    this.error = null;
    this.#listen();
    const mine = ++this.#seq;
    api.listClips(vodId).then(
      (list) => mine === this.#seq && (this.list = list),
      (e) => mine === this.#seq && (this.error = String(e)),
    );
  }

  #listen() {
    if (this.#listening) return;
    this.#listening = true;
    events.clip((clip) => {
      if (clip.status !== "cutting") this.progress.delete(clip.id);
      this.#put(clip);
    });
    events.clipProgress((id, progress) => this.progress.set(id, progress));
    events.clipRemoved((id) => (this.list = this.list.filter((c) => c.id !== id)));
  }

  /** Adds or updates `clip`, if it's of the VOD shown. */
  #put(clip: Clip) {
    if (clip.vod_id !== this.vodId) return;
    const at = this.list.findIndex((c) => c.id === clip.id);
    this.list = at < 0 ? [...this.list, clip] : this.list.map((c, i) => (i === at ? clip : c));
  }

  /** The clip of exactly `start`..`end`, if one's been made. */
  find(start: number, end: number): Clip | undefined {
    return this.list.find((c) => Math.abs(c.start - start) < 0.001 && Math.abs(c.end - end) < 0.001);
  }

  /** The clips in the clip bar: saved there, not only dragged out of the window. */
  get shown(): Clip[] {
    return this.list.filter((c) => c.shown);
  }

  /**
   * Makes `start`..`end` a clip, or gives the one made already (putting it
   * in the clip bar if `shown`). Before the first downloaded from a site, the
   * note on permission is answered first. Null if it's declined or fails.
   */
  make(start: number, end: number, shown = true): Promise<Clip | null> {
    // Asked twice before the first is answered (E pressed twice), it's made once.
    const key = `${this.vodId} ${start} ${end}`;
    const making = this.#making.get(key);
    if (making) return making;
    const made = this.#make(start, end, shown).finally(() => this.#making.delete(key));
    this.#making.set(key, made);
    return made;
  }

  async #make(start: number, end: number, shown: boolean): Promise<Clip | null> {
    const vodId = this.vodId;
    const made = this.find(start, end);
    // One that failed is tried again.
    if (made?.status === "error") {
      await this.#run(api.setClipRange(made.id, made.start, made.end));
      if (shown && !made.shown) await this.#run(api.keepClip(made.id));
      return this.list.find((c) => c.id === made.id) ?? made;
    }
    if (made && shown && !made.shown) {
      await this.#run(api.keepClip(made.id));
      return this.list.find((c) => c.id === made.id) ?? made;
    }
    if (made) return made;
    if (vodId === null || this.asking) return null;
    if (this.#fromSite && !noteHidden()) {
      const download = await new Promise<boolean>((answer) => (this.asking = answer));
      this.asking = null;
      if (!download) return null;
    }
    try {
      const clip = await api.addClip(vodId, start, end, shown);
      this.#put(clip);
      this.error = null;
      return clip;
    } catch (e) {
      this.error = String(e);
      return null;
    }
  }

  /** Waits, while `still()`, for clip `id`'s file: the clip once made, else null.
   *  A clip of a VOD other than the one shown (`vodId`) is asked for. */
  async ready(id: number, still: () => boolean, vodId?: number): Promise<Clip | null> {
    for (;;) {
      let clip = this.list.find((c) => c.id === id);
      if (!clip && vodId !== undefined && vodId !== this.vodId)
        clip = (await api.listClips(vodId).catch(() => [])).find((c) => c.id === id);
      if (clip?.status === "error") this.error = `Couldn't make the clip: ${clip.error ?? "unknown error"}`;
      if (!clip || clip.status === "error") return null;
      if (clip.status === "ready") return clip;
      if (!still()) return null;
      await new Promise((r) => setTimeout(r, 100));
    }
  }

  async #run(action: Promise<Clip | void>) {
    try {
      const clip = await action;
      if (clip) this.#put(clip);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Answers the note before downloading; `hide` keeps it from showing again. */
  answer(download: boolean, hide = false) {
    if (download && hide) hideNote();
    this.asking?.(download);
  }

  rename = (id: number, title: string) => this.#run(api.renameClip(id, title));
  move = (id: number, start: number, end: number) => this.#run(api.setClipRange(id, start, end));
  remove = (id: number) => {
    // Its pill goes from under the pointer, which may not say it left.
    this.hovered = null;
    return this.#run(api.deleteClip(id));
  };
  show = (id: number) => this.#run(api.showClip(id));
}

export const clips = new Clips();
