// The open VOD's marks, or the open edit's (its cuts, chapters and
// highlights, on its timeline: crates/core edits.rs `place_marks`). Cuts
// between frames split it into stretches, each of which can become a clip
// (in an edit, a delete between them makes them a real split): Split (B)
// puts one where the playhead is (or takes away the one there). Chapters (N)
// name the part from their start to the next one's, and highlights (M) note
// a moment. A selected mark moves by frames with , and . or by seconds with
// the arrows, J and L (`markKey` below), or by dragging it on the
// timeline (Scrubber.svelte). Saved per VOD or clip (crates/core db.rs
// `marks`); changes show at once and are saved after. It also carries what
// the open VOD's or edit's timeline and transcript share: where its player
// is, and a moment pointed at in either.

import { api, docKey, type Doc, type Mark, type MarkKind } from "./api";
import { DEFAULT_FPS, frameStart, inFrame, nudge } from "./media";
import { is } from "./keys.svelte";
import { inOut, isEscape, markFramesOf } from "./playerKeys";
import { clearsStretch, snippets, stretchMade } from "./selection.svelte";
import { changed, shownMarks, sourceOf, type MarkSources } from "./markSources";
import { layers } from "./layers.svelte";

class Cuts {
  /** Whose marks these are: a VOD's or an edit's. */
  doc = $state.raw<Doc | null>(null);
  /** The VOD whose marks these are, if they're a VOD's. */
  vodId = $derived(this.doc?.kind === "vod" ? this.doc.id : null);
  /** Every mark, in time order. */
  all = $state.raw<Mark[]>([]);
  /** Whether `doc`'s marks have been read since it was opened (`all` is
   *  empty until then, not because it has none). */
  read = $state(false);
  /** The marks shown: all the cuts, and the chapters and highlights of those
   *  the Timeline panel's Marks toggles show (layers.svelte.ts). */
  shown = $derived(shownMarks(this.all, layers.marks));
  /** The cuts. */
  list = $derived(this.all.filter((m) => m.kind === "cut"));
  chapters = $derived(this.shown.filter((m) => m.kind === "chapter"));
  highlights = $derived(this.shown.filter((m) => m.kind === "highlight"));
  /** The mark or cut selected: one thing of its own kind, so selecting it
   *  lets go of a set picked (selection.svelte.ts), as a stretch made does,
   *  and Delete is then the mark's. */
  #selected = $state<number | null>(null);
  get selected() {
    return this.#selected;
  }
  set selected(id: number | null) {
    this.#selected = id;
    if (id !== null) stretchMade();
  }
  /** A chapter or highlight whose title or note is being written. */
  editing = $state<number | null>(null);
  /** The mark just added whose title or note is being written: Escape takes it away. */
  fresh: number | null = null;
  /** A stretch picked (in the transcript, or with I and O) but not cut yet,
   *  of the VOD or edit open (`doc`), the one selection both players,
   *  the transcript and the timeline share: Split cuts both its ends, and in
   *  an edit Delete takes it out. Its edges drag on the timeline and move a
   *  frame at a time once selected (`selectedEdge`). Made, it lets go of a
   *  set picked (selection.svelte.ts) and of the mark selected. */
  #selection = $state<{ start: number; end: number } | null>(null);
  get selection() {
    return this.#selection;
  }
  set selection(picked: { start: number; end: number } | null) {
    this.#selection = picked;
    if (picked) {
      stretchMade();
      // Delete is then the selection's, not a mark's left selected.
      this.selected = null;
    }
  }
  /** The selection's edge selected, to drag or move a frame at a time. */
  selectedEdge = $state<"start" | "end" | null>(null);
  /** The open VOD's or edit's frame rate, as its player says, for marks placed elsewhere. */
  fps = DEFAULT_FPS;
  /** Where its player is (the time it shows) and whether it plays, as its
   *  player says: the transcript follows it (Transcript.svelte). */
  now = $state(0);
  playing = $state(false);
  /** A moment the pointer is over on its timeline (Scrubber.svelte) or in its
   *  transcript (a line or a chat message), marked on the other. */
  pointed = $state<{ time: number; on: "timeline" | "transcript" } | null>(null);
  error = $state<string | null>(null);
  #seq = 0;

  constructor() {
    // A set picked lets go of the stretch, and of a mark selected: Delete
    // is then the set's.
    clearsStretch(() => {
      this.clearSelection();
      this.selected = null;
    });
  }

  /** Shows or hides the user's own chapters and highlights, or those imported; a mark
   *  hidden is no longer selected or being written. */
  setSources(change: Partial<MarkSources>) {
    layers.showMarks(change);
    const shown = (id: number | null) => id !== null && this.shown.some((m) => m.id === id);
    if (!shown(this.selected)) this.selected = null;
    if (!shown(this.editing)) this.editing = null;
  }

  open(vodId: number) {
    this.openDoc({ kind: "vod", id: vodId });
  }

  /** Shows `doc`'s marks (a VOD's or an edit's). */
  openDoc(doc: Doc) {
    if (this.doc && docKey(this.doc) === docKey(doc)) return;
    this.doc = doc;
    this.all = [];
    this.read = false;
    this.now = 0;
    this.playing = false;
    this.pointed = null;
    this.selected = null;
    this.editing = null;
    this.fresh = null;
    this.selection = null;
    this.selectedEdge = null;
    // What was picked is of the last one's timeline and clip bar.
    snippets.clear();
    this.#reload();
  }

  /** Whether these are `doc`'s marks (it's the VOD or edit open). */
  isOf(doc: Doc | null | undefined): boolean {
    return !!doc && !!this.doc && docKey(this.doc) === docKey(doc);
  }

  /** Loads `doc`'s marks again (undo or redo changed them, or an edit's
   *  pieces moved), if it's the one open. */
  reload(doc: Doc) {
    if (this.doc && docKey(this.doc) === docKey(doc)) this.#reload();
  }

  #reload() {
    const doc = this.doc;
    if (doc === null) return;
    const mine = ++this.#seq;
    (doc.kind === "vod" ? api.listMarks(doc.id) : api.editMarks(doc.id)).then(
      (list) => {
        if (mine !== this.#seq) return;
        this.all = list;
        this.read = true;
        // A mark gone (undone, or a save that failed) is selected or open no more.
        const there = (id: number | null) => id !== null && list.some((m) => m.id === id);
        if (!there(this.selected)) this.selected = null;
        if (!there(this.editing)) this.editing = null;
        if (!there(this.fresh)) this.fresh = null;
      },
      (e) => {
        if (mine !== this.#seq) return;
        this.error = String(e);
        this.read = true;
      },
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

  /** Adds a mark of `kind` to `doc` (a VOD's or an edit's) at `start`. */
  #addTo = (doc: Doc, start: number, kind: MarkKind) =>
    doc.kind === "vod" ? api.addMark(doc.id, start, kind) : api.addEditMark(doc.id, start, kind);

  /** Whether `doc`'s marks are the ones shown (it may have been left and opened again since). */
  #showing = (doc: Doc) => this.doc !== null && docKey(this.doc) === docKey(doc);

  /** Lists `mark`, unless it's listed already (a reload since may have it). */
  #put(mark: Mark) {
    if (!this.all.some((m) => m.id === mark.id)) this.all = this.#sorted([...this.all, mark]);
  }

  /** Puts a mark on the frame showing at `time` (selected, if `select`), or takes away the one there. */
  async toggle(time: number, fps: number, select = true) {
    const doc = this.doc;
    if (doc === null) return;
    const there = this.at(time, fps);
    if (there) return this.remove(there.id);
    const start = frameStart(time, fps);
    try {
      const mark = await this.#addTo(doc, start, "cut");
      // Another VOD or edit open by now: the mark is that one's, not to select here.
      if (!this.#showing(doc)) return;
      this.#put(mark);
      if (select) this.selected = mark.id;
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Puts a chapter or highlight on the frame showing at `time`, selected, and opens its title to write. */
  async add(kind: Exclude<MarkKind, "cut">, time: number, fps: number) {
    const doc = this.doc;
    if (doc === null) return;
    const start = frameStart(time, fps);
    // One of its kind on that frame already: that one's title instead.
    const there = this.all.find((m) => m.kind === kind && Math.abs(m.start - start) < 0.5 / fps);
    try {
      const mark = there ?? (await this.#addTo(doc, start, kind));
      if (!this.#showing(doc)) return;
      if (!there) this.#put(mark);
      // Made (or opened) with its kind hidden by the Marks toggles: shown again, to be seen and named.
      if (!layers.marks[sourceOf(mark)]) layers.showMarks({ [sourceOf(mark)]: true });
      this.clearSelection();
      this.selected = mark.id;
      this.editing = mark.id;
      this.fresh = there ? null : mark.id;
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Sets mark `id`'s title or note (none, if blank). */
  setText(id: number, text: string) {
    const clean = text.trim() || null;
    const mark = this.all.find((m) => m.id === id);
    if (!mark || mark.text === clean) return;
    this.all = this.all.map((m) => (m.id === id ? changed(m, { text: clean }) : m));
    this.#save(api.setMarkText(id, clean));
  }

  /** The chapter `time` is in, if any. */
  chapterAt(time: number): Mark | undefined {
    let found: Mark | undefined;
    for (const c of this.chapters) if (c.start <= time + 1e-6) found = c;
    return found;
  }

  /** Cuts both ends of the selection, which then goes (the stretch between the cuts shows it);
   *  returns that stretch, from cut to cut. With `length`, an end at the start
   *  or the end of what plays (an edit's) is no place for a cut and gets none. */
  async cutSelection(length?: number): Promise<{ start: number; end: number } | null> {
    const picked = this.selection;
    const doc = this.doc;
    if (!picked) return null;
    this.clearSelection();
    for (const t of [picked.start, picked.end]) {
      if (length !== undefined && (t <= 0 || t >= length)) continue;
      await this.ensure(t);
      // Another VOD or edit opened meanwhile: its own marks aren't cut, nor is a clip made.
      if (!doc || !this.#showing(doc)) return null;
    }
    return { start: frameStart(picked.start, this.fps), end: frameStart(picked.end, this.fps) };
  }

  /** I or O (`key`) at `at`: the selection's in or out point there, or with
   *  none a new one from there to the next of `stops` (cuts, an edit's piece
   *  ends) or the end of `length`. The arrows then go on moving the
   *  playhead, not an edge or a mark. False if there's nothing to select. */
  inOut(key: "i" | "o", at: number, stops: number[], length: number): boolean {
    const picked = length ? inOut(key, at, this.selection, stops, length) : null;
    if (!picked) return false;
    this.selection = picked;
    this.selectedEdge = null;
    this.selected = null;
    return true;
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
    this.all = this.#sorted(this.all.map((m) => (m.id === id ? changed(m, { start: time }) : m)));
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
    const mark = this.all.find((m) => m.id === this.selected);
    if (!mark) return null;
    const time = nudge(mark.start, frames, fps, duration);
    this.move(mark.id, time);
    return time;
  }

  remove(id: number) {
    this.all = this.all.filter((m) => m.id !== id);
    if (this.selected === id) this.selected = null;
    if (this.editing === id) this.editing = null;
    this.#save(api.deleteMark(id));
  }

  /** Takes away mark `id`, just added by mistake, as if it had never been. */
  cancel(id: number) {
    this.all = this.all.filter((m) => m.id !== id);
    if (this.selected === id) this.selected = null;
    if (this.editing === id) this.editing = null;
    this.#save(api.cancelMark(id));
  }
}

export const cuts = new Cuts();

/**
 * The selected mark's keys (cuts.svelte.ts), the same in a VOD's player and
 * an edit's: Enter renames a chapter or highlight, as a double-click does;
 * , and . move it a frame (ten with Shift), the arrows, J and L as far as
 * they'd move the playhead, in whole frames, the playback stopping on its
 * frame, which the picture then shows; Delete takes it away (a press, not
 * its repeats); Escape lets go of it, or of the selection. True if `e` was
 * one of them, done. The keys are the map's (keys.svelte.ts), Escape aside.
 */
export function markKey(
  e: KeyboardEvent,
  at: { fps: number; length: number; pause: () => void; seekTo: (time: number) => void },
): boolean {
  if (is(e, "mark.rename") && cuts.all.some((m) => m.id === cuts.selected && m.kind !== "cut")) {
    cuts.editing = cuts.selected;
    return true;
  }
  const escape = isEscape(e);
  if (escape && cuts.selection) {
    cuts.clearSelection();
    return true;
  }
  const moving = cuts.selected !== null || (cuts.selection && cuts.selectedEdge);
  const frames = markFramesOf(e, is, at.fps);
  if (moving && frames !== undefined) {
    at.pause();
    const time = cuts.nudgeSelected(frames, at.fps, at.length);
    if (time !== null) at.seekTo(inFrame(time, at.fps, at.length));
    return true;
  }
  if (cuts.selected !== null && (is(e, "mark.remove") || escape)) {
    if (escape) cuts.selected = null;
    else if (!e.repeat) cuts.remove(cuts.selected);
    return true;
  }
  return false;
}
