// The live keyboard controls (keys.svelte.ts): this OS's defaults with the
// user's own bindings over them, kept in the settings. Compiled as the app's
// build does (types stripped, then Svelte's runes), with the settings and
// the OS faked:
// node --test apps/desktop/src/lib/keys.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// The settings as the page holds them: a rune, and `update` taking its turn
// after the calls made before it, as appSettings.svelte.ts does.
const settingsSource = `
class Settings {
  value = $state({ theme: "dark", key_bindings: {} });
  turn = Promise.resolve();
  update(change) {
    this.turn = this.turn.then(() => {
      this.value = { ...this.value, ...(typeof change === "function" ? change(this.value) : change) };
    });
    return this.turn;
  }
}
export const settings = new Settings();`;
// The fake settings are a module that isn't on disk, beside this file.
const FAKE = new URL("./keys.fake-settings.svelte.js", import.meta.url).href;
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
export async function resolve(specifier, context, next) {
  const parent = context.parentURL ?? "";
  if (specifier === ${JSON.stringify(FAKE)}) return { url: specifier, shortCircuit: true };
  if (parent.includes("/keys.svelte.ts")) {
    if (specifier === "./appSettings.svelte") return { url: ${JSON.stringify(FAKE)}, shortCircuit: true };
    if (specifier === "./platform")
      return { url: "test-fake:platform?os=" + new URL(parent).searchParams.get("os"), shortCircuit: true };
  }
  if (specifier.startsWith("./") && !/\\.[a-z]+(\\?.*)?$/.test(specifier) && parent.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}
export async function load(url, context, next) {
  if (url === ${JSON.stringify(FAKE)}) {
    const { js } = compileModule(${JSON.stringify(settingsSource)}, { filename: url, generate: "client" });
    return { format: "module", source: js.code, shortCircuit: true };
  }
  if (url.startsWith("test-fake:platform"))
    return { format: "module", shortCircuit: true,
      source: "export const os = " + JSON.stringify(new URL(url).searchParams.get("os")) + ";" };
  if (!new URL(url).pathname.endsWith(".svelte.ts")) return next(url, context);
  const source = stripTypeScriptTypes(await readFile(new URL(url), "utf8"), { mode: "transform" });
  const { js } = compileModule(source, { filename: url, generate: "client" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

const { settings } = await import(FAKE);
/** The live keys module as one OS has them. @param {"linux" | "mac"} os */
const load = async (os) => /** @type {typeof import("./keys.svelte.ts")} */ (await import(`./keys.svelte.ts?os=${os}`));
const linux = (await load("linux")).keys;
const { keys: mac, keyHint, is, labelOf, bindingsOf } = await load("mac");

/** @param {string} key @param {string} mods */
const press = (key, mods = "") => ({
  key,
  ctrlKey: mods.includes("ctrl"),
  altKey: mods.includes("alt"),
  shiftKey: mods.includes("shift"),
  metaKey: mods.includes("meta"),
});
/** Settings as they are at the start. */
const fresh = () => (settings.value = { theme: "dark", key_bindings: {} });

/** "Every keyboard control can be changed. In Settings › Controls, give an
 *  action several keys, modifiers allowed, and reset one or all."
 *  (CHANGELOG.md, 0.6.0). */
test("an action given several keys is fired by each, and by no key it lost", async () => {
  fresh();
  assert.ok(linux.is(press(" "), "play.toggle"));
  await linux.set({ "play.toggle": ["p", "Ctrl+Shift+p"] });
  assert.deepEqual(linux.bindingsOf("play.toggle"), ["p", "Ctrl+Shift+p"]);
  assert.ok(linux.is(press("p"), "play.toggle"));
  assert.ok(linux.is(press("P", "ctrl shift"), "play.toggle"));
  assert.ok(!linux.is(press(" "), "play.toggle"));
  assert.ok(linux.is(press("ArrowLeft"), "seek.back5"), "the other actions keep theirs");
});

/** keys.svelte.ts: "an empty one: no key". */
test("an action given no key is fired by none and labelled with none", async () => {
  fresh();
  await linux.set({ "mute.toggle": [] });
  assert.deepEqual(linux.bindingsOf("mute.toggle"), []);
  assert.ok(!linux.is(press("M", "shift"), "mute.toggle"));
  assert.equal(linux.labelOf("mute.toggle"), "");
});

/** keys.svelte.ts: "`id`'s first key as this OS writes it (\"⌘F\", \"Ctrl+F\")". */
test("a control's first key is written as this OS writes it", async () => {
  fresh();
  assert.equal(linux.labelOf("search.focus"), "Ctrl+F");
  assert.equal(mac.labelOf("search.focus"), "⌘F");
  await mac.set({ "search.focus": ["Shift+Meta+k", "Meta+f"] });
  assert.equal(mac.labelOf("search.focus"), "⇧⌘K");
});

/** keys.svelte.ts `keyHint`: "`id`'s key for a control's title: \" (⌘F)\", or
 *  nothing if it has none." */
test("a control's title names its key in brackets, or nothing if it has none", async () => {
  fresh();
  assert.equal(keyHint("search.focus"), " (⌘F)");
  await mac.set({ "search.focus": [] });
  assert.equal(keyHint("search.focus"), "");
});

/** keys.svelte.ts `changed`: "Whether `id`'s bindings are other than its
 *  defaults." */
test("an action reads as changed only while its keys are other than its defaults", async () => {
  fresh();
  assert.equal(linux.changed("play.toggle"), false);
  await linux.set({ "play.toggle": ["p"] });
  assert.equal(linux.changed("play.toggle"), true);
  assert.equal(linux.changed("seek.back5"), false);
  await linux.set({ "play.toggle": [" ", "k"] });
  assert.equal(linux.changed("play.toggle"), false);
  // The order is part of it: the first key is the one shown.
  await linux.set({ "play.toggle": ["k", " "] });
  assert.equal(linux.changed("play.toggle"), true);
});

/** keys.svelte.ts `set`: "a list that's the defaults again is no override." */
test("keys put back to the defaults are saved as no override", async () => {
  fresh();
  await linux.set({ "play.toggle": ["p"], undo: ["Ctrl+u"] });
  await linux.set({ "play.toggle": [" ", "k"] });
  assert.deepEqual(settings.value.key_bindings, { undo: ["Ctrl+u"] });
});

/** keys.svelte.ts `set`: "so two quick changes can't drop one" (CHANGELOG.md, 0.3.0
 *  Fixes: "Two quick changes in Settings could be saved out of order."). */
test("two quick changes made before either is saved both stay", async () => {
  fresh();
  const first = linux.set({ "play.toggle": ["p"] });
  const second = linux.set({ undo: ["Ctrl+u"] });
  await Promise.all([first, second]);
  assert.deepEqual(settings.value.key_bindings, { "play.toggle": ["p"], undo: ["Ctrl+u"] });
});

/** keys.svelte.ts: "Ids the table doesn't have (from a later version, say)
 *  are kept, and saved back as they came." */
test("keys of an action a later version has are saved back as they came", async () => {
  settings.value = { theme: "dark", key_bindings: { "later.action": ["Ctrl+9"] } };
  assert.deepEqual(linux.overrides, { "later.action": ["Ctrl+9"] });
  await linux.set({ undo: ["Ctrl+u"] });
  assert.deepEqual(settings.value.key_bindings, { "later.action": ["Ctrl+9"], undo: ["Ctrl+u"] });
});

/** keys.svelte.ts `reset` and `resetAll`: "`id`'s default bindings back",
 *  "Every action's default bindings back (the other settings stay)." */
test("reset gives one action its keys back, and reset all gives every action its keys back", async () => {
  fresh();
  await linux.set({ "play.toggle": ["p"], undo: ["Ctrl+u"] });
  await linux.reset("play.toggle");
  assert.deepEqual(settings.value.key_bindings, { undo: ["Ctrl+u"] });
  assert.ok(linux.is(press(" "), "play.toggle"));
  await linux.set({ redo: ["Ctrl+r"] });
  await linux.resetAll();
  assert.deepEqual(settings.value.key_bindings, {});
  assert.equal(settings.value.theme, "dark", "the other settings stay");
  assert.ok(linux.is(press("z", "ctrl"), "undo"));
});

/** keymap.ts `conflicts`, as the live map answers it (Settings › Controls'
 *  "Used by …"). */
test("a key another live action has is named, from the keys in force", async () => {
  fresh();
  assert.deepEqual(
    linux.conflicts("k", "frame.back10").map((/** @type {{ id: string }} */ a) => a.id),
    ["play.toggle"],
  );
  await linux.set({ "play.toggle": [" "] });
  assert.deepEqual(linux.conflicts("k", "frame.back10"), []);
});

/** keys.svelte.ts: "The handlers ask `is(e, id)`; legends and titles show `labelOf(id)` or `bindingsOf(id)`", over the keys in force. */
test("the functions the page's handlers and titles ask answer from the keys in force", async () => {
  fresh();
  assert.equal(is(press("f", "meta"), "search.focus"), true);
  assert.equal(labelOf("search.focus"), "⌘F");
  assert.deepEqual(bindingsOf("search.focus"), ["Meta+f"]);
  await mac.set({ "search.focus": ["Meta+k"] });
  assert.equal(is(press("f", "meta"), "search.focus"), false);
  assert.equal(is(press("k", "meta"), "search.focus"), true);
  assert.equal(labelOf("search.focus"), "⌘K");
  assert.deepEqual(bindingsOf("search.focus"), ["Meta+k"]);
});

/** keys.svelte.ts: the keys are "loaded once at the start as the theme is": until the settings are in, the defaults hold. */
test("until the settings are in, the defaults are the keys", () => {
  settings.value = null;
  assert.deepEqual(linux.bindingsOf("play.toggle"), [" ", "k"]);
  assert.ok(linux.is(press("k"), "play.toggle"));
  assert.equal(linux.changed("play.toggle"), false);
});

/** keys.svelte.ts `changed`: every key counts, not only the first. */
test("an action with one of its keys changed reads as changed", async () => {
  fresh();
  await linux.set({ "play.toggle": [" ", "x"] });
  assert.equal(linux.changed("play.toggle"), true);
  await linux.set({ "play.toggle": ["x", "k"] });
  assert.equal(linux.changed("play.toggle"), true);
});
