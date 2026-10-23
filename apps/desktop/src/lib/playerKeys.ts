// The keys a VOD's player and an edit's share (play, seek, step a frame,
// full screen, a selection's in and out points; the selected mark's are
// cuts.svelte.ts `markKey`), and the shortcuts each lists in its legend
// (Shortcuts.svelte).

/** Seconds the arrows and J/L move the playhead (Shift with an arrow: 1 s). */
export const STEPS: Record<string, number> = { ArrowLeft: -5, ArrowRight: 5, j: -10, l: 10 };

/** How far `key` moves the playhead, with Shift held or not; undefined for other keys. */
export function stepOf(key: string, shift: boolean): number | undefined {
  if (!(key in STEPS)) return undefined;
  return shift && key.startsWith("Arrow") ? Math.sign(STEPS[key]) : STEPS[key];
}

/** `e.key`, lower-cased if it's a letter or sign, so Shift or Caps Lock don't change it. */
export const keyOf = (e: KeyboardEvent) => (e.key.length === 1 ? e.key.toLowerCase() : e.key);

/** `e`'s key for a player (as `keyOf` gives it), or null if it isn't for
 *  one: with Alt (the app's, back and forward), or typed into a field or a
 *  dialog. */
export function playerKey(e: KeyboardEvent): string | null {
  const target = e.target as HTMLElement | null;
  if (e.altKey || target?.closest?.("input, textarea, select, [contenteditable], [role=dialog]")) return null;
  return keyOf(e);
}

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

/** Does what `key` means for any player; false if it means nothing here. */
export function transportKey(key: string, shift: boolean, t: Transport): boolean {
  if (key === "f" || (key === "Escape" && t.fullscreen)) t.setFullscreen(key === "f" ? !t.fullscreen : false);
  else if (key === " " || key === "k") t.togglePlay();
  else if ((key === "," || key === ".") && t.step) t.step(key === "," ? -1 : 1);
  else if (stepOf(key, shift) !== undefined) t.seekBy(stepOf(key, shift)!);
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

const undo = (mac: boolean): Shortcut =>
  mac
    ? { keys: ["⌘", "Z", "or", "⇧", "⌘", "Z"], does: "undo or redo" }
    : { keys: ["Ctrl", "+", "Z", "or", "Ctrl", "+", "Shift", "+", "Z"], does: "undo or redo" };

const playing = (framed: boolean): Shortcut[] => [
  { keys: ["Space", "or", "K"], does: "play/pause" },
  { keys: ["←", "→"], does: "5 s" },
  { keys: ["Shift", "+", "←", "→"], does: "1 s" },
  { keys: ["J", "L"], does: "10 s" },
  ...(framed ? [{ keys: [",", "."], does: "a frame back or on" }] : []),
];

/** A VOD's player: `framed` if it can step frames; on a Mac or not. */
export const vodShortcuts = (framed: boolean, mac: boolean): Shortcut[] => [
  ...playing(framed),
  { keys: ["B"], does: "cut here" },
  { keys: ["I", "O"], does: "start or end the selection here" },
  { keys: ["C"], does: "clip the stretch" },
  { keys: ["T"], does: "transcribe the selection, or from here" },
  { keys: ["M"], does: "highlight here" },
  { keys: ["N"], does: "new chapter here" },
  { keys: ["Enter"], does: "rename the selected chapter or highlight" },
  { keys: [",", ".", "or", "←", "→", "or", "J", "L"], does: "move the selected cut (Shift with , . ten frames)" },
  { keys: ["Delete"], does: "remove the selected cut" },
  { keys: ["Esc"], does: "deselect, or take back a new chapter or highlight" },
  { keys: ["Shift", "+", "C"], does: "captions" },
  { keys: ["Shift", "+", "M"], does: "mute" },
  { keys: ["F"], does: "full screen" },
  undo(mac),
  { keys: [mac ? "Option" : "Alt", "+", "drag"], does: "on the timeline, or drag Clip: the stretch out as a clip" },
];

/** An edit's player. */
export const editShortcuts = (framed: boolean, mac: boolean): Shortcut[] => [
  ...playing(framed),
  { keys: ["B"], does: "cut here (or at both ends of the selection)" },
  { keys: ["I", "O"], does: "start or end the selection here" },
  { keys: ["M"], does: "highlight here" },
  { keys: ["N"], does: "new chapter here" },
  { keys: ["Enter"], does: "rename the selected chapter or highlight" },
  {
    keys: [",", ".", "or", "←", "→", "or", "J", "L"],
    does: "move the selected cut or mark (Shift with , . ten frames)",
  },
  {
    keys: ["Delete"],
    does: "remove the selected lines or mark; else the stretch between cuts here (a real split then), or the piece",
  },
  { keys: [mac ? "⌘" : "Ctrl", "+", "←", "→"], does: "move the piece here earlier or later" },
  { keys: ["C"], does: "clip the selection, or the stretch here" },
  { keys: ["E"], does: "export: render the edit" },
  { keys: ["Shift", "+", "C"], does: "captions" },
  { keys: ["Shift", "+", "M"], does: "mute" },
  { keys: ["F"], does: "full screen" },
  undo(mac),
];
