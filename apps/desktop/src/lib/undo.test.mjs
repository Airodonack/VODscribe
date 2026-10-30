// Undo and redo (undo.ts) after a change still being saved, with the app
// faked: each call answers when the test says.
// node --test apps/desktop/src/lib/undo.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

const fakes = {
  "./api": `export const api = globalThis.fake.api;`,
  "./demo": `export const DEMO = false;`,
  "./toasts.svelte": `export const toast = { show() {} };`,
};
const hooks = `
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/undo.ts") && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

/** @type {string[]} */
const asked = [];
globalThis.fake = {
  api: {
    undo: async () => (asked.push("undo"), null),
    redo: async () => (asked.push("redo"), null),
  },
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const { undoRedo } = await import("./undo.ts");
const { saving } = await import("./writes.ts");

test("Ctrl+Z right after a change waits for the change to be saved", async () => {
  /** @type {() => void} */
  let saved = () => {};
  saving(new Promise((resolve) => (saved = () => resolve(undefined))));
  undoRedo(false, { kind: "vod", id: 1 });
  await settle();
  // Asked now, the app could take the undo before the change and undo the one before it.
  assert.deepEqual(asked, []);
  saved();
  await settle();
  assert.deepEqual(asked, ["undo"]);
});

test("a change that fails to save still lets the undo through", async () => {
  asked.length = 0;
  /** @type {(e: Error) => void} */
  let failed = () => {};
  saving(new Promise((_, reject) => (failed = reject)));
  undoRedo(true, { kind: "vod", id: 1 });
  failed(new Error("gone"));
  await settle();
  await settle();
  assert.deepEqual(asked, ["redo"]);
});
