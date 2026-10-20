// Edits (crates/core edits.rs): videos made of pieces of VODs. They're rows of
// the library's list among the VODs (Library.svelte, EditRow.svelte), and
// open in the player's place (EditView.svelte). Pieces get in by a clip, or a
// stretch of the timeline, being dropped on an edit's row or on "Make new
// edit" atop the list (dragout.ts), or with a clip's "Add to an edit"
// (ClipStrip.svelte). A piece is a copy: changing it changes nothing else.

import { SvelteMap } from "svelte/reactivity";
import { playedSpans } from "./media";
import { nav } from "./nav.svelte";
import { api, events, type Edit, type EditPiece, type EditView, type Fetching } from "./api";

/** A stretch of a VOD dropped on an edit, and its name if it was a clip's. */
export type Stretch = { vodId: number; start: number; end: number; title?: string };

class Edits {
  list = $state.raw<Edit[]>([]);
  /** The edit open in the Edit view, and its pieces. */
  openId = $state<number | null>(null);
  pieces = $state.raw<EditPiece[]>([]);
  /** Counts each VOD's footage downloaded for edits, for what shows it. */
  footage = new SvelteMap<number, number>();
  /** Each VOD's section downloading for edits, and how far it is. */
  fetching = new SvelteMap<number, Fetching>();
  /** How far each edit being rendered is, 0..1. */
  progress = new SvelteMap<number, number>();
  error = $state<string | null>(null);
  /** The edit whose link in a clip bar is hovered: what it plays of that VOD lights up. */
  hovered = $state<number | null>(null);
  /** An edit whose source link was followed to VOD `vodId`: what it plays of it stays lit there. */
  lit = $state<{ editId: number; vodId: number } | null>(null);
  /** Where a carried clip would go: an edit's pill, "Make new edit", or neither. */
  dropTarget = $state<number | "new" | null>(null);
  /** A stretch of the open edit's timeline chosen in its transcript: what Delete takes out. */
  selection = $state<{ start: number; end: number } | null>(null);
  /** Where the open edit's player was asked to go (a transcript line clicked), once each. */
  wanted = $state.raw<{ time: number } | null>(null);
  /** A clip of a clip bar open in the editor (not in the library's list). */
  #snippet = $state.raw<Edit | null>(null);
  #started = false;
  #seq = 0;

  get open(): Edit | undefined {
    return (
      this.list.find((e) => e.id === this.openId) ?? (this.#snippet?.id === this.openId ? this.#snippet : undefined)
    );
  }

  start() {
    if (this.#started) return;
    this.#started = true;
    this.#reloadList();
    events.edit((edit) => {
      if (edit.status !== "cutting") this.progress.delete(edit.id);
      this.#put(edit);
    });
    events.editProgress((id, p) => this.progress.set(id, p));
    events.editRemoved((id) => {
      nav.forget({ kind: "edit", id });
      this.#gone(id);
    });
    events.footageProgress((f) => this.fetching.set(f.vod_id, f));
    events.footage(({ vod_id, error }) => {
      this.fetching.delete(vod_id);
      this.footage.set(vod_id, (this.footage.get(vod_id) ?? 0) + 1);
      if (error && this.open?.vod_ids.includes(vod_id)) this.error = `Couldn't download the footage: ${error}`;
    });
    events.vodRemoved(() => {
      this.#reloadList();
      if (this.openId !== null) this.#load(this.openId);
    });
  }

  #reloadList() {
    api.listEdits().then(
      (list) => (this.list = list),
      (e) => (this.error = String(e)),
    );
  }

  /** The edits with pieces of VOD `vodId`. */
  forVod(vodId: number): Edit[] {
    return this.list.filter((e) => e.vod_ids.includes(vodId));
  }

  /** A new edit holding a stretch of a VOD. */
  async createWith(stretch: Stretch): Promise<Edit | null> {
    const edit = await this.create();
    if (edit) await this.addRange(edit.id, stretch);
    return edit;
  }

  #put(edit: Edit) {
    // A clip of a clip bar is the editor's while it's open, never a row.
    if (edit.snippet_of !== null) {
      if (edit.id === this.openId) this.#snippet = edit;
      return;
    }
    const at = this.list.findIndex((e) => e.id === edit.id);
    this.list = at < 0 ? [...this.list, edit] : this.list.map((e, i) => (i === at ? edit : e));
  }

  #gone(id: number) {
    this.list = this.list.filter((e) => e.id !== id);
    if (this.openId === id) {
      this.openId = null;
      this.pieces = [];
    }
  }

  /** Loads edit `id` again (undo or redo changed it), if it's in the list or open. */
  reload(id: number) {
    if (id === this.openId) this.#load(id);
    else if (this.list.some((e) => e.id === id))
      api.getEdit(id).then(
        (view) => this.#put(view.edit),
        () => {},
      );
  }

  #load(id: number) {
    const mine = ++this.#seq;
    api.getEdit(id).then(
      (view) => mine === this.#seq && this.#show(view),
      (e) => mine === this.#seq && (this.error = String(e)),
    );
  }

  #show(view: EditView) {
    this.#put(view.edit);
    if (view.edit.id === this.openId) this.pieces = view.pieces;
  }

  /** Runs a change to an edit; what comes back is the edit as it is now. */
  async #change(action: Promise<EditView>): Promise<boolean> {
    try {
      this.#show(await action);
      this.error = null;
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    }
  }

  openEdit(id: number | null) {
    this.hovered = null;
    this.lit = null;
    this.openId = id;
    this.pieces = [];
    this.selection = null;
    this.wanted = null;
    this.error = null;
    if (id !== null) this.#load(id);
  }

  /** How long the open edit plays: its pieces' played stretches end to end. */
  get length(): number {
    return this.pieces.reduce(
      (sum, p) => sum + playedSpans(p.start, p.end, p.struck).reduce((t, [a, b]) => t + b - a, 0),
      0,
    );
  }

  /** Plays the open edit from `time` (seconds of its timeline). */
  playAt(time: number) {
    this.wanted = { time };
  }

  /** Takes the selected stretch out of the open edit, as one change (undo brings it back). */
  async removeSelection(): Promise<boolean> {
    const pick = this.selection;
    if (this.openId === null || !pick) return false;
    this.selection = null;
    return this.#change(api.removeStretch(this.openId, pick.start, pick.end));
  }

  /** Takes `from`..`to` of the open edit out (between split marks, which
   *  become a real split), as one change. */
  async removeStretch(from: number, to: number): Promise<boolean> {
    if (this.openId === null) return false;
    this.selection = null;
    return this.#change(api.removeStretch(this.openId, from, to));
  }

  async create(): Promise<Edit | null> {
    try {
      const edit = await api.createEdit();
      this.#put(edit);
      return edit;
    } catch (e) {
      this.error = String(e);
      return null;
    }
  }

  async remove(id: number) {
    try {
      await api.deleteEdit(id);
      this.#gone(id);
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Puts a copy of clip `clipId` in edit `editId` (last, or at `index`). */
  add = (editId: number, clipId: number, index?: number) => this.#change(api.addToEdit(editId, clipId, index));
  /** Puts a stretch of a VOD in edit `editId` (last, or at `index`). */
  addRange = (editId: number, s: Stretch, index?: number) =>
    this.#change(api.addRangeToEdit(editId, s.vodId, s.start, s.end, s.title, index));
  rename = (id: number, title: string) => this.#change(api.renameEdit(id, title));
  move = (pieceId: number, index: number) => this.#change(api.movePiece(pieceId, index));
  trim = (pieceId: number, start: number, end: number) => this.#change(api.trimPiece(pieceId, start, end));
  /** Strikes `struck` of piece `pieceId`: shown at once, and saved after any
   *  strikes before it, so a line clicked before the last is saved builds on it. */
  strike(pieceId: number, struck: [number, number][]): Promise<unknown> {
    this.pieces = this.pieces.map((p) => (p.id === pieceId ? { ...p, struck } : p));
    this.#striking++;
    const save = async () => {
      try {
        const view = await api.strikePiece(pieceId, struck);
        // What's saved shows once the last strike is (each one's answer would undo the next on screen).
        if (--this.#striking === 0) this.#show(view);
        this.error = null;
      } catch (e) {
        this.#striking--;
        this.error = String(e);
        if (this.openId !== null) this.#load(this.openId);
      }
    };
    this.#strikes = this.#strikes.then(save);
    return this.#strikes;
  }
  #striking = 0;
  #strikes: Promise<unknown> = Promise.resolve();
  removePiece = (pieceId: number) => this.#change(api.removePiece(pieceId));
  split = (pieceId: number, at: number) => this.#change(api.splitPiece(pieceId, at));

  async render(id: number) {
    this.error = null;
    this.progress.set(id, 0);
    await api.renderEdit(id).catch((e) => (this.error = String(e)));
  }

  stop = (id: number) => api.stopRender(id).catch((e) => (this.error = String(e)));
}

export const edits = new Edits();
