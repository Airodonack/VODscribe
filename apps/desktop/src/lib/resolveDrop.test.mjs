// The note at a drop on DaVinci Resolve (resolveDrop.svelte.ts), with the
// app's settings and ffmpeg faked: Switch to MOV makes clips as MOV DNxHR,
// so it's offered only while the ffmpeg in use can make that.
// node --test apps/desktop/src/lib/resolveDrop.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// resolveDrop.svelte.ts's imports from the app are the fakes below, each
// kept on globalThis.fake for the test to read.
const fakes = {
  "./api": `export const api = globalThis.fake.api;`,
  "./appSettings.svelte": `export const settings = globalThis.fake.settings;`,
  "./toasts.svelte": `export const toast = globalThis.fake.toast;`,
};
const hooks = `
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/resolveDrop.svelte.ts") && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  return next(specifier, context);
}`;
register("data:text/javascript," + encodeURIComponent(hooks));

const fake = {
  /** What the ffmpeg in use can't make, as the app answers (`missing_clip_codecs`). @type {{ missing: string[], own: boolean }} */
  codecs: { missing: [], own: true },
  /** Each settings change saved. @type {object[]} */
  saved: [],
  /** Each toast shown. @type {string[]} */
  toasts: [],
  api: { missingClipCodecs: async () => fake.codecs },
  settings: { update: async (/** @type {object} */ change) => void fake.saved.push(change) },
  toast: { showUnlessUp: (/** @type {string} */ text) => void fake.toasts.push(text) },
};
Object.assign(globalThis, { fake, $state: (/** @type {unknown} */ v) => v });

const { resolveNote } = await import("./resolveDrop.svelte.ts");

/** The note shown as at a refused drop of one clip, with the ffmpeg answering `codecs`; and how many times its clips were made again.
 *  @param {{ missing: string[], own: boolean }} codecs */
async function shown(codecs) {
  Object.assign(fake, { codecs, saved: [], toasts: [] });
  const remade = { count: 0 };
  resolveNote.show("refused", 1, () => remade.count++);
  // The app's answer about the ffmpeg in.
  await new Promise((r) => setTimeout(r));
  return remade;
}

test("with DNxHR there, Switch to MOV is offered and makes clips as MOV DNxHR, and them again", async () => {
  const remade = await shown({ missing: ["h265"], own: true });
  assert.equal(resolveNote.cantSwitch, null);
  await resolveNote.answer(true, false);
  assert.deepEqual(fake.saved, [{ clip_codec: "dnxhr", clip_format: "mov" }]);
  assert.equal(remade.count, 1);
  assert.deepEqual(fake.toasts, ["Clips are MOV now. Drag it again from the clip bar."]);
});

test("with DNxHR missing, Switch to MOV isn't offered, and an answer of it switches nothing", async () => {
  const remade = await shown({ missing: ["dnxhr"], own: true });
  const offered = resolveNote.cantSwitch === null;
  await resolveNote.answer(true, false);
  assert.deepEqual(fake.saved, [], "settings saved");
  assert.equal(remade.count, 0, "clips made again");
  assert.equal(offered, false, "Switch to MOV offered");
});

test("with DNxHR missing but ProRes there, the note says to pick MOV ProRes, naming whose ffmpeg", async () => {
  await shown({ missing: ["dnxhr"], own: true });
  assert.equal(
    resolveNote.cantSwitch,
    "VODscribe's ffmpeg can't make MOV DNxHR, so pick MOV ProRes in Settings › Encoding.",
  );
  await shown({ missing: ["dnxhr"], own: false });
  assert.equal(
    resolveNote.cantSwitch,
    "Your system's ffmpeg can't make MOV DNxHR, so pick MOV ProRes in Settings › Encoding.",
  );
});

test("with neither DNxHR nor ProRes, the note says no MOV clips can be made, naming whose ffmpeg", async () => {
  await shown({ missing: ["dnxhr", "prores"], own: true });
  assert.equal(resolveNote.cantSwitch, "VODscribe's ffmpeg can't make MOV clips.");
  await shown({ missing: ["dnxhr", "prores"], own: false });
  assert.equal(resolveNote.cantSwitch, "Your system's ffmpeg can't make MOV clips.");
});

test("answered before the app says DNxHR is missing, it switches nothing and says why", async () => {
  Object.assign(fake, { codecs: { missing: ["dnxhr"], own: false }, saved: [], toasts: [] });
  let remade = 0;
  resolveNote.show("refused", 1, () => remade++);
  await resolveNote.answer(true, true);
  assert.deepEqual(fake.saved, [{ suggest_mov_for_resolve: false }], "only Don't ask again kept");
  assert.equal(remade, 0, "clips made again");
  assert.deepEqual(fake.toasts, [
    "Your system's ffmpeg can't make MOV DNxHR, so pick MOV ProRes in Settings › Encoding.",
  ]);
});
