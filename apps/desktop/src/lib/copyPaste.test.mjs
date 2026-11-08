// Ctrl+X and Ctrl+V (copyPaste.ts, with bulk.ts and where.ts as they are),
// the app faked: the copy answers when the test says.
// node --test apps/desktop/src/lib/copyPaste.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// copyPaste.ts's imports from the app, and bulk.ts's, are the fakes below,
// each kept on globalThis.fake for the test to read; bulk is the .ts file it is.
const both = {
  "./api": `export const api = globalThis.fake.api;`,
  "./clips.svelte": `export const clips = globalThis.fake.clips;`,
  "./cuts.svelte": `export const cuts = globalThis.fake.cuts;`,
  "./edits.svelte": `export const edits = globalThis.fake.edits;`,
  "./keys.svelte": `export const is = (e, id) => e.id === id; export const bindingsOf = () => [];`,
  "./selection.svelte": `export const snippets = globalThis.fake.snippets;
    export const library = globalThis.fake.library;
    export const count = (parts) => parts.filter(([n]) => n > 0).map(([n, one, many]) => n + " " + (n === 1 ? one : many)).join(" and ");`,
  "./toasts.svelte": `export const toast = globalThis.fake.toast;`,
  "./undo": `export const undoIn = (doc) => void globalThis.fake.undone.push(doc);`,
};
const fakes = {
  "/copyPaste.ts": {
    ...both,
    "@tauri-apps/plugin-clipboard-manager": `export const writeText = async () => {};`,
    svelte: `export const tick = async () => {};`,
    "./clipboardText": `export const copiedText = () => "copied"; export const copiedSaid = () => "Copied";
      export const pastedSaid = () => "Pasted";`,
    "./demo": `export const DEMO = false; export const keysForApp = () => true;`,
  },
  "/bulk.ts": {
    ...both,
    "./actions": `export const removeEntries = async () => {};`,
    "./dragout": `export const carrySet = () => {}; export const exportStretch = async () => null;
      export const exportStretches = async () => {}; export const stretchSaid = () => "";`,
    "./media": `export const inEdit = () => undefined;`,
    "./playerKeys": `export const isEscape = () => false;`,
    "./where": `export const where = () => globalThis.fake.where;`,
  },
};
const hooks = `
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  const parent = Object.keys(fakes).find((f) => context.parentURL?.endsWith(f));
  if (parent && fakes[parent][specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[parent][specifier]), shortCircuit: true };
  if (parent === "/copyPaste.ts" && (specifier === "./bulk" || specifier === "./where"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`;
register("data:text/javascript," + encodeURIComponent(hooks));

/** Lets the copy in flight answer. */
let answer = () => {};
const fake = {
  /** The clips deleted, by each call. @type {number[][]} */
  deleted: [],
  /** The clips cut (held for a paste), by each call. @type {number[][]} */
  cut: [],
  /** What each edit's removeFrom was asked: pieces, stretches, clips, cut. @type {unknown[][]} */
  removed: [],
  /** Where Undo was asked. @type {unknown[]} */
  undone: [],
  /** What the toasts said, with their action if any. @type {{ message: string, action?: { run: () => void } }[]} */
  toasts: [],
  toast: {
    /** @param {string} message @param {{ run: () => void }} [action] */
    show: (message, action) => void fake.toasts.push({ message, action }),
    /** @param {string} message */
    error: (message) => void fake.toasts.push({ message }),
  },
  api: {
    /** @param {unknown[]} items */
    copyItems: (items) => new Promise((done) => (answer = () => done(items.map(() => ({}))))),
    copiedText: async () => {},
    /** @param {number[]} ids */
    deleteClips: async (ids) => void fake.deleted.push(ids),
    /** @param {number[]} ids */
    cutClips: async (ids) => void fake.cut.push(ids),
    /** @param {unknown[]} args */
    pasteItems: async (...args) => void fake.pastes.push(args),
  },
  /** What each paste was asked: its target, on the bar or not, at, after, text. @type {unknown[][]} */
  pastes: [],
  /** Where Ctrl+V acts (where.ts `where`, faked). @type {string | null} */
  where: null,
  clips: {
    /** @type {{ kind: string, id: number } | null} */
    doc: { kind: "vod", id: 1 },
    /** @type {{ id: number }[]} */
    shown: [],
    hovered: null,
    error: null,
  },
  cuts: {
    /** @type {{ kind: string, id: number } | null} */
    doc: null,
    /** @type {{ start: number, end: number } | null} */
    selection: null,
    /** @type {number | null} */
    selected: null,
    /** @type {{ id: number, start: number }[]} */
    all: [],
    now: 0,
    length: 0,
    /** @param {{ kind: string, id: number } | null} doc */
    isOf(doc) {
      return !!doc && !!this.doc && doc.kind === this.doc.kind && doc.id === this.doc.id;
    },
    clearSelection() {},
  },
  edits: {
    /** @type {number | null} */
    openId: null,
    open: null,
    /** @param {unknown[]} args */
    removeFrom: async (...args) => (fake.removed.push(args), true),
  },
  snippets: {
    /** @type {({ kind: string, id: number } | { kind: string, start: number, end: number })[]} */
    items: [],
    get size() {
      return this.items.length;
    },
    clear() {
      this.items = [];
    },
  },
  library: { picking: false, items: [] },
};
Object.assign(globalThis, {
  fake,
  window: { getSelection: () => null },
  document: { querySelector: () => null, activeElement: null },
});
const { clipboardKey, clipboardPaste } = await import("./copyPaste.ts");

/** VOD 1 open, its clips 1, 2 and 3 on the clip bar, `picked` of them picked. @param {number[]} picked */
function reset(picked) {
  fake.deleted.length = 0;
  fake.cut.length = 0;
  fake.removed.length = 0;
  fake.undone.length = 0;
  fake.edits.openId = null;
  fake.toasts.length = 0;
  fake.clips.doc = { kind: "vod", id: 1 };
  fake.clips.shown = [{ id: 1 }, { id: 2 }, { id: 3 }];
  fake.snippets.items = picked.map((id) => ({ kind: "clip", id }));
}
/** Ctrl+X pressed (as keys.svelte.ts would match it): true if the app took it. */
const cut = () => clipboardKey(/** @type {any} */ ({ id: "edit.cut", repeat: false, target: null }), () => undefined);
/** The copy answers, and what follows it runs. */
async function copied() {
  answer();
  await new Promise((r) => setTimeout(r, 0));
}

test("Ctrl+X takes out the clips picked, with Undo for their clip bar", async () => {
  reset([1, 3]);
  assert.equal(cut(), true);
  await copied();
  assert.deepEqual(fake.cut, [[1, 3]]);
  assert.deepEqual(fake.snippets.items, []);
  fake.toasts.at(-1)?.action?.run();
  assert.deepEqual(fake.undone, [{ kind: "vod", id: 1 }]);
});

test("Ctrl+X takes out the clips it copied, not another clicked while the copy is made", async () => {
  reset([1]);
  cut();
  fake.snippets.items = [{ kind: "clip", id: 2 }];
  await copied();
  assert.deepEqual(fake.cut, [[1]]);
});

test("Ctrl+X takes out the clips it copied, not those of another VOD opened meanwhile", async () => {
  reset([1]);
  cut();
  fake.clips.doc = { kind: "vod", id: 2 };
  fake.clips.shown = [{ id: 7 }];
  fake.snippets.items = [{ kind: "clip", id: 7 }];
  await copied();
  assert.deepEqual(fake.cut, [[1]]);
  fake.toasts.at(-1)?.action?.run();
  assert.deepEqual(fake.undone, [{ kind: "vod", id: 1 }]);
});

// Owner, walkthrough 2026-11-07: "Shouldn't Ctrl-X be cut? As in it's
// intended to be pasted somewhere? So the video should not actually be
// deleted until the clipboard is erased (through quit or overwritten or etc)".
test("Ctrl+X holds the clips it takes off a VOD's bar for a paste, not deleted", async () => {
  reset([2]);
  cut();
  await copied();
  assert.deepEqual([fake.cut, fake.deleted], [[[2]], []]);
});

test("Ctrl+X holds the clips it takes off an edit's bar for a paste, not deleted", async () => {
  reset([2]);
  fake.clips.doc = { kind: "edit", id: 5 };
  fake.edits.openId = 5;
  cut();
  await copied();
  assert.deepEqual(fake.removed, [[[], [], [2], true]]);
  assert.deepEqual(fake.deleted, []);
});

/** Ctrl+V's paste, bringing what was copied: true if the app took it. */
function paste() {
  let taken = false;
  clipboardPaste(
    /** @type {any} */ ({
      defaultPrevented: false,
      target: null,
      clipboardData: { getData: () => "copied" },
      preventDefault: () => (taken = true),
    }),
  );
  return taken;
}

test("Ctrl+V on an edit's timeline goes after the stretch picked, not at the playhead", async () => {
  reset([]);
  fake.pastes.length = 0;
  fake.where = "timeline";
  fake.clips.doc = { kind: "edit", id: 5 };
  Object.assign(fake.cuts, { doc: { kind: "edit", id: 5 }, selection: null, selected: null, now: 0.5, length: 10 });
  // An Alt-click picked 2-3 s; the playhead stayed at 0.5 s.
  fake.snippets.items = [{ kind: "segment", start: 2, end: 3 }];
  assert.equal(paste(), true);
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(fake.pastes, [[{ kind: "edit", id: 5 }, false, 3, null, "copied"]]);
});
