// Each VOD's captions, read for the players (captions.svelte.ts), through the
// page's own api.ts with the app faked: Tauri's invoke answers `captions`
// when the test says, and its events are told when the test says.
// node --test apps/desktop/src/lib/captions.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// The app, behind Tauri's two modules: each `captions` asked waits in
// `asked` till the test answers it, and `tell` sends an event as the app does.
const fakes = {
  "@tauri-apps/api/core": `export const invoke = (cmd, args) => globalThis.fake.invoke(cmd, args);`,
  "@tauri-apps/api/event": `export const listen = (name, handler) => globalThis.fake.listen(name, handler);`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (fakes[specifier]) return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
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

/** @typedef {{ vodId: number, answer: (cues: unknown[]) => Promise<void> }} Asked */
/** @type {Asked[]} */
const asked = [];
/** @type {Map<string, Set<(e: { payload: unknown }) => void>>} */
const listeners = new Map();
const settle = () => new Promise((r) => setTimeout(r, 0));
globalThis.fake = {
  /** @param {string} cmd @param {{ vodId: number }} args */
  invoke(cmd, args) {
    if (cmd !== "captions") throw new Error(`the page asked ${cmd}`);
    return new Promise((resolve) =>
      asked.push({
        vodId: args.vodId,
        answer: (cues) => (resolve(cues), settle()),
      }),
    );
  },
  /** @param {string} name @param {(e: { payload: unknown }) => void} handler */
  async listen(name, handler) {
    const set = listeners.get(name) ?? new Set();
    listeners.set(name, set);
    set.add(handler);
    return () => set.delete(handler);
  },
};
/** The app says VOD `id` changed, as emit_vod does. @param {number} id */
const changed = async (id) => {
  for (const h of listeners.get("vod") ?? []) h({ payload: { id } });
  await settle();
};
/** The one `captions` the page is waiting on, taken. @returns {Asked} */
const next = () => {
  assert.equal(asked.length, 1, `asked ${asked.length} times`);
  return /** @type {Asked} */ (asked.shift());
};
/** @param {number} n */
const cuesOf = (n) => Array.from({ length: n }, (_, i) => ({ start: i, end: i + 1, text: `line ${i}` }));

const { captions } = await import("./captions.svelte.ts");

/**
 * A file opened before its transcript exists shows its captions once a part
 * is transcribed, without opening it again (CHANGELOG.md, 0.6.0: "a video
 * file's captions come as soon as a part of it is transcribed, without
 * opening it again"; plans/0.7.4.md, the caption bug).
 */
test("a VOD opened before it's transcribed gets its captions once it is", async () => {
  const done = captions.watch(1);
  assert.equal(captions.read(1), false, "not read yet");
  await next().answer([]);
  assert.equal(captions.read(1), true);
  assert.deepEqual(captions.of(1), [], "nothing transcribed yet");
  // A stretch transcribed: the worker's stage ends (worker.rs `run`, emit_vod).
  await changed(1);
  await next().answer(cuesOf(3));
  assert.deepEqual(captions.of(1), cuesOf(3));
  done();
});

/**
 * Captions read before a change never replace those read after it: two
 * changes close together, the first one's answer coming last, leave the
 * newer captions (quality report 074, item 4: "An older answer landing last
 * leaves stale captions until the next event").
 */
test("captions read before a change never replace those read after it", async () => {
  const done = captions.watch(2);
  await next().answer([]);
  await changed(2); // before the stretch's last lines are in
  const older = next();
  await changed(2); // after
  // One read at a time: the newer is asked once the older is answered.
  await older.answer(cuesOf(10));
  assert.deepEqual(captions.of(2), [], "the older answer isn't shown");
  await next().answer(cuesOf(15));
  assert.deepEqual(captions.of(2), cuesOf(15));
  done();
});

/**
 * Every player of a VOD shows the same captions, read once a change for all
 * of them (quality report 074, item 4: two `captions` reads per "vod" event
 * for a VOD playing here, its panel's and its player's).
 */
test("the players of one VOD read its captions once a change", async () => {
  const panel = captions.watch(3);
  const player = captions.watch(3);
  await next().answer(cuesOf(1));
  await changed(3);
  await next().answer(cuesOf(2));
  assert.equal(asked.length, 0, "read once for both");
  assert.deepEqual(captions.of(3), cuesOf(2));
  // The player closed, the panel still open: still read at each change.
  player();
  await changed(3);
  await next().answer(cuesOf(4));
  assert.deepEqual(captions.of(3), cuesOf(4));
  panel();
});

/** A VOD no player shows any more isn't read, and an answer on its way is let go. */
test("a VOD no player shows is read no more", async () => {
  const done = captions.watch(4);
  const first = next();
  done();
  await first.answer(cuesOf(2));
  assert.equal(captions.read(4), false, "the answer after it closed isn't kept");
  await changed(4);
  assert.equal(asked.length, 0, "not read at a change once closed");
  // Another VOD's change asks nothing of this one.
  const other = captions.watch(5);
  await next().answer([]);
  await changed(4);
  assert.equal(asked.length, 0);
  other();
});
