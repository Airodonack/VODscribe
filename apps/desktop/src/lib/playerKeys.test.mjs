// The keys both players share: node --test apps/desktop/src/lib/playerKeys.test.mjs
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
const { LEGEND_GROUPS, buildMap, matches } = await import("./keymap.ts");
const { editShortcuts, forPlayer, inOut, seekOf, transportKey, vodShortcuts } = await import("./playerKeys.ts");

/** The default map on `os`, as `is` and `bindings`, with `overrides` over it.
 *  @param {import("./keymap.ts").Os} os @param {Record<string, string[]>} overrides */
const keysOn = (os, overrides = {}) => {
  const map = buildMap(os, overrides);
  /** @type {import("./keymap.ts").Is} */
  const is = (e, id) => matches(e, map.get(id) ?? []);
  /** @type {import("./playerKeys.ts").Bindings} */
  const bindings = (id) => map.get(id) ?? [];
  return { is, bindings };
};
const { is, bindings } = keysOn("linux");
/** @param {string} key */
const press = (key, shift = false) => ({ key, shiftKey: shift, ctrlKey: false, altKey: false, metaKey: false });

test("arrows step 5 s, 1 s with Shift; J and L 10 s", () => {
  assert.equal(seekOf(press("ArrowLeft"), is), -5);
  assert.equal(seekOf(press("ArrowRight", true), is), 1);
  assert.equal(seekOf(press("J"), is), -10);
  assert.equal(seekOf(press("j", true), is), undefined);
  assert.equal(seekOf(press("l"), is), 10);
  assert.equal(seekOf(press("x"), is), undefined);
});

test("transport keys do what they say, and nothing else", () => {
  /** @type {string[]} */
  const did = [];
  /** @type {import("./playerKeys.ts").Transport} */
  const t = {
    togglePlay: () => did.push("play"),
    seekBy: (s) => did.push(`seek ${s}`),
    step: (n) => did.push(`step ${n}`),
    fullscreen: true,
    setFullscreen: (on) => did.push(`full ${on}`),
  };
  for (const key of [" ", "k", ",", ".", "ArrowLeft", "f", "Escape"]) assert.ok(transportKey(press(key), t, is), key);
  assert.deepEqual(did, ["play", "play", "step -1", "step 1", "seek -5", "full false", "full false"]);
  assert.equal(transportKey(press("q"), t, is), false);
  // Without frame steps (a site's player), , and . mean nothing; out of full
  // screen, neither does Escape.
  assert.equal(transportKey(press(","), { ...t, step: undefined }, is), false);
  assert.equal(transportKey(press("Escape"), { ...t, fullscreen: false }, is), false);
});

test("a player has every key the app hasn't taken, Alt ones too, but none in a field or dialog", () => {
  /** @param {object} e @param {string | null} inside */
  const event = (e, inside = null) =>
    /** @type {KeyboardEvent} */ (
      /** @type {unknown} */ ({
        ...press("p"),
        defaultPrevented: false,
        target: { closest: (/** @type {string} */ sel) => (inside && sel.includes(inside) ? {} : null) },
        ...e,
      })
    );
  assert.equal(forPlayer(event({})), true);
  // Alt+P bound to a player action reaches it; Alt+← is back, taken first.
  assert.equal(forPlayer(event({ altKey: true })), true);
  assert.equal(forPlayer(event({ key: "ArrowLeft", altKey: true, defaultPrevented: true })), false);
  assert.equal(forPlayer(event({}, "input")), false);
  assert.equal(forPlayer(event({}, "[role=dialog]")), false);
  // A modal window open, the focus left on the page body: still not the player's.
  const page = /** @type {{ document?: unknown }} */ (/** @type {unknown} */ (globalThis));
  page.document = { querySelector: () => ({}) };
  try {
    assert.equal(forPlayer(event({})), false);
  } finally {
    delete page.document;
  }
});

/** A legend's groups as headings and lines, and its lines all together.
 *  @param {import("./playerKeys.ts").ShortcutGroup[]} groups */
const flat = (groups) => groups.flatMap((g) => g.shortcuts);
/** @param {import("./playerKeys.ts").ShortcutGroup[]} groups @param {string} does */
const keysOf = (groups, does) => flat(groups).find((s) => s.does === does)?.keys;

test("each legend lists a key once for each thing it does", () => {
  for (const groups of [
    vodShortcuts(true, "linux", bindings),
    editShortcuts(true, "linux", bindings),
    vodShortcuts(true, "mac", keysOn("mac").bindings),
  ]) {
    const does = flat(groups).map((s) => s.does);
    assert.equal(new Set(does).size, does.length, does.join(" | "));
    assert.ok(does.includes("undo"));
  }
  assert.ok(!keysOf(vodShortcuts(false, "linux", bindings), "a frame"), "no frame steps on a site's player");
});

test("a legend's columns are its own groups, in order, none long", () => {
  const vod = vodShortcuts(true, "linux", bindings);
  const edit = editShortcuts(true, "linux", bindings);
  for (const groups of [vod, edit]) {
    assert.deepEqual(
      groups.map((g) => g.heading),
      LEGEND_GROUPS.map((g) => g.heading),
    );
    assert.deepEqual(
      groups.map((g) => g.heading),
      ["Playing", "Cutting", "Marks", "More"],
    );
    // Short enough to fit under the controls.
    for (const g of groups) assert.ok(g.shortcuts.length <= 8, `${g.heading} has ${g.shortcuts.length} lines`);
  }
  // An edit's own keys join Cutting; B is there once, as what it does in an edit.
  assert.deepEqual(edit[1].shortcuts, [
    { keys: ["B"], does: "cut here, or selection ends" },
    { keys: ["I", "O"], does: "selection start, end" },
    { keys: ["C"], does: "clip the stretch" },
    { keys: ["Ctrl", "+", "←", "→"], does: "move the piece" },
    { keys: ["Delete"], does: "remove the stretch or piece" },
    { keys: ["E"], does: "render" },
  ]);
  assert.ok(!keysOf(edit, "cut here"));
  assert.ok(!keysOf(edit, "transcribe"));
  assert.ok(!keysOf(edit, "drag a clip out"));
  assert.ok(!keysOf(vod, "render"));
});

// The default legends, on Linux and Windows; a Mac's writes ⌘Z and ⇧⌘Z for
// undo and redo, and "Option" for Alt.
const playing = [
  { keys: ["Space", "or", "K"], does: "play/pause" },
  { keys: ["←", "→"], does: "5 s" },
  { keys: ["Shift", "+", "←", "→"], does: "1 s" },
  { keys: ["J", "L"], does: "10 s" },
  { keys: [",", "."], does: "a frame" },
];
const cutting = [
  { keys: ["B"], does: "cut here" },
  { keys: ["I", "O"], does: "selection start, end" },
  { keys: ["C"], does: "clip the stretch" },
  { keys: ["T"], does: "transcribe" },
  { keys: ["Alt", "+", "drag"], does: "drag a clip out" },
];
const marks = [
  { keys: ["M"], does: "highlight here" },
  { keys: ["N"], does: "chapter here" },
  { keys: ["Enter"], does: "rename" },
  { keys: ["Delete"], does: "remove" },
  { keys: [",", "."], does: "move a frame" },
  { keys: ["Shift", "+", "<", ">"], does: "move ten frames" },
  { keys: ["←", "→", "or", "J", "L"], does: "move 5 s or 10 s" },
];
const more = [
  { keys: ["Shift", "+", "M"], does: "mute" },
  { keys: ["Shift", "+", "C"], does: "captions" },
  { keys: ["F"], does: "full screen" },
  { keys: ["Ctrl", "+", "Z"], does: "undo" },
  { keys: ["Ctrl", "+", "Shift", "+", "Z"], does: "redo" },
  { keys: ["Esc"], does: "deselect" },
];

test("the default legends", () => {
  for (const os of /** @type {const} */ (["linux", "windows"])) {
    assert.deepEqual(
      vodShortcuts(true, os, keysOn(os).bindings).map((g) => g.shortcuts),
      [playing, cutting, marks, more],
    );
    const edit = editShortcuts(true, os, keysOn(os).bindings);
    assert.deepEqual(
      [edit[0], edit[2], edit[3]].map((g) => g.shortcuts),
      [playing, marks, more],
    );
  }
  const mac = vodShortcuts(true, "mac", keysOn("mac").bindings);
  assert.deepEqual(keysOf(mac, "undo"), ["⌘", "Z"]);
  assert.deepEqual(keysOf(mac, "redo"), ["⇧", "⌘", "Z"]);
  assert.deepEqual(keysOf(mac, "drag a clip out"), ["Option", "+", "drag"]);
});

test("a rebound key shows in the legend; an action with none drops out", () => {
  const { bindings } = keysOn("linux", { "play.toggle": ["p"], "cut.here": [], "seek.back5": ["Shift+h"] });
  const groups = vodShortcuts(true, "linux", bindings);
  assert.deepEqual(keysOf(groups, "play/pause"), ["P"]);
  assert.ok(!keysOf(groups, "cut here"));
  assert.deepEqual(keysOf(groups, "5 s"), ["Shift", "+", "H", "or", "→"]);
  // Those keys taken away, More keeps only Escape, which is no action's.
  const none = keysOn("linux", {
    "mute.toggle": [],
    "captions.toggle": [],
    "fullscreen.toggle": [],
    undo: [],
    redo: [],
  }).bindings;
  assert.deepEqual(vodShortcuts(true, "linux", none).find((g) => g.group === "more")?.shortcuts, [
    { keys: ["Esc"], does: "deselect" },
  ]);
});

test("I and O move the selection's ends, swapping past the other end", () => {
  const picked = { start: 10, end: 20 };
  assert.deepEqual(inOut("i", 12, picked, [], 100), { start: 12, end: 20 });
  assert.deepEqual(inOut("o", 15, picked, [], 100), { start: 10, end: 15 });
  // O before the in point: the selection runs from O to I; I after the out
  // point likewise.
  assert.deepEqual(inOut("o", 4, picked, [], 100), { start: 4, end: 10 });
  assert.deepEqual(inOut("i", 30, picked, [], 100), { start: 20, end: 30 });
  // Onto the other end it would be empty: nothing changes.
  assert.equal(inOut("i", 20, picked, [], 100), null);
  assert.equal(inOut("o", 10, picked, [], 100), null);
});

test("with no selection, I runs one on to the next cut, O back to the one before", () => {
  const cuts = [30, 10, 50];
  assert.deepEqual(inOut("i", 12, null, cuts, 100), { start: 12, end: 30 });
  assert.deepEqual(inOut("o", 12, null, cuts, 100), { start: 10, end: 12 });
  // On a cut, it's the stretch on from it, or back from it.
  assert.deepEqual(inOut("i", 30, null, cuts, 100), { start: 30, end: 50 });
  assert.deepEqual(inOut("o", 30, null, cuts, 100), { start: 10, end: 30 });
  // No cuts: to the end, or from the start.
  assert.deepEqual(inOut("i", 60, null, [], 100), { start: 60, end: 100 });
  assert.deepEqual(inOut("o", 60, null, [], 100), { start: 0, end: 60 });
  // At the end (I) or the start (O) there's nothing to select.
  assert.equal(inOut("i", 100, null, [], 100), null);
  assert.equal(inOut("o", 0, null, [], 100), null);
});
