// The open VOD's clips (crates/core clips.rs), shared by what makes them (the
// player's I and O keys, the clip bar, a drag across transcript lines) and the
// chips under the player that show them. A range picked but not yet kept is the
// "ghost": shown as a chip of its own until it's kept (or dropped).

import { SvelteMap } from "svelte/reactivity";
import { api, events, type Clip } from "./api";

/** A range picked but not kept yet. */
export interface Range {
  start: number;
  end: number;
}

/** Remembers "Don't show this again" on the note before downloading a clip. */
const NOTE_HIDDEN = "vodscribe.clipNoteHidden";

function noteHidden(): boolean {
  try {
    return localStorage.getItem(NOTE_HIDDEN) === "true";
  } catch {
    return false;
  }
}

/** How long a clip started (or ended) with I or O alone runs. */
export const DEFAULT_LENGTH = 30;

class Clips {
  vodId = $state<number | null>(null);
  list = $state.raw<Clip[]>([]);
  /** How far each clip being cut is, 0..1. */
  progress = new SvelteMap<number, number>();
  ghost = $state<Range | null>(null);
  error = $state<string | null>(null);
  /** While the note before downloading a clip is up: how it's answered. */
  asking = $state<((download: boolean) => void) | null>(null);
  /** Whether the VOD shown is from a site, so its clips are downloaded. */
  #fromSite = false;
  #listening = false;
  #seq = 0;

  /** Shows VOD `vodId`'s clips (dropping a range picked on another); `fromSite`
   *  if they're downloaded from the site it's on rather than cut from a file here. */
  open(vodId: number, fromSite: boolean) {
    this.#fromSite = fromSite;
    if (this.vodId === vodId) return;
    this.vodId = vodId;
    this.list = [];
    this.ghost = null;
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

  /** Picks `start`..`end` (either way round) as the range to keep, within `duration` if known. */
  pick(start: number, end: number, duration?: number | null) {
    let [a, b] = start <= end ? [start, end] : [end, start];
    a = Math.max(0, a);
    if (duration) b = Math.min(duration, b);
    this.ghost = b - a >= 0.1 ? { start: a, end: b } : null;
  }

  /** The I key: the range starts at `time`. */
  markIn(time: number, duration?: number | null) {
    const end = this.ghost && this.ghost.end > time ? this.ghost.end : time + DEFAULT_LENGTH;
    this.pick(time, end, duration);
  }

  /** The O key: the range ends at `time`. */
  markOut(time: number, duration?: number | null) {
    const start = this.ghost && this.ghost.start < time ? this.ghost.start : time - DEFAULT_LENGTH;
    this.pick(start, time, duration);
  }

  /** Makes the picked range a clip. */
  async keep() {
    const ghost = this.ghost;
    const vodId = this.vodId;
    if (!ghost || vodId === null || this.asking) return;
    // Before the first clip downloaded from a site: a word on permission.
    if (this.#fromSite && !noteHidden()) {
      const download = await new Promise<boolean>((answer) => (this.asking = answer));
      this.asking = null;
      if (!download) return;
    }
    this.ghost = null;
    try {
      this.#put(await api.addClip(vodId, ghost.start, ghost.end));
      this.error = null;
    } catch (e) {
      this.ghost = ghost;
      this.error = String(e);
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
    if (download && hide) {
      try {
        localStorage.setItem(NOTE_HIDDEN, "true");
      } catch {
        // Shown again next time.
      }
    }
    this.asking?.(download);
  }

  rename = (id: number, title: string) => this.#run(api.renameClip(id, title));
  move = (id: number, start: number, end: number) => this.#run(api.setClipRange(id, start, end));
  remove = (id: number) => this.#run(api.deleteClip(id));
  show = (id: number) => this.#run(api.showClip(id));
}

export const clips = new Clips();
