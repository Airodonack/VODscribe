// The small choices the page remembers (stored.ts), with the app faked: each
// call to it answers when the test says.
// node --test apps/desktop/src/lib/stored.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// stored.ts asks the app through Tauri's `invoke`, which here is the test's.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(specifier, context, next) {
  if (specifier === "@tauri-apps/api/core")
    return { url: "data:text/javascript," + encodeURIComponent(
      "export const invoke = (...asked) => globalThis.fakeInvoke(...asked);"), shortCircuit: true };
  return next(specifier, context);
}`),
);

/** What the page asked of the app, in order: the command and its arguments. @type {[string, any][]} */
let asked = [];
/** The choices the app keeps, or an error to answer `kept_choices` with. @type {Record<string, string> | Error} */
let kept = {};
/** Held `keep_choice` answers, by key, for the test to let go. @type {Map<string, () => void>} */
const held = new Map();
/** Whether `keep_choice` answers at once, or when the test lets the key go. */
let holding = false;
/** The keys a `keep_choice` is to fail for. @type {Set<string>} */
let failing = new Set();
Object.assign(globalThis, {
  /** @param {string} cmd @param {any} args */
  fakeInvoke: (cmd, args) => {
    asked.push([cmd, args]);
    if (cmd === "kept_choices") return kept instanceof Error ? Promise.reject(kept) : Promise.resolve(kept);
    if (failing.has(args.key)) return Promise.reject(new Error("not kept"));
    return holding ? new Promise((resolve) => held.set(args.key, () => resolve(null))) : Promise.resolve(null);
  },
});

/** The page's web view storage, as far as `start` reads it: nothing, or these. @param {Record<string, string> | null} items */
const storage = (items) => {
  if (items === null) {
    delete (/** @type {any} */ (globalThis).localStorage);
    return;
  }
  const keys = Object.keys(items);
  /** @type {any} */ (globalThis).localStorage = {
    length: keys.length,
    key: (/** @type {number} */ i) => keys[i] ?? null,
    getItem: (/** @type {string} */ k) => items[k] ?? null,
  };
};

let fresh = 0;
/** The module anew, as the page's start has it: nothing read, nothing sent. */
const stored = async () => {
  asked = [];
  kept = {};
  held.clear();
  holding = false;
  failing = new Set();
  storage(null);
  return import(`./stored.ts?${fresh++}`);
};
/** What was sent to keep, in order. */
const sent = () => asked.filter(([cmd]) => cmd === "keep_choice").map(([, args]) => [args.key, args.value]);
// Every promise the page chains has run when the next turn of the event loop begins.
const settle = () => new Promise((resolve) => setImmediate(resolve));

/** stored.ts: "They're read once, before the page starts (`start`), so `load`
 *  can answer at once" (0.7.0 Fixes, "Remembered choices last after a restart"). */
test("the choices the app kept are read once at the start, and answered at once", async () => {
  const s = await stored();
  kept = { "vodscribe.volume": "0.4", "vodscribe.speed": "1.5" };
  await s.start();
  assert.equal(s.load("vodscribe.volume"), "0.4");
  assert.equal(s.load("vodscribe.speed"), "1.5");
  assert.deepEqual(
    asked.map(([cmd]) => cmd),
    ["kept_choices"],
  );
  assert.equal(s.load("vodscribe.never"), null);
});

/** stored.ts `start`: "If they can't be read, nothing is remembered this
 *  time, which is fine." */
test("choices the app can't read are none, and the page starts", async () => {
  const s = await stored();
  kept = new Error("no");
  await s.start();
  assert.equal(s.load("vodscribe.volume"), null);
});

/** stored.ts `load`: "The value kept under `key`, or null." */
test("a choice is none before the start has read them", async () => {
  const s = await stored();
  assert.equal(s.load("vodscribe.volume"), null);
});

/** stored.ts `start`: "A dev build's page has kept the same origin all along,
 *  so what it remembered is still in the web view's storage: brought over the
 *  first time." */
test("a dev build brings over what its web view's storage kept, the page's keys only", async () => {
  const s = await stored();
  storage({ "vodscribe.volume": "0.3", "vodscribe.speed": "2", "other.site": "x" });
  await s.start();
  await settle();
  assert.equal(s.load("vodscribe.volume"), "0.3");
  assert.equal(s.load("other.site"), null);
  assert.deepEqual(sent().sort(), [
    ["vodscribe.speed", "2"],
    ["vodscribe.volume", "0.3"],
  ]);
});

/** stored.ts `start`: "brought over the first time": only when the app has
 *  kept nothing yet. */
test("what the web view's storage kept isn't brought over once the app keeps choices", async () => {
  const s = await stored();
  kept = { "vodscribe.library": "hidden" };
  storage({ "vodscribe.volume": "0.3" });
  await s.start();
  await settle();
  assert.equal(s.load("vodscribe.volume"), null);
  assert.deepEqual(sent(), []);
});

/** stored.ts `start`: "(An installed app's is empty, a new origin each
 *  launch.)" and "No storage here: nothing to bring over." */
test("a page with no web view storage starts with no choices", async () => {
  const s = await stored();
  await s.start();
  assert.equal(s.load("vodscribe.volume"), null);
  assert.deepEqual(sent(), []);
});

/** stored.ts `save`: "Keeps `value` under `key`, if it can." and `load`. */
test("a choice saved is loaded at once and kept by the app", async () => {
  const s = await stored();
  await s.start();
  s.save("vodscribe.volume", "0.7");
  assert.equal(s.load("vodscribe.volume"), "0.7");
  await settle();
  assert.deepEqual(sent(), [["vodscribe.volume", "0.7"]]);
});

/** stored.ts `save`: a value the page already has is no change, so nothing is sent. */
test("a choice saved as it already is isn't sent", async () => {
  const s = await stored();
  kept = { "vodscribe.speed": "1.5" };
  await s.start();
  s.save("vodscribe.speed", "1.5");
  await settle();
  assert.deepEqual(sent(), []);
});

/** stored.ts: "Sent one at a time, in order, so the last value given is the
 *  one kept". */
test("choices are sent one at a time, in the order they were saved", async () => {
  const s = await stored();
  await s.start();
  holding = true;
  s.save("a", "1");
  s.save("b", "2");
  s.save("c", "3");
  await settle();
  assert.deepEqual(sent(), [["a", "1"]], "the next waits for the one under way");
  held.get("a")?.();
  await settle();
  assert.deepEqual(sent(), [
    ["a", "1"],
    ["b", "2"],
  ]);
  held.get("b")?.();
  await settle();
  held.get("c")?.();
  await settle();
  assert.deepEqual(sent(), [
    ["a", "1"],
    ["b", "2"],
    ["c", "3"],
  ]);
});

/** stored.ts: "one changed again while waiting (the volume dragged) is sent
 *  once, as it is then." */
test("a choice changed again while it waits is sent once, as it is then", async () => {
  const s = await stored();
  await s.start();
  holding = true;
  s.save("a", "1");
  s.save("volume", "0.1");
  s.save("volume", "0.2");
  s.save("volume", "0.3");
  await settle();
  held.get("a")?.();
  await settle();
  held.get("volume")?.();
  await settle();
  assert.deepEqual(sent(), [
    ["a", "1"],
    ["volume", "0.3"],
  ]);
  assert.equal(s.load("volume"), "0.3");
});

/** stored.ts `save`: "Not remembered this time" — a choice the app can't keep
 *  doesn't stop the ones after it. */
test("a choice the app can't keep doesn't stop the next", async () => {
  const s = await stored();
  await s.start();
  failing = new Set(["a"]);
  s.save("a", "1");
  s.save("b", "2");
  await settle();
  assert.deepEqual(sent(), [
    ["a", "1"],
    ["b", "2"],
  ]);
  s.save("c", "3");
  await settle();
  assert.deepEqual(
    sent(),
    [
      ["a", "1"],
      ["b", "2"],
      ["c", "3"],
    ],
    "later ones are sent, and none again",
  );
});

/** stored.ts `start`: a web view whose storage has no key at some place is read on to the rest. */
test("a web view's storage that gives no key at a place is read on", async () => {
  const s = await stored();
  storage({ "vodscribe.volume": "0.3" });
  const ls = /** @type {any} */ (globalThis).localStorage;
  const keys = [null, "vodscribe.volume"];
  ls.length = 2;
  ls.key = (/** @type {number} */ i) => keys[i];
  await s.start();
  assert.equal(s.load("vodscribe.volume"), "0.3");
});
