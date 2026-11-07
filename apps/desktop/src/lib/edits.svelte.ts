// Edits (crates/core edits.rs): videos made of pieces of VODs. They're rows of
// the library's list among the VODs (Library.svelte, EditRow.svelte), and
// open in the player's place (EditView.svelte). Pieces get in by a clip, or a
// stretch of the timeline, being dropped on an edit's row or on "Make new
// edit" atop the list (dragout.ts), or with the library's "Add to edit…"
// for what's picked there (AddToEdit.svelte). A piece is a copy: changing it
// changes nothing else.

import { SvelteMap } from "svelte/reactivity";
import { playedLength } from "./media";
import { nav } from "./nav.svelte";
import { api, events, type Edit, type EditPiece, type EditView, type Fetching, type Put } from "./api";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { toast } from "./toasts.svelte";
import { saving } from "./writes";

/** A stretch of a VOD dropped on an edit, and its name if it was a clip's;
 *  `clip` if it's a clip of the clip bar, which an edit takes as itself (a
 *  group, with the clip's own cuts), as `add` puts it. */
export type Stretch = {
  vodId: number;
  start: number;
  end: number;
  title?: string;
  clip?: number;
  /** A highlight's (the annotations pane's): it goes in as a group named for it, as a clip's pill does. */
  highlight?: boolean;
};

/** What `s` goes in an edit as: a highlight's a group named for it, else a stretch. */
export const putOf = (s: Stretch): Put =>
  s.highlight
    ? { kind: "highlight", vodId: s.vodId, start: s.start, end: s.end, title: s.title ?? "Highlight" }
    : { kind: "range", vodId: s.vodId, start: s.start, end: s.end, title: s.title };

class Edits {
  list = $state.raw<Edit[]>([]);
  /** The edit open in the Edit view, and its pieces. */
  openId = $state<number | null>(null);
  pieces = $state.raw<EditPiece[]>([]);
  /** Whether all the open edit's footage is in files here (each piece's VOD
   *  plays from one, EditView.svelte): a clip of it dragged is made as the
   *  carry begins, needing no download (dragout.ts). */
  footageHere = $state(false);
  /** Counts each VOD's footage downloaded for edits, for what shows it. */
  footage = new SvelteMap<number, number>();
  /** Each VOD's section downloading (for edits, or all of it for a VOD played
   *  as its video downloads, videos.rs), and how far it is. */
  fetching = new SvelteMap<number, Fetching>();
  /** How far each edit being rendered is, 0..1. */
  progress = new SvelteMap<number, number>();
  /** What last went wrong, if anything: shown as a toast (toasts.svelte.ts)
   *  wherever the user is, as the VOD's and the edit's errors are, rather than
   *  in one view's bar, where it went unseen from another view or with the
   *  player collapsed. Put right (set back to null), it goes. */
  #error = $state<string | null>(null);
  #errorShown = 0;
  get error() {
    return this.#error;
  }
  set error(message: string | null) {
    this.#error = message;
    if (message) this.#errorShown = toast.error(message);
    else toast.drop(this.#errorShown);
  }
  /** The edit whose link in a clip bar is hovered: what it plays of that VOD lights up. */
  hovered = $state<number | null>(null);
  /** An edit whose source link was followed to VOD `vodId`: what it plays of it stays lit there. */
  lit = $state<{ editId: number; vodId: number } | null>(null);
  /** Where a carried clip would go: an edit's pill, "Make new edit", or neither. */
  dropTarget = $state<number | "new" | null>(null);
  /** Where on the open edit's timeline a carried clip would go (its time there), while it's over it. */
  inserting = $state<number | null>(null);
  /** Where the open edit's player was asked to go (a transcript line clicked:
   *  playing; the end of what was pasted: not), once each. */
  wanted = $state.raw<{ time: number; play?: boolean } | null>(null);
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
      this.#heard(edit.id);
      this.#put(edit);
    });
    events.editProgress((id, p) => this.progress.set(id, p));
    events.editRemoved((id) => {
      this.#heard(id);
      nav.forget({ kind: "edit", id });
      this.#gone(id);
    });
    // A clip of a clip bar open in the editor closes when the clip is
    // deleted (from its bar, or with the edit or VOD whose bar it's on), as
    // an edit does.
    events.clipRemoved((id) => {
      if (this.openId !== id || this.list.some((e) => e.id === id)) return;
      this.#heard(id);
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
    const since = this.#serial;
    api.listEdits().then(
      (list) => (this.list = this.#listed(list, since)),
      (e) => (this.error = String(e)),
    );
  }

  // Each edit's latest news (its "edit" or "edit-removed" event), numbered,
  // as the clips store numbers its clips': a command's answer is older than
  // news that came after it was sent (on Linux an event sent from another
  // thread can reach the page before an answer sent earlier: page.rs), and
  // isn't put over it. A rename's answer saying "rendering", over the
  // render's "ready" just before, left the row rendering with no progress.
  #serial = 0;
  #news = new Map<number, number>();

  /** How far the edits' news has come: `took` takes what a command sent at this point answers. */
  get news(): number {
    return this.#serial;
  }

  #heard(id: number) {
    this.#news.set(id, ++this.#serial);
  }

  /** Whether edit `id` has had no news since `since`. */
  #current(id: number, since: number): boolean {
    return (this.#news.get(id) ?? 0) <= since;
  }

  /** `list`, the edits as listed when the news was at `since`, with the news
   *  since: an edit with news after it as that has it, or gone if deleted. */
  #listed(list: Edit[], since: number): Edit[] {
    const now = new Map(this.list.map((e) => [e.id, e]));
    const listed = new Set(list.map((e) => e.id));
    const kept = list.flatMap((e) => (this.#current(e.id, since) ? [e] : (now.get(e.id) ?? [])));
    return [...kept, ...this.list.filter((e) => !listed.has(e.id) && !this.#current(e.id, since))];
  }

  /** How far (the VOD's time) the section downloading of VOD `vodId` plays
   *  already, or a guess from what's downloaded till that's known; null with
   *  none downloading. */
  usable(vodId: number): number | null {
    const f = this.fetching.get(vodId);
    return f ? (f.usable ?? f.from + f.progress * (f.to - f.from) - 1) : null;
  }

  /** The edits with pieces of VOD `vodId`. */
  forVod(vodId: number): Edit[] {
    return this.list.filter((e) => e.vod_ids.includes(vodId));
  }

  /** A new edit holding a stretch of a VOD, or several things picked (in their order). */
  async createWith(what: Stretch | Put[]): Promise<Edit | null> {
    const edit = await this.create();
    if (!edit) return edit;
    if (Array.isArray(what)) await this.put(edit.id, what, false);
    else if (what.clip !== undefined) await this.add(edit.id, what.clip);
    else if (what.highlight) await this.put(edit.id, [putOf(what)], false);
    else await this.addRange(edit.id, what);
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
    if (this.#snippet?.id === id) this.#snippet = null;
    if (this.openId === id) {
      this.openId = null;
      this.pieces = [];
    }
  }

  /** Loads edit `id` again (undo or redo changed it), if it's in the list or open. */
  reload(id: number) {
    if (id === this.openId) this.#load(id);
    else if (this.list.some((e) => e.id === id)) {
      const since = this.#serial;
      api.getEdit(id).then(
        (view) => this.#current(id, since) && this.#put(view.edit),
        () => {},
      );
    }
  }

  #load(id: number) {
    const mine = ++this.#seq;
    const since = this.#serial;
    api.getEdit(id).then(
      (view) => mine === this.#seq && this.#show(view, since),
      (e) => mine === this.#seq && (this.error = String(e)),
    );
  }

  /** Shows `view`, a command's answer to one sent when the news was at
   *  `since`: its pieces (only commands change them), and the edit itself
   *  unless news of it has come since. */
  #show(view: EditView, since: number) {
    if (this.#current(view.edit.id, since)) this.#put(view.edit);
    if (view.edit.id === this.openId) this.pieces = view.pieces;
  }

  /** Shows `view`, what a change made elsewhere (a paste: copyPaste.ts) left
   *  the edit as, sent when the news was at `since` (`news`). */
  took(view: EditView, since: number) {
    this.#show(view, since);
  }

  /** Runs a change to an edit; what comes back is the edit as it is now. */
  async #change(action: Promise<EditView>): Promise<boolean> {
    const since = this.#serial;
    try {
      this.#show(await saving(action), since);
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
    if (cuts.doc?.kind === "edit") cuts.clearSelection();
    this.wanted = null;
    this.error = null;
    if (id !== null) this.#load(id);
  }

  /** How long the open edit plays: its pieces' played stretches end to end. */
  get length(): number {
    return this.pieces.reduce((sum, p) => sum + playedLength(p), 0);
  }

  /** Plays the open edit from `time` (seconds of its timeline). */
  playAt(time: number) {
    this.wanted = { time };
  }

  /** Takes the stretch selected (cuts.svelte.ts, the open edit's) out of
   *  it, as one change (undo brings it back). */
  async removeSelection(): Promise<boolean> {
    const pick = cuts.selection;
    if (this.openId === null || !pick || !cuts.isOf({ kind: "edit", id: this.openId })) return false;
    cuts.clearSelection();
    return this.#change(api.removeStretch(this.openId, pick.start, pick.end));
  }

  /** Takes `from`..`to` of the open edit out (between cuts, which
   *  become a real split), as one change. */
  async removeStretch(from: number, to: number): Promise<boolean> {
    if (this.openId === null) return false;
    if (cuts.isOf({ kind: "edit", id: this.openId })) cuts.clearSelection();
    return this.#change(api.removeStretch(this.openId, from, to));
  }

  /** Takes what's picked out of the open edit, as one change: pieces
   *  `pieces`, stretches `stretches` of its timeline, clips `clipIds` of its clip bar. */
  async removeFrom(pieces: number[], stretches: [number, number][], clipIds: number[]): Promise<boolean> {
    if (this.openId === null) return false;
    return this.#change(api.removeFromEdit(this.openId, pieces, stretches, clipIds));
  }

  async create(): Promise<Edit | null> {
    const since = this.#serial;
    try {
      const edit = await api.createEdit();
      if (this.#current(edit.id, since)) this.#put(edit);
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
  /** Puts several things picked in edit `editId`, in their order, as one change: on its clip
   *  bar if `bar` (shown again if it's open), else on its timeline at `index` (or last). */
  async put(editId: number, items: Put[], bar: boolean, index?: number): Promise<boolean> {
    if (!items.length) return false;
    const done = await this.#change(api.putInEdit(editId, items, bar, index));
    if (done && bar) clips.reload({ kind: "edit", id: editId });
    return done;
  }
  rename = (id: number, title: string) => this.#change(api.renameEdit(id, title));
  move = (pieceId: number, index: number) => this.#change(api.movePiece(pieceId, index));
  trim = (pieceId: number, start: number, end: number) => this.#change(api.trimPiece(pieceId, start, end));
  removePiece = (pieceId: number) => this.#change(api.removePiece(pieceId));

  async render(id: number) {
    this.error = null;
    this.progress.set(id, 0);
    await api.renderEdit(id).catch((e) => (this.error = String(e)));
  }

  stop = (id: number) => api.stopRender(id).catch((e) => (this.error = String(e)));
}

export const edits = new Edits();
