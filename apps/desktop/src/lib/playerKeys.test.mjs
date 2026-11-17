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
const { editShortcuts, forPlayer, inOut, isEscape, markFramesOf, seekOf, transportKey, vodShortcuts } =
  await import("./playerKeys.ts");

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
/** @param {string} key @param {string} mods "ctrl alt shift meta" */
const chord = (key, mods) => ({
  key,
  ctrlKey: mods.includes("ctrl"),
  altKey: mods.includes("alt"),
  shiftKey: mods.includes("shift"),
  metaKey: mods.includes("meta"),
});

/** playerKeys.ts `SEEKS` (CHANGELOG.md, 0.7.0: "the arrow keys and J and L"; 0.6.0 legend: "5 s", "1 s", "10 s"). */
test("arrows step 5 s, 1 s with Shift; J and L 10 s", () => {
  assert.equal(seekOf(press("ArrowLeft"), is), -5);
  assert.equal(seekOf(press("ArrowRight", true), is), 1);
  assert.equal(seekOf(press("J"), is), -10);
  assert.equal(seekOf(press("j", true), is), undefined);
  assert.equal(seekOf(press("l"), is), 10);
  assert.equal(seekOf(press("x"), is), undefined);
});

/** playerKeys.ts `transportKey`: "Does what `e` means for any player; false if it means nothing here." */
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

/** playerKeys.ts `forPlayer`: "not taken already ..., nor typed into a field or a dialog, nor pressed while a modal window (Settings) is open ... Alt held is no reason of its own". */
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

/** playerKeys.ts `shortcuts`: a legend line for each thing a key does; a site's player has no frame steps (CHANGELOG.md, 0.6.0: "The keyboard button opens a cheat sheet"). */
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

/** CHANGELOG.md, 0.6.0: "with the keys in four short columns, Playing, Cutting, Marks and More ... an edit's own keys under Cutting". */
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
    { keys: ["Ctrl", "+", "C", "V"], does: "copy, paste" },
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
  { keys: ["Ctrl", "+", "C", "V"], does: "copy, paste" },
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

/** CHANGELOG.md, 0.6.0: the cheat sheet's keys, each with a few words on what it does; a Mac's writes ⌘ and Option (playerKeys.ts `shortcuts`). */
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
  assert.deepEqual(keysOf(mac, "copy, paste"), ["⌘", "C", "V"]);
});

/** playerKeys.ts `legend`: "written from the live map so a rebound key shows ... A line whose actions have no key is left out." */
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

/** playerKeys.ts `inOut`: "the selection's in (I) or out (O) point put at `at`, the playhead. Put past the other end, the two swap" (CHANGELOG.md, 0.7.1 Fixes: "An edge can't empty the selection."). */
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

/** playerKeys.ts `inOut`: "With no selection, I starts one that runs on to the next of `stops` ... or the end ... and O one back to the one before or the start." */
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

/** playerKeys.ts `isEscape`: "it always lets go (of full screen, a selection, a mark), Shift or not; with Ctrl, Alt or ⌘ it's the system's". */
test("Escape lets go with Shift held, but not with Ctrl, Alt or ⌘", () => {
  assert.equal(isEscape(chord("Escape", "")), true);
  assert.equal(isEscape(chord("Escape", "shift")), true);
  for (const mods of ["ctrl", "alt", "meta"]) assert.equal(isEscape(chord("Escape", mods)), false, mods);
  assert.equal(isEscape(chord("Enter", "")), false);
});

/** playerKeys.ts `markFramesOf` (CHANGELOG.md, 0.7.0: an edge selected moves "a frame at a time with , and . (or as far as the arrows, J and L go)"; 0.6.0 legend: "move ten frames", "move 5 s or 10 s"). */
test("a selected mark moves a frame, ten with Shift, or as far as a seek key goes", () => {
  const fps = 25;
  assert.equal(markFramesOf(press(","), is, fps), -1);
  assert.equal(markFramesOf(press("."), is, fps), 1);
  assert.equal(markFramesOf(press("<", true), is, fps), -10);
  assert.equal(markFramesOf(press(">", true), is, fps), 10);
  assert.equal(markFramesOf(press("ArrowLeft"), is, fps), -125);
  assert.equal(markFramesOf(press("ArrowRight", true), is, fps), 25);
  assert.equal(markFramesOf(press("j"), is, fps), -250);
  assert.equal(markFramesOf(press("l"), is, fps), 250);
  // A seek's distance in whole frames: 5 s at 29.97 frames a second.
  assert.equal(markFramesOf(press("ArrowRight"), is, 29.97), 150);
  assert.equal(markFramesOf(press("q"), is, fps), undefined);
  assert.equal(markFramesOf(press("<"), is, fps), undefined, "< without Shift is a key of its own");
});

/** playerKeys.ts `transportKey`: F toggles full screen, in and out; Escape leaves it whatever F is bound to. */
test("the full screen key toggles it, and Escape leaves it even with the key rebound", () => {
  /** @type {boolean[]} */
  const set = [];
  /** @param {boolean} fullscreen @returns {import("./playerKeys.ts").Transport} */
  const t = (fullscreen) => ({ togglePlay() {}, seekBy() {}, fullscreen, setFullscreen: (on) => set.push(on) });
  assert.ok(transportKey(press("f"), t(false), is));
  assert.ok(transportKey(press("f"), t(true), is));
  const rebound = keysOn("linux", { "fullscreen.toggle": ["g"] }).is;
  assert.ok(transportKey(press("Escape"), t(true), rebound));
  assert.equal(transportKey(press("f"), t(false), rebound), false);
  assert.deepEqual(set, [true, false, false]);
});

/** playerKeys.ts `inOut`: a cut within a millionth of a second of the playhead is the playhead's own, not the next stop. */
test("a cut where the playhead is isn't a stop for I or O", () => {
  assert.deepEqual(inOut("i", 30, null, [30 + 1e-9, 50], 100), { start: 30, end: 50 });
  assert.deepEqual(inOut("o", 30, null, [30 - 1e-9, 10], 100), { start: 10, end: 30 });
});

/** playerKeys.ts `forPlayer`: a press with no target at all, or one that can't look for its field, is for the player. */
test("a press whose target can't be asked about a field is the player's", () => {
  const e = (/** @type {object} */ x) =>
    /** @type {KeyboardEvent} */ (/** @type {unknown} */ ({ ...press("p"), defaultPrevented: false, ...x }));
  assert.equal(forPlayer(e({ target: null })), true);
  assert.equal(forPlayer(e({ target: {} })), true);
  assert.equal(forPlayer(e({})), true);
});

/** playerKeys.ts `forPlayer`: "nor pressed while a modal window (Settings) is open": it's the open modal the page is asked for. */
test("only an open modal window, as the page marks one, takes the player's keys", () => {
  const page = /** @type {{ document?: unknown }} */ (/** @type {unknown} */ (globalThis));
  page.document = { querySelector: (/** @type {string} */ sel) => (sel === '[aria-modal="true"]' ? null : {}) };
  try {
    const e = /** @type {KeyboardEvent} */ (
      /** @type {unknown} */ ({ ...press("p"), defaultPrevented: false, target: null })
    );
    assert.equal(forPlayer(e), true);
  } finally {
    delete page.document;
  }
});

/** playerKeys.ts `inOut`: "Null if it would be empty: nothing changes." Past the end, I has nothing to select, nor O before the start; with a selection, they still move its edge. */
test("I past the end or O before the start selects nothing, with or without cuts, but moves a selection's edge", () => {
  assert.equal(inOut("i", 120, null, [], 100), null);
  assert.equal(inOut("i", 120, null, [150], 100), null);
  assert.equal(inOut("o", -5, null, [], 100), null);
  assert.deepEqual(inOut("i", 100, { start: 10, end: 50 }, [], 100), { start: 50, end: 100 });
  assert.deepEqual(inOut("o", 0, { start: 10, end: 20 }, [], 100), { start: 0, end: 10 });
});

/** playerKeys.ts `legend`: "each line names its keys in groups joined with \"or\"": a group with no key leaves no dangling "or", and two groups written alike are written once. */
test("a legend line joins only the groups that have keys, and writes keys alike once", () => {
  const nothing = { "seek.back10": [], "seek.forward10": [] };
  const five = keysOn("linux", nothing).bindings;
  assert.deepEqual(keysOf(vodShortcuts(true, "linux", five), "move 5 s or 10 s"), ["←", "→"]);
  const same = keysOn("linux", { "seek.back10": ["ArrowLeft"], "seek.forward10": ["ArrowRight"] }).bindings;
  assert.deepEqual(keysOf(vodShortcuts(true, "linux", same), "move 5 s or 10 s"), ["←", "→"]);
  const other = keysOn("linux", { "seek.back5": [], "seek.forward5": [] }).bindings;
  assert.deepEqual(keysOf(vodShortcuts(true, "linux", other), "move 5 s or 10 s"), ["J", "L"]);
});

/** CHANGELOG.md, 0.6.0: "the keys in four short columns"; a column with no line left (every key of it taken away) isn't shown. */
test("a legend column whose keys are all taken away isn't shown", () => {
  const ids = [
    "mark.highlight",
    "mark.chapter",
    "mark.rename",
    "mark.remove",
    "frame.back",
    "frame.forward",
    "frame.back10",
    "frame.forward10",
    "seek.back5",
    "seek.forward5",
    "seek.back10",
    "seek.forward10",
  ];
  const { bindings } = keysOn("linux", Object.fromEntries(ids.map((id) => [id, []])));
  assert.deepEqual(
    vodShortcuts(true, "linux", bindings).map((g) => g.heading),
    ["Playing", "Cutting", "More"],
  );
});
