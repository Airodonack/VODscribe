// The clips store (clips.svelte.ts), with the app faked: compiled as the
// app's build does (types stripped, then Svelte's runes); each call to the
// app answers when the test says, and the test sends the app's events.
// node --test apps/desktop/src/lib/clips.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// clips.svelte.ts's imports from the app are the fakes below; where.ts is the .ts file it is.
const fakes = {
  "./api": `export const api = globalThis.fake.api; export const events = globalThis.fake.events;
    export const docKey = (doc) => doc.kind + ":" + doc.id;`,
  "./permission": `export const hideNote = () => {}; export const noteHidden = () => true;`,
  "./selection.svelte": `export const snippets = { items: [], set() {} };`,
  "./toasts.svelte": `export const toast = { error: () => 0, drop() {} };`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/clips.svelte.ts") && fakes[specifier])
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
// The page's window: only listened to.
Object.assign(globalThis, { window: { addEventListener() {} } });
globalThis.fake = {
  api: {
    forgetGoneClips: async () => {},
    listClips: later("listClips"),
    renameClip: later("renameClip"),
  },
  events: { clip: on("clip"), clipProgress: on("clipProgress"), clipRemoved: on("clipRemoved") },
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
/** @param {number} id @param {string} status */
const clip = (id, status, title = "Clip") => ({
  id,
  vod_id: 1,
  bar: null,
  start: 1,
  end: 2,
  title,
  status,
  shown: true,
});
const pills = () => clips.list.map((c) => [c.id, c.status]);

const { clips } = await import("./clips.svelte.ts");

test("news of a clip that came before the bar's list answers isn't undone by the list", async () => {
  clips.open(1, false);
  await settle();
  // Listed while it was cut; its "ready" reaches the page first.
  heard.clip(clip(1, "ready"));
  answer("listClips", [clip(1, "cutting"), clip(2, "ready")]);
  await settle();
  assert.deepEqual(pills(), [
    [1, "ready"],
    [2, "ready"],
  ]);
});

test("an answer that comes after the clip's deletion doesn't bring its pill back", async () => {
  const renamed = clips.rename(2, "Renamed");
  heard.clipRemoved(2);
  answer("renameClip", clip(2, "ready", "Renamed"));
  await renamed;
  assert.deepEqual(pills(), [[1, "ready"]], "a deleted clip's pill is back");
});

test("a list that comes after a clip's deletion doesn't bring its pill back", async () => {
  clips.reload({ kind: "vod", id: 1 });
  heard.clipRemoved(1);
  answer("listClips", [clip(1, "ready")]);
  await settle();
  assert.deepEqual(pills(), []);
});

test("an answer with no news since is shown", async () => {
  heard.clip(clip(3, "ready"));
  const renamed = clips.rename(3, "Renamed");
  answer("renameClip", clip(3, "ready", "Renamed"));
  await renamed;
  assert.deepEqual(
    clips.list.map((c) => c.title),
    ["Renamed"],
  );
});
