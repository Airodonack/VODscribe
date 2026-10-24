// The keys a VOD's player and an edit's share (play, seek, step a frame,
// full screen, a selection's in and out points; the selected mark's are
// cuts.svelte.ts `markKey`), and the legend each shows under its controls
// (Shortcuts.svelte), in a few short columns of its own. Which key does what
// is the action table's (keymap.ts); the live map is passed in as `is` and
// `bindings`.

import {
  ACTIONS,
  LEGEND_GROUPS,
  caps,
  plainest,
  type Action,
  type ActionId,
  type Is,
  type KeyLike,
  type LegendGroup,
  type Os,
} from "./keymap";

/** Seconds each seek action moves the playhead. */
export const SEEKS = {
  "seek.back5": -5,
  "seek.forward5": 5,
  "seek.back1": -1,
  "seek.forward1": 1,
  "seek.back10": -10,
  "seek.forward10": 10,
} as const satisfies Partial<Record<ActionId, number>>;

/** How far `e` moves the playhead, in seconds; undefined if it's no seek key. */
export function seekOf(e: KeyLike, is: Is): number | undefined {
  for (const [id, seconds] of Object.entries(SEEKS)) if (is(e, id as keyof typeof SEEKS)) return seconds;
  return undefined;
}

/** The frames each frame key moves a selected mark. */
const MARK_FRAMES = [
  ["frame.back10", -10],
  ["frame.forward10", 10],
  ["frame.back", -1],
  ["frame.forward", 1],
] as const;

/** How many frames `e` moves a selected mark (cuts.svelte.ts `markKey`): a
 *  frame key's, or a seek key's distance in whole frames at `fps`;
 *  undefined if it's neither. */
export function markFramesOf(e: KeyLike, is: Is, fps: number): number | undefined {
  const frames = MARK_FRAMES.find(([id]) => is(e, id))?.[1];
  const seconds = seekOf(e, is);
  return frames ?? (seconds === undefined ? undefined : Math.round(seconds * fps));
}

/** Whether `e` is for a player at all: not taken already (the app's keys,
 *  asked first in App.svelte `onWindowKey`, as Alt+← for back; a row's
 *  Alt+↑), nor typed into a field or a dialog, nor pressed while a modal
 *  window (Settings) is open, whatever has the focus: a press on its backdrop
 *  that isn't a click leaves the focus on the page body. Alt held is no
 *  reason of its own: a player's action can be given an Alt key in Settings. */
export function forPlayer(e: KeyboardEvent): boolean {
  const target = e.target as HTMLElement | null;
  if (e.defaultPrevented || target?.closest?.("input, textarea, select, [contenteditable], [role=dialog]"))
    return false;
  return typeof document === "undefined" || !document.querySelector('[aria-modal="true"]');
}

/** Escape, which is no action of the table's: it always lets go (of full
 *  screen, a selection, a mark), Shift or not; with Ctrl, Alt or ⌘ it's the
 *  system's (Alt+Esc goes to another window). */
export const isEscape = (e: KeyLike) => e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.metaKey;

/** What the shared keys act on. */
export type Transport = {
  togglePlay(): void;
  /** Moves the playhead by seconds from where it is (or is going). */
  seekBy(seconds: number): void;
  /** Steps a frame back or on, where the player can. */
  step?: (frames: number) => void;
  fullscreen: boolean;
  setFullscreen(on: boolean): void;
};

/** Does what `e` means for any player; false if it means nothing here. */
export function transportKey(e: KeyLike, t: Transport, is: Is): boolean {
  const seconds = seekOf(e, is);
  if (is(e, "fullscreen.toggle")) t.setFullscreen(!t.fullscreen);
  else if (isEscape(e) && t.fullscreen) t.setFullscreen(false);
  else if (is(e, "play.toggle")) t.togglePlay();
  else if ((is(e, "frame.back") || is(e, "frame.forward")) && t.step) t.step(is(e, "frame.back") ? -1 : 1);
  else if (seconds !== undefined) t.seekBy(seconds);
  else return false;
  return true;
}

/** A stretch of a timeline, in seconds. */
export type Span = { start: number; end: number };

/**
 * I and O, as in video editors: the selection's in (I) or out (O) point put
 * at `at`, the playhead. Put past the other end, the two swap, so the
 * selection runs between them either way round. With no selection, I starts
 * one that runs on to the next of `stops` (the cuts; an edit's piece ends
 * too) or the end, the stretch Clip would take from here, and O one back to
 * the one before or the start. Null if it would be empty: nothing changes.
 */
export function inOut(key: "i" | "o", at: number, picked: Span | null, stops: number[], length: number): Span | null {
  const near = (a: number, b: number) => Math.abs(a - b) < 1e-6;
  let other: number;
  if (picked) other = key === "i" ? picked.end : picked.start;
  else if (key === "i") other = Math.min(length, ...stops.filter((t) => t > at && !near(t, at)));
  else other = Math.max(0, ...stops.filter((t) => t < at && !near(t, at)));
  if (near(other, at) || (!picked && (key === "i" ? at >= length : at <= 0))) return null;
  return { start: Math.min(at, other), end: Math.max(at, other) };
}

/** A line of a legend: the keys (each its own key cap; "+" and "or" are
 *  written between them) and what they do, in a few words. */
export type Shortcut = { keys: string[]; does: string };

/** A column of a legend (keymap.ts LEGEND_GROUPS): its heading and lines. */
export type ShortcutGroup = { group: LegendGroup; heading: string; shortcuts: Shortcut[] };

/** The live map's bindings for an action (keys.svelte.ts `bindingsOf`). */
export type Bindings = (id: ActionId) => readonly string[];

/**
 * The legends, written from the live map so a rebound key shows: each line
 * names its keys in groups joined with "or", a group an action's first
 * binding, or two actions' side by side ("← →"). A line whose actions have
 * no key is left out.
 */
function legend(os: Os, bindings: Bindings) {
  const first = (...ids: ActionId[]) => ids.flatMap((id) => bindings(id).slice(0, 1));
  return {
    first,
    line(does: string, ...groups: string[][]): Shortcut[] {
      const keys: string[] = [];
      const seen = new Set<string>();
      for (const group of groups) {
        const written = caps(group, os);
        if (!written.length || seen.has(written.join(" "))) continue;
        seen.add(written.join(" "));
        if (keys.length) keys.push("or");
        keys.push(...written);
      }
      return keys.length ? [{ keys, does }] : [];
    },
  };
}

/** The legend column action `id` is in (keymap.ts `legend`). */
const columnOf = (id: ActionId): LegendGroup | undefined =>
  (ACTIONS as readonly Action[]).find((a) => a.id === id)?.legend;

/**
 * A player's legend in columns (keymap.ts LEGEND_GROUPS), each line's
 * wording shorter than Settings › Controls'. A line is in the column of its
 * first action; the selected mark's moves, made with the player's keys, are
 * in Marks. A VOD's leaves out what only an edit does, an edit's what only a
 * VOD does (Transcribe; Alt+drag a clip out); B is once in each, as what it
 * does there. `framed` if the player can step frames.
 */
function shortcuts(edit: boolean, framed: boolean, os: Os, bindings: Bindings): ShortcutGroup[] {
  const { first, line } = legend(os, bindings);
  const by = (id: ActionId, does: string, ...groups: string[][]) =>
    line(does, ...(groups.length ? groups : [first(id)])).map((s) => [columnOf(id), s] as const);
  const as = (group: LegendGroup, lines: Shortcut[]) => lines.map((s) => [group, s] as const);
  const lines = [
    // Space or K: each of play's keys, less Shift+Space and the like.
    ...by("play.toggle", "play/pause", ...plainest(bindings("play.toggle")).map((b) => [b])),
    ...by("seek.back5", "5 s", first("seek.back5", "seek.forward5")),
    ...by("seek.back1", "1 s", first("seek.back1", "seek.forward1")),
    ...by("seek.back10", "10 s", first("seek.back10", "seek.forward10")),
    ...(framed ? by("frame.back", "a frame", first("frame.back", "frame.forward")) : []),
    // B in an edit also cuts at both ends of the selection.
    ...by("cut.here", edit ? "cut here, or selection ends" : "cut here"),
    ...by("selection.in", "selection start, end", first("selection.in", "selection.out")),
    ...by("clip.make", "clip the stretch"),
    ...(edit
      ? [
          ...by("edit.pieceEarlier", "move the piece", first("edit.pieceEarlier", "edit.pieceLater")),
          ...by("edit.remove", "remove the stretch or piece"),
          ...by("edit.render", "render"),
        ]
      : [
          ...by("transcribe.here", "transcribe"),
          ...as("cutting", [{ keys: [os === "mac" ? "Option" : "Alt", "+", "drag"], does: "drag a clip out" }]),
        ]),
    ...by("mark.highlight", "highlight here"),
    ...by("mark.chapter", "chapter here"),
    ...by("mark.rename", "rename"),
    ...by("mark.remove", "remove"),
    ...as("marks", line("move a frame", first("frame.back", "frame.forward"))),
    ...by("frame.back10", "move ten frames", first("frame.back10", "frame.forward10")),
    ...as(
      "marks",
      line("move 5 s or 10 s", first("seek.back5", "seek.forward5"), first("seek.back10", "seek.forward10")),
    ),
    ...by("mute.toggle", "mute"),
    ...by("captions.toggle", "captions"),
    ...by("fullscreen.toggle", "full screen"),
    ...by("undo", "undo"),
    ...by("redo", "redo"),
    // Escape lets go of whatever is selected (a mark, a stretch, clips), so
    // it's with the keys that work anywhere; Marks stays short too.
    ...as("more", [{ keys: ["Esc"], does: "deselect" }]),
  ];
  return LEGEND_GROUPS.map((g) => ({
    ...g,
    shortcuts: lines.filter(([group]) => group === g.group).map(([, s]) => s),
  })).filter((g) => g.shortcuts.length);
}

/** A VOD's player's legend: `framed` if it can step frames; this OS's wording. */
export const vodShortcuts = (framed: boolean, os: Os, bindings: Bindings) => shortcuts(false, framed, os, bindings);

/** An edit's player's legend. */
export const editShortcuts = (framed: boolean, os: Os, bindings: Bindings) => shortcuts(true, framed, os, bindings);
