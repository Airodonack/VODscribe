// The app's keyboard controls, in one table: each action (what a key does),
// where it can fire, and the keys it has by default on each OS. Bindings are
// written as text, "Ctrl+Shift+ArrowLeft", "Meta+[", "Shift+<", " " for
// Space: the modifiers in the order Ctrl, Alt, Shift, Meta, then the key as
// `e.key` names it, a letter or sign lower-cased (`keyName`, so Shift or Caps
// Lock don't change it). A press matches a binding when its key and all four
// modifiers are the same, so "b" and "Ctrl+b" are different bindings. The key is `e.key`, not `e.code`: what the user pressed
// is what the binding shows, on any layout; the price is that a binding
// moves with the layout (Shift+, is "<" on a US keyboard, ";" on a German
// one). The live map, with the user's own bindings over these defaults, is
// keys.svelte.ts. Pure: node --test apps/desktop/src/lib/keymap.test.mjs.
//
// Not in the table, the same everywhere: Escape (cancels, deselects, leaves
// full screen, closes), Enter and the other keys in a text field, Tab, and
// the mouse (Alt+drag a clip out, the mouse's back and forward buttons).
// They're how a user gets out of any state, or the system's own.
//
// The defaults are the keys the app has always meant, one binding for each,
// so the Controls page shows them as they are. The handlers before the table
// tested more loosely and let some presses through by accident: B or Z with
// Super held as well, Shift+Space, Shift+F, Shift+J, Shift+N on a VOD, Shift
// with Enter or Delete, Ctrl+Super+Z and the like. Those now do nothing;
// keymap.test.mjs walks every old press and lists each such one. So does
// "<" or ">" pressed without Shift (a key of its own on European keyboards),
// which used to move a selected mark a frame.
//
// Shift with , and . is written as US keyboards give it, "Shift+<" and
// "Shift+>", which is what a press there records. Other layouts give
// other keys (German Shift+, is ";"; Russian "," itself needs Shift), so
// someone there rebinds them.

export type Os = "linux" | "mac" | "windows";

/**
 * Where an action can fire, in the order the handlers ask: `app` anywhere,
 * `player` in either player (a VOD's or an edit's), `edit` in an edit's
 * only, `mark` while a mark or an end of the selection is selected (the
 * seek keys then move it rather than the playhead).
 */
export type Scope = "app" | "player" | "edit" | "mark";

export type Action = {
  id: string;
  label: string;
  scope: Scope;
  /** Bindings, the first the one shown; Windows has Linux's unless it says. */
  defaults: { linux: string[]; mac: string[]; windows?: string[] };
};

const MODS = ["Ctrl", "Alt", "Shift", "Meta"] as const;
type Mod = (typeof MODS)[number];

/** A binding taken apart. */
export type Chord = { mods: Set<Mod>; key: string };

/** What a key press says about itself: a KeyboardEvent, or in tests one made up. */
export type KeyLike = Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

/** `e.key`s that are a modifier on its own: never a binding's key. */
const LONE = new Set(["Control", "Shift", "Alt", "AltGraph", "Meta", "OS"]);

/** A key as bindings write it: a letter or sign lower-cased. */
const keyName = (key: string) => (key.length === 1 ? key.toLowerCase() : key);

/** `binding` taken apart, or null if it isn't one. */
export function parse(binding: string): Chord | null {
  const mods = new Set<Mod>();
  let rest = binding;
  for (;;) {
    const mod = MODS.find((m) => rest.startsWith(m + "+") && rest.length > m.length + 1);
    if (!mod) break;
    if (mods.has(mod)) return null;
    mods.add(mod);
    rest = rest.slice(mod.length + 1);
  }
  // One character (a letter, a sign, " "), or a named key: Enter, ArrowLeft, F2.
  if (LONE.has(rest) || MODS.some((m) => m === rest) || !(rest.length === 1 || /^[A-Z][A-Za-z0-9]+$/.test(rest)))
    return null;
  return { mods, key: keyName(rest) };
}

/** `chord` written as a binding, its modifiers in their order. */
export function format(chord: Chord): string {
  return [...MODS.filter((m) => chord.mods.has(m)), chord.key].join("+");
}

/** `binding` written the one way it's matched, or null if it isn't one. */
export function canonical(binding: string): string | null {
  const chord = parse(binding);
  return chord && format(chord);
}

/** The binding a key press is, or null for a modifier pressed on its own. */
export function bindingOf(e: KeyLike): string | null {
  if (LONE.has(e.key) || e.key === "") return null;
  const flags = [e.ctrlKey, e.altKey, e.shiftKey, e.metaKey];
  return format({ mods: new Set(MODS.filter((_, i) => flags[i])), key: keyName(e.key) });
}

/**
 * Whether `e` types into a text field: a character, with neither Ctrl nor ⌘
 * held. An action of the app's that's given such a key (Search on "/")
 * leaves it to the field the user is typing in, as the field's own keys are.
 */
export const typesText = (e: KeyLike) => e.key.length === 1 && !e.ctrlKey && !e.metaKey;

/** The same keys on every OS. */
const all = (...bindings: string[]) => ({ linux: bindings, mac: bindings });

export const ACTIONS = [
  // Anywhere.
  { id: "undo", label: "Undo", scope: "app", defaults: { linux: ["Ctrl+z"], mac: ["Meta+z"] } },
  { id: "redo", label: "Redo", scope: "app", defaults: { linux: ["Ctrl+Shift+z", "Ctrl+y"], mac: ["Shift+Meta+z"] } },
  { id: "nav.back", label: "Back", scope: "app", defaults: { linux: ["Alt+ArrowLeft"], mac: ["Meta+["] } },
  { id: "nav.forward", label: "Forward", scope: "app", defaults: { linux: ["Alt+ArrowRight"], mac: ["Meta+]"] } },
  { id: "search.focus", label: "Search", scope: "app", defaults: { linux: ["Ctrl+f"], mac: ["Meta+f"] } },
  // Where the pointer or the focus is: the clip bar or the library's list.
  {
    id: "select.all",
    label: "Select all the clips, or all the library",
    scope: "app",
    defaults: { linux: ["Ctrl+a"], mac: ["Meta+a"] },
  },
  {
    id: "library.remove",
    label: "Remove the selected VODs and edits",
    scope: "app",
    defaults: all("Delete", "Backspace"),
  },
  // Either player.
  { id: "play.toggle", label: "Play or pause", scope: "player", defaults: all(" ", "k") },
  { id: "seek.back5", label: "Back 5 s", scope: "player", defaults: all("ArrowLeft") },
  { id: "seek.forward5", label: "Forward 5 s", scope: "player", defaults: all("ArrowRight") },
  { id: "seek.back1", label: "Back 1 s", scope: "player", defaults: all("Shift+ArrowLeft") },
  { id: "seek.forward1", label: "Forward 1 s", scope: "player", defaults: all("Shift+ArrowRight") },
  { id: "seek.back10", label: "Back 10 s", scope: "player", defaults: all("j") },
  { id: "seek.forward10", label: "Forward 10 s", scope: "player", defaults: all("l") },
  { id: "frame.back", label: "A frame back", scope: "player", defaults: all(",") },
  { id: "frame.forward", label: "A frame on", scope: "player", defaults: all(".") },
  // B cuts with Ctrl (⌘) held too, on purpose: as in video editors.
  { id: "cut.here", label: "Cut here", scope: "player", defaults: { linux: ["b", "Ctrl+b"], mac: ["b", "Meta+b"] } },
  { id: "selection.in", label: "Start the selection here", scope: "player", defaults: all("i") },
  { id: "selection.out", label: "End the selection here", scope: "player", defaults: all("o") },
  { id: "clip.make", label: "Clip the stretch, or each one selected", scope: "player", defaults: all("c") },
  { id: "clips.remove", label: "Remove the selected clips", scope: "player", defaults: all("Delete", "Backspace") },
  {
    id: "transcribe.here",
    label: "Transcribe the selection, or the whole VOD",
    scope: "player",
    defaults: all("t"),
  },
  { id: "mark.highlight", label: "Highlight here", scope: "player", defaults: all("m") },
  { id: "mark.chapter", label: "New chapter here", scope: "player", defaults: all("n") },
  { id: "captions.toggle", label: "Captions", scope: "player", defaults: all("Shift+c") },
  { id: "mute.toggle", label: "Mute", scope: "player", defaults: all("Shift+m") },
  { id: "fullscreen.toggle", label: "Full screen", scope: "player", defaults: all("f") },
  // An edit's player only.
  { id: "edit.render", label: "Export: render the edit", scope: "edit", defaults: all("e") },
  {
    id: "edit.pieceEarlier",
    label: "Move the piece here earlier",
    scope: "edit",
    defaults: { linux: ["Ctrl+ArrowLeft"], mac: ["Meta+ArrowLeft"] },
  },
  {
    id: "edit.pieceLater",
    label: "Move the piece here later",
    scope: "edit",
    defaults: { linux: ["Ctrl+ArrowRight"], mac: ["Meta+ArrowRight"] },
  },
  {
    id: "edit.remove",
    label: "Remove the selected lines, pieces and clips, the stretch here or the piece",
    scope: "edit",
    defaults: all("Delete", "Backspace"),
  },
  // A selected mark (or end of the selection).
  { id: "mark.rename", label: "Rename the selected chapter or highlight", scope: "mark", defaults: all("Enter") },
  { id: "mark.remove", label: "Remove the selected mark", scope: "mark", defaults: all("Delete", "Backspace") },
  { id: "frame.back10", label: "Ten frames back", scope: "mark", defaults: all("Shift+<") },
  { id: "frame.forward10", label: "Ten frames on", scope: "mark", defaults: all("Shift+>") },
] as const satisfies readonly Action[];

export type ActionId = (typeof ACTIONS)[number]["id"];

/** Whether a key press fires an action (keys.svelte.ts `is`, or in tests a map of their own). */
export type Is = (e: KeyLike, id: ActionId) => boolean;

/** `action`'s default bindings on `os`. */
export const defaultsOf = (action: Action, os: Os): readonly string[] =>
  (os === "windows" ? action.defaults.windows : undefined) ??
  (os === "mac" ? action.defaults.mac : action.defaults.linux);

/**
 * Every action's bindings on `os`: its defaults, or the user's own list where
 * `overrides` has one (an empty list: no key). Bindings that aren't are
 * dropped; ids the table doesn't have are left alone.
 */
export function buildMap(os: Os, overrides: Record<string, readonly string[]>): Map<ActionId, string[]> {
  const map = new Map<ActionId, string[]>();
  for (const action of ACTIONS) {
    const list = Object.hasOwn(overrides, action.id) ? overrides[action.id] : defaultsOf(action, os);
    const bindings = list.map(canonical).filter((b): b is string => b !== null);
    map.set(action.id, [...new Set(bindings)]);
  }
  return map;
}

/** Whether `e` is one of `bindings`. */
export function matches(e: KeyLike, bindings: readonly string[]): boolean {
  const pressed = bindingOf(e);
  return pressed !== null && bindings.includes(pressed);
}

const MAC_MODS: Record<Mod, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Meta: "⌘" };
const metaName = (os: Os) => (os === "mac" ? "⌘" : os === "windows" ? "Win" : "Super");
const KEY_NAMES: Record<string, string> = {
  " ": "Space",
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Escape: "Esc",
};

/** A key as the user reads it: "Space", "←", "Z". */
const keyLabel = (key: string) => KEY_NAMES[key] ?? (key.length === 1 ? key.toUpperCase() : key);

/** A chord's modifiers as key caps, in their order: "⇧", "⌘" on a Mac, else "Ctrl", "Shift". */
const modLabels = (chord: Chord, os: Os) =>
  MODS.filter((m) => chord.mods.has(m)).map((m) => (os === "mac" ? MAC_MODS[m] : m === "Meta" ? metaName(os) : m));

/** `binding` as the user reads it: "⇧⌘Z" on a Mac, "Ctrl+Shift+Z" elsewhere. */
export function label(binding: string, os: Os): string {
  const chord = parse(binding);
  if (!chord) return binding;
  const parts = [...modLabels(chord, os), keyLabel(chord.key)];
  return parts.join(os === "mac" ? "" : "+");
}

/**
 * `bindings` as a legend's key caps (Shortcuts.svelte): each modifier and key
 * a cap, "+" between them except on a Mac, which writes ⇧⌘Z. Bindings that
 * share their modifiers are written once with the keys side by side,
 * "Shift + ← →"; others are joined with "or".
 */
export function caps(bindings: readonly string[], os: Os): string[] {
  const chords = bindings.map(parse).filter((c): c is Chord => c !== null);
  const out: string[] = [];
  for (let i = 0; i < chords.length;) {
    const mods = modLabels(chords[i], os);
    let j = i + 1;
    while (j < chords.length && modLabels(chords[j], os).join() === mods.join()) j++;
    if (out.length) out.push("or");
    for (const m of mods) out.push(m, ...(os === "mac" ? [] : ["+"]));
    for (const c of chords.slice(i, j)) out.push(keyLabel(c.key));
    i = j;
  }
  return out;
}

/** `bindings` less those that are another of them with more modifiers held
 *  (Shift+K beside K): the ones worth showing in a legend. */
export function plainest(bindings: readonly string[]): string[] {
  const chords = bindings.map((b) => [b, parse(b)] as const).filter((x): x is [string, Chord] => x[1] !== null);
  return chords
    .filter(
      ([, c]) =>
        !chords.some(
          ([, d]) => d !== c && d.key === c.key && d.mods.size < c.mods.size && [...d.mods].every((m) => c.mods.has(m)),
        ),
    )
    .map(([b]) => b);
}

/** Whether actions in scopes `a` and `b` can both be live for one key press. */
function together(a: Scope, b: Scope): boolean {
  if (a === b || a === "app" || b === "app") return true;
  const pair = [a, b].sort().join(" ");
  return pair === "edit player" || pair === "mark player" || pair === "edit mark";
}

/**
 * The actions other than `id` that `binding` would be taken from: those live
 * at the same time with the binding in `map`. Keys two actions have by
 * default are shared on purpose (Delete removes the selected mark, or else
 * the piece; the handlers' order decides), so they don't count. All of them,
 * not just the first: Delete is four actions' by default, and a key given to
 * another action must leave each of them (Settings › Controls' Replace).
 * Empty if none.
 */
export function conflicts(map: Map<ActionId, readonly string[]>, binding: string, id: ActionId, os: Os): Action[] {
  const b = canonical(binding);
  const mine = ACTIONS.find((a) => a.id === id);
  if (b === null || !mine) return [];
  const byDefault = (a: Action) => defaultsOf(a, os).some((d) => canonical(d) === b);
  return ACTIONS.filter(
    (other) =>
      other.id !== id &&
      together(mine.scope, other.scope) &&
      !!map.get(other.id)?.includes(b) &&
      !(byDefault(mine) && byDefault(other)),
  );
}
