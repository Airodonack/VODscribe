// The open VOD's marks: cuts between frames that split it into stretches,
// each of which can become a clip. X puts one where the playhead is (or takes
// away the one there); a selected one moves a frame at a time with , and .
// (Player.svelte), or by dragging its flag on the timeline (Scrubber.svelte).
// Saved per VOD (crates/core db.rs `marks`); changes show at once and are
// saved after.

import { api, type Mark } from "./api";
import { frameStart, nudge } from "./media";

class Cuts {
  vodId = $state<number | null>(null);
  list = $state.raw<Mark[]>([]);
  selected = $state<number | null>(null);
  error = $state<string | null>(null);
  #seq = 0;

  open(vodId: number) {
    if (this.vodId === vodId) return;
    this.vodId = vodId;
    this.list = [];
    this.selected = null;
    this.#reload();
  }

  #reload() {
    const vodId = this.vodId;
    if (vodId === null) return;
    const mine = ++this.#seq;
    api.listMarks(vodId).then(
      (list) => mine === this.#seq && (this.list = list),
      (e) => mine === this.#seq && (this.error = String(e)),
    );
  }

  /** Saves a change made on screen already; on failure, shows what's saved. */
  #save(change: Promise<unknown>) {
    this.error = null;
    change.catch((e) => {
      this.error = String(e);
      this.#reload();
    });
  }

  #sorted(list: Mark[]) {
    return [...list].sort((a, b) => a.start - b.start || a.id - b.id);
  }

  /** The mark on the frame showing at `time`, if any. */
  at(time: number, fps: number): Mark | undefined {
    const start = frameStart(time, fps);
    return this.list.find((m) => Math.abs(m.start - start) < 0.5 / fps);
  }

  /** Puts a mark on the frame showing at `time`, or takes away the one there. */
  async toggle(time: number, fps: number) {
    const vodId = this.vodId;
    if (vodId === null) return;
    const there = this.at(time, fps);
    if (there) return this.remove(there.id);
    try {
      const mark = await api.addMark(vodId, frameStart(time, fps));
      if (this.vodId === vodId) this.list = this.#sorted([...this.list, mark]);
      this.selected = mark.id;
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  move(id: number, time: number) {
    this.list = this.#sorted(this.list.map((m) => (m.id === id ? { ...m, start: time } : m)));
    this.#save(api.moveMark(id, time));
  }

  /** Moves the selected mark `frames` frames; returns where it is now. */
  nudgeSelected(frames: number, fps: number, duration: number): number | null {
    const mark = this.list.find((m) => m.id === this.selected);
    if (!mark) return null;
    const time = nudge(mark.start, frames, fps, duration);
    this.move(mark.id, time);
    return time;
  }

  remove(id: number) {
    this.list = this.list.filter((m) => m.id !== id);
    if (this.selected === id) this.selected = null;
    this.#save(api.deleteMark(id));
  }
}

export const cuts = new Cuts();
