// Undo and redo (undo.ts) with the app and the toasts faked: what each asks
// the app, when, and what the message says and offers.
// node --test apps/desktop/src/lib/undo.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// undo.ts?demo is a second copy of it, loaded as on the website's demo.
const fakes = {
  "./api": `export const api = globalThis.fake.api;`,
  "./demo": `export const DEMO = false;`,
  "./toasts.svelte": `export const toast = globalThis.fake.toast;`,
};
const hooks = `
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  const parent = context.parentURL ?? "";
  if (parent.endsWith("/undo.ts?demo") && specifier === "./demo")
    return { url: "data:text/javascript," + encodeURIComponent("export const DEMO = true;"), shortCircuit: true };
  if (/\\/undo\\.ts(\\?demo)?$/.test(parent) && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier.startsWith("./") && !/\\.[a-z]+(\\?\\w+)?$/.test(specifier) && parent.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

/** What the app was asked, in order: "undo vod 1". @type {string[]} */
const asked = [];
/** What the app answers an undo or redo: a `Done`, or null. @type {{ undo: unknown, redo: unknown }} */
const answers = { undo: null, redo: null };
/** The messages shown, each with its button. @type {{ message: string, action?: { label: string, run: () => void } }[]} */
const shown = [];
/** @param {"undo" | "redo"} what */
const answer = (what) => async (/** @type {{ kind: string, id: number }} */ doc) => {
  asked.push(`${what} ${doc.kind} ${doc.id}`);
  return answers[what];
};
globalThis.fake = {
  api: { undo: answer("undo"), redo: answer("redo") },
  toast: { show: (/** @type {string} */ message, /** @type {any} */ action) => shown.push({ message, action }) },
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const fresh = () => {
  asked.length = 0;
  shown.length = 0;
  answers.undo = answers.redo = null;
};
/** The messages shown, each with its button's label. */
const said = () => shown.map((s) => [s.message, s.action?.label]);

const { undoRedo, undoIn } = await import("./undo.ts");
const { saving } = await import("./writes.ts");

// undo.ts `undoRedo`: "after the changes still being saved (writes.ts), so
// it's never one from before them"; 0.7.1 Fixes: "Undo undoes the latest change."
test("Ctrl+Z right after a change waits for the change to be saved", async () => {
  fresh();
  /** @type {() => void} */
  let saved = () => {};
  saving(new Promise((resolve) => (saved = () => resolve(undefined))));
  undoRedo(false, { kind: "vod", id: 1 });
  await settle();
  // Asked now, the app could take the undo before the change and undo the one before it.
  assert.deepEqual(asked, []);
  saved();
  await settle();
  assert.deepEqual(asked, ["undo vod 1"]);
});

// writes.ts `saved`: "Resolves once every change sent so far has been answered, or failed."
test("a change that fails to save still lets the undo through", async () => {
  fresh();
  /** @type {(e: Error) => void} */
  let failed = () => {};
  saving(new Promise((_, reject) => (failed = reject)));
  undoRedo(true, { kind: "vod", id: 1 });
  failed(new Error("gone"));
  await settle();
  await settle();
  assert.deepEqual(asked, ["redo vod 1"]);
});

// 0.7.0 Fixes: "Undo in the message offers Redo. ... Undo in the message
// offers Redo once used, as Ctrl+Z (⌘Z) does"; undo.ts: "each says what it
// did, offers the way back (Redo after an undo ...)".
test("an undo says what it undid and offers Redo, which redoes it in the same document", async () => {
  fresh();
  answers.undo = { doc: { kind: "edit", id: 4 }, label: "Piece removed", revision: 7 };
  undoRedo(false, { kind: "edit", id: 4 });
  await settle();
  assert.deepEqual(said(), [["Undone: Piece removed", "Redo"]]);
  shown[0].action?.run();
  await settle();
  assert.deepEqual(asked, ["undo edit 4", "redo edit 4"]);
});

// undo.ts: "offers the way back (... Undo after a redo)".
test("a redo says what it redid and offers Undo, which undoes it in the same document", async () => {
  fresh();
  answers.redo = { doc: { kind: "vod", id: 2 }, label: "Cut added", revision: 3 };
  undoRedo(true, { kind: "vod", id: 2 });
  await settle();
  assert.deepEqual(said(), [["Redone: Cut added", "Undo"]]);
  shown[0].action?.run();
  await settle();
  assert.deepEqual(asked, ["redo vod 2", "undo vod 2"]);
});

// 0.7.0 Fixes: "Undo in the message offers Redo": it "says when there's
// nothing left to undo"; undo.ts: "says when there's nothing to undo".
test("with nothing to undo or redo, the message says so and offers nothing", async () => {
  fresh();
  undoRedo(false, { kind: "vod", id: 1 });
  undoRedo(true, { kind: "vod", id: 1 });
  await settle();
  assert.deepEqual(said(), [
    ["Nothing to undo", undefined],
    ["Nothing to redo", undefined],
  ]);
});

// undo.ts `undoIn`: "Undoes the newest change in `doc`'s history (a toast's
// Undo)"; 0.7.0 Fixes: "After removing pieces, lines, clips or stretches,
// Undo in the message offers Redo once used".
test("a removal's Undo undoes in its document and then offers Redo", async () => {
  fresh();
  answers.undo = { doc: { kind: "edit", id: 9 }, label: "Lines removed", revision: 2 };
  undoIn({ kind: "edit", id: 9 });
  await settle();
  assert.deepEqual(asked, ["undo edit 9"]);
  assert.deepEqual(said(), [["Undone: Lines removed", "Redo"]]);
});

// undo.ts: "does nothing on the website's demo".
test("on the website's demo, undo and redo do nothing", async () => {
  fresh();
  // A name the type checker doesn't resolve: the query is the hooks' alone.
  const copy = "./undo.ts?demo";
  const demo = await import(copy);
  demo.undoRedo(false, { kind: "vod", id: 1 });
  demo.undoRedo(true, { kind: "vod", id: 1 });
  demo.undoIn({ kind: "edit", id: 2 });
  await settle();
  assert.deepEqual(asked, []);
  assert.deepEqual(shown, []);
});
