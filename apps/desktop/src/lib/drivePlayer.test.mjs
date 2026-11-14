// Where a player's playhead is remembered (drivePlayer.svelte.ts into
// nav.svelte.ts), with the animation frames in the test's hands: the
// modules compiled as the app's build does (types stripped, then Svelte's
// runes), the app's api and marks faked.
// node --test apps/desktop/src/lib/drivePlayer.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

const fakes = {
  "./api": `export const docKey = (doc) => doc.kind + ":" + doc.id;`,
  "./cuts.svelte": `export const cuts = { isOf: () => false };`,
};
// A component's lifetime: drivePlayer run inside an effect root, which the test ends.
const mounted = `import { drivePlayer } from "./drivePlayer.svelte";
export const mount = (...args) => $effect.root(() => drivePlayer(...args));`;
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
const mounted = ${JSON.stringify(mounted)};
export async function resolve(specifier, context, next) {
  if (specifier.endsWith("/mounted.svelte.ts")) return { url: specifier, shortCircuit: true };
  if (fakes[specifier] && context.parentURL?.endsWith(".svelte.ts"))
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier.startsWith("./") && !/\\.(ts|js|mjs)$/.test(specifier) && context.parentURL?.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}
export async function load(url, context, next) {
  if (!url.endsWith(".svelte.ts")) return next(url, context);
  const file = url.endsWith("/mounted.svelte.ts") ? mounted : await readFile(new URL(url), "utf8");
  const source = stripTypeScriptTypes(file, { mode: "transform" });
  const { js } = compileModule(source, { filename: url, generate: "client" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

// Animation frames come only when the test draws one.
/** @type {Map<number, FrameRequestCallback>} */
const waiting = new Map();
let frames = 0;
/** @param {FrameRequestCallback} f */
globalThis.requestAnimationFrame = (f) => (waiting.set(++frames, f), frames);
/** @param {number} id */
globalThis.cancelAnimationFrame = (id) => waiting.delete(id);
// Effects run once the page's work is done (a microtask, say).
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
function frame() {
  const due = [...waiting.values()];
  waiting.clear();
  for (const f of due) f(performance.now());
}

const { nav } = await import("./nav.svelte.ts");
const { PlayerReading } = await import("./playerReading.svelte.ts");
const { mount } = await import(new URL("./mounted.svelte.ts", import.meta.url).href);

test("a playhead the page moves is remembered before the next frame", async () => {
  /** @type {import("./api").Doc} */
  const doc = { kind: "vod", id: 1 };
  const reading = new PlayerReading(0);
  // A paused player at 3 s, as the element reads.
  let time = 3;
  const player = { time: () => time, playing: () => false };
  const unmount = mount(reading, () => doc, { player: () => player });
  await settle();
  frame();
  assert.equal(nav.lastSeen(doc)?.time, 3);
  nav.arrived(doc, { query: "", all: false, audio: true, chat: true, within: 20, from: "", aside: false });

  // A step on (Shift+→, Player.svelte's seekTo): the time shown goes there at
  // once; the view is left before a frame is drawn (a busy machine).
  reading.at(4);
  time = 4;
  await settle();
  nav.arrived(null, { query: "", all: false, audio: true, chat: true, within: 20, from: "", aside: false });
  unmount();
  assert.equal(nav.back.at(-1)?.left.time, 4, "the step back to it");
  assert.equal(nav.lastSeen(doc)?.time, 4, "where it reopens");
});

// A player whose `settled` says undefined doesn't say when its seeks settle
// (a site's, while the app's player doesn't draw its picture: embed.ts
// `clocked`): paused, the time shown is the frame shown, with no hold after
// a seek, as for a player with no `settled` at all.
test("a player that doesn't say when its seek settled holds no time paused", () => {
  const reading = new PlayerReading(0);
  const player = { time: () => 35, shown: () => 5, fps: () => 30, playing: () => false, settled: () => undefined };
  reading.read(player);
  reading.at(35);
  reading.read(player);
  assert.equal(reading.actual, 5);
});
