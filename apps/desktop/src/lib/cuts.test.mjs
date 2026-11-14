// The open VOD's or edit's marks (cuts.svelte.ts), with the app faked: the
// module is compiled as the app's build does (types stripped, then Svelte's
// runes), and each call to the app answers when the test says.
// node --test apps/desktop/src/lib/cuts.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// cuts.svelte.ts's imports from the app are the fakes below; media.ts and
// the key helpers are the .ts files they are.
const fakes = {
  "./api": `export const api = globalThis.fake.api;
    export const docKey = (doc) => doc.kind + ":" + doc.id;`,
  "./keys.svelte": `export const is = () => false;`,
  "./selection.svelte": `export const clearsStretch = () => {};
    export const clearsMark = () => {};
    export const snippets = { clear() {} };
    export const stretchMade = () => {};`,
  "./layers.svelte": `export const layers = { marks: { own: true, imported: true }, showMarks() {} };`,
  "./toasts.svelte": `export const toast = { error: () => 0, drop() {} };`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/cuts.svelte.ts") && fakes[specifier])
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

// The app: marks added get ids from 1 and are kept; nothing else is asked.
/** @type {[string, number][]} */
const sent = [];
/** @type {(() => void)[] | null} */
let held = null;
/** Whether the app refuses the cuts asked for, and how many marks it has made. */
let failing = false;
let made = 0;
/** @type {(id: number, start: number, kind: string) => Promise<unknown>} */
const addMark = (id, start, kind) =>
  failing
    ? (sent.push(["add", start]), Promise.reject(new Error("the app refused")))
    : answer({ id: ++made, start, kind, text: null }, ["add", start]);
globalThis.fake = {
  api: {
    listMarks: async () => [],
    editMarks: async () => [],
    addEditMark: addMark,
    addMark,
    /** @param {number} _id @param {number} start */
    moveMark: async (_id, start) => void sent.push(["move", start]),
    deleteMark: async () => {},
  },
};
/**
 * Answers `value`, at once or once the test lets it (`hold`).
 * @param {unknown} value
 * @param {[string, number]} call
 */
function answer(value, call) {
  sent.push(call);
  const waiting = held;
  if (!waiting) return Promise.resolve(value);
  return new Promise((resolve) => waiting.push(() => resolve(value)));
}
const hold = () => (held = []);
const release = () => {
  const waiting = held;
  held = null;
  for (const go of waiting ?? []) go();
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const { cuts, edgeNudged } = await import("./cuts.svelte.ts");
let opened = 0;
/** @param {"vod" | "edit"} kind */
async function fresh(kind = "edit") {
  cuts.openDoc({ kind, id: ++opened });
  await settle();
  sent.length = 0;
  failing = false;
  cuts.fps = 30;
}

test("an edge of the selection on a join of the edit's pieces gets no cut", async () => {
  await fresh();
  // A piece 10.013 s long, then another: a selection to its end, on the 30 fps grid elsewhere.
  cuts.selection = { start: 5, end: 10.013 };
  await cuts.cutSelection(20, [10.013]);
  assert.deepEqual(sent, [["add", 5]], "a cut sent a sliver inside the earlier piece");
});

test("an edge nudged onto the other stops a frame short of it", async () => {
  await fresh();
  cuts.selection = { start: 1, end: 1 + 1 / 30 };
  cuts.selectedEdge = "end";
  cuts.nudgeSelected(-1, 30, 60);
  const s = cuts.selection;
  assert.ok(s.end > s.start, `a zero span: ${JSON.stringify(s)}`);
  cuts.selectedEdge = "start";
  cuts.nudgeSelected(1, 30, 60);
  assert.ok(cuts.selection.end > cuts.selection.start, `a zero span: ${JSON.stringify(cuts.selection)}`);
});

test("another VOD or edit opened goes back to the usual frame rate until its player says", async () => {
  await fresh();
  cuts.fps = 60;
  cuts.openDoc({ kind: "vod", id: ++opened });
  assert.equal(cuts.fps, 30);
});

test("B pressed again before the first cut is made makes one cut, not two on one frame", async () => {
  await fresh("vod");
  hold();
  const first = cuts.toggle(4, 30);
  const second = cuts.toggle(4, 30);
  release();
  await Promise.all([first, second]);
  await settle();
  assert.equal(sent.length, 1, `cuts asked for: ${JSON.stringify(sent)}`);
  assert.equal(cuts.list.length, 1);
});

test("a piece's edge dragged is the selection, that edge selected, and nothing else", async () => {
  await fresh("edit");
  cuts.selected = 9;
  // Piece 4 plays 2-5 s of the edit; its end dragged.
  cuts.edgeHandled?.(4, { start: 2, end: 5 }, "end");
  assert.deepEqual(
    [cuts.selection, cuts.selectedEdge, cuts.piece, cuts.selected],
    [{ start: 2, end: 5 }, "end", 4, null],
  );
  // Another selection made (I, O, lines picked): the piece is let go.
  cuts.selection = { start: 1, end: 2 };
  assert.equal(cuts.piece, null);
});

test(", and . move a piece's edge a frame, on the grid, inside its VOD and half a second long", () => {
  const range = { start: 10, end: 20, max: 20.5 };
  assert.deepEqual(edgeNudged(range, "end", -1, 25), { start: 10, end: 19.96 });
  assert.deepEqual(edgeNudged(range, "start", 1, 25), { start: 10.04, end: 20 });
  // Not past the VOD's end, nor before its start, nor shorter than half a second.
  assert.deepEqual(edgeNudged(range, "end", 50, 25), { start: 10, end: 20.5 });
  assert.deepEqual(edgeNudged(range, "start", -1000, 25), { start: 0, end: 20 });
  assert.deepEqual(edgeNudged(range, "start", 1000, 25), { start: 19.5, end: 20 });
});

/**
 * cuts.svelte.ts `cutSelection`: "Cuts both ends of the selection, which then
 * goes (the stretch between the cuts shows it); returns that stretch, from
 * cut to cut. With `length`, an end at the start or the end of what plays (an
 * edit's) is no place for a cut and gets none."
 */
test("Split on a selection cuts both its ends, but not an end at the start or end of an edit", async () => {
  await fresh("vod");
  cuts.selection = { start: 2, end: 5 };
  assert.deepEqual(await cuts.cutSelection(), { start: 2, end: 5 });
  assert.deepEqual(sent, [
    ["add", 2],
    ["add", 5],
  ]);
  assert.equal(cuts.selection, null, "the selection goes");

  await fresh("edit");
  cuts.selection = { start: 0, end: 5 };
  assert.deepEqual(await cuts.cutSelection(20), { start: 0, end: 5 });
  cuts.selection = { start: 8, end: 20 };
  assert.deepEqual(await cuts.cutSelection(20), { start: 8, end: 20 });
  assert.deepEqual(sent, [
    ["add", 5],
    ["add", 8],
  ]);
});

/** cuts.svelte.ts's doc: "Split (B) puts one where the playhead is (or takes away the one there)." */
test("Split where a cut is takes it away, and where none is puts one", async () => {
  await fresh("vod");
  await cuts.toggle(4, 30);
  assert.equal(cuts.list.length, 1);
  await cuts.toggle(4, 30);
  await settle();
  assert.equal(cuts.list.length, 0);
  assert.deepEqual(sent, [["add", 4]], "taking it away asks for no cut");
});

/** cuts.svelte.ts `setEdge`: "Moves the selection's `edge` to `time`; dragged past the other edge, it becomes that one." */
test("an edge of the selection dragged past the other becomes that other edge", async () => {
  await fresh();
  cuts.selection = { start: 2, end: 5 };
  cuts.setEdge("start", 8);
  assert.deepEqual([cuts.selection, cuts.selectedEdge], [{ start: 5, end: 8 }, "end"]);
  cuts.setEdge("end", 1);
  assert.deepEqual([cuts.selection, cuts.selectedEdge], [{ start: 1, end: 5 }, "start"]);
  cuts.setEdge("end", 7);
  assert.deepEqual([cuts.selection, cuts.selectedEdge], [{ start: 1, end: 7 }, "end"]);
});

/** cuts.svelte.ts `toggle`'s doc: "Puts a mark on the frame showing at `time` (selected, if `select`)". */
test("a cut put by Split is selected, but the cuts a selection's Split makes are not", async () => {
  await fresh("vod");
  await cuts.toggle(4, 30);
  assert.equal(cuts.selected, cuts.list[0].id);
  cuts.selected = null;
  cuts.selection = { start: 8, end: 9 };
  await cuts.cutSelection();
  assert.equal(cuts.selected, null);
});

/** 0.7.1 Fixes: "B twice quickly makes one cut. It made two on the same frame." (two frames are two cuts) */
test("Split pressed on two frames before either cut is made makes a cut on each", async () => {
  await fresh("vod");
  hold();
  const both = [cuts.toggle(4, 30), cuts.toggle(5, 30)];
  release();
  await Promise.all(both);
  assert.deepEqual(sent, [
    ["add", 4],
    ["add", 5],
  ]);
  assert.equal(cuts.list.length, 2);
});

/** cuts.svelte.ts's doc: "Split (B) puts one where the playhead is (or takes away the one there)." */
test("Split on a frame again after its cut was made and taken away makes a cut again", async () => {
  await fresh("vod");
  await cuts.toggle(4, 30);
  await cuts.toggle(4, 30);
  await settle();
  await cuts.toggle(4, 30);
  assert.equal(cuts.list.length, 1);
  assert.deepEqual(sent, [
    ["add", 4],
    ["add", 4],
  ]);
});

/** cuts.svelte.ts `Cuts.error`: "What last went wrong, if anything: shown as a toast". */
test("a cut the app refuses isn't listed, says what went wrong, and Split can ask again", async () => {
  await fresh("vod");
  failing = true;
  await cuts.toggle(4, 30);
  assert.equal(cuts.list.length, 0);
  assert.match(cuts.error ?? "", /the app refused/);
  failing = false;
  await cuts.toggle(4, 30);
  assert.equal(cuts.list.length, 1);
  assert.equal(cuts.error, null, "put right, the error goes");
});

/** cuts.svelte.ts `toggle`: "Another VOD or edit open by now: the mark is that one's, not to select here." */
test("a cut made after another VOD was opened isn't listed or selected there", async () => {
  await fresh("vod");
  hold();
  const first = cuts.toggle(4, 30);
  cuts.openDoc({ kind: "vod", id: ++opened });
  release();
  await first;
  await settle();
  assert.equal(cuts.list.length, 0);
  assert.equal(cuts.selected, null);
});

/** cuts.svelte.ts `cutSelection`: "With `length`, an end at the start or the end of what plays (an edit's) is no place for a cut". */
test("Split on a selection from the very start of a VOD, which plays no edit's length, cuts there too", async () => {
  await fresh("vod");
  cuts.selection = { start: 0, end: 5 };
  await cuts.cutSelection();
  assert.deepEqual(sent, [
    ["add", 0],
    ["add", 5],
  ]);
});

/** cuts.svelte.ts `cutSelection`: "Another VOD or edit opened meanwhile: its own marks aren't cut, nor is a clip made." */
test("Split on a selection while another VOD is opened makes no more cuts and no stretch", async () => {
  await fresh("vod");
  cuts.selection = { start: 2, end: 5 };
  hold();
  const split = cuts.cutSelection();
  cuts.openDoc({ kind: "vod", id: ++opened });
  release();
  assert.equal(await split, null);
  await settle();
  assert.deepEqual(sent, [["add", 2]], "the second end isn't cut");
});

/** cuts.svelte.ts's doc: "A selected mark moves by frames with , and ." */
test("a selected mark moves a frame on, is saved there and the new time comes back", async () => {
  await fresh("vod");
  await cuts.toggle(4, 30);
  sent.length = 0;
  const to = cuts.nudgeSelected(1, 30, 60);
  assert.equal(to, 121 / 30);
  assert.equal(cuts.list[0].start, 121 / 30);
  assert.deepEqual(sent, [["move", 121 / 30]]);
  cuts.selected = null;
  assert.equal(cuts.nudgeSelected(1, 30, 60), null, "nothing selected, nothing moves");
});

/** cuts.svelte.ts `nudgeSelected`: "Moves the selected mark, or the selection's selected edge, `frames` frames". */
test("a selection's selected edge moves a frame away from the other edge", async () => {
  await fresh();
  cuts.selection = { start: 1, end: 5 };
  cuts.selectedEdge = "start";
  assert.equal(cuts.nudgeSelected(1, 30, 60), 31 / 30);
  assert.deepEqual(cuts.selection, { start: 31 / 30, end: 5 });
  cuts.selectedEdge = "end";
  assert.equal(cuts.nudgeSelected(-1, 30, 60), 149 / 30);
  assert.deepEqual(cuts.selection, { start: 31 / 30, end: 149 / 30 });
});

/** cuts.svelte.ts `edgeNudged`: "inside the VOD and half a second long at least". */
test(", on a piece's end never leaves it shorter than half a second", () => {
  assert.deepEqual(edgeNudged({ start: 10, end: 20, max: 20.5 }, "end", -1000, 25), { start: 10, end: 10.5 });
});

/** cuts.svelte.ts `cutSelection`'s answer is the stretch "or null": with nothing selected there is none and no cut is asked for. */
test("Split with nothing selected cuts nothing", async () => {
  await fresh("vod");
  assert.equal(await cuts.cutSelection(), null);
  assert.deepEqual(sent, []);
});

/** cuts.svelte.ts `setEdge`: "Moves the selection's `edge`": with no selection there is none to move. */
test("an edge set with no selection makes no selection", async () => {
  await fresh();
  cuts.setEdge("start", 3);
  assert.equal(cuts.selection, null);
  assert.equal(cuts.selectedEdge, null);
});

/** cuts.svelte.ts's doc: "A selected mark moves by frames with , and ." (a selection with no edge selected stays as it is) */
test(", and . move the selected mark, not a selection whose edge isn't selected", async () => {
  await fresh("vod");
  await cuts.toggle(4, 30);
  cuts.selection = { start: 10, end: 12 };
  cuts.selected = cuts.list[0].id;
  assert.equal(cuts.nudgeSelected(1, 30, 60), 121 / 30);
  assert.deepEqual(cuts.selection, { start: 10, end: 12 });
});
