// Whole videos downloading (videos.svelte.ts), with the app faked: compiled
// as the app's build does; the list of downloads answers when the test
// says, and the test sends the app's events.
// node --test apps/desktop/src/lib/videos.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

const fakes = {
  "./api": `export const api = globalThis.fake.api; export const events = globalThis.fake.events;`,
  "./demo": `export const DEMO = false;`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/videos.svelte.ts") && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  return next(specifier, context);
}
export async function load(url, context, next) {
  if (!url.endsWith(".svelte.ts")) return next(url, context);
  const source = stripTypeScriptTypes(await readFile(new URL(url), "utf8"), { mode: "transform" });
  const { js } = compileModule(source, { filename: url, generate: "client" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

/** @type {(p: { id: number, progress: number | null, error: string | null, streamed: boolean }) => void} */
let progress = () => {};
/** @type {(ids: number[]) => void} */
let listed = () => {};
globalThis.fake = {
  api: { videoDownloads: () => new Promise((resolve) => (listed = resolve)) },
  events: {
    /** @param {typeof progress} cb */
    videoProgress: (cb) => ((progress = cb), Promise.resolve(() => {})),
  },
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const { videoDownloads } = await import("./videos.svelte.ts");

test("a download that ended before the list of downloads answers isn't shown waiting", async () => {
  videoDownloads.start();
  // Listed while it ran; its end reaches the page first.
  progress({ id: 3, progress: null, error: null, streamed: false });
  listed([3, 4]);
  await settle();
  assert.deepEqual([...videoDownloads.progress], [[4, 0]]);
});
