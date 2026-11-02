// Ctrl+X (copyPaste.ts, with bulk.ts as it is), the app faked: the copy
// answers when the test says. node --test apps/desktop/src/lib/copyPaste.test.mjs
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
    "./where": `export const where = () => null;`,
  },
};
const hooks = `
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  const parent = Object.keys(fakes).find((f) => context.parentURL?.endsWith(f));
  if (parent && fakes[parent][specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[parent][specifier]), shortCircuit: true };
  if (parent === "/copyPaste.ts" && specifier === "./bulk") return next("./bulk.ts", context);
  return next(specifier, context);
}`;
register("data:text/javascript," + encodeURIComponent(hooks));

/** Lets the copy in flight answer. */
let answer = () => {};
const fake = {
  /** The clips deleted, by each call. @type {number[][]} */
  deleted: [],
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
  },
  clips: {
    /** @type {{ kind: string, id: number } | null} */
    doc: { kind: "vod", id: 1 },
    /** @type {{ id: number }[]} */
    shown: [],
    hovered: null,
    error: null,
  },
  cuts: { doc: null, selection: null, selected: null, clearSelection() {} },
  edits: { openId: null },
  snippets: {
    /** @type {{ kind: string, id: number }[]} */
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
Object.assign(globalThis, { fake, window: { getSelection: () => null } });
const { clipboardKey } = await import("./copyPaste.ts");

/** VOD 1 open, its clips 1, 2 and 3 on the clip bar, `picked` of them picked. @param {number[]} picked */
function reset(picked) {
  fake.deleted.length = 0;
  fake.undone.length = 0;
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
  assert.deepEqual(fake.deleted, [[1, 3]]);
  assert.deepEqual(fake.snippets.items, []);
  fake.toasts.at(-1)?.action?.run();
  assert.deepEqual(fake.undone, [{ kind: "vod", id: 1 }]);
});

test("Ctrl+X takes out the clips it copied, not another clicked while the copy is made", async () => {
  reset([1]);
  cut();
  fake.snippets.items = [{ kind: "clip", id: 2 }];
  await copied();
  assert.deepEqual(fake.deleted, [[1]]);
});

test("Ctrl+X takes out the clips it copied, not those of another VOD opened meanwhile", async () => {
  reset([1]);
  cut();
  fake.clips.doc = { kind: "vod", id: 2 };
  fake.clips.shown = [{ id: 7 }];
  fake.snippets.items = [{ kind: "clip", id: 7 }];
  await copied();
  assert.deepEqual(fake.deleted, [[1]]);
  fake.toasts.at(-1)?.action?.run();
  assert.deepEqual(fake.undone, [{ kind: "vod", id: 1 }]);
});
