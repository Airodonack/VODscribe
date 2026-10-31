// What's done with a set selected (selection.svelte.ts) all at once, and
// the keys for it: C clips each stretch selected (the players call
// `clipSegments`), Delete removes the clips selected (or, in an edit, its
// pieces too: EditView.svelte) and, in the library, the VODs and edits
// selected, after one question. Dragged, they go together (`carryChosen`,
// `carryEntries`: dragout.ts `carrySet`), into an edit where they're
// dropped, or out of the window as their files; "Add to edit…" puts them in
// one without a mouse (`addTo`). Transcribe on a VOD picked with others
// transcribes them all (`transcribeChosen`). Each is one change, so undo
// takes it back whole. Escape lets go of a set, Ctrl+A (⌘A) selects by
// where the pointer is (where.ts, the rule Ctrl+V pastes by too): over the
// clip bar, all its clips; over the library's pane, all its list;
// elsewhere, the whole timeline of what's open, from its start to its end.

import { removeEntries } from "./actions";
import { api, type Clip, type Doc, type Edit, type Put, type Vod } from "./api";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { carrySet, exportStretch, exportStretches, stretchSaid, type CarryHome, type Face } from "./dragout";
import { edits } from "./edits.svelte";
import { is } from "./keys.svelte";
import { inEdit } from "./media";
import { isEscape } from "./playerKeys";
import { count, library, snippets, type Entry, type Snippet } from "./selection.svelte";
import { toast } from "./toasts.svelte";
import { undoIn } from "./undo";
import { where, type Where } from "./where";

/** The stretches selected (or of `items`), in the order they were. */
export const chosenSegments = (items = snippets.items) =>
  items.filter((i): i is Extract<Snippet, { kind: "segment" }> => i.kind === "segment");

/** The clips selected (or of `items`) that are still in the clip bar, in the order they were. */
export const chosenClips = (items = snippets.items) =>
  items.flatMap((i) => (i.kind === "clip" && clips.shown.some((c) => c.id === i.id) ? [i.id] : []));

/** A clip in the clip bar of each stretch selected, in the order they were, as one change; the set then goes. */
export async function clipSegments() {
  const ranges = chosenSegments().map((s) => ({ start: s.start, end: s.end }));
  if (!ranges.length) return;
  snippets.clear();
  if (clips.doc?.kind === "edit") await clips.makeAll(ranges);
  else await exportStretches(ranges);
}

/** Takes the clips selected off a VOD's clip bar, as one change; the set then goes. */
export async function removeChosenClips() {
  const ids = chosenClips();
  const doc = clips.doc;
  if (!ids.length || !doc) return;
  snippets.clear();
  clips.hovered = null;
  try {
    await api.deleteClips(ids);
    toast.show(`${count([[ids.length, "clip", "clips"]])} deleted`, { label: "Undo", run: () => undoIn(doc) });
  } catch (e) {
    clips.error = String(e);
  }
}

/** What's picked on the timeline and in the clip bar, in the order picked, as
 *  an edit takes it: of a VOD, its stretches (a clip's, with its name); of an
 *  edit, stretches of its timeline and the clips of its clip bar as they are. */
export function chosenPuts(items = snippets.items): Put[] {
  const doc = clips.doc;
  if (!doc) return [];
  return items.flatMap((i): Put[] => {
    if (i.kind === "segment")
      return [
        doc.kind === "vod"
          ? { kind: "range", vodId: doc.id, start: i.start, end: i.end }
          : { kind: "span", clip: doc.id, from: i.start, to: i.end },
      ];
    const clip = clips.shown.find((c) => c.id === i.id);
    if (!clip) return [];
    return [
      doc.kind === "vod"
        ? { kind: "range", vodId: doc.id, start: clip.start, end: clip.end, title: clip.title }
        : { kind: "clip", id: clip.id },
    ];
  });
}

/** What's picked in the clip bar and on the timeline, said: "2 stretches and 1 clip". */
const chosenSaid = (items: Snippet[]) =>
  count([
    [chosenSegments(items).length, "stretch", "stretches"],
    [chosenClips(items).length, "clip", "clips"],
  ]);

/** Each one picked as a clip whose file can be dragged out of the window, in
 *  the order picked: a clip's own, a stretch's made for the drag (not in the
 *  clip bar). None if one of them can't be made (those made go again). */
async function chosenFiles(items: Snippet[]): Promise<Clip[]> {
  const doc = clips.doc;
  const made: Clip[] = [];
  for (const i of items) {
    const clip =
      i.kind === "clip"
        ? (clips.shown.find((c) => c.id === i.id) ?? null)
        : doc?.kind === "edit"
          ? await clips.make(i.start, i.end, false)
          : await exportStretch(i.start, i.end, false);
    if (!clip) {
      for (const m of made) if (!m.shown) api.deleteClip(m.id).catch((e) => console.error("forget dragged clip", e));
      return [];
    }
    made.push(clip);
  }
  return made;
}

/** How `item` (a stretch of the open timeline, a clip of its clip bar) looks
 *  carried: the stretch of its VOD it starts with. */
function faceOf(item: Snippet): Face | undefined {
  const doc = clips.doc;
  if (item.kind === "clip") {
    const clip = clips.shown.find((c) => c.id === item.id);
    return clip && { vodId: clip.vod_id, start: clip.start, end: clip.end };
  }
  if (doc?.kind === "vod") return { vodId: doc.id, start: item.start, end: item.end };
  return doc && edits.openId === doc.id ? inEdit(edits.pieces, item.start, item.end) : undefined;
}

/** Carries what's picked on the timeline and in the clip bar from (`x`, `y`)
 *  (a drag of one of them, flying from `from`; back over `home`, if given, the
 *  carry ends and it takes the press on): see dragout.ts `carrySet`. */
export function carryChosen(from: DOMRect, x: number, y: number, home?: CarryHome): Promise<void> {
  return carrySnippets(snippets.items, from, x, y, home);
}

/** Carries `items` (stretches of the open timeline, clips of its clip bar), as
 *  `carryChosen` carries what's picked: a stretch between an edit's cuts
 *  Alt-dragged goes this way, as itself rather than its whole piece. */
export function carrySnippets(list: Snippet[], from: DOMRect, x: number, y: number, home?: CarryHome): Promise<void> {
  const items = chosenPuts(list);
  const doc = clips.doc;
  if (!items.length || !doc) return Promise.resolve();
  const faces = list.slice(0, 3).flatMap((i) => faceOf(i) ?? []);
  // One alone is said as a stretch carried by itself is; several, by how many.
  const one = items.length === 1 && faces[0];
  return carrySet({
    items,
    label: one ? stretchSaid(one.start, one.end ?? one.start) : chosenSaid(list),
    faces,
    stretches: doc.kind === "vod" ? chosenSegments(list).map((s) => ({ start: s.start, end: s.end })) : [],
    files: () => chosenFiles(list),
    // Made as the carry begins where that's quick: a VOD playing from a file
    // here. A site's would be downloaded for every carry, even one dropped
    // inside the window; an edit's pieces may be a site's too.
    early: doc.kind === "vod" && !clips.fromSite,
    vodId: doc.kind === "vod" ? doc.id : undefined,
    home,
    from,
    x,
    y,
  });
}

/** What's picked in the library, in the order picked, as an edit takes it: each VOD whole, each edit as it is. */
export const chosenEntries = (): Put[] =>
  library.items.map((e): Put => (e.kind === "vod" ? { kind: "vod", id: e.id } : { kind: "clip", id: e.id }));

/** How the first few rows of the library carried look: a VOD whole, an edit by its first piece. */
async function entryFaces(items: Put[]): Promise<Face[]> {
  const faces = await Promise.all(
    items.slice(0, 3).map(async (p): Promise<Face | undefined> => {
      if (p.kind === "vod") return { vodId: p.id, start: 0 };
      if (p.kind !== "clip") return undefined;
      const view = await api.getEdit(p.id).catch(() => null);
      return view ? inEdit(view.pieces, 0, Infinity) : undefined;
    }),
  );
  return faces.flatMap((f) => f ?? []);
}

/** Carries the rows picked in the library from (`x`, `y`) (a drag of one of
 *  them, flying from `from`), into an edit: see dragout.ts `carrySet`. */
export function carryEntries(from: DOMRect, x: number, y: number) {
  const items = chosenEntries();
  if (!items.length) return;
  const vods = items.filter((p) => p.kind === "vod").length;
  const label = count([
    [vods, "VOD", "VODs"],
    [items.length - vods, "edit", "edits"],
  ]);
  void carrySet({ items, label, faces: entryFaces(items), stretches: [], from, x, y });
}

/**
 * Puts `items` (what's picked) in edit `editId`, called `title`, as one
 * change: on its clip bar if `bar`, else at the end of its timeline; as a
 * drop there would (not an edit into itself, nor its own stretches again on
 * its timeline). Says so, with Undo.
 */
export async function addTo(editId: number, title: string, bar: boolean, items: Put[]) {
  const take = items.filter(
    (p) => !(p.kind === "clip" && p.id === editId) && !(!bar && p.kind === "span" && p.clip === editId),
  );
  if (!take.length) return;
  if (await edits.put(editId, take, bar))
    toast.show(`Added to “${title}”${bar ? ", on its clip bar" : ""}`, {
      label: "Undo",
      run: () => undoIn({ kind: "edit", id: editId }),
    });
  else if (edits.error) toast.show(`Couldn't add to “${title}”: ${edits.error}`);
}

/** Whether VOD `vod` can be transcribed from its row (nothing asked of it, and not all of it done). */
export const transcribable = (vod: Vod) => vod.status === "none" && vod.transcribed !== "all";

/** Transcribes VOD `vod` from its row: the rest of it, if some is done; else all of it. */
export const transcribe = (vod: Vod) =>
  vod.transcribed === "some" ? api.retryVod(vod.id) : api.transcribeStretch(vod.id, 0, null);

/** The VODs picked in the library (of `rows`) that can be transcribed, in the order picked. */
export function chosenToTranscribe(rows: Row[]): Vod[] {
  const byId = new Map(rows.flatMap((r) => (r.kind === "vod" ? [[r.vod.id, r.vod] as const] : [])));
  return library.items.flatMap((e) => {
    const vod = e.kind === "vod" ? byId.get(e.id) : undefined;
    return vod && transcribable(vod) ? [vod] : [];
  });
}

/** Asks for each of `vods` to be transcribed, in order: they queue (one
 *  download and one transcription run at a time). The set then goes. */
export async function transcribeChosen(vods: Vod[], onerror: (message: string) => void) {
  library.clear();
  for (const vod of vods) await transcribe(vod).catch((e) => onerror(`Couldn't transcribe “${vod.title}”: ${e}`));
}

/** A row of the library's list, as Library.svelte has them. */
type Row = { kind: "vod"; vod: Vod } | { kind: "edit"; edit: Edit };

/**
 * Removes the VODs and edits selected in the library (of `rows`), after one
 * question naming them; `onremove` is told of each VOD before it goes (it
 * may be the one open). Each goes as its row's Remove takes it: a VOD that
 * edits use, with its video or without, as the answer says.
 */
export async function removeChosenEntries(rows: Row[], onremove: (vodId: number) => void) {
  const picked = (kind: Entry["kind"], id: number) => library.has({ kind, id });
  const vods = rows.flatMap((r) => (r.kind === "vod" && picked("vod", r.vod.id) ? [r.vod] : []));
  const gone = rows.flatMap((r) => (r.kind === "edit" && picked("edit", r.edit.id) ? [r.edit] : []));
  await removeEntries(vods, gone, { onremove, confirmed: () => library.clear() });
}

/** Where the pointer last was in the window (null before it has moved). */
let pointer: { x: number; y: number } | null = null;

/** The pointer moved to (`x`, `y`) (App.svelte tells, for Ctrl+A and Ctrl+V). */
export function pointerAt(x: number, y: number) {
  pointer = { x, y };
}

/** What Ctrl+A selects and Ctrl+V pastes into now, the page as it is (where.ts). */
export function whereKeys(): Where | null {
  const box = (selector: string) => document.querySelector(selector)?.getBoundingClientRect() ?? null;
  const focused = document.activeElement;
  return where({
    pointer,
    pane: box(".library-pane"),
    bar: box(".clip-bar"),
    focus: focused?.closest(".library-list") ? "library" : focused?.closest(".clip-bar") ? "clips" : "other",
    timeline: !!document.querySelector(".scrubber") && cuts.doc !== null && cuts.length > 0,
  });
}

/**
 * The keys for a set selected, asked first (App.svelte `onWindowKey`, before
 * either player): true if `e` was one, done (Ctrl+A always is, outside a
 * text field). `rows` is the library's list; `onremove` is told of each VOD
 * removed. A VOD's or an edit's own Delete and C for the snippet set are
 * theirs (Player.svelte, EditView.svelte).
 */
export function selectionKey(e: KeyboardEvent, rows: Row[], onremove: (vodId: number) => void): boolean {
  if (isEscape(e)) {
    // Innermost first: a mark's title being written, or a menu open, has it.
    if (cuts.editing !== null || document.querySelector("[role=dialog]")) return false;
    // The library's open row alone isn't a set: Escape goes on to what's next.
    if (!snippets.picking && !library.picking) return false;
    snippets.clear();
    library.clear();
    return true;
  }
  if (is(e, "select.all")) {
    const into = whereKeys();
    if (into === "clips") snippets.set(clips.shown.map((c) => ({ kind: "clip", id: c.id })));
    else if (into === "library")
      library.set(
        rows.map((r) => (r.kind === "vod" ? { kind: "vod", id: r.vod.id } : { kind: "edit", id: r.edit.id })),
      );
    else if (into === "timeline") {
      // The whole of it, selected (not cut): what Ctrl+C then copies.
      cuts.selection = { start: 0, end: cuts.length };
      cuts.selectedEdge = null;
    }
    // The key is the app's even with nothing to pick (the main side with
    // nothing open): the web view's own select-all would light up the page's text.
    return true;
  }
  // Not the open row alone: Delete then is the player's or the edit's.
  if (library.picking && is(e, "library.remove")) {
    if (!e.repeat) void removeChosenEntries(rows, onremove);
    return true;
  }
  return false;
}
