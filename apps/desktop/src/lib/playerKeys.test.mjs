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
const { buildMap, matches } = await import("./keymap.ts");
const { editShortcuts, inOut, playerKey, seekOf, transportKey, vodShortcuts } = await import("./playerKeys.ts");

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
  assert.equal(playerKey(event({})), "p");
  // Alt+P bound to a player action reaches it; Alt+← is back, taken first.
  assert.equal(playerKey(event({ altKey: true })), "p");
  assert.equal(playerKey(event({ key: "ArrowLeft", altKey: true, defaultPrevented: true })), null);
  assert.equal(playerKey(event({}, "input")), null);
  assert.equal(playerKey(event({}, "[role=dialog]")), null);
  // A modal window open, the focus left on the page body: still not the player's.
  const page = /** @type {{ document?: unknown }} */ (/** @type {unknown} */ (globalThis));
  page.document = { querySelector: () => ({}) };
  try {
    assert.equal(playerKey(event({})), null);
  } finally {
    delete page.document;
  }
});

test("each legend lists a key once for each thing it does", () => {
  for (const list of [
    vodShortcuts(true, "linux", bindings),
    editShortcuts(true, "linux", bindings),
    vodShortcuts(true, "mac", keysOn("mac").bindings),
  ]) {
    const does = list.map((s) => s.does);
    assert.equal(new Set(does).size, does.length, does.join(" | "));
    assert.ok(does.includes("undo or redo"));
  }
  assert.ok(
    !vodShortcuts(false, "linux", bindings).some((s) => s.does === "a frame back or on"),
    "no frame steps on a site's player",
  );
});

// The legends as they were written by hand before the action table, on
// Linux and Windows; a Mac's wrote ⌘Z and ⇧⌘Z for undo and "Option" for Alt.
const playing = [
  { keys: ["Space", "or", "K"], does: "play/pause" },
  { keys: ["←", "→"], does: "5 s" },
  { keys: ["Shift", "+", "←", "→"], does: "1 s" },
  { keys: ["J", "L"], does: "10 s" },
  { keys: [",", "."], does: "a frame back or on" },
];
const undo = { keys: ["Ctrl", "+", "Z", "or", "Ctrl", "+", "Shift", "+", "Z"], does: "undo or redo" };
const VOD = [
  ...playing,
  { keys: ["B"], does: "cut here" },
  { keys: ["I", "O"], does: "start or end the selection here" },
  { keys: ["C"], does: "clip the stretch" },
  { keys: ["T"], does: "transcribe the selection, or the whole VOD" },
  { keys: ["M"], does: "highlight here" },
  { keys: ["N"], does: "new chapter here" },
  { keys: ["Enter"], does: "rename the selected chapter or highlight" },
  { keys: [",", ".", "or", "←", "→", "or", "J", "L"], does: "move the selected cut (Shift with , . ten frames)" },
  { keys: ["Delete"], does: "remove the selected cut" },
  { keys: ["Esc"], does: "deselect, or take back a new chapter or highlight" },
  { keys: ["Shift", "+", "C"], does: "captions" },
  { keys: ["Shift", "+", "M"], does: "mute" },
  { keys: ["F"], does: "full screen" },
  undo,
  { keys: ["Alt", "+", "drag"], does: "on the timeline, or drag Clip: the stretch out as a clip" },
];
const EDIT = [
  ...playing,
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
  { keys: ["Ctrl", "+", "←", "→"], does: "move the piece here earlier or later" },
  { keys: ["C"], does: "clip the selection, or the stretch here" },
  { keys: ["E"], does: "export: render the edit" },
  { keys: ["Shift", "+", "C"], does: "captions" },
  { keys: ["Shift", "+", "M"], does: "mute" },
  { keys: ["F"], does: "full screen" },
  undo,
];

test("the default legends read as the hand-written ones did", () => {
  for (const os of /** @type {const} */ (["linux", "windows"])) {
    assert.deepEqual(vodShortcuts(true, os, keysOn(os).bindings), VOD);
    assert.deepEqual(editShortcuts(true, os, keysOn(os).bindings), EDIT);
  }
  const mac = vodShortcuts(true, "mac", keysOn("mac").bindings);
  assert.deepEqual(mac.find((s) => s.does === "undo or redo")?.keys, ["⌘", "Z", "or", "⇧", "⌘", "Z"]);
  assert.deepEqual(mac.at(-1)?.keys, ["Option", "+", "drag"]);
});

test("a rebound key shows in the legend; an action with none drops out", () => {
  const { bindings } = keysOn("linux", { "play.toggle": ["p"], "cut.here": [], "seek.back5": ["Shift+h"] });
  const list = vodShortcuts(true, "linux", bindings);
  assert.deepEqual(list.find((s) => s.does === "play/pause")?.keys, ["P"]);
  assert.ok(!list.some((s) => s.does === "cut here"));
  assert.deepEqual(list.find((s) => s.does === "5 s")?.keys, ["Shift", "+", "H", "or", "→"]);
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
