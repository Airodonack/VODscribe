// Ctrl+C, Ctrl+X and Ctrl+V (⌘ on a Mac), the app's own copies (src-tauri
// clipboard.rs): Ctrl+C and Ctrl+X from the window's keys (App.svelte
// `onWindowKey`, before either player), Ctrl+V from the paste the web view
// makes of it (`clipboardPaste`, the page's `paste` event). What's copied
// is the selection (I and O, lines picked, Ctrl+A), or else what's picked
// on the timeline and the clip bar, or else the library's rows picked, as
// it is at that moment; a paste makes a copy of that, however the original
// has changed since, or if it's gone. It goes where Ctrl+A would select
// (where.ts): over the clip bar, a clip of each thing copied on it;
// elsewhere on the main side, an edit's timeline, or a VOD's clip bar (a
// VOD's timeline takes no pieces). In either, after what's selected, never
// in its place, the playhead only when nothing is (where.ts
// `insertionPoint`); what's pasted is selected then, so pasting again lines
// copies up in order. A stretch pasted on a bar where it's a clip already
// is that clip, selected, not a second one. The system's clipboard gets
// a line of text for each (clipboardText.ts), and a paste goes ahead only
// if what it brings is still that: something copied since, in any app, is
// what's meant.
//
// The clipboard is never read by the app itself, only handed over by the
// paste a person makes: macOS asks the user whether to allow an app that
// reads it on its own.
//
// In a text field the keys are the field's, and with text of the page
// selected (a chat message) Ctrl+C is the web view's own copy. On the
// website (the demo) they're the browser's.

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { tick } from "svelte";
import { api, type Doc, type Put, type Vod } from "./api";
import { chosenClips, chosenEntries, chosenSegments, removeChosenClips, whereKeys } from "./bulk";
import { copiedSaid, copiedText, pastedSaid } from "./clipboardText";
import { clips } from "./clips.svelte";
import { cuts } from "./cuts.svelte";
import { DEMO, keysForApp } from "./demo";
import { edits } from "./edits.svelte";
import { bindingsOf, is } from "./keys.svelte";
import type { ActionId } from "./keymap";
import { count, library, snippets } from "./selection.svelte";
import { toast } from "./toasts.svelte";
import { undoIn } from "./undo";
import { insertionPoint } from "./where";

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
 * copy does nothing more). `vodOf` finds the library's VODs, for the text
 * copied. A key held down is one press. Ctrl+V is left to the web view,
 * which makes a paste of it (`clipboardPaste`); a held one's repeats are
 * taken, so they paste nothing more.
 */
export function clipboardKey(e: KeyboardEvent, vodOf: (id: number) => Vod | undefined): boolean {
  if (DEMO) return false;
  const paste = is(e, "edit.paste");
  const cut = is(e, "edit.cut");
  if (!paste && !cut && !is(e, "edit.copy")) return false;
  if ((e.target as HTMLElement | null)?.closest?.("[role=dialog]")) return false;
  if (paste) return e.repeat;
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
  if (picked.clips.length) return void (await removeChosenClips(picked.clips, doc));
  toast.show(`${said} (a VOD's stretch stays)`);
}

/**
 * The page's `paste` (Ctrl+V let through by `clipboardKey`, or Edit › Paste
 * on a Mac), outside a text field: the text it brings, pasted as the app's
 * copy (`pasteHere`) if it's that copy's. What a text field or a dialog
 * takes is theirs; on the website, the browser's.
 */
export function clipboardPaste(e: ClipboardEvent) {
  if (DEMO || e.defaultPrevented || !keysForApp()) return;
  const target = e.target as HTMLElement | null;
  if (target?.closest?.("input, textarea, select, [contenteditable], [role=dialog]")) return;
  if (document.querySelector('[aria-modal="true"]')) return;
  e.preventDefault();
  // Windows's clipboard may give the lines back ended as \r\n; the copy's are \n.
  const text = e.clipboardData?.getData("text/plain").replace(/\r\n/g, "\n") ?? "";
  if (text) void pasteHere(text);
}

/** Ctrl+V of `text`: what was copied last, pasted where Ctrl+A would select
 *  (`whereKeys`); nothing at all if `text` isn't that copy's (something else
 *  copied since), or nothing was copied. */
async function pasteHere(text: string) {
  const into = whereKeys();
  const doc = cuts.doc;
  if (!into || into === "library" || !doc) return;
  // A clip open in the editor has no bar of its own: its timeline. Over the
  // clip bar, the bar's; a VOD's timeline takes no pieces: its bar.
  const snippet = doc.kind === "edit" && edits.open?.id === doc.id && edits.open.snippet_of !== null;
  const onBar = !snippet && (into === "clips" || doc.kind === "vod");
  const target = onBar ? clips.doc : doc;
  if (!target) return;
  // After what's selected (never in its place), else at the playhead, or last on a bar.
  const mark = cuts.all.find((m) => m.id === cuts.selected);
  const point = insertionPoint(
    onBar ? "bar" : "timeline",
    {
      stretch: cuts.isOf(target) ? cuts.selection : null,
      picked: snippets.items,
      mark: mark && cuts.isOf(target) ? mark.start : null,
      playhead: cuts.now,
    },
    clips.shown.map((c) => c.id),
  );
  const at = "at" in point ? point.at : null;
  const after = "after" in point ? point.after : null;
  try {
    const since = clips.news;
    const pasted = await api.pasteItems(target, onBar, at, after, text);
    if (!pasted) return;
    if (pasted.view) edits.took(pasted.view);
    if (onBar) {
      // What's pasted is selected: the clips made, and those it already was.
      const all = [...pasted.clips, ...pasted.found];
      clips.answered(all, since);
      // Made beside a clip, not last: the bar's order as it is now.
      if (after !== null && pasted.clips.length) clips.reload(target);
      snippets.set(all.map((c) => ({ kind: "clip", id: c.id })));
      await tick();
      document
        .querySelector(`.clip-bar .chip[data-clip="${all[0]?.id}"]`)
        ?.scrollIntoView({ block: "nearest", inline: "nearest" });
      if (!pasted.clips.length) return void toast.show("Already on the clip bar.");
    } else if (pasted.span && cuts.isOf(target)) {
      // Selected, the playhead at its start: the next paste goes after it.
      const [start, end] = pasted.span;
      cuts.selection = { start, end };
      edits.wanted = { time: start, play: false };
    }
    toast.show(pastedSaid({ ...pasted, clips: pasted.clips.length }), { label: "Undo", run: () => undoIn(target) });
  } catch (e) {
    toast.error(String(e));
  }
}
