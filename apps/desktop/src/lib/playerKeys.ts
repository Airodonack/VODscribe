// The keys a VOD's player and an edit's share (play, seek, step a frame,
// full screen, a selection's in and out points; the selected mark's are
// cuts.svelte.ts `markKey`), and the shortcuts each lists in its legend
// (Shortcuts.svelte). Which key does what is the action table's
// (keymap.ts); the live map is passed in as `is` and `bindings`.

import { caps, plainest, type ActionId, type Is, type KeyLike, type Os } from "./keymap";

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

/** `e.key`, lower-cased if it's a letter or sign, so Shift or Caps Lock don't change it. */
export const keyOf = (e: KeyboardEvent) => (e.key.length === 1 ? e.key.toLowerCase() : e.key);

/** `e`'s key for a player (as `keyOf` gives it), or null if it isn't for
 *  one: taken already (the app's keys, asked first in App.svelte
 *  `onWindowKey`, as Alt+← for back; a row's Alt+↑), or typed into a field
 *  or a dialog, or while a modal window (Settings) is open, whatever has the
 *  focus: a press on its backdrop that isn't a click leaves the focus on the
 *  page body. Alt held is no reason of its own: a player's action can be
 *  given an Alt key in Settings. */
export function playerKey(e: KeyboardEvent): string | null {
  const target = e.target as HTMLElement | null;
  if (e.defaultPrevented || target?.closest?.("input, textarea, select, [contenteditable], [role=dialog]")) return null;
  if (typeof document !== "undefined" && document.querySelector('[aria-modal="true"]')) return null;
  return keyOf(e);
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
 *  written between them) and what they do. */
export type Shortcut = { keys: string[]; does: string };

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

function playing(framed: boolean, os: Os, bindings: Bindings): Shortcut[] {
  const { first, line } = legend(os, bindings);
  return [
    // Space or K: each of play's keys, less Shift+Space and the like.
    ...line("play/pause", ...plainest(bindings("play.toggle")).map((b) => [b])),
    ...line("5 s", first("seek.back5", "seek.forward5")),
    ...line("1 s", first("seek.back1", "seek.forward1")),
    ...line("10 s", first("seek.back10", "seek.forward10")),
    ...(framed ? line("a frame back or on", first("frame.back", "frame.forward")) : []),
  ];
}

/** The selected cut's or mark's moves: a frame, or a seek key's distance. */
const moves = (first: (...ids: ActionId[]) => string[]) => [
  first("frame.back", "frame.forward"),
  first("seek.back5", "seek.forward5"),
  first("seek.back10", "seek.forward10"),
];

/** A VOD's player: `framed` if it can step frames; this OS's wording. */
export function vodShortcuts(framed: boolean, os: Os, bindings: Bindings): Shortcut[] {
  const { first, line } = legend(os, bindings);
  return [
    ...playing(framed, os, bindings),
    ...line("cut here", first("cut.here")),
    ...line("start or end the selection here", first("selection.in", "selection.out")),
    ...line("clip the stretch", first("clip.make")),
    ...line("transcribe the selection, or from here", first("transcribe.here")),
    ...line("highlight here", first("mark.highlight")),
    ...line("new chapter here", first("mark.chapter")),
    ...line("rename the selected chapter or highlight", first("mark.rename")),
    ...line("move the selected cut (Shift with , . ten frames)", ...moves(first)),
    ...line("remove the selected cut", first("mark.remove")),
    { keys: ["Esc"], does: "deselect, or take back a new chapter or highlight" },
    ...line("captions", first("captions.toggle")),
    ...line("mute", first("mute.toggle")),
    ...line("full screen", first("fullscreen.toggle")),
    ...line("undo or redo", first("undo"), first("redo")),
    {
      keys: [os === "mac" ? "Option" : "Alt", "+", "drag"],
      does: "on the timeline, or drag Clip: the stretch out as a clip",
    },
  ];
}

/** An edit's player. */
export function editShortcuts(framed: boolean, os: Os, bindings: Bindings): Shortcut[] {
  const { first, line } = legend(os, bindings);
  return [
    ...playing(framed, os, bindings),
    ...line("cut here (or at both ends of the selection)", first("cut.here")),
    ...line("start or end the selection here", first("selection.in", "selection.out")),
    ...line("highlight here", first("mark.highlight")),
    ...line("new chapter here", first("mark.chapter")),
    ...line("rename the selected chapter or highlight", first("mark.rename")),
    ...line("move the selected cut or mark (Shift with , . ten frames)", ...moves(first)),
    ...line(
      "remove the selected lines or mark; else the stretch between cuts here (a real split then), or the piece",
      first("mark.remove"),
      first("edit.remove"),
    ),
    ...line("move the piece here earlier or later", first("edit.pieceEarlier", "edit.pieceLater")),
    ...line("clip the selection, or the stretch here", first("clip.make")),
    ...line("export: render the edit", first("edit.render")),
    ...line("captions", first("captions.toggle")),
    ...line("mute", first("mute.toggle")),
    ...line("full screen", first("fullscreen.toggle")),
    ...line("undo or redo", first("undo"), first("redo")),
  ];
}
