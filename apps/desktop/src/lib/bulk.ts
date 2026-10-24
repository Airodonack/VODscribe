// What's done with a set selected (selection.svelte.ts) all at once, and
// the keys for it: C clips each stretch selected (the players call
// `clipSegments`), Delete removes the clips selected (or, in an edit, its
// pieces too: EditView.svelte) and, in the library, the VODs and edits
// selected, after one question. Each is one change, so undo takes it back
// whole. Escape lets go of a set, Ctrl+A (⌘A) selects all of the clip bar
// or the library's list, whichever the pointer or the focus is in.

import { api, type Doc, type Edit, type Vod } from "./api";
import { ask, choose } from "./ask";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { exportStretches } from "./dragout";
import { edits } from "./edits.svelte";
import { is } from "./keys.svelte";
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
    const list = using.length === 1 ? using[0] : `${using.slice(0, -1).join(", ")} and ${using.at(-1)}`;
    const answer = await choose(
      `Remove ${what}?${vodText}${editText} ${theirs} pieces in ${using.length === 1 ? "the edit" : "the edits"} ${list} stay: removed from the library, the video stays on this computer for them; deleted, only the stretches they use are kept.`,
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

/** Where a press of Ctrl+A is: the clip bar or the library's list (the pointer's, or else the focus's). */
function selectingIn(): "clips" | "library" | null {
  const where = (el: Element | null) =>
    el?.closest(".clip-bar") ? ("clips" as const) : el?.closest(".library-list") ? ("library" as const) : null;
  return where(document.querySelector(".clip-bar:hover, .library-list:hover")) ?? where(document.activeElement);
}

/**
 * The keys for a set selected, asked first (App.svelte `onWindowKey`, before
 * either player): true if `e` was one, done. `rows` is the library's list;
 * `onremove` is told of each VOD removed. A VOD's or an edit's own Delete
 * and C for the snippet set are theirs (Player.svelte, EditView.svelte).
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
    return into !== null;
  }
  if (library.size && is(e, "library.remove")) {
    if (!e.repeat) void removeChosenEntries(rows, onremove);
    return true;
  }
  return false;
}
