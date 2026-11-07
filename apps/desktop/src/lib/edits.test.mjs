// The edits store (edits.svelte.ts), with the app faked: compiled as the
// app's build does (types stripped, then Svelte's runes); each call to the
// app answers when the test says, and the test sends the app's events.
// node --test apps/desktop/src/lib/edits.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// edits.svelte.ts's imports from the app are the fakes below; media.ts is the .ts file it is.
const fakes = {
  "./api": `export const api = globalThis.fake.api; export const events = globalThis.fake.events;`,
  "./nav.svelte": `export const nav = { forget() {} };`,
  "./clips.svelte": `export const clips = { reload() {} };`,
  "./cuts.svelte": `export const cuts = { doc: null, isOf: () => false, clearSelection() {} };`,
  "./toasts.svelte": `export const toast = { error: () => 0, drop() {} };`,
  "./writes": `export const saving = (change) => change;`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/edits.svelte.ts") && fakes[specifier])
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

/** The events the store listens to, by name, and the answers held back.
 *  @type {Record<string, (value: any) => void>} */
const heard = {};
/** @type {Map<string, (value: unknown) => void>} */
const held = new Map();
/** A call to the app answered once the test says (`answer`). @param {string} name */
const later = (name) => () => new Promise((resolve) => held.set(name, resolve));
/** @param {string} name @param {unknown} value */
const answer = (name, value) => held.get(name)?.(value);
/** @param {string} name */
const on = (name) => (/** @type {(value: any) => void} */ cb) => ((heard[name] = cb), Promise.resolve(() => {}));
globalThis.fake = {
  api: { listEdits: later("listEdits"), renameEdit: later("renameEdit"), getEdit: later("getEdit") },
  events: {
    edit: on("edit"),
    editProgress: on("editProgress"),
    editRemoved: on("editRemoved"),
    clipRemoved: on("clipRemoved"),
    footageProgress: on("footageProgress"),
    footage: on("footage"),
    vodRemoved: on("vodRemoved"),
  },
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
/** @param {number} id @param {string} status */
const edit = (id, status, title = "Edit") => ({ id, title, status, snippet_of: null, vod_ids: [] });

const { edits } = await import("./edits.svelte.ts");

test("news of an edit that came before the list answers isn't undone by the list", async () => {
  edits.start();
  // Read as rendering; its render's "ready" reaches the page first.
  heard.edit(edit(1, "ready"));
  answer("listEdits", [edit(1, "cutting"), edit(2, "none")]);
  await settle();
  assert.deepEqual(
    edits.list.map((e) => [e.id, e.status]),
    [
      [1, "ready"],
      [2, "none"],
    ],
  );
});

test("a rename's answer that comes after the render's ready doesn't show it rendering again", async () => {
  heard.edit(edit(3, "cutting"));
  const renamed = edits.rename(3, "Renamed");
  heard.edit(edit(3, "ready", "Renamed"));
  answer("renameEdit", { edit: edit(3, "cutting", "Renamed"), pieces: [] });
  assert.equal(await renamed, true);
  assert.deepEqual(
    edits.list.filter((e) => e.id === 3).map((e) => [e.status, e.title]),
    [["ready", "Renamed"]],
    "the row shows rendering with no progress",
  );
});

test("an answer that comes after the edit's deletion doesn't bring its row back", async () => {
  heard.edit(edit(4, "none"));
  const renamed = edits.rename(4, "Renamed");
  heard.editRemoved(4);
  answer("renameEdit", { edit: edit(4, "none", "Renamed"), pieces: [] });
  await renamed;
  assert.ok(!edits.list.some((e) => e.id === 4), "a deleted edit's row is back");
});

test("an answer with no news since is shown, pieces and all", async () => {
  edits.openEdit(5);
  answer("getEdit", { edit: edit(5, "none"), pieces: [{ id: 50 }] });
  await settle();
  const renamed = edits.rename(5, "Renamed");
  answer("renameEdit", { edit: edit(5, "none", "Renamed"), pieces: [{ id: 51 }] });
  await renamed;
  assert.equal(edits.list.find((e) => e.id === 5)?.title, "Renamed");
  assert.deepEqual(
    edits.pieces.map((p) => p.id),
    [51],
  );
});
