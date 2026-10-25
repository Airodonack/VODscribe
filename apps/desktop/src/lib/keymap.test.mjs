// The action table and its bindings (keymap.ts), and that its defaults do
// what the handlers' own key tests did before it, press for press:
// node --test apps/desktop/src/lib/keymap.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// playerKeys.ts imports "./keymap" as the page's bundler finds it; here it
// is the .ts file it is.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(specifier, context, next) {
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith(".ts"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`),
);
const { ACTIONS, bindingOf, buildMap, canonical, caps, conflicts, label, matches, parse, plainest, typesText } =
  await import("./keymap.ts");
const { isEscape, markFramesOf, transportKey } = await import("./playerKeys.ts");

/** @typedef {import("./keymap.ts").KeyLike} Press */
/** @typedef {import("./keymap.ts").Os} Os */
/** @typedef {import("./keymap.ts").Is} Is */
/** @typedef {import("./keymap.ts").ActionId} ActionId */
/** @typedef {{ moving: boolean, fullscreen: boolean, os?: Os }} At */

/** A key press: `key` with the modifiers named in `mods` ("ctrl alt shift meta").
 *  @param {string} key @returns {Press} */
const press = (key, mods = "") => ({
  key,
  ctrlKey: mods.includes("ctrl"),
  altKey: mods.includes("alt"),
  shiftKey: mods.includes("shift"),
  metaKey: mods.includes("meta"),
});

/** `is` over the defaults on `os`, as keys.svelte.ts makes it.
 *  @param {Os} os @param {Record<string, string[]>} overrides @returns {Is} */
const isOn = (os, overrides = {}) => {
  const map = buildMap(os, overrides);
  return (e, id) => matches(e, map.get(id) ?? []);
};
/** @type {Os[]} */
const OSES = ["linux", "mac", "windows"];

test("bindings are written one way: Ctrl, Alt, Shift, Meta, then the key", () => {
  assert.equal(canonical("Shift+Ctrl+Z"), "Ctrl+Shift+z");
  assert.equal(canonical("Meta+Shift+ArrowLeft"), "Shift+Meta+ArrowLeft");
  assert.equal(canonical(" "), " ");
  assert.equal(canonical("Shift+ "), "Shift+ ");
  assert.equal(canonical("Ctrl++"), "Ctrl++");
  assert.deepEqual([...(parse("Ctrl++")?.mods ?? [])], ["Ctrl"]);
  assert.equal(parse("Ctrl++")?.key, "+");
  for (const bad of ["", "Ctrl", "Shift+Control", "Ctrl+Ctrl+z", "Ctrl+", "arrowleft"])
    assert.equal(canonical(bad), null, bad);
  for (const action of ACTIONS)
    for (const b of [...action.defaults.linux, ...action.defaults.mac])
      assert.equal(canonical(b), b, `${action.id} ${b} is written as matched`);
});

test("a press is its key, lower-cased, and exactly its modifiers", () => {
  assert.equal(bindingOf(press("K", "shift")), "Shift+k");
  assert.equal(bindingOf(press("<", "shift")), "Shift+<");
  assert.equal(bindingOf(press("z", "ctrl meta")), "Ctrl+Meta+z");
  assert.equal(bindingOf(press("Shift", "shift")), null);
  assert.equal(bindingOf(press("Control", "ctrl")), null);
  assert.ok(matches(press("B"), ["b"]), "Caps Lock doesn't matter");
  assert.ok(!matches(press("b", "ctrl"), ["b"]));
  assert.ok(!matches(press("b"), ["Ctrl+b"]));
});

test("the user's bindings replace an action's defaults, and only its", () => {
  const map = buildMap("linux", { "play.toggle": ["P", "nonsense key"], "cut.here": [], "gone.action": ["x"] });
  assert.deepEqual(map.get("play.toggle"), ["p"]);
  assert.deepEqual(map.get("cut.here"), []);
  assert.deepEqual(map.get("seek.back5"), ["ArrowLeft"]);
  // @ts-expect-error: an id the table doesn't have
  assert.ok(!map.has("gone.action"));
  const is = isOn("linux", { "play.toggle": ["p"] });
  assert.ok(is(press("p"), "play.toggle"));
  assert.ok(!is(press(" "), "play.toggle"));
});

test("each OS writes keys its own way", () => {
  assert.equal(label("Shift+Meta+z", "mac"), "⇧⌘Z");
  assert.equal(label("Meta+[", "mac"), "⌘[");
  assert.equal(label("Ctrl+Alt+f", "mac"), "⌃⌥F");
  assert.equal(label("Ctrl+Shift+z", "linux"), "Ctrl+Shift+Z");
  assert.equal(label("Alt+ArrowLeft", "windows"), "Alt+←");
  assert.equal(label("Meta+f", "linux"), "Super+F");
  assert.equal(label("Meta+f", "windows"), "Win+F");
  assert.equal(label(" ", "linux"), "Space");
  assert.deepEqual(caps(["Shift+ArrowLeft", "Shift+ArrowRight"], "linux"), ["Shift", "+", "←", "→"]);
  assert.deepEqual(caps(["Shift+ArrowLeft", "Shift+ArrowRight"], "mac"), ["⇧", "←", "→"]);
  assert.deepEqual(caps(["Ctrl+z", "Shift+z"], "linux"), ["Ctrl", "+", "Z", "or", "Shift", "+", "Z"]);
  assert.deepEqual(plainest([" ", "k", "Shift+ ", "Shift+k", "Ctrl+Shift+k"]), [" ", "k"]);
  // The first default is the one shown: the OS's own.
  /** @param {Os} os @param {ActionId} id */
  const first = (os, id) => label(buildMap(os, {}).get(id)?.[0] ?? "", os);
  assert.equal(first("mac", "search.focus"), "⌘F");
  assert.equal(first("linux", "search.focus"), "Ctrl+F");
  assert.equal(first("windows", "nav.back"), "Alt+←");
  assert.equal(first("mac", "nav.forward"), "⌘]");
  assert.equal(first("mac", "redo"), "⇧⌘Z");
  assert.equal(first("linux", "mute.toggle"), "Shift+M");
});

test("ids are unique, and the defaults clash nowhere they could both be live", () => {
  const ids = ACTIONS.map((a) => a.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const os of OSES) {
    const map = buildMap(os, {});
    for (const [id, bindings] of map)
      for (const b of bindings) assert.deepEqual(conflicts(map, b, id, os), [], `${os}: ${id} ${b}`);
  }
});

test("a key taken by an action live at the same time is named", () => {
  const map = buildMap("linux", {});
  /** @param {string} binding @param {ActionId} id @param {Map<ActionId, string[]>} m */
  const ids = (binding, id, m = map) => conflicts(m, binding, id, "linux").map((a) => a.id);
  assert.deepEqual(ids("k", "frame.back10"), ["play.toggle"]);
  assert.deepEqual(ids("Ctrl+z", "cut.here"), ["undo"]);
  assert.deepEqual(ids("j", "edit.render"), ["seek.back10"]);
  assert.deepEqual(ids("q", "play.toggle"), []);
  // Delete is the selected mark's, or else the edit's: on purpose.
  assert.deepEqual(ids("Delete", "edit.remove"), []);
  // Given away, it's free again.
  assert.deepEqual(ids("k", "frame.back10", buildMap("linux", { "play.toggle": [" "] })), []);
});

test("a key several actions have is named for each, so Replace takes it from them all", () => {
  // Delete removes four things by default; given to Play, it must leave each,
  // or Delete would go on removing as well as playing.
  const map = buildMap("linux", {});
  const others = conflicts(map, "Delete", "play.toggle", "linux").map((a) => a.id);
  assert.deepEqual(others, ["library.remove", "clips.remove", "edit.remove", "mark.remove"]);
  /** @type {Record<string, string[]>} */
  const replaced = { "play.toggle": [...(map.get("play.toggle") ?? []), "Delete"] };
  for (const id of others) replaced[id] = (map.get(/** @type {ActionId} */ (id)) ?? []).filter((b) => b !== "Delete");
  const after = buildMap("linux", replaced);
  for (const id of others) assert.ok(!after.get(/** @type {ActionId} */ (id))?.includes("Delete"), id);
  assert.deepEqual(conflicts(after, "Delete", "play.toggle", "linux"), []);
});

// What the handlers did before the action table, transcribed from them
// (Player.svelte, EditView.svelte and App.svelte `onKey`/`onWindowKey`,
// cuts.svelte.ts `markKey`, playerKeys.ts `transportKey` and `stepOf`), as
// the effect each press had. `moving`: a highlight is selected (Enter
// renames it, the frame and seek keys move it, Delete removes it).

/** @param {Press} e */
const keyOf = (e) => (e.key.length === 1 ? e.key.toLowerCase() : e.key);
/** @type {Record<string, number>} */
const STEPS = { ArrowLeft: -5, ArrowRight: 5, j: -10, l: 10 };
/** @param {string} key @param {boolean} shift */
const stepOf = (key, shift) =>
  key in STEPS ? (shift && key.startsWith("Arrow") ? Math.sign(STEPS[key]) : STEPS[key]) : undefined;
const FPS = 30;

/** @param {string} key @param {Press} e @param {boolean} moving */
function oldMark(key, e, moving) {
  if (!moving) return undefined;
  if (key === "Enter") return "rename";
  if (key === "Escape") return "let go";
  const frameKey = key === "," || key === "." || key === "<" || key === ">";
  if (frameKey || stepOf(key, e.shiftKey) !== undefined) {
    const frames = frameKey
      ? (key === "," || key === "<" ? -1 : 1) * (e.shiftKey ? 10 : 1)
      : Math.round((stepOf(key, e.shiftKey) ?? 0) * FPS);
    return `mark ${frames}`;
  }
  if (key === "Delete" || key === "Backspace") return "remove mark";
  return undefined;
}

/** @param {string} key @param {boolean} shift @param {boolean} fullscreen */
function oldTransport(key, shift, fullscreen) {
  if (key === "f" || (key === "Escape" && fullscreen)) return `full ${key === "f" ? !fullscreen : false}`;
  if (key === " " || key === "k") return "play";
  if (key === "," || key === ".") return `step ${key === "," ? -1 : 1}`;
  if (stepOf(key, shift) !== undefined) return `seek ${stepOf(key, shift)}`;
  return null;
}

/** @param {Press} e @param {At} at */
function oldVod(e, { moving, fullscreen }) {
  if (e.altKey) return null;
  const key = keyOf(e);
  const bare = !e.shiftKey && !e.ctrlKey && !e.metaKey;
  if (key === "b") return "cut";
  if (key === "c" && bare) return "clip";
  if (key === "t" && bare) return "transcribe";
  if (e.ctrlKey || e.metaKey) return null;
  if ((key === "i" || key === "o") && !e.shiftKey) return key;
  if (e.shiftKey && (key === "c" || key === "m")) return key === "c" ? "captions" : "mute";
  if (key === "m" || key === "n") return key === "m" ? "highlight" : "chapter";
  return oldMark(key, e, moving) ?? oldTransport(key, e.shiftKey, fullscreen);
}

/** @param {Press} e @param {At} at */
function oldEdit(e, { moving, fullscreen, os }) {
  if (e.altKey) return null;
  const key = keyOf(e);
  if (e.shiftKey && (key === "c" || key === "m") && !e.ctrlKey && !e.metaKey) return key === "c" ? "captions" : "mute";
  if (key === "b" || key === "e") return key === "b" ? "cut" : "render";
  if ((os === "mac" ? e.metaKey : e.ctrlKey) && !e.shiftKey && (key === "ArrowLeft" || key === "ArrowRight"))
    return `piece ${key === "ArrowLeft" ? -1 : 1}`;
  if (e.ctrlKey || e.metaKey) return null;
  if ((key === "i" || key === "o") && !e.shiftKey) return key;
  if (key === "c" && !e.shiftKey) return "clip";
  if ((key === "m" || key === "n") && !e.shiftKey) return key === "m" ? "highlight" : "chapter";
  const mark = oldMark(key, e, moving);
  if (mark) return mark;
  if (key === "Delete" || key === "Backspace") return "remove";
  return oldTransport(key, e.shiftKey, fullscreen);
}

/** @param {Press} e @param {{ os: Os, typing: boolean }} at */
function oldApp(e, { os, typing }) {
  const key = e.key.toLowerCase();
  if ((e.ctrlKey || e.metaKey) && !e.altKey && !typing && (key === "z" || key === "y"))
    return key === "y" || e.shiftKey ? "redo" : "undo";
  const back = os === "mac" ? e.metaKey && !e.altKey && key === "[" : e.altKey && !e.ctrlKey && key === "arrowleft";
  const forward = os === "mac" ? e.metaKey && !e.altKey && key === "]" : e.altKey && !e.ctrlKey && key === "arrowright";
  if (!typing && (back || forward)) return back ? "back" : "forward";
  if ((e.ctrlKey || e.metaKey) && !e.altKey && key === "f") return "search";
  return null;
}

// The same handlers as they are now, asking the map in their order.

/** @param {Press} e @param {Is} is @param {boolean} moving */
function newMark(e, is, moving) {
  if (!moving) return undefined;
  if (is(e, "mark.rename")) return "rename";
  if (isEscape(e)) return "let go";
  const frames = markFramesOf(e, is, FPS);
  if (frames !== undefined) return `mark ${frames}`;
  if (is(e, "mark.remove")) return "remove mark";
  return undefined;
}

/** @param {Press} e @param {Is} is @param {boolean} fullscreen */
function newTransport(e, is, fullscreen) {
  /** @type {string | null} */
  let did = null;
  transportKey(
    e,
    {
      togglePlay: () => (did = "play"),
      seekBy: (s) => (did = `seek ${s}`),
      step: (n) => (did = `step ${n}`),
      fullscreen,
      setFullscreen: (on) => (did = `full ${on}`),
    },
    is,
  );
  return did;
}

/** @param {Press} e @param {Is} is @param {At} at */
function newVod(e, is, { moving, fullscreen }) {
  if (is(e, "cut.here")) return "cut";
  if (is(e, "clip.make")) return "clip";
  if (is(e, "transcribe.here")) return "transcribe";
  if (is(e, "selection.in")) return "i";
  if (is(e, "selection.out")) return "o";
  if (is(e, "captions.toggle")) return "captions";
  if (is(e, "mute.toggle")) return "mute";
  if (is(e, "mark.highlight")) return "highlight";
  if (is(e, "mark.chapter")) return "chapter";
  const mark = newMark(e, is, moving);
  if (mark) return mark;
  // New: the clips selected in the clip bar go (Player.svelte `onKey`).
  if (is(e, "clips.remove")) return "remove clips";
  return newTransport(e, is, fullscreen);
}

/** @param {Press} e @param {Is} is @param {At} at */
function newEdit(e, is, { moving, fullscreen }) {
  if (is(e, "captions.toggle")) return "captions";
  if (is(e, "mute.toggle")) return "mute";
  if (is(e, "cut.here")) return "cut";
  if (is(e, "edit.render")) return "render";
  if (is(e, "edit.pieceEarlier")) return "piece -1";
  if (is(e, "edit.pieceLater")) return "piece 1";
  if (is(e, "selection.in")) return "i";
  if (is(e, "selection.out")) return "o";
  if (is(e, "clip.make")) return "clip";
  if (is(e, "mark.highlight")) return "highlight";
  if (is(e, "mark.chapter")) return "chapter";
  const mark = newMark(e, is, moving);
  if (mark) return mark;
  if (is(e, "edit.remove")) return "remove";
  return newTransport(e, is, fullscreen);
}

/** @param {Press} e @param {Is} is @param {{ typing: boolean }} at */
function newApp(e, is, { typing }) {
  if (!typing && is(e, "undo")) return "undo";
  if (!typing && is(e, "redo")) return "redo";
  if (!typing && is(e, "nav.back")) return "back";
  if (!typing && is(e, "nav.forward")) return "forward";
  if (is(e, "search.focus") && !(typing && typesText(e))) return "search";
  // New, with something to select or selected (App.svelte `onWindowKey`,
  // bulk.ts `selectionKey`): all of the clip bar or the library, and the
  // VODs and edits selected removed.
  if (!typing && is(e, "select.all")) return "select all";
  if (!typing && is(e, "library.remove")) return "remove from the library";
  // New (App.svelte `onWindowKey`): the library and the pane's column shown or hidden.
  if (is(e, "view.library")) return "library shown or hidden";
  if (is(e, "view.pane")) return "pane shown or hidden";
  return null;
}

const KEYS = [
  ..."aAbBcCtTiIoOmMnNeEzZyYfFkKjJlLqQ ,.<>[]{};",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "Enter",
  "Delete",
  "Backspace",
  "Escape",
  "F2",
];
const MODS = ["ctrl", "alt", "shift", "meta"];
const PRESSES = KEYS.flatMap((key) =>
  Array.from({ length: 16 }, (_, mask) => press(key, MODS.filter((_, i) => mask & (1 << i)).join(" "))),
);
/** @param {Press} e */
const named = (e) =>
  [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "meta", JSON.stringify(e.key)]
    .filter(Boolean)
    .join("+");

/** Every default binding on `os`: the keys the app means. @param {Os} os */
const meant = (os) => new Set([...buildMap(os, {}).values()].flat());

/** The window's keys added since the map, on `os`. @param {Os} os */
const windowsNow = (os) => {
  const map = buildMap(os, {});
  return new Set([...(map.get("view.library") ?? []), ...(map.get("view.pane") ?? [])]);
};

/**
 * Why a press that did something before the map does nothing now, or null
 * if it shouldn't have stopped. The old tests were looser than the keys the
 * app meant, and these were let through by accident:
 * - "held as well": the meant key with a modifier more (Super+B, Shift+Space,
 *   Shift+F, Shift+J, Shift+N on a VOD, Shift+Enter, Shift+Delete,
 *   Ctrl+Super+Z, Alt+Shift+←). Among them Shift+"," (a layout whose comma
 *   needs Shift, such as Russian), which stepped a frame, or ten for a mark:
 *   there it is rebound.
 * - "the other way round": Ctrl for ⌘ on a Mac (Ctrl+Z, Ctrl+F), ⌘ (Super)
 *   for Ctrl elsewhere (Super+Z, Super+F).
 * - "Y on a Mac": ⌘Y redid; a Mac's redo is ⇧⌘Z.
 * - "< or > alone": their own key on European keyboards, which moved a
 *   selected mark a frame; US Shift+, and Shift+. still move it ten.
 * - "the window's now": a press a player took by accident that's since the
 *   window's own (Ctrl+Shift+B cut; it shows or hides the library).
 * @param {Os} os @param {Press} e
 */
function dropped(os, e) {
  const binding = bindingOf(e);
  const chord = binding === null ? null : parse(binding);
  if (!chord) return null;
  if (windowsNow(os).has(binding ?? "")) return "the window's now";
  if (meant(os).has(binding ?? "")) return null;
  const keys = [...meant(os)].map(parse).filter((c) => c !== null && c.key === chord.key);
  /** @param {Set<string>} mods */
  const covers = (mods) => keys.some((c) => c !== null && [...c.mods].every((m) => mods.has(m)));
  if (covers(chord.mods)) return "held as well";
  const swapped = new Set([...chord.mods].map((m) => (m === "Ctrl" ? "Meta" : m === "Meta" ? "Ctrl" : m)));
  if (covers(swapped)) return "the other way round";
  if (os === "mac" && chord.key === "y") return "Y on a Mac";
  if ((chord.key === "<" || chord.key === ">") && chord.mods.size === 0) return "< or > alone";
  return null;
}

/** What actions added since the map do: each on presses that did nothing before. */
const NEW = new Set([
  "select all",
  "remove from the library",
  "remove clips",
  "library shown or hidden",
  "pane shown or hidden",
]);

/** The deliberate differences met, by reason, with a press or two each. */
/** @type {Map<string, Set<string>>} */
const met = new Map();
/** @param {Os} os @param {string} where @param {Press} e @param {string | null} now @param {string | null} before */
function check(os, where, e, now, before, context = "") {
  const label = `${os} ${where} ${named(e)}${context}`;
  // An action added since the map, on a press that did nothing: so it may.
  if (before === null && now !== null && NEW.has(now)) {
    if (!met.has("new")) met.set("new", new Set());
    met.get("new")?.add(`${os} ${named(e)} ${now}`);
    return;
  }
  if (now === null && before !== null) {
    const reason = dropped(os, e);
    assert.ok(reason, `${label} did "${before}" and now does nothing`);
    if (!met.has(reason)) met.set(reason, new Set());
    met.get(reason)?.add(`${os} ${named(e)}`);
    return;
  }
  assert.equal(now, before, label);
}

test("on every OS, every press does in either player what it did before the map, or nothing", () => {
  for (const os of OSES) {
    const is = isOn(os);
    for (const moving of [false, true])
      for (const fullscreen of [false, true])
        for (const e of PRESSES) {
          const at = { moving, fullscreen, os };
          const context = ` moving=${moving} fullscreen=${fullscreen}`;
          check(os, "vod", e, newVod(e, is, at), oldVod(e, at), context);
          check(os, "edit", e, newEdit(e, is, at), oldEdit(e, at), context);
        }
  }
});

test("an edit's E renders, bare, even before its player is there", () => {
  for (const os of OSES) {
    const is = isOn(os);
    for (const e of PRESSES) {
      const before = !e.altKey && keyOf(e) === "e" ? "render" : null;
      check(os, "edit, no player", e, is(e, "edit.render") ? "render" : null, before);
    }
  }
});

test("on every OS, the app's keys do what they did, in a text field or not, or nothing", () => {
  for (const os of OSES) {
    const is = isOn(os);
    for (const typing of [false, true])
      for (const e of PRESSES)
        check(os, "app", e, newApp(e, is, { typing }), oldApp(e, { os, typing }), ` typing=${typing}`);
  }
});

test("an app key that types a character leaves it to the text field being typed in", () => {
  // Search given "/": it jumps to the box from anywhere but a text field,
  // where "/" is typed as ever (the box itself, a clip's name, a mark's note).
  const is = isOn("linux", { "search.focus": ["/", "Ctrl+f"] });
  assert.equal(newApp(press("/"), is, { typing: false }), "search");
  assert.equal(newApp(press("/"), is, { typing: true }), null);
  assert.equal(newApp(press("f", "ctrl"), is, { typing: true }), "search", "a chord still reaches it");
  assert.equal(typesText(press(" ")), true);
  assert.equal(typesText(press("A", "shift")), true);
  assert.equal(typesText(press("a", "ctrl")), false);
  assert.equal(typesText(press("a", "meta")), false);
  assert.equal(typesText(press("ArrowLeft")), false);
});

test("the presses let through by accident before are the ones listed", () => {
  // Runs after the three above, which collected them.
  assert.deepEqual([...met.keys()].sort(), [
    "< or > alone",
    "Y on a Mac",
    "held as well",
    "new",
    "the other way round",
    "the window's now",
  ]);
  const all = new Set([...met.values()].flatMap((s) => [...s]));
  for (const example of [
    'linux meta+"b"',
    'linux shift+" "',
    'linux shift+"N"',
    'linux shift+"f"',
    'linux shift+"j"',
    'linux shift+"Enter"',
    'linux shift+"Delete"',
    'linux ctrl+meta+"z"',
    'linux alt+shift+"ArrowLeft"',
    'linux meta+"f"',
    'linux shift+","',
    'mac ctrl+"z"',
    'mac meta+"y"',
    'linux "<"',
    'linux ctrl+"a" select all',
    'mac meta+"a" select all',
    'linux "Delete" remove from the library',
    'linux "Backspace" remove clips',
    'linux ctrl+shift+"B"',
    'mac shift+meta+"B"',
    'linux ctrl+shift+"B" library shown or hidden',
    'linux ctrl+alt+"b" pane shown or hidden',
  ])
    assert.ok(all.has(example), example);
});
