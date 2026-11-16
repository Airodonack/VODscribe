// What's done with a set picked (bulk.ts): which things are chosen and how they're taken, and the keys Escape,
// Ctrl+A and Delete. The page's own pick, where, selection and platform are the real ones (selection.svelte.ts
// compiled as the app's build does); what's outside them, the backend and the stores of the open video, is the
// fakes below, kept on globalThis.fake for the test to read and set.
// node --test apps/desktop/src/lib/bulk.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

const fakes = {
  "./api": `export const api = globalThis.fake.api;`,
  "./clips.svelte": `export const clips = globalThis.fake.clips;`,
  "./cuts.svelte": `export const cuts = globalThis.fake.cuts;`,
  "./edits.svelte": `export const edits = globalThis.fake.edits;`,
  "./keys.svelte": `export const is = (e, id) => e.id === id;`,
  "./toasts.svelte": `export const toast = globalThis.fake.toast;`,
  "./undo": `export const undoIn = (doc) => void globalThis.fake.undone.push(doc);`,
  "./actions": `export const removeEntries = async (vods, gone, o) => void globalThis.fake.removed.push({ vods, gone, o });`,
  "./dragout": `export const carrySet = (o) => void globalThis.fake.carried.push(o);
    export const pullStretch = (start, end, shown) => globalThis.fake.pull(start, end, shown);
    export const exportStretches = async (ranges) => void globalThis.fake.exported.push(ranges);
    export const stretchSaid = (start, end) => "stretch " + start + "-" + end;`,
  "./media": `export const inEdit = (pieces, start, end) => ({ inEdit: true, start, end });`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/bulk.ts") && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier.startsWith("./") && (!/\\.[a-z]+$/.test(specifier) || specifier.endsWith(".svelte")) && context.parentURL?.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}
export async function load(url, context, next) {
  if (!url.endsWith(".svelte.ts")) return next(url, context);
  const source = stripTypeScriptTypes(await readFile(new URL(url), "utf8"), { mode: "transform" });
  const { js } = compileModule(source, { filename: url, generate: "client" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

/** @typedef {{ kind: "vod" | "edit", id: number }} Doc */
const fake = {
  /** What the backend was asked to do to a VOD: [what, id]. @type {unknown[][]} */
  asked: [],
  /** Ids the backend refuses to transcribe. @type {number[]} */
  refuses: /** @type {number[]} */ ([]),
  /** @type {number[][]} */
  deleted: [],
  /** @type {unknown[]} */
  undone: [],
  /** @type {{ vods: { id: number }[], gone: { id: number }[], o: { onremove: (id: number) => void, confirmed: () => void } }[]} */
  removed: [],
  /** @type {{ start: number, end: number }[][]} */
  exported: [],
  /** What the toasts said. @type {string[]} */
  toasts: [],
  toast: {
    /** @param {string} message @param {{ label: string, run: () => void }} [action] */
    show: (message, action) => {
      fake.toasts.push(message);
      fake.actions.push(action);
    },
  },
  /** The action of each toast, if it had one. @type {({ label: string, run: () => void } | undefined)[]} */
  actions: [],
  /** What the clip bar of an edit was asked to make at once. @type {unknown[]} */
  madeAll: [],
  /** What carrySet was given, each time. @type {any[]} */
  carried: [],
  /** The clip a stretch is pulled out as for a drag: by default, a clip not on the bar. */
  pull: async (/** @type {number} */ start, /** @type {number} */ end, /** @type {boolean} */ shown) => {
    fake.pulls.push([start, end, shown]);
    return fake.pullFails ? null : { id: 1000 + start, shown };
  },
  /** @type {unknown[][]} */
  pulls: [],
  pullFails: false,
  /** Clips made in an edit for a drag: [start, end, shown]. @type {unknown[][]} */
  made: [],
  /** @type {number[]} */
  forgotten: [],
  /** What edits.put was asked: [id, puts, bar]. @type {unknown[][]} */
  put: [],
  putOk: true,
  api: {
    /** @param {number} id */
    retryVod: async (id) => void fake.asked.push(["rest", id]),
    /** @param {number} id @param {number} from @param {number | null} to */
    transcribeStretch: async (id, from, to) => {
      if (fake.refuses.includes(id)) throw new Error("no room");
      fake.asked.push(["all", id, from, to]);
    },
    /** @param {number} id */
    deleteClip: async (id) => void fake.forgotten.push(id),
    /** @param {number} id */
    getEdit: async (id) => (id === 2 ? { pieces: [{ piece: id }] } : Promise.reject(new Error("gone"))),
    /** @param {number[]} ids */
    deleteClips: async (ids) => {
      if (ids.includes(13)) throw new Error("locked");
      fake.deleted.push(ids);
    },
  },
  clips: {
    /** @type {{ kind: string, id: number } | null} */
    doc: { kind: "vod", id: 1 },
    /** @type {{ id: number, vod_id: number, start: number, end: number, title: string }[]} */
    shown: [],
    hovered: /** @type {unknown} */ (null),
    error: /** @type {string | null} */ (null),
    fromSite: false,
    /** @param {{ start: number, end: number }[]} ranges */
    /** @param {number} start @param {number} end @param {boolean} shown */
    make: async (start, end, shown) => {
      fake.made.push([start, end, shown]);
      return start === 99 ? null : { id: 2000 + start, shown };
    },
    makeAll: async (/** @type {unknown} */ ranges) => void fake.madeAll.push(ranges),
  },
  cuts: {
    editing: /** @type {unknown} */ (null),
    length: 0,
    doc: /** @type {unknown} */ (null),
    selection: /** @type {{ start: number, end: number } | null} */ (null),
    selectedEdge: /** @type {unknown} */ ("x"),
    selected: /** @type {unknown} */ (null),
  },
  edits: {
    openId: /** @type {number | null} */ (null),
    pieces: [],
    footageHere: false,
    error: /** @type {string | null} */ (null),
    /** @param {number} id @param {unknown[]} puts @param {boolean} bar */
    put: async (id, puts, bar) => {
      fake.put.push([id, puts, bar]);
      return fake.putOk;
    },
  },
};
Object.assign(globalThis, { fake });

const selection = await import("./selection.svelte.ts");
const { snippets, library } = selection;
const bulk = await import("./bulk.ts");

/** A page: what `document` finds by selector (boxes), what has the focus, and whether dialogs are open. */
const page = {
  /** @type {Record<string, { left: number, right: number, top: number, bottom: number }>} */
  boxes: {},
  /** The selector the focus is inside of. @type {string | null} */
  focusIn: null,
  dialog: false,
};
Object.assign(globalThis, {
  document: {
    /** @param {string} selector */
    querySelector: (selector) => {
      if (selector === "[role=dialog]") return page.dialog ? {} : null;
      const box = page.boxes[selector];
      return box ? { getBoundingClientRect: () => box } : null;
    },
    activeElement: { closest: (/** @type {string} */ selector) => (selector === page.focusIn ? {} : null) },
  },
});

const clip = (/** @type {number} */ id) => /** @type {const} */ ({ kind: "clip", id });
const segment = (/** @type {number} */ start, /** @type {number} */ end) =>
  /** @type {const} */ ({ kind: "segment", start, end });
const vod = (/** @type {number} */ id) => /** @type {const} */ ({ kind: "vod", id });
const edit = (/** @type {number} */ id) => /** @type {const} */ ({ kind: "edit", id });
const row = (/** @type {number} */ id, /** @type {object} */ over = {}) => ({
  kind: /** @type {const} */ ("vod"),
  vod: /** @type {any} */ ({ id, title: `VOD ${id}`, status: "none", transcribed: "none", ...over }),
});
const editRow = (/** @type {number} */ id) => ({
  kind: /** @type {const} */ ("edit"),
  edit: /** @type {any} */ ({ id, title: `Edit ${id}` }),
});
const shown = (/** @type {number[]} */ ids) =>
  ids.map((id) => ({ id, vod_id: 1, start: id * 10, end: id * 10 + 5, title: `Clip ${id}`, shown: true }));
const key = (/** @type {string} */ id, /** @type {object} */ over = {}) =>
  /** @type {KeyboardEvent} */ (/** @type {unknown} */ ({ id, key: id, repeat: false, ...over }));

/** Everything as at the start: nothing picked, nothing selected, nothing asked, a VOD open with no clips. */
function fresh() {
  library.follow(() => null);
  library.set([]);
  snippets.set([]);
  fake.asked.length = fake.deleted.length = fake.undone.length = fake.removed.length = 0;
  fake.madeAll.length = fake.exported.length = fake.toasts.length = fake.actions.length = fake.carried.length = 0;
  fake.pulls.length = fake.made.length = fake.forgotten.length = fake.put.length = 0;
  fake.pullFails = false;
  fake.putOk = true;
  fake.edits.openId = null;
  fake.edits.footageHere = false;
  fake.edits.error = null;
  fake.clips.fromSite = false;
  fake.refuses = [];
  fake.clips.doc = { kind: "vod", id: 1 };
  fake.clips.shown = [];
  fake.clips.error = null;
  fake.cuts.editing = null;
  fake.cuts.length = 0;
  fake.cuts.selection = null;
  fake.cuts.selectedEdge = "x";
  page.boxes = {};
  page.focusIn = null;
  page.dialog = false;
}

const pane = { left: 0, right: 300, top: 0, bottom: 900 };
const bar = { left: 300, right: 1400, top: 600, bottom: 660 };
/** The page as with a VOD open: library pane, clip bar, timeline, and a clip bar of clips 1 and 2. */
function withVodOpen() {
  fake.clips.shown = shown([1, 2]);
  fake.cuts.doc = {};
  fake.cuts.length = 120;
  page.boxes = { ".library-pane": pane, ".clip-bar": bar, ".scrubber": pane };
}

// bulk.ts `whereKeys`: "'Over' is where the pointer is; before the pointer has moved, where the focus is" (where.ts).
// First in this file: the pointer, once moved, stays where it was last.
test("before the pointer has moved, Ctrl+A goes by the focus", () => {
  fresh();
  withVodOpen();
  page.focusIn = ".clip-bar";
  assert.equal(bulk.whereKeys(), "clips");
  page.focusIn = ".library-list";
  assert.equal(bulk.whereKeys(), "library");
  page.focusIn = null;
  assert.equal(bulk.whereKeys(), "timeline");
});

// bulk.ts `chosenSegments`: "The stretches selected (or of `items`), in the order they were."
// CHANGELOG 0.6.0: "C (or Clip) makes a clip of each [stretch] in the order you picked them".
test("the stretches chosen are those picked, in the order picked, clips left out", () => {
  fresh();
  snippets.set([segment(5, 6), clip(2), segment(1, 2)]);
  assert.deepEqual(bulk.chosenSegments(), [segment(5, 6), segment(1, 2)]);
  assert.deepEqual(bulk.chosenSegments([clip(1)]), []);
});

// bulk.ts `chosenClips`: "The clips selected ... that are still in the clip bar, in the order they were."
test("the clips chosen are those still on the clip bar, in the order picked", () => {
  fresh();
  fake.clips.shown = shown([1, 2, 3]);
  snippets.set([clip(3), segment(1, 2), clip(9), clip(1)]);
  assert.deepEqual(bulk.chosenClips(), [3, 1]);
});

// bulk.ts `chosenPuts`: "of a VOD, its stretches (a clip's, with its name); of an edit, stretches of its timeline and the clips of its clip bar as they are."
// CHANGELOG 0.6.0: "drag them into an edit together"; "in the order you picked them".
test("what's picked on a VOD goes into an edit as its stretches, a clip's with its name, in the order picked", () => {
  fresh();
  fake.clips.shown = shown([2]);
  snippets.set([clip(2), segment(1, 3), clip(99)]);
  assert.deepEqual(bulk.chosenPuts(), [
    { kind: "range", vodId: 1, start: 20, end: 25, title: "Clip 2" },
    { kind: "range", vodId: 1, start: 1, end: 3 },
  ]);
});

test("what's picked on an edit goes in as spans of its timeline and the clips of its bar as they are", () => {
  fresh();
  fake.clips.doc = { kind: "edit", id: 4 };
  fake.clips.shown = shown([2]);
  snippets.set([segment(1, 3), clip(2)]);
  assert.deepEqual(bulk.chosenPuts(), [
    { kind: "span", clip: 4, from: 1, to: 3 },
    { kind: "clip", id: 2 },
  ]);
});

test("with nothing open, nothing is chosen to put", () => {
  fresh();
  fake.clips.doc = null;
  snippets.set([segment(1, 3)]);
  assert.deepEqual(bulk.chosenPuts(), []);
});

// bulk.ts `chosenEntries`: "What's picked in the library, in the order picked, as an edit takes it: each VOD whole, each edit as it is."
// CHANGELOG 0.6.0: "a VOD picked in the library goes in whole, as one piece" (0.7.0 Drag what you picked into an edit).
test("the rows picked go into an edit, a VOD whole and an edit as it is, in the order picked", () => {
  fresh();
  library.set([edit(2), vod(1), vod(3)]);
  assert.deepEqual(bulk.chosenEntries(), [
    { kind: "clip", id: 2 },
    { kind: "vod", id: 1 },
    { kind: "vod", id: 3 },
  ]);
});

// bulk.ts `transcribable`: "nothing asked of it, and not all of it done".
test("a VOD can be transcribed from its row when nothing is asked of it and not all of it is done", () => {
  assert.equal(bulk.transcribable(/** @type {any} */ ({ status: "none", transcribed: "none" })), true);
  assert.equal(bulk.transcribable(/** @type {any} */ ({ status: "none", transcribed: "some" })), true);
  assert.equal(bulk.transcribable(/** @type {any} */ ({ status: "none", transcribed: "all" })), false);
  assert.equal(bulk.transcribable(/** @type {any} */ ({ status: "running", transcribed: "none" })), false);
});

// bulk.ts `chosenToTranscribe`: "The VODs picked in the library (of `rows`) that can be transcribed, in the order picked."
test("of the rows picked, the VODs that can be transcribed, in the order picked; edits and others are left", () => {
  fresh();
  const rows = [row(1), row(2, { transcribed: "all" }), editRow(1), row(3), row(4, { status: "queued" })];
  library.set([vod(3), edit(1), vod(2), vod(4), vod(1), vod(77)]);
  assert.deepEqual(
    bulk.chosenToTranscribe(/** @type {any} */ (rows)).map((v) => v.id),
    [3, 1],
  );
});

// bulk.ts `transcribe`: "the rest of it, if some is done; else all of it."
// CHANGELOG 0.6.0: "One partly transcribed says how much", and "Transcribe the rest does what's left".
test("transcribing each of those picked asks for them in order, the rest of one part done and all of the others, and the set goes", async () => {
  fresh();
  library.set([vod(1), vod(2)]);
  await bulk.transcribeChosen(/** @type {any} */ ([row(2).vod, row(1, { transcribed: "some" }).vod]), () =>
    assert.fail("no error"),
  );
  assert.deepEqual(fake.asked, [
    ["all", 2, 0, null],
    ["rest", 1],
  ]);
  assert.equal(library.size, 0);
});

test("one that can't be asked for says so by its title, and the rest are asked for", async () => {
  fresh();
  fake.refuses = [2];
  /** @type {string[]} */
  const said = [];
  await bulk.transcribeChosen(/** @type {any} */ ([row(1).vod, row(2).vod, row(3).vod]), (m) => said.push(m));
  assert.deepEqual(said, ["Couldn't transcribe “VOD 2”: Error: no room"]);
  assert.deepEqual(
    fake.asked.map((a) => a[1]),
    [1, 3],
  );
});

// bulk.ts `clipSegments`: "A clip in the clip bar of each stretch selected, in the order they were, as one change; the set then goes."
test("C makes a clip of each stretch picked, in the order picked, and the set goes", async () => {
  fresh();
  snippets.set([segment(5, 6), clip(2), segment(1, 2)]);
  await bulk.clipSegments();
  assert.deepEqual(fake.exported, [
    [
      { start: 5, end: 6 },
      { start: 1, end: 2 },
    ],
  ]);
  assert.equal(snippets.size, 0);
});

test("C with no stretch picked makes nothing and leaves the clips picked", async () => {
  fresh();
  fake.clips.shown = shown([2]);
  snippets.set([clip(2)]);
  await bulk.clipSegments();
  assert.deepEqual(fake.exported, []);
  assert.equal(snippets.size, 1);
});

test("in an edit the clips are made on its clip bar", async () => {
  fresh();
  fake.clips.doc = { kind: "edit", id: 4 };
  snippets.set([segment(1, 2)]);
  await bulk.clipSegments();
  assert.deepEqual(fake.madeAll, [[{ start: 1, end: 2 }]]);
  assert.deepEqual(fake.exported, []);
});

// bulk.ts `removeChosenClips`: "Takes the clips selected ... off their clip bar, as one change; the set then goes."
// CHANGELOG 0.6.0: "Delete takes the picked clips out of the clip bar ... in one go that a single undo brings back."
test("Delete takes the clips picked off the bar, says how many with an Undo, and the set goes", async () => {
  fresh();
  fake.clips.shown = shown([1, 2, 3]);
  snippets.set([clip(3), clip(1)]);
  await bulk.removeChosenClips();
  assert.deepEqual(fake.deleted, [[3, 1]]);
  assert.deepEqual(fake.toasts, ["2 clips deleted"]);
  assert.equal(snippets.size, 0);
});

test("a clip that can't be deleted leaves the error for the clip bar", async () => {
  fresh();
  fake.clips.shown = shown([13]);
  snippets.set([clip(13)]);
  await bulk.removeChosenClips();
  assert.equal(fake.clips.error, "Error: locked");
  assert.deepEqual(fake.toasts, []);
});

test("Delete with no clip picked, or nothing open, deletes nothing", async () => {
  fresh();
  snippets.set([segment(1, 2)]);
  await bulk.removeChosenClips();
  fake.clips.doc = null;
  fake.clips.shown = shown([1]);
  snippets.set([clip(1)]);
  await bulk.removeChosenClips();
  assert.deepEqual(fake.deleted, []);
});

// bulk.ts `selectionKey`: "Escape lets go of a set"; CHANGELOG 0.6.0: "Escape, or a click on empty space, lets them go".
test("Escape lets go of both sets and is taken", () => {
  fresh();
  fake.clips.shown = shown([1, 2]);
  snippets.set([clip(1), clip(2)]);
  assert.equal(
    bulk.selectionKey(key("Escape"), [], () => {}),
    true,
  );
  assert.equal(snippets.size, 0);
  library.set([vod(1), vod(2)]);
  assert.equal(
    bulk.selectionKey(key("Escape"), [], () => {}),
    true,
  );
  assert.equal(library.size, 0);
});

// bulk.ts `selectionKey`: "The library's open row alone isn't a set: Escape goes on to what's next."
test("Escape with nothing picked, or only the open row, goes on to what's next", () => {
  fresh();
  assert.equal(
    bulk.selectionKey(key("Escape"), [], () => {}),
    false,
  );
  library.follow(() => vod(1));
  library.set([vod(1)]);
  assert.equal(
    bulk.selectionKey(key("Escape"), [], () => {}),
    false,
  );
  assert.equal(library.size, 1);
});

// bulk.ts `selectionKey`: "Innermost first: a mark's title being written, or a menu open, has it."
test("Escape in a mark's title or with a dialog open is theirs, and leaves the set", () => {
  fresh();
  snippets.set([clip(1), clip(2)]);
  fake.cuts.editing = 3;
  assert.equal(
    bulk.selectionKey(key("Escape"), [], () => {}),
    false,
  );
  fake.cuts.editing = null;
  page.dialog = true;
  assert.equal(
    bulk.selectionKey(key("Escape"), [], () => {}),
    false,
  );
  assert.equal(snippets.size, 2);
});

test("Escape with Ctrl, Alt or ⌘ held is the system's", () => {
  fresh();
  snippets.set([clip(1), clip(2)]);
  assert.equal(
    bulk.selectionKey(key("Escape", { altKey: true }), [], () => {}),
    false,
  );
  assert.equal(snippets.size, 2);
});

// CHANGELOG 0.7.1: "over the clip bar it still selects all the clips, and over the library its list".
test("Ctrl+A over the clip bar picks all its clips, over the library's pane all its rows, in their order", () => {
  fresh();
  withVodOpen();
  bulk.pointerAt(800, 630);
  assert.equal(
    bulk.selectionKey(key("select.all"), [], () => {}),
    true,
  );
  assert.deepEqual(snippets.items, [clip(1), clip(2)]);
  bulk.pointerAt(100, 300);
  const rows = /** @type {any} */ ([row(5), editRow(2), row(3)]);
  assert.equal(
    bulk.selectionKey(key("select.all"), rows, () => {}),
    true,
  );
  assert.deepEqual(library.items, [vod(5), edit(2), vod(3)]);
});

// CHANGELOG 0.7.1: "Ctrl+A (⌘A) over the player, the timeline or the transcript selects the whole timeline, ready to copy".
test("Ctrl+A over the main side selects the whole timeline, from its start to its end, and picks no set", () => {
  fresh();
  withVodOpen();
  fake.cuts.selectedEdge = "x";
  bulk.pointerAt(800, 300);
  assert.equal(
    bulk.selectionKey(key("select.all"), [], () => {}),
    true,
  );
  assert.deepEqual(fake.cuts.selection, { start: 0, end: 120 });
  assert.equal(fake.cuts.selectedEdge, null);
  assert.equal(snippets.size + library.size, 0);
});

// CHANGELOG 0.7.0 Fixes: "Ctrl+A doesn't select the page's text. Over places such as the library page's main side, Ctrl+A (⌘A) selected all the text on the page."
test("Ctrl+A with nothing open on the main side is the app's and selects nothing", () => {
  fresh();
  page.boxes = { ".library-pane": pane };
  bulk.pointerAt(800, 300);
  assert.equal(
    bulk.selectionKey(key("select.all"), [], () => {}),
    true,
  );
  assert.equal(fake.cuts.selection, null);
  assert.equal(snippets.size + library.size, 0);
});

test("a timeline with no length isn't one to select", () => {
  fresh();
  withVodOpen();
  fake.cuts.length = 0;
  bulk.pointerAt(800, 300);
  bulk.selectionKey(key("select.all"), [], () => {});
  assert.equal(fake.cuts.selection, null);
});

// bulk.ts `selectionKey`: "Not the open row alone: Delete then is the player's or the edit's."
// CHANGELOG 0.6.0: "Delete ... in the library, the VODs and edits selected, after one question."
test("Delete with rows picked removes them, naming those picked; with the open row alone it isn't taken", () => {
  fresh();
  const rows = /** @type {any} */ ([row(1), editRow(2), row(3)]);
  library.follow(() => vod(1));
  library.set([vod(1)]);
  assert.equal(
    bulk.selectionKey(key("library.remove"), rows, () => {}),
    false,
  );
  library.set([vod(1), edit(2)]);
  assert.equal(
    bulk.selectionKey(key("library.remove"), rows, () => {}),
    true,
  );
  assert.equal(fake.removed.length, 1);
  assert.deepEqual(
    fake.removed[0].vods.map((v) => v.id),
    [1],
  );
  assert.deepEqual(
    fake.removed[0].gone.map((e) => e.id),
    [2],
  );
});

test("Delete held down asks the question once", () => {
  fresh();
  library.set([vod(1), vod(2)]);
  const rows = /** @type {any} */ ([row(1), row(2)]);
  assert.equal(
    bulk.selectionKey(key("library.remove"), rows, () => {}),
    true,
  );
  assert.equal(
    bulk.selectionKey(key("library.remove", { repeat: true }), rows, () => {}),
    true,
  );
  assert.equal(fake.removed.length, 1);
});

test("once the question is answered the set goes", () => {
  fresh();
  library.set([vod(1), vod(2)]);
  bulk.selectionKey(key("library.remove"), /** @type {any} */ ([row(1), row(2)]), () => {});
  fake.removed[0].o.confirmed();
  assert.equal(library.size, 0);
});

test("a key that isn't one for a set isn't taken", () => {
  fresh();
  library.set([vod(1), vod(2)]);
  assert.equal(
    bulk.selectionKey(key("play.toggle"), [], () => {}),
    false,
  );
});

const box = /** @type {DOMRect} */ (/** @type {unknown} */ ({ left: 1 }));

// bulk.ts `carrySnippets`: "Carries `items` (stretches of the open timeline, clips of its clip bar)".
// CHANGELOG 0.7.0: "Dragging several shows a pile ... and a count such as 3 stretches".
test("carrying several says how many of each, takes them in the order picked and gives where it began", async () => {
  fresh();
  fake.clips.shown = shown([2]);
  const items = [segment(5, 6), { kind: /** @type {const} */ ("clip"), id: 2 }, segment(1, 2)];
  const home = /** @type {any} */ ({ home: true });
  await bulk.carrySnippets(items, box, 11, 12, home);
  const [carried] = fake.carried;
  assert.equal(fake.carried.length, 1);
  assert.equal(carried.label, "2 stretches and 1 clip");
  assert.deepEqual(carried.items, bulk.chosenPuts(items));
  assert.deepEqual([carried.from, carried.x, carried.y, carried.home, carried.vodId], [box, 11, 12, home, 1]);
  assert.deepEqual(carried.stretches, [
    { start: 5, end: 6 },
    { start: 1, end: 2 },
  ]);
});

// CHANGELOG 0.7.0: "one thing picked by itself drags as if it weren't picked".
test("one stretch carried alone is said as a stretch is, not by a count", async () => {
  fresh();
  await bulk.carrySnippets([segment(5, 6)], box, 0, 0);
  assert.equal(fake.carried[0].label, "stretch 5-6");
  fake.carried.length = 0;
  fake.clips.shown = shown([2]);
  await bulk.carrySnippets([clip(2)], box, 0, 0);
  assert.equal(fake.carried[0].label, "stretch 20-25");
});

// CHANGELOG 0.7.0: "with up to two more stacked behind it and blank cards past three".
test("only the first three are shown as the pile's faces, a VOD's by its stretch or its clip's", async () => {
  fresh();
  fake.clips.shown = shown([2]);
  await bulk.carrySnippets([segment(1, 2), clip(2), segment(3, 4), segment(5, 6)], box, 0, 0);
  assert.deepEqual(fake.carried[0].faces, [
    { vodId: 1, start: 1, end: 2 },
    { vodId: 1, start: 20, end: 25 },
    { vodId: 1, start: 3, end: 4 },
  ]);
  // A clip no longer on the bar has no face, and takes none of the three.
  fake.carried.length = 0;
  await bulk.carrySnippets([clip(8), segment(1, 2)], box, 0, 0);
  assert.deepEqual(fake.carried[0].faces, [{ vodId: 1, start: 1, end: 2 }]);
});

// bulk.ts `faceOf`: of an edit's stretch, the piece of the edit it starts in, only while that edit is open.
test("on an edit a stretch's face is where the edit puts it, and none once the edit isn't open", async () => {
  fresh();
  fake.clips.doc = { kind: "edit", id: 4 };
  fake.edits.openId = 4;
  await bulk.carrySnippets([segment(1, 2)], box, 0, 0);
  assert.deepEqual(fake.carried[0].faces, [{ inEdit: true, start: 1, end: 2 }]);
  assert.deepEqual(fake.carried[0].stretches, []);
  assert.equal(fake.carried[0].vodId, undefined);
  fake.carried.length = 0;
  fake.edits.openId = 5;
  await bulk.carrySnippets([segment(1, 2)], box, 0, 0);
  assert.deepEqual(fake.carried[0].faces, []);
});

// bulk.ts `carrySnippets`: "Made as the carry begins where the footage is here".
// CHANGELOG 0.7.0 (clips): "A site's is downloaded once the drag leaves the window".
test("a VOD's clips are made at once when its video is here, an edit's when all its footage is", async () => {
  fresh();
  await bulk.carrySnippets([segment(1, 2)], box, 0, 0);
  assert.equal(fake.carried[0].early, true);
  fake.clips.fromSite = true;
  await bulk.carrySnippets([segment(1, 2)], box, 0, 0);
  assert.equal(fake.carried[1].early, false);
  fake.clips.doc = { kind: "edit", id: 4 };
  fake.edits.footageHere = true;
  await bulk.carrySnippets([segment(1, 2)], box, 0, 0);
  assert.equal(fake.carried[2].early, true);
  fake.edits.footageHere = false;
  await bulk.carrySnippets([segment(1, 2)], box, 0, 0);
  assert.equal(fake.carried[3].early, false);
});

test("carrying nothing, or with nothing open, carries nothing", async () => {
  fresh();
  await bulk.carrySnippets([], box, 0, 0);
  fake.clips.doc = null;
  await bulk.carrySnippets([segment(1, 2)], box, 0, 0);
  snippets.set([]);
  await bulk.carryChosen(box, 0, 0);
  assert.deepEqual(fake.carried, []);
});

test("carrying what's picked carries the set in the order picked", async () => {
  fresh();
  snippets.set([segment(5, 6), segment(1, 2)]);
  await bulk.carryChosen(box, 3, 4);
  assert.deepEqual(fake.carried[0].items, [
    { kind: "range", vodId: 1, start: 5, end: 6 },
    { kind: "range", vodId: 1, start: 1, end: 2 },
  ]);
});

// bulk.ts `chosenFiles`: "a clip's own, a stretch's made for the drag (not in the clip bar)".
// CHANGELOG 0.7.0: "Several clips drag out in one go. Picked clips dragged out ... take all their files in one drag".
test("the files carried are each clip's own and each stretch's made for the drag, in the order picked", async () => {
  fresh();
  fake.clips.shown = shown([2]);
  await bulk.carrySnippets([segment(5, 6), clip(2), segment(1, 2)], box, 0, 0);
  const files = await fake.carried[0].files();
  assert.deepEqual(
    files.map((/** @type {{ id: number }} */ f) => f.id),
    [1005, 2, 1001],
  );
  assert.deepEqual(fake.pulls, [
    [5, 6, false],
    [1, 2, false],
  ]);
});

test("on an edit a stretch's file is made on its clip bar's account, not shown there", async () => {
  fresh();
  fake.clips.doc = { kind: "edit", id: 4 };
  await bulk.carrySnippets([segment(5, 6)], box, 0, 0);
  const files = await fake.carried[0].files();
  assert.deepEqual(fake.made, [[5, 6, false]]);
  assert.deepEqual(files, [{ id: 2005, shown: false }]);
});

// bulk.ts `chosenFiles`: "None if one of them can't be made (those made go again)."
test("if one file can't be made none are carried, and the ones made for the drag go again", async () => {
  fresh();
  fake.clips.doc = { kind: "edit", id: 4 };
  fake.clips.shown = shown([2]);
  await bulk.carrySnippets([segment(5, 6), clip(2), segment(99, 100)], box, 0, 0);
  assert.deepEqual(await fake.carried[0].files(), []);
  await new Promise((done) => setImmediate(done));
  // The stretch made goes; the clip that was on the bar already stays.
  assert.deepEqual(fake.forgotten, [2005]);
  // A clip gone from the bar can't be carried either.
  fake.carried.length = 0;
  await bulk.carrySnippets([clip(77), segment(1, 2)], box, 0, 0);
  assert.deepEqual(await fake.carried[0].files(), []);
});

// bulk.ts `carryEntries`: "Carries the rows picked in the library from (`x`, `y`) ... into an edit".
// CHANGELOG 0.6.0: "a VOD picked in the library goes in whole"; bulk.ts `chosenEntries`.
test("carrying rows says how many VODs and edits, takes each VOD whole and an edit by its first piece", async () => {
  fresh();
  library.set([vod(1), edit(2), vod(3), edit(7)]);
  bulk.carryEntries(box, 5, 6);
  const [carried] = fake.carried;
  assert.equal(carried.label, "2 VODs and 2 edits");
  assert.deepEqual(carried.items, bulk.chosenEntries());
  assert.deepEqual(carried.stretches, []);
  assert.deepEqual([carried.from, carried.x, carried.y], [box, 5, 6]);
  // The first three show: a VOD at its start, an edit by what the backend says of it (one gone shows nothing).
  assert.deepEqual(await carried.faces, [
    { vodId: 1, start: 0 },
    { inEdit: true, start: 0, end: Infinity },
    { vodId: 3, start: 0 },
  ]);
});

test("carrying one VOD says 1 VOD, and no row picked carries nothing", async () => {
  fresh();
  library.set([vod(1)]);
  bulk.carryEntries(box, 0, 0);
  assert.equal(fake.carried[0].label, "1 VOD");
  fake.carried.length = 0;
  library.set([]);
  bulk.carryEntries(box, 0, 0);
  assert.deepEqual(fake.carried, []);
});

test("an edit the backend can't show has no face, and the others still do", async () => {
  fresh();
  library.set([edit(7), vod(3)]);
  bulk.carryEntries(box, 0, 0);
  assert.deepEqual(await fake.carried[0].faces, [{ vodId: 3, start: 0 }]);
});

// bulk.ts `addTo`: "Puts `items` (what's picked) in edit `editId`, called `title`, as one change ... Says so, with Undo."
// CHANGELOG 0.6.0: "Add to edit… ... puts them on the edit's timeline, at the end, or on its clip bar."
test("adding to an edit puts them there, says so with an Undo of that edit, on its clip bar if asked", async () => {
  fresh();
  const puts = /** @type {any[]} */ ([{ kind: "vod", id: 1 }]);
  await bulk.addTo(4, "Montage", false, puts);
  await bulk.addTo(4, "Montage", true, puts);
  assert.deepEqual(fake.put, [
    [4, puts, false],
    [4, puts, true],
  ]);
  assert.deepEqual(fake.toasts, ["Added to “Montage”", "Added to “Montage”, on its clip bar"]);
  fake.actions[0]?.run();
  assert.deepEqual(fake.undone, [{ kind: "edit", id: 4 }]);
  assert.equal(fake.actions[0]?.label, "Undo");
});

// bulk.ts `addTo`: "as a drop there would (not an edit into itself, nor its own stretches again on its timeline)".
test("an edit isn't added to itself, nor its own stretches again on its timeline, but they go on its clip bar", async () => {
  fresh();
  const self = /** @type {any} */ ({ kind: "clip", id: 4 });
  const span = /** @type {any} */ ({ kind: "span", clip: 4, from: 1, to: 2 });
  const other = /** @type {any} */ ({ kind: "span", clip: 5, from: 1, to: 2 });
  await bulk.addTo(4, "M", false, [self, span, other]);
  assert.deepEqual(fake.put[0].slice(1), [[other], false]);
  await bulk.addTo(4, "M", true, [self, span]);
  assert.deepEqual(fake.put[1].slice(1), [[span], true]);
  fake.put.length = 0;
  await bulk.addTo(4, "M", false, [self, span]);
  assert.deepEqual(fake.put, []);
});

test("adding that fails says why, once, with no Undo", async () => {
  fresh();
  fake.putOk = false;
  fake.edits.error = "disk full";
  await bulk.addTo(4, "M", false, /** @type {any} */ ([{ kind: "vod", id: 1 }]));
  assert.deepEqual(fake.toasts, ["Couldn't add to “M”: disk full"]);
  fake.toasts.length = 0;
  fake.edits.error = null;
  await bulk.addTo(4, "M", false, /** @type {any} */ ([{ kind: "vod", id: 1 }]));
  assert.deepEqual(fake.toasts, []);
});

// bulk.ts `removeChosenClips`: the Undo in the toast takes back the document's own change.
test("the Undo of clips deleted undoes in the document they were deleted from", async () => {
  fresh();
  fake.clips.shown = shown([1]);
  snippets.set([clip(1)]);
  await bulk.removeChosenClips();
  assert.equal(fake.actions[0]?.label, "Undo");
  fake.actions[0]?.run();
  assert.deepEqual(fake.undone, [{ kind: "vod", id: 1 }]);
});

test("one clip deleted is said as one, and the clip bar's hover is let go", async () => {
  fresh();
  fake.clips.shown = shown([1]);
  fake.clips.hovered = 1;
  snippets.set([clip(1)]);
  await bulk.removeChosenClips();
  assert.deepEqual(fake.toasts, ["1 clip deleted"]);
  assert.equal(fake.clips.hovered, null);
});

test("a stretch carried with a clip says both counts, each in its number", async () => {
  fresh();
  fake.clips.shown = shown([2]);
  await bulk.carrySnippets([segment(1, 2), clip(2), clip(2)], box, 0, 0);
  assert.equal(fake.carried[0].label, "1 stretch and 2 clips");
});

test("only the first three rows carried show a face", async () => {
  fresh();
  library.set([vod(1), vod(2), vod(3), vod(4)]);
  bulk.carryEntries(box, 0, 0);
  assert.equal((await fake.carried[0].faces).length, 3);
});

test("one edit carried says 1 edit", () => {
  fresh();
  library.set([edit(2)]);
  bulk.carryEntries(box, 0, 0);
  assert.equal(fake.carried[0].label, "1 edit");
});

test("adding keeps a VOD whose number is the edit's, and another edit's clip", async () => {
  fresh();
  const sameNumber = /** @type {any} */ ({ kind: "vod", id: 4 });
  const another = /** @type {any} */ ({ kind: "clip", id: 5 });
  await bulk.addTo(4, "M", false, [sameNumber, another]);
  assert.deepEqual(fake.put[0].slice(1), [[sameNumber, another], false]);
});

// bulk.ts `whereKeys`: "the timeline" is where a scrubber shows and a video with a length is open.
test("Ctrl+A over the main side selects the timeline only with a scrubber, a document and a length", () => {
  for (const [scrubber, doc, length] of [
    [false, {}, 120],
    [true, null, 120],
    [true, {}, 0],
  ]) {
    fresh();
    withVodOpen();
    if (!scrubber) delete page.boxes[".scrubber"];
    fake.cuts.doc = doc;
    fake.cuts.length = /** @type {number} */ (length);
    bulk.pointerAt(800, 300);
    assert.equal(bulk.whereKeys(), null, JSON.stringify([scrubber, doc, length]));
  }
});

test("with nothing focused, the keys go by the pointer alone", () => {
  fresh();
  withVodOpen();
  const document = /** @type {any} */ (globalThis.document);
  const was = document.activeElement;
  document.activeElement = null;
  bulk.pointerAt(800, 300);
  assert.equal(bulk.whereKeys(), "timeline");
  document.activeElement = was;
});
