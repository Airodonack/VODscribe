// The app's settings as the page keeps them (appSettings.svelte.ts):
// loaded once, saved whole, one save at a time in the order made, a reset
// in turn with them. Through the page's own api.ts and stored.ts, compiled
// as the app's build does, with the app faked behind Tauri's invoke: each
// command asked waits in `asked` till the test answers it.
// node --test apps/desktop/src/lib/appSettings.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

const fakes = {
  "@tauri-apps/api/core": `export const invoke = (cmd, args) => globalThis.fake.invoke(cmd, args);`,
  "@tauri-apps/api/event": `export const listen = async () => () => {};`,
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
  if (!url.split("?")[0].endsWith(".svelte.ts")) return next(url, context);
  const source = stripTypeScriptTypes(await readFile(new URL(url), "utf8"), { mode: "transform" });
  const { js } = compileModule(source, { filename: url, generate: "client" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

const stored = await import("./stored.ts");

/** Every setting as it starts, as `get_settings` answers on a new library (in part). */
const DEFAULTS = Object.freeze({
  theme: "system",
  video_height: null,
  show_keys: false,
  ask_before_downloading: true,
  key_bindings: {},
});

/** @typedef {{ cmd: string, args: any, answer: (value?: unknown) => Promise<void>, fail: (why: string) => Promise<void> }} Asked */
/** @type {Asked[]} */
let asked = [];
/** The app's answers to the commands that aren't held: the settings it has, and the choices kept. */
/** @type {{ settings: Record<string, unknown>, choices: Record<string, string> }} */
let has = { settings: {}, choices: {} };
/** How many times the page has asked for the settings (`get_settings`). */
let loads = 0;
const settle = () => new Promise((r) => setTimeout(r, 0));
globalThis.fake = {
  /** @param {string} cmd @param {any} args */
  invoke(cmd, args) {
    if (cmd === "get_settings") return (loads++, Promise.resolve(structuredClone(has.settings)));
    if (cmd === "kept_choices") return Promise.resolve({ ...has.choices });
    return new Promise((resolve, reject) =>
      asked.push({
        cmd,
        args: structuredClone(args),
        answer: (value) => (resolve(value), settle()),
        fail: (why) => (reject(why), settle()),
      }),
    );
  },
};

let made = 0;
/**
 * The page's settings, new as at the page's start, over an app that has `settings` and `choices` kept.
 * @param {Record<string, unknown>} settings @param {Record<string, string>} [choices]
 */
async function page(settings, choices = {}) {
  has = { settings, choices };
  asked = [];
  await stored.start();
  const { settings: kept } = await import(new URL(`./appSettings.svelte.ts?${++made}`, import.meta.url).href);
  return kept;
}

/** The commands asked so far and not yet answered, by name. */
const waiting = () => asked.map((a) => a.cmd);

/** The next command asked, which must be `cmd`, taken off the list to answer. @param {string} cmd @returns {Asked} */
function next(cmd) {
  const first = asked.shift();
  if (!first) throw new Error(`${cmd} not asked`);
  assert.equal(first.cmd, cmd, `asked ${first.cmd}, not ${cmd}`);
  return first;
}

test("two quick changes in Settings are saved in the order made", async () => {
  // CHANGELOG.md, 0.3.0 Fixes: "Two quick changes in Settings could be saved out of order."
  // appSettings.svelte.ts: "Saves go one after another, in the order made".
  const settings = await page({ ...DEFAULTS });
  await settings.load();
  const dark = settings.update({ theme: "dark" });
  const height = settings.update({ video_height: 480 });
  await settle();
  assert.deepEqual(waiting(), ["set_settings"], "the second sent before the first landed");
  // The first sends the settings as they are when its turn comes.
  const first = next("set_settings");
  assert.equal(first.args.settings.theme, "dark");
  await first.answer();
  const second = next("set_settings");
  assert.deepEqual([second.args.settings.theme, second.args.settings.video_height], ["dark", 480]);
  await second.answer();
  await Promise.all([dark, height]);
  assert.deepEqual(waiting(), []);
});

test("a change saves every setting, so none is dropped", async () => {
  // appSettings.svelte.ts: "saved whole on each change, so changing one setting never drops another".
  const had = { ...DEFAULTS, video_height: 720, key_bindings: { play: ["k"] } };
  const settings = await page(had);
  const saved = settings.update({ theme: "dark" });
  await settle();
  const sent = next("set_settings");
  assert.deepEqual(sent.args.settings, { ...had, theme: "dark" });
  await sent.answer();
  await saved;
});

test("the settings are asked of the app once, whoever loads them", async () => {
  // appSettings.svelte.ts: "shared by the views that use them: loaded once".
  const settings = await page({ ...DEFAULTS, theme: "dark" });
  loads = 0;
  await Promise.all([settings.load(), settings.load()]);
  await settings.load();
  assert.equal(loads, 1);
  assert.equal(settings.value.theme, "dark");
});

test("a reset asked between saves is made in turn, and a change asked after it is made over the settings as they start", async () => {
  // appSettings.svelte.ts `reset`: "in turn with the saves: one asked for meanwhile is made after it, over the
  // settings as they start"; `update`: "it's then worked out again over whatever came before it, so it never
  // undoes another one made meanwhile".
  const settings = await page({ ...DEFAULTS, key_bindings: { play: ["k"] } });
  await settings.load();
  const dark = settings.update({ theme: "dark" });
  await settle();
  const reset = settings.reset();
  const mute = settings.update((/** @type {any} */ now) => ({ key_bindings: { ...now.key_bindings, mute: ["m"] } }));
  await settle();
  assert.deepEqual(waiting(), ["set_settings"], "the reset made before the save landed");
  await next("set_settings").answer();
  await next("reset_settings").answer({ ...DEFAULTS });
  const sent = next("set_settings");
  assert.deepEqual(sent.args.settings, { ...DEFAULTS, key_bindings: { mute: ["m"] } });
  await sent.answer();
  await Promise.all([dark, reset, mute]);
  assert.deepEqual(settings.value, { ...DEFAULTS, key_bindings: { mute: ["m"] } });
});

test("the app is told the settings once each save or reset has landed", async () => {
  // appSettings.svelte.ts `onSaved`: "Has `told` called once each save or reset has landed: the app reads the
  // settings as saved from then on".
  const settings = await page({ ...DEFAULTS });
  /** @type {unknown[]} */
  const told = [];
  settings.onSaved((/** @type {unknown} */ saved) => told.push(JSON.parse(JSON.stringify(saved))));
  const dark = settings.update({ theme: "dark" });
  await settle();
  assert.deepEqual(told, [], "told before the save landed");
  await next("set_settings").answer();
  await dark;
  assert.deepEqual(told, [{ ...DEFAULTS, theme: "dark" }]);
  const reset = settings.reset();
  await settle();
  await next("reset_settings").answer({ ...DEFAULTS });
  await reset;
  assert.deepEqual(told, [{ ...DEFAULTS, theme: "dark" }, { ...DEFAULTS }]);
});

test("a note stopped before it was a setting is brought over, its old key marked once that's saved", async () => {
  // appSettings.svelte.ts `#bringOver`: "The notes stopped before they were settings (asks.ts), made so; each
  // old key is marked once that's saved, so a later Reset brings the note back".
  const settings = await page({ ...DEFAULTS }, { "vodscribe.clipNoteHidden": "true" });
  await settings.load();
  await settle();
  const sent = next("set_settings");
  assert.equal(sent.args.settings.ask_before_downloading, false);
  assert.deepEqual(waiting(), [], "the old key marked before the save landed");
  await sent.answer();
  await settle();
  const marked = next("keep_choice");
  assert.deepEqual(marked.args, { key: "vodscribe.clipNoteHidden", value: "in settings" });
  await marked.answer();

  // A save that fails leaves the old key as it was, to be brought over again.
  const again = await page({ ...DEFAULTS }, { "vodscribe.clipNoteHidden": "true" });
  await again.load();
  await settle();
  await next("set_settings").fail("the disk is full");
  await settle();
  assert.deepEqual(waiting(), [], "the old key marked though the save failed");
});
