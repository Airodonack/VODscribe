// The timeline's layers store (layers.svelte.ts), with the app faked: compiled
// as the app's build does (types stripped, then Svelte's runes); the app's
// answers come when the test says, and the save's moment is the test's clock.
// node --test apps/desktop/src/lib/layers.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { mock, test } from "node:test";

// layers.svelte.ts's one import from the app is the fake below; markSources.ts is the .ts file it is.
const fakes = { "./api": `export const api = globalThis.layersApp;` };
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.includes("/layers.svelte.ts") && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}
export async function load(url, context, next) {
  if (!new URL(url).pathname.endsWith(".svelte.ts")) return next(url, context);
  const source = stripTypeScriptTypes(await readFile(new URL(url), "utf8"), { mode: "transform" });
  const { js } = compileModule(source, { filename: url, generate: "client" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

/** The app: `getTimeline` answers when the test says, and every timeline saved is kept, in order.
 *  @type {{ asked: number, answer: (t: any) => void, fail: (e: unknown) => void, saved: any[], saveFails: boolean }} */
let app;
/** A fresh store on a fresh app, as a page starts with. */
let copies = 0;
async function start() {
  app = { asked: 0, answer: () => {}, fail: () => {}, saved: [], saveFails: false };
  /** @type {any} */ (globalThis).layersApp = {
    getTimeline: () =>
      new Promise((resolve, reject) => {
        app.asked++;
        app.answer = resolve;
        app.fail = reject;
      }),
    setTimeline: (/** @type {any} */ timeline) =>
      app.saveFails ? Promise.reject("disk full") : (app.saved.push(timeline), Promise.resolve()),
  };
  mock.timers.enable({ apis: ["setTimeout"] });
  return (await import(`./layers.svelte.ts?copy=${copies++}`)).layers;
}
/** Lets what's been answered reach the store. */
const settle = () => new Promise((resolve) => setImmediate(resolve));
/** The moment a save waits, passing. */
const wait = async () => (mock.timers.tick(400), await settle());

/** @param {string} id @param {Record<string, unknown>} [more] */
const layer = (id, more = {}) => ({ id, name: id, color: "#000000", terms: [], visible: true, channel: null, ...more });
/** @param {string} [platform] @param {string} [channel] */
const vod = (platform = "twitch", channel = "barky") => /** @type {any} */ ({ platform, channel });
/** A store whose saved timeline has arrived. @param {any[]} list */
async function loaded(
  /** @type {any[]} */ list = [],
  /** @type {any[]} */ events = [],
  marks = { user: true, imported: true },
) {
  const layers = await start();
  const loading = layers.load();
  app.answer({ layers: list, events, marks });
  await loading;
  return layers;
}

test("the saved layers, events and marks are shown once they arrive", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded([layer("a")], ["sub"], { user: false, imported: true });
  assert.deepEqual(
    layers.list.map((/** @type {any} */ l) => l.id),
    ["a"],
  );
  assert.deepEqual([...layers.events], ["sub"]);
  assert.deepEqual({ ...layers.marks }, { user: false, imported: true });
  assert.equal(layers.loaded, true);
});

// "Whose chapters and highlights are shown ...; both until the saved choice is in" (layers.svelte.ts, marks).
// "The saved layers have arrived. Until then ... the list is empty" and "both until the saved choice is in" (layers.svelte.ts, `loaded`, `marks`).
test("before the saved layers arrive there are none, no events, and both kinds of marks shown", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await start();
  layers.load();
  assert.equal(layers.loaded, false);
  assert.equal(layers.list.length, 0);
  assert.equal(layers.events.length, 0);
  assert.deepEqual({ ...layers.marks }, { user: true, imported: true });
});

test("a kind of marks the saved choice says nothing of is shown", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded([], [], /** @type {any} */ ({ user: false }));
  assert.deepEqual({ ...layers.marks }, { user: false, imported: true });
});

test("the saved timeline is asked for once, however many parts of the page want it", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await start();
  const first = layers.load();
  const second = layers.load();
  app.answer({ layers: [], events: [], marks: { user: true, imported: true } });
  await Promise.all([first, second]);
  await layers.load();
  assert.equal(app.asked, 1);
});

// 0.3.0 Fixes: "Timeline layers that failed to load could be overwritten." (layers.svelte.ts, `loaded`:
// "Until then nothing is saved: the list is empty, and saving it would replace every saved layer.")
test("until the saved layers arrive, a change saves nothing", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await start();
  layers.load();
  layers.toggleEvent("sub");
  layers.showMarks({ user: false });
  await wait();
  assert.equal(app.saved.length, 0);
});

test("layers that failed to load are not saved over, and asking again brings them", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await start();
  const failed = layers.load();
  app.fail("no answer");
  await failed;
  assert.equal(layers.error, "no answer");
  assert.equal(layers.loaded, false);
  layers.toggleEvent("gift");
  await wait();
  assert.equal(app.saved.length, 0, "the empty list was saved over the layers");
  const again = layers.load();
  app.answer({ layers: [layer("a")], events: [], marks: { user: true, imported: true } });
  await again;
  assert.equal(layers.error, null);
  assert.deepEqual(
    layers.list.map((/** @type {any} */ l) => l.id),
    ["a"],
  );
  layers.toggleEvent("gift");
  await wait();
  assert.equal(app.saved.length, 1);
});

// "saved shortly after each change" / "Saves a moment after the last change, so typing doesn't write on every key" (layers.svelte.ts, `save`).
test("a change is saved a moment after the last one, once", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded([layer("a")]);
  layers.toggleEvent("sub");
  mock.timers.tick(300);
  layers.toggleEvent("tip");
  mock.timers.tick(300);
  await settle();
  assert.equal(app.saved.length, 0, "saved before the moment was up");
  await wait();
  assert.equal(app.saved.length, 1);
  assert.deepEqual(app.saved[0].events, ["sub", "tip"]);
  assert.deepEqual(
    app.saved[0].layers.map((/** @type {any} */ l) => l.id),
    ["a"],
  );
  assert.deepEqual(app.saved[0].marks, { user: true, imported: true });
});

test("a save that fails says so, and the next that works clears it", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded();
  app.saveFails = true;
  layers.toggleEvent("sub");
  await wait();
  assert.equal(layers.error, "disk full");
  app.saveFails = false;
  layers.toggleEvent("tip");
  await wait();
  assert.equal(layers.error, null);
});

// "A few layers are drawn at once, so the timeline stays legible" / 0.2.0: "Up to three show at once".
test("up to three visible layers are drawn, the first three in the list", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded([layer("a"), layer("b", { visible: false }), layer("c"), layer("d"), layer("e")]);
  const v = vod();
  assert.deepEqual(
    layers.shownFor(v).map((/** @type {any} */ l) => l.id),
    ["a", "c", "d"],
  );
  assert.equal(layers.canShowMore(v), false);
});

test("with fewer than three drawn, one more can be", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded([layer("a"), layer("b", { visible: false }), layer("c")]);
  assert.equal(layers.canShowMore(vod()), true);
});

// "Only for this channel's VODs; `None` for every channel" (crates/core layers.rs, `Layer::channel`).
test("a layer for one channel applies to that channel's VODs only", async (t) => {
  t.after(() => mock.timers.reset());
  const barky = layer("barky", { channel: { platform: "twitch", channel: "barky" } });
  const layers = await loaded([layer("all"), barky]);
  const ids = (/** @type {any} */ v) => layers.forVod(v).map((/** @type {any} */ l) => l.id);
  assert.deepEqual(ids(vod("twitch", "barky")), ["all", "barky"]);
  assert.deepEqual(ids(vod("twitch", "other")), ["all"], "another channel");
  assert.deepEqual(ids(vod("kick", "barky")), ["all"], "the same name on another site");
});

test("a layer for another channel doesn't take one of the three places", async (t) => {
  t.after(() => mock.timers.reset());
  const elsewhere = { platform: "kick", channel: "someone" };
  const layers = await loaded([
    layer("a", { channel: elsewhere }),
    layer("b", { channel: elsewhere }),
    layer("c", { channel: elsewhere }),
    layer("d"),
  ]);
  assert.deepEqual(
    layers.shownFor(vod()).map((/** @type {any} */ l) => l.id),
    ["d"],
  );
});

// "Colors new layers take, in order, skipping ones in use" (layers.svelte.ts, PALETTE).
test("a new layer takes the first color not in use", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded([layer("a", { color: "#f59e0b" }), layer("b", { color: "#ef4444" })]);
  const added = layers.add(vod());
  assert.equal(added.color, "#ec4899");
  assert.equal(layers.add(vod()).color, "#14b8a6", "the next one after that");
});

test("with every color in use, a new layer takes the palette's turn", async (t) => {
  t.after(() => mock.timers.reset());
  const used = ["#f59e0b", "#ec4899", "#ef4444", "#14b8a6", "#0ea5e9", "#84cc16", "#a855f7"];
  const layers = await loaded(used.map((color, i) => layer(`l${i}`, { color, visible: false })));
  assert.equal(layers.add(vod()).color, "#f59e0b");
});

// "visible: Drawn on the timeline (the UI shows a few at once)"; a new layer is shown if there's room.
test("a new layer is drawn if there's room for it, and kept out of the way if not", async (t) => {
  t.after(() => mock.timers.reset());
  const room = await loaded([layer("a"), layer("b")]);
  assert.equal(room.add(vod()).visible, true);
  assert.equal(room.add(vod()).visible, false, "a fourth");
  assert.equal(room.list.length, 4);
});

test("a new layer starts with no words, for every channel, and its own id", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded();
  const one = layers.add(vod());
  const two = layers.add(vod());
  assert.deepEqual([one.name, one.terms, one.channel], ["New layer", [], null]);
  assert.notEqual(one.id, two.id);
});

test("a new layer is saved with the rest", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded([layer("a")]);
  const added = layers.add(vod());
  await wait();
  assert.deepEqual(
    app.saved[0].layers.map((/** @type {any} */ l) => l.id),
    ["a", added.id],
  );
});

test("a removed layer is gone, and the rest saved", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded([layer("a"), layer("b")]);
  layers.remove("a");
  await wait();
  assert.deepEqual(
    app.saved[0].layers.map((/** @type {any} */ l) => l.id),
    ["b"],
  );
  layers.remove("b");
  await wait();
  assert.deepEqual(app.saved[1].layers, [], "the last one too");
});

test("an event's toggle turns it on and off, and is saved", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded();
  layers.toggleEvent("sub");
  layers.toggleEvent("gift");
  layers.toggleEvent("sub");
  await wait();
  assert.deepEqual(app.saved[0].events, ["gift"]);
});

test("showing one kind of marks leaves the other as it was, and is saved", async (t) => {
  t.after(() => mock.timers.reset());
  const layers = await loaded();
  layers.showMarks({ user: false });
  assert.deepEqual({ ...layers.marks }, { user: false, imported: true });
  layers.showMarks({ imported: false });
  assert.deepEqual({ ...layers.marks }, { user: false, imported: false });
  await wait();
  assert.deepEqual(app.saved[0].marks, { user: false, imported: false });
});
