// The open VOD's or edit's marks (cuts.svelte.ts), with the app faked: the
// module is compiled as the app's build does (types stripped, then Svelte's
// runes), and each call to the app answers when the test says.
// node --test apps/desktop/src/lib/cuts.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// cuts.svelte.ts's imports from the app are the fakes below; media.ts and
// the key helpers are the .ts files they are.
const fakes = {
  "./api": `export const api = globalThis.fake.api;
    export const docKey = (doc) => doc.kind + ":" + doc.id;`,
  "./keys.svelte": `export const is = () => false;`,
  "./selection.svelte": `export const clearsStretch = () => {};
    export const snippets = { clear() {} };
    export const stretchMade = () => {};`,
  "./layers.svelte": `export const layers = { marks: { own: true, imported: true }, showMarks() {} };`,
  "./toasts.svelte": `export const toast = { error: () => 0, drop() {} };`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/cuts.svelte.ts") && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.startsWith("file:"))
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

// The app: marks added get ids from 1 and are kept; nothing else is asked.
/** @type {[string, number][]} */
const sent = [];
/** @type {(() => void)[] | null} */
let held = null;
/** @type {(id: number, start: number, kind: string) => Promise<unknown>} */
const addMark = (id, start, kind) => answer({ id: sent.length + 1, start, kind, text: null }, ["add", start]);
globalThis.fake = {
  api: {
    listMarks: async () => [],
    editMarks: async () => [],
    addEditMark: addMark,
    addMark,
    moveMark: async () => {},
    deleteMark: async () => {},
  },
};
/**
 * Answers `value`, at once or once the test lets it (`hold`).
 * @param {unknown} value
 * @param {[string, number]} call
 */
function answer(value, call) {
  sent.push(call);
  const waiting = held;
  if (!waiting) return Promise.resolve(value);
  return new Promise((resolve) => waiting.push(() => resolve(value)));
}
const hold = () => (held = []);
const release = () => {
  const waiting = held;
  held = null;
  for (const go of waiting ?? []) go();
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const { cuts } = await import("./cuts.svelte.ts");
let opened = 0;
/** @param {"vod" | "edit"} kind */
async function fresh(kind = "edit") {
  cuts.openDoc({ kind, id: ++opened });
  await settle();
  sent.length = 0;
  cuts.fps = 30;
}

test("an edge of the selection on a join of the edit's pieces gets no cut", async () => {
  await fresh();
  // A piece 10.013 s long, then another: a selection to its end, on the 30 fps grid elsewhere.
  cuts.selection = { start: 5, end: 10.013 };
  await cuts.cutSelection(20, [10.013]);
  assert.deepEqual(sent, [["add", 5]], "a cut sent a sliver inside the earlier piece");
});

test("an edge nudged onto the other stops a frame short of it", async () => {
  await fresh();
  cuts.selection = { start: 1, end: 1 + 1 / 30 };
  cuts.selectedEdge = "end";
  cuts.nudgeSelected(-1, 30, 60);
  const s = cuts.selection;
  assert.ok(s.end > s.start, `a zero span: ${JSON.stringify(s)}`);
  cuts.selectedEdge = "start";
  cuts.nudgeSelected(1, 30, 60);
  assert.ok(cuts.selection.end > cuts.selection.start, `a zero span: ${JSON.stringify(cuts.selection)}`);
});

test("another VOD or edit opened goes back to the usual frame rate until its player says", async () => {
  await fresh();
  cuts.fps = 60;
  cuts.openDoc({ kind: "vod", id: ++opened });
  assert.equal(cuts.fps, 30);
});

test("B pressed again before the first cut is made makes one cut, not two on one frame", async () => {
  await fresh("vod");
  hold();
  const first = cuts.toggle(4, 30);
  const second = cuts.toggle(4, 30);
  release();
  await Promise.all([first, second]);
  await settle();
  assert.equal(sent.length, 1, `cuts asked for: ${JSON.stringify(sent)}`);
  assert.equal(cuts.list.length, 1);
});
