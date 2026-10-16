// Edits (crates/core edits.rs): videos made of pieces of VODs. They're rows of
// the library's list among the VODs (Library.svelte, EditRow.svelte), and
// open in the player's place (EditView.svelte). Pieces get in by a clip, or a
// stretch of the timeline, being dropped on an edit's row or on "Make new
// edit" atop the list (dragout.ts), or with a clip's "Add to an edit"
// (ClipStrip.svelte). A piece is a copy: changing it changes nothing else.

import { SvelteMap } from "svelte/reactivity";
import { api, events, type Edit, type EditPiece, type EditView } from "./api";

/** A stretch of a VOD dropped on an edit, and its name if it was a clip's. */
export type Stretch = { vodId: number; start: number; end: number; title?: string };

class Edits {
  list = $state.raw<Edit[]>([]);
  /** The edit open in the Edit view, and its pieces. */
  openId = $state<number | null>(null);
  pieces = $state.raw<EditPiece[]>([]);
  /** Counts each VOD's footage downloaded for edits, for what shows it. */
  footage = new SvelteMap<number, number>();
  /** Each VOD's section downloading for edits: `from`..`to` of it, and how far, 0..1. */
  fetching = new SvelteMap<number, { from: number; to: number; progress: number }>();
  /** How far each edit being rendered is, 0..1. */
  progress = new SvelteMap<number, number>();
  error = $state<string | null>(null);
  /** The edit whose link in a clip bar is hovered: what it plays of that VOD lights up. */
  hovered = $state<number | null>(null);
  /** Where a carried clip would go: an edit's pill, "Make new edit", or neither. */
  dropTarget = $state<number | "new" | null>(null);
  #started = false;
  #seq = 0;

  get open(): Edit | undefined {
    return this.list.find((e) => e.id === this.openId);
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
    events.editRemoved((id) => this.#gone(id));
    events.footageProgress(({ vod_id, from, to, progress }) => this.fetching.set(vod_id, { from, to, progress }));
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
  async #change(action: Promise<EditView>) {
    try {
      this.#show(await action);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  openEdit(id: number | null) {
    this.hovered = null;
    this.openId = id;
    this.pieces = [];
    this.error = null;
    if (id !== null) this.#load(id);
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
  strike = (pieceId: number, struck: [number, number][]) => this.#change(api.strikePiece(pieceId, struck));
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
