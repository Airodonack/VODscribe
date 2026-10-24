// What's done with a set selected (selection.svelte.ts) all at once, and
// the keys for it: C clips each stretch selected (the players call
// `clipSegments`), Delete removes the clips selected (or, in an edit, its
// pieces too: EditView.svelte) and, in the library, the VODs and edits
// selected, after one question. Dragged, they go together (`carryChosen`,
// `carryEntries`: dragout.ts `carrySet`), into an edit where they're
// dropped, or out of the window as their files; "Add to edit…" puts them in
// one without a mouse (`addTo`). Transcribe on a VOD picked with others
// transcribes them all (`transcribeChosen`). Each is one change, so undo
// takes it back whole. Escape lets go of a set, Ctrl+A (⌘A) selects all of
// the clip bar or the library's list, by which side of the window the
// pointer is on (the focus's, before the pointer has moved).

import { api, type Clip, type Doc, type Edit, type Put, type Vod } from "./api";
import { ask, choose } from "./ask";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { carrySet, exportStretch, exportStretches } from "./dragout";
import { edits } from "./edits.svelte";
import { is } from "./keys.svelte";
import { listed } from "./platform";
import { isEscape } from "./playerKeys";
import { DELETE, REMOVE, removeVod } from "./removal.svelte";
import { count, library, snippets, type Entry, type Snippet } from "./selection.svelte";
import { toast } from "./toasts.svelte";

/** The stretches selected, in the order they were. */
export const chosenSegments = () =>
  snippets.items.filter((i): i is Extract<Snippet, { kind: "segment" }> => i.kind === "segment");

/** The clips selected that are still in the clip bar, in the order they were. */
export const chosenClips = () =>
  snippets.items.flatMap((i) => (i.kind === "clip" && clips.shown.some((c) => c.id === i.id) ? [i.id] : []));

/** Undoes the newest change in `doc`'s history (a toast's Undo). */
export function undoIn(doc: Doc) {
  api.undo(doc).then(
    (done) => done && toast.show(`Undone: ${done.label}`),
    (e) => toast.show(`Couldn't undo: ${e}`),
  );
}

/** A clip in the clip bar of each stretch selected, in the order they were, as one change; the set then goes. */
export async function clipSegments() {
  const ranges = chosenSegments().map((s) => ({ start: s.start, end: s.end }));
  if (!ranges.length) return;
  snippets.clear();
  if (clips.doc?.kind === "edit") await clips.makeAllOfEdit(ranges);
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
export function chosenPuts(): Put[] {
  const doc = clips.doc;
  if (!doc) return [];
  return snippets.items.flatMap((i): Put[] => {
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
const chosenSaid = () =>
  count([
    [chosenSegments().length, "stretch", "stretches"],
    [chosenClips().length, "clip", "clips"],
  ]);

/** Each one picked as a clip whose file can be dragged out of the window, in
 *  the order picked: a clip's own, a stretch's made for the drag (not in the
 *  clip bar). None if one of them can't be made (those made go again). */
async function chosenFiles(): Promise<Clip[]> {
  const doc = clips.doc;
  const made: Clip[] = [];
  for (const i of snippets.items) {
    const clip =
      i.kind === "clip"
        ? (clips.shown.find((c) => c.id === i.id) ?? null)
        : doc?.kind === "edit"
          ? await clips.makeOfEdit(i.start, i.end, false)
          : await exportStretch(i.start, i.end, false);
    if (!clip) {
      for (const m of made) if (!m.shown) api.deleteClip(m.id).catch((e) => console.error("forget dragged clip", e));
      return [];
    }
    made.push(clip);
  }
  return made;
}

/** Carries what's picked on the timeline and in the clip bar from (`x`, `y`)
 *  (a drag of one of them, flying from `from`): see dragout.ts `carrySet`. */
export function carryChosen(from: DOMRect, x: number, y: number) {
  const items = chosenPuts();
  const doc = clips.doc;
  if (!items.length || !doc) return;
  void carrySet({
    items,
    label: chosenSaid(),
    stretches: doc.kind === "vod" ? chosenSegments().map((s) => ({ start: s.start, end: s.end })) : [],
    files: chosenFiles,
    vodId: doc.kind === "vod" ? doc.id : undefined,
    from,
    x,
    y,
  });
}

/** What's picked in the library, in the order picked, as an edit takes it: each VOD whole, each edit as it is. */
export const chosenEntries = (): Put[] =>
  library.items.map((e): Put => (e.kind === "vod" ? { kind: "vod", id: e.id } : { kind: "clip", id: e.id }));

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
  void carrySet({ items, label, stretches: [], from, x, y });
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
  if (!vods.length && !gone.length) return;
  const what = count([
    [vods.length, "VOD", "VODs"],
    [gone.length, "edit", "edits"],
  ]);
  // Edits staying that use a VOD going: its video can stay for them, or go.
  const using = [
    ...new Set(vods.flatMap((v) => edits.forVod(v.id).filter((e) => !gone.some((g) => g.id === e.id)))),
  ].map((e) => `“${e.title}”`);
  const theirs = vods.length === 1 ? "Its" : "Their";
  const vodText = vods.length
    ? ` ${vods.length === 1 ? "The VOD's transcript" : "The VODs' transcripts"} and any downloaded audio or video are erased from this computer, and ${theirs.toLowerCase()} clips leave VODscribe (their files stay in your Videos folder).`
    : "";
  const editText = gone.length
    ? ` ${gone.length === 1 ? "The edit is" : "The edits are"} deleted${gone.some((e) => e.file_path) ? ", with ".concat(gone.length === 1 ? "its video" : "their videos") : ""}.`
    : "";
  let deleteVideo = false;
  if (using.length) {
    const answer = await choose(
      `Remove ${what}?${vodText}${editText} ${theirs} pieces in ${using.length === 1 ? "the edit" : "the edits"} ${listed(using)} stay: removed from the library, the video stays on this computer for them; deleted, only the stretches they use are kept.`,
      REMOVE,
      DELETE,
      "Keep",
    );
    if (!answer) return;
    deleteVideo = answer === DELETE;
  } else if (!(await ask(`Remove ${what}?${vodText}${editText}`, "Remove", "Keep"))) return;
  library.clear();
  for (const edit of gone) await edits.remove(edit.id);
  for (const vod of vods) {
    onremove(vod.id);
    await removeVod(vod, deleteVideo, (message) => toast.show(`Couldn't remove “${vod.title}”: ${message}`));
  }
}

/** Where the pointer last was in the window, across (null before it has moved). */
let pointerX: number | null = null;

/** The pointer moved to `x` (App.svelte tells, for Ctrl+A). */
export function pointerAt(x: number) {
  pointerX = x;
}

/**
 * What a press of Ctrl+A selects: the library's list with the pointer on
 * the library pane's side of the window (it's on the left: App.svelte),
 * the clip bar with it on the main view's side, anywhere there (the player,
 * the timeline, the transcript), not only over the bar itself. Before the
 * pointer has moved, where the focus is.
 */
function selectingIn(): "clips" | "library" | null {
  const pane = document.querySelector(".library-pane");
  const side =
    pointerX !== null && pane
      ? pointerX < pane.getBoundingClientRect().right
        ? "library"
        : "clips"
      : document.activeElement?.closest(".library-list")
        ? "library"
        : document.activeElement?.closest(".clip-bar")
          ? "clips"
          : null;
  // No clip bar shown (the library's page), nothing to select there.
  return side === "clips" && !document.querySelector(".clip-bar") ? null : side;
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
    if (!snippets.size && !library.size) return false;
    snippets.clear();
    library.clear();
    return true;
  }
  if (is(e, "select.all")) {
    const into = selectingIn();
    if (into === "clips") snippets.set(clips.shown.map((c) => ({ kind: "clip", id: c.id })));
    else if (into === "library")
      library.set(
        rows.map((r) => (r.kind === "vod" ? { kind: "vod", id: r.vod.id } : { kind: "edit", id: r.edit.id })),
      );
    // The key is the app's even with nothing to pick (the main side with no
    // clip bar): the web view's own select-all would light up the page's text.
    return true;
  }
  if (library.size && is(e, "library.remove")) {
    if (!e.repeat) void removeChosenEntries(rows, onremove);
    return true;
  }
  return false;
}
