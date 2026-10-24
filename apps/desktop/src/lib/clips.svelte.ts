// The open VOD's clips (crates/core clips.rs): stretches between marks
// (cuts.svelte.ts) made with the Clip button, an Alt-drag on the
// timeline or a press held on the picture, shown as pills in the clip bar
// under the player (ClipStrip.svelte). Or the open edit's: stretches of it
// made clips with C (crates/core edits.rs `add_edit_clip`).

import { SvelteMap } from "svelte/reactivity";
import { api, docKey, events, type Clip, type Doc } from "./api";
import { hideNote, noteHidden } from "./permission";

class Clips {
  /** Whose clip bar this is: a VOD's or an edit's. */
  doc = $state.raw<Doc | null>(null);
  /** The VOD whose clips these are, if they're a VOD's. */
  vodId = $derived(this.doc?.kind === "vod" ? this.doc.id : null);
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
    this.#open({ kind: "vod", id: vodId });
  }

  /** Shows edit `id`'s clip bar. */
  openEdit(id: number) {
    this.#fromSite = false;
    this.#open({ kind: "edit", id });
  }

  #open(doc: Doc) {
    if (this.doc && docKey(this.doc) === docKey(doc)) return;
    this.answer(false);
    this.hovered = null;
    this.doc = doc;
    this.list = [];
    this.error = null;
    this.#listen();
    const mine = ++this.#seq;
    // Those whose file was deleted outside the app go first, then the rest are listed.
    this.#forgetGone(doc)
      .then(() => this.#listOf(doc))
      .then(
        (list) => mine === this.#seq && (this.list = list),
        (e) => mine === this.#seq && (this.error = String(e)),
      );
  }

  #listOf = (doc: Doc) => (doc.kind === "vod" ? api.listClips(doc.id) : api.editClips(doc.id));

  /** Loads `doc`'s clips again (undo or redo changed its clip bar), if it's the one shown. */
  reload(doc: Doc) {
    if (!this.doc || docKey(doc) !== docKey(this.doc)) return;
    const mine = ++this.#seq;
    this.#listOf(doc).then(
      (list) => mine === this.#seq && (this.list = list),
      (e) => mine === this.#seq && (this.error = String(e)),
    );
  }

  /** Takes off `doc`'s clip bar the clips whose file was deleted outside the app. */
  #forgetGone(doc: Doc): Promise<void> {
    return api.forgetGoneClips(doc).catch((e) => console.error("forget gone clips", e));
  }

  #listen() {
    if (this.#listening) return;
    this.#listening = true;
    // Back from another app (a file manager, where a clip's file may have been
    // deleted): the clip bar is checked again.
    window.addEventListener("focus", () => this.doc && this.#forgetGone(this.doc));
    events.clip((clip) => {
      if (clip.status !== "cutting") this.progress.delete(clip.id);
      this.#news.set(clip.id, ++this.#serial);
      this.#put(clip);
    });
    events.clipProgress((id, progress) => this.progress.set(id, progress));
    events.clipRemoved((id) => (this.list = this.list.filter((c) => c.id !== id)));
  }

  // Each clip's latest news (its "clip" event), numbered: a command's answer
  // is older than news that came after it was sent (a quick cut's "ready"
  // can beat the answer's "waiting"), and isn't put over it.
  #serial = 0;
  #news = new Map<number, number>();

  /** Puts `clip`, a command's answer to one sent when the news was at `since`,
   *  unless news of it has come since. */
  #answer(clip: Clip, since: number) {
    if ((this.#news.get(clip.id) ?? 0) <= since) this.#put(clip);
  }

  /** Adds or updates `clip`, if it's on the clip bar shown. */
  #put(clip: Clip) {
    const bar = clip.bar ?? { kind: "vod", id: clip.vod_id };
    if (!this.doc || docKey(bar) !== docKey(this.doc)) return;
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
      const since = this.#serial;
      const clip = await api.addClip(vodId, start, end, shown);
      this.#answer(clip, since);
      this.error = null;
      return clip;
    } catch (e) {
      this.error = String(e);
      return null;
    }
  }

  /**
   * Makes each of `ranges` a clip in the clip bar, in their order, as `make`
   * makes one (the note on permission first, once): those not made yet as
   * one change, so undo takes them all back; any made already as `make`
   * gives them.
   */
  async makeAll(ranges: { start: number; end: number }[]): Promise<void> {
    const vodId = this.vodId;
    if (vodId === null || this.asking) return;
    const fresh = ranges.filter((r) => !this.find(r.start, r.end));
    for (const r of ranges) if (this.find(r.start, r.end)) await this.make(r.start, r.end);
    if (!fresh.length) return;
    if (this.#fromSite && !noteHidden()) {
      const download = await new Promise<boolean>((answer) => (this.asking = answer));
      this.asking = null;
      if (!download) return;
    }
    try {
      const since = this.#serial;
      const made = await api.addClips(
        vodId,
        fresh.map((r) => [r.start, r.end]),
      );
      for (const clip of made) this.#answer(clip, since);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Makes each of `ranges` of the open edit's timeline a clip on its clip bar, in their order, as one change. */
  async makeAllOfEdit(ranges: { start: number; end: number }[]): Promise<void> {
    if (this.doc?.kind !== "edit") return;
    try {
      const since = this.#serial;
      const made = await api.addEditClips(
        this.doc.id,
        ranges.map((r) => [r.start, r.end]),
      );
      for (const clip of made) this.#answer(clip, since);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Makes `from`..`to` of the open edit's timeline a clip on its clip bar. */
  async makeOfEdit(from: number, to: number): Promise<Clip | null> {
    if (this.doc?.kind !== "edit") return null;
    try {
      const since = this.#serial;
      const clip = await api.addEditClip(this.doc.id, from, to);
      this.#answer(clip, since);
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
    const since = this.#serial;
    try {
      const clip = await action;
      if (clip) this.#answer(clip, since);
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
