// Ctrl+C, Ctrl+X and Ctrl+V (⌘ on a Mac), from the window's keys
// (App.svelte `onWindowKey`, before either player), the app's own copies
// (src-tauri clipboard.rs): what's copied is the selection (I and O, lines
// picked, Ctrl+A), or else what's picked on the timeline and the clip bar,
// or else the library's rows picked, as it is at that moment; a paste makes
// a copy of that, however the original has changed since, or if it's gone.
// It goes where Ctrl+A would select (where.ts): over the clip bar, a clip
// of each thing copied on it; elsewhere on the main side, an edit's
// timeline at the playhead (a selection there replaced), or a VOD's clip
// bar (a VOD's timeline takes no pieces). The system's clipboard gets a
// line of text for each (clipboardText.ts), and a paste goes ahead only
// while it still holds that: something copied since, in any app, is what's
// meant.
//
// In a text field the keys are the field's, and with text of the page
// selected (a chat message) Ctrl+C is the web view's own copy. On the
// website (the demo) they're the browser's.

import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { tick } from "svelte";
import { api, type Doc, type Put, type Vod } from "./api";
import { chosenClips, chosenEntries, chosenSegments, removeChosenClips, whereKeys } from "./bulk";
import { copiedSaid, copiedText, pastedSaid } from "./clipboardText";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { DEMO } from "./demo";
import { edits } from "./edits.svelte";
import { bindingsOf, is } from "./keys.svelte";
import type { ActionId } from "./keymap";
import { count, library, snippets } from "./selection.svelte";
import { toast } from "./toasts.svelte";
import { undoIn } from "./undo";

/** What's said when Ctrl+C has nothing to copy. */
const NOTHING = "Select a stretch (I and O) or clips to copy";

/** Ctrl+C's, Ctrl+X's and Ctrl+V's bindings for a legend: none on the
 *  website, where the keys are the browser's. */
export const legendBindings = (id: ActionId): readonly string[] =>
  DEMO && (id === "edit.copy" || id === "edit.cut" || id === "edit.paste") ? [] : bindingsOf(id);

/** Whether some of the page's own text is selected (a chat message, an error's words). */
const textSelected = () => {
  const picked = window.getSelection();
  return !!picked && !picked.isCollapsed && picked.toString().trim() !== "";
};

/**
 * Ctrl+C, Ctrl+X or Ctrl+V, outside a text field: true if `e` was one and
 * the app's (its default is then to be prevented, so the web view's own
 * copy or paste does nothing more). `vodOf` finds the library's VODs, for
 * the text copied. A key held down is one press.
 */
export function clipboardKey(e: KeyboardEvent, vodOf: (id: number) => Vod | undefined): boolean {
  if (DEMO) return false;
  const paste = is(e, "edit.paste");
  const cut = is(e, "edit.cut");
  if (!paste && !cut && !is(e, "edit.copy")) return false;
  if ((e.target as HTMLElement | null)?.closest?.("[role=dialog]")) return false;
  if (paste) {
    if (!e.repeat) void pasteHere();
    return true;
  }
  // The page's text selected: the web view's copy. A mark selected: nothing of the app's to copy.
  if (textSelected() || cuts.selected !== null) return false;
  if (!e.repeat) void copyPicked(cut, vodOf);
  return true;
}

/** What Ctrl+C takes, as an edit would: the selection, else what's picked
 *  on the timeline and the clip bar (in the order picked), else the
 *  library's rows picked; [] if nothing is. */
function copied(): Put[] {
  const doc = cuts.doc;
  const s = cuts.selection;
  if (s && doc)
    return [
      doc.kind === "vod"
        ? { kind: "range", vodId: doc.id, start: s.start, end: s.end }
        : { kind: "span", clip: doc.id, from: s.start, to: s.end },
    ];
  const bar = clips.doc;
  if (snippets.size && bar)
    return snippets.items.flatMap((i): Put[] => {
      if (i.kind === "clip") return clips.shown.some((c) => c.id === i.id) ? [{ kind: "clip", id: i.id }] : [];
      return [
        bar.kind === "vod"
          ? { kind: "range", vodId: bar.id, start: i.start, end: i.end }
          : { kind: "span", clip: bar.id, from: i.start, to: i.end },
      ];
    });
  return library.picking ? chosenEntries() : [];
}

/** Copies what's selected or picked (`copied`), writes its text to the
 *  system's clipboard and says so; with `cut`, then takes it out as Delete
 *  would: clips picked off their bar, and in an edit its stretches too. */
async function copyPicked(cut: boolean, vodOf: (id: number) => Vod | undefined) {
  const items = copied();
  if (!items.length) return void toast.show(NOTHING);
  // What's picked now, to take out once it's copied (the set may change meanwhile).
  const doc = clips.doc;
  const picked = { segments: chosenSegments(), clips: chosenClips(), selection: cuts.selection };
  try {
    const said = await api.copyItems(items);
    const text = copiedText(said, vodOf);
    await writeText(text);
    await api.copiedText(text);
    if (!cut) return void toast.show(copiedSaid(said));
    await cutOut(doc, picked, copiedSaid(said));
  } catch (e) {
    toast.error(`Couldn't copy: ${e}`);
  }
}

/** Ctrl+X's taking out, once copied (`said`): the clips picked go, as
 *  Delete takes them; in an edit (`doc`, the open one), the stretches
 *  picked or the selection too. A VOD's stretches stay: they're splits. */
async function cutOut(
  doc: Doc | null,
  picked: {
    segments: { start: number; end: number }[];
    clips: number[];
    selection: { start: number; end: number } | null;
  },
  said: string,
) {
  if (doc?.kind === "edit" && edits.openId === doc.id) {
    const stretches: [number, number][] = picked.selection
      ? [[picked.selection.start, picked.selection.end]]
      : picked.segments.map((s) => [s.start, s.end]);
    if (!stretches.length && !picked.clips.length) return void toast.show(said);
    snippets.clear();
    cuts.clearSelection();
    const what = count([
      [stretches.length, "stretch", "stretches"],
      [picked.clips.length, "clip", "clips"],
    ]);
    if (await edits.removeFrom([], stretches, picked.clips))
      toast.show(`Cut ${what}`, { label: "Undo", run: () => undoIn(doc) });
    return;
  }
  if (picked.clips.length) return void (await removeChosenClips());
  toast.show(`${said} (a VOD's stretch stays)`);
}

/** Ctrl+V: what was copied last, pasted where Ctrl+A would select
 *  (`whereKeys`); nothing at all if the system's clipboard holds something
 *  else since, or nothing was copied. */
async function pasteHere() {
  const into = whereKeys();
  const doc = cuts.doc;
  if (!into || into === "library" || !doc) return;
  let text: string;
  try {
    text = await readText();
  } catch {
    // Nothing there as text (an image copied elsewhere, or nothing): not ours.
    return;
  }
  // A clip open in the editor has no bar of its own: its timeline. Over the
  // clip bar, the bar's; a VOD's timeline takes no pieces: its bar.
  const snippet = doc.kind === "edit" && edits.open?.id === doc.id && edits.open.snippet_of !== null;
  const onBar = !snippet && (into === "clips" || doc.kind === "vod");
  const target = onBar ? clips.doc : doc;
  if (!target) return;
  const replace = !onBar && cuts.isOf(target) ? cuts.selection : null;
  try {
    const since = clips.news;
    const pasted = await api.pasteItems(
      target,
      onBar,
      onBar ? null : cuts.now,
      replace ? [replace.start, replace.end] : null,
      text,
    );
    if (!pasted) return;
    if (pasted.view) edits.took(pasted.view);
    if (onBar) {
      clips.answered(pasted.clips, since);
      snippets.set(pasted.clips.map((c) => ({ kind: "clip", id: c.id })));
      await tick();
      document
        .querySelector(`.clip-bar .chip[data-clip="${pasted.clips[0]?.id}"]`)
        ?.scrollIntoView({ block: "nearest", inline: "nearest" });
    } else if (pasted.span && cuts.isOf(target)) {
      // Selected, the playhead at its end: the next paste follows it, as a caret.
      const [start, end] = pasted.span;
      cuts.selection = { start, end };
      edits.wanted = { time: end, play: false };
    }
    toast.show(pastedSaid({ ...pasted, clips: pasted.clips.length }), { label: "Undo", run: () => undoIn(target) });
  } catch (e) {
    toast.error(String(e));
  }
}
