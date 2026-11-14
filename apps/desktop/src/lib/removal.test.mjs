// Removing a VOD (removal.svelte.ts), with the app faked: compiled as the
// app's build does (types stripped, then Svelte's runes); Remove answers when
// the test says.
// node --test apps/desktop/src/lib/removal.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// removal.svelte.ts's imports from the app are the fakes below; platform.ts is the .ts file it is.
const fakes = {
  "./api": `export const api = globalThis.fake.api;`,
  "./ask": `export const choose = async () => "Keep the video";`,
  "./edits.svelte": `export const edits = { forVod: () => [] };`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/removal.svelte.ts") && fakes[specifier])
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

/** Remove's answer, given when the test says. @type {(value: string | null) => void} */
let answer = () => {};
globalThis.fake = {
  api: { deleteVod: () => new Promise((resolve) => (answer = resolve)) },
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const { removing, removeVod } = await import("./removal.svelte.ts");

// vods.rs `delete_vod`: added back while it waits for the VOD's work, Remove
// sends its row as it is now, not the news that it's gone.
test("a VOD whose Remove has answered isn't shown being removed, though no news came that it's gone", async () => {
  const vod = /** @type {any} */ ({ id: 7, title: "A talk" });
  const removed = removeVod(vod, true, () => assert.fail("no error was given"));
  assert.ok(removing.has(7), "not shown being removed while Remove runs");
  answer(null);
  await removed;
  await settle();
  assert.ok(!removing.has(7), "still shown being removed once Remove answered");
});
