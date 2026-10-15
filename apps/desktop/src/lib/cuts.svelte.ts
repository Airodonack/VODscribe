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
  /** A stretch picked (in the transcript) but not cut yet: Clip cuts both its ends. */
  selection = $state<{ start: number; end: number } | null>(null);
  /** The selection's edge selected, to drag or move a frame at a time. */
  selectedEdge = $state<"start" | "end" | null>(null);
  /** The open VOD's frame rate, as its player says (Player.svelte), for marks placed elsewhere. */
  fps = 30;
  error = $state<string | null>(null);
  #seq = 0;

  open(vodId: number) {
    if (this.vodId === vodId) return;
    this.vodId = vodId;
    this.list = [];
    this.selected = null;
    this.selection = null;
    this.selectedEdge = null;
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

  /** Puts a mark on the frame showing at `time` (selected, if `select`), or takes away the one there. */
  async toggle(time: number, fps: number, select = true) {
    const vodId = this.vodId;
    if (vodId === null) return;
    const there = this.at(time, fps);
    if (there) return this.remove(there.id);
    try {
      const mark = await api.addMark(vodId, frameStart(time, fps));
      if (this.vodId === vodId) this.list = this.#sorted([...this.list, mark]);
      if (select) this.selected = mark.id;
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Cuts both ends of the selection, which then goes (the stretch between the cuts shows it). */
  async cutSelection() {
    const picked = this.selection;
    if (!picked) return;
    this.clearSelection();
    await this.ensure(picked.start);
    await this.ensure(picked.end);
  }

  clearSelection() {
    this.selection = null;
    this.selectedEdge = null;
  }

  /** Moves the selection's `edge` to `time`; dragged past the other edge, it becomes that one. */
  setEdge(edge: "start" | "end", time: number) {
    const s = this.selection;
    if (!s) return;
    const other = edge === "start" ? s.end : s.start;
    if ((edge === "start" && time > other) || (edge === "end" && time < other)) {
      edge = edge === "start" ? "end" : "start";
    }
    this.selection = edge === "start" ? { start: time, end: other } : { start: other, end: time };
    this.selectedEdge = edge;
  }

  /** Puts a mark on the frame showing at `time`, unless there's one already. */
  async ensure(time: number) {
    if (!this.at(time, this.fps)) await this.toggle(time, this.fps, false);
  }

  move(id: number, time: number) {
    this.list = this.#sorted(this.list.map((m) => (m.id === id ? { ...m, start: time } : m)));
    this.#save(api.moveMark(id, time));
  }

  /** Moves the selected mark, or the selection's selected edge, `frames` frames; returns where it is now. */
  nudgeSelected(frames: number, fps: number, duration: number): number | null {
    const s = this.selection;
    if (s && this.selectedEdge) {
      const time = nudge(s[this.selectedEdge], frames, fps, duration);
      this.setEdge(this.selectedEdge, time);
      return time;
    }
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
