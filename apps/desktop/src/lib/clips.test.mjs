// The clips store (clips.svelte.ts), with the app faked: compiled as the
// app's build does (types stripped, then Svelte's runes); each call to the
// app answers when the test says, and the test sends the app's events. A
// stretch of the VOD shown dragged out goes through the store as the page's
// carry engine (dragout.ts, the real one, its other imports faked) asks it.
// node --test apps/desktop/src/lib/clips.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// clips.svelte.ts's imports from the app are the fakes below; where.ts is the .ts file it is.
const fakes = {
  "./api": `export const api = globalThis.fake.api; export const events = globalThis.fake.events;
    export const docKey = (doc) => doc.kind + ":" + doc.id;`,
  "./permission": `export const hideNote = () => {}; export const noteHidden = () => globalThis.fake.noteHidden;`,
  "./selection.svelte": `export const snippets = { items: [], set() {} };`,
  "./toasts.svelte": `export const toast = { error: () => 0, drop() {} };`,
};
// dragout.ts's, but for the store (the real one above) and pressToDrag (the .ts file it is).
const dragFakes = {
  "./api": fakes["./api"] + `export const thumbUrl = async () => null;`,
  "./appSettings.svelte": `export const settings = { load: async () => {},
    value: { clip_format: "mp4", clip_codec: "h264", clip_quality: "standard" } };`,
  "./resolveDrop.svelte": `export const resolveNote = { show() {} };`,
  "./toasts.svelte": `export const toast = { show: () => 0, showUnlessUp: () => 0, drop() {} };`,
  "./cuts.svelte": `export const cuts = { list: [] };`,
  "./edits.svelte": `export const edits = { openId: null }; export const putOf = (s) => ({ kind: "range", ...s });`,
  "./media": `export const PEAKS_RATE = 1; export const columns = () => []; export const stretchIn = () => undefined;`,
  "./platform": `export const formatTime = (t) => String(t); export const playsFromFile = () => true;`,
};
const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
const dragFakes = ${JSON.stringify(dragFakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/clips.svelte.ts") && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (context.parentURL?.endsWith("/dragout.ts") && dragFakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(dragFakes[specifier]), shortCircuit: true };
  if (specifier === "./clips.svelte" && context.parentURL?.endsWith("/dragout.ts"))
    return next("./clips.svelte.ts", context);
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

/** The events the store listens to, by name, and the answers held back.
 *  @type {Record<string, (value: any) => void>} */
const heard = {};
/** @type {Map<string, (value: unknown) => void>} */
const held = new Map();
/** A call to the app answered once the test says (`answer`). @param {string} name */
const later = (name) => () => new Promise((resolve) => held.set(name, resolve));
/** @param {string} name @param {unknown} value */
const answer = (name, value) => held.get(name)?.(value);
/** @param {string} name */
const on = (name) => (/** @type {(value: any) => void} */ cb) => ((heard[name] = cb), Promise.resolve(() => {}));
// The page's window and document, as far as the store and the carry engine use them.
/** @type {Map<string, Set<(e: any) => void>>} */
const listening = new Map();
/** @param {{ type: string, clientX?: number, clientY?: number, buttons?: number }} e */
const send = (e) => {
  for (const cb of [...(listening.get(e.type) ?? [])]) cb({ preventDefault() {}, ...e });
};
const element = () => ({
  style: {},
  dataset: {},
  classList: { add() {}, remove() {} },
  append() {},
  remove() {},
  animate() {},
  getContext: () => null,
});
Object.assign(globalThis, {
  window: {
    /** @param {string} type @param {(e: any) => void} cb */
    addEventListener: (type, cb) => listening.set(type, new Set([...(listening.get(type) ?? []), cb])),
    /** @param {string} type @param {(e: any) => void} cb */
    removeEventListener: (type, cb) => listening.get(type)?.delete(cb),
    getSelection: () => null,
  },
  innerWidth: 1000,
  innerHeight: 800,
  document: {
    createElement: element,
    body: { appendChild() {}, classList: { add() {}, remove() {} } },
    elementFromPoint: () => null,
    querySelector: () => null,
  },
});
/** What the page asked of `add_clip`, in order (and of `add_piece_clip`, named). @type {unknown[][]} */
const added = [];
/** The clips' files dragged out of the window, by each drag's ids. @type {unknown[]} */
const dragged = [];
/** What listens for the system's drags to end. @type {Set<(e: any) => void>} */
const dragEnds = new Set();
globalThis.fake = {
  noteHidden: true,
  api: {
    addClip: (/** @type {unknown[]} */ ...asked) => (added.push(asked), later("addClip")()),
    addPieceClip: (/** @type {unknown[]} */ ...asked) => (added.push(["piece", ...asked]), later("addPieceClip")()),
    addPieceClips: (/** @type {unknown[]} */ ...asked) => (added.push(["pieces", ...asked]), later("addPieceClips")()),
    forgetGoneClips: async () => {},
    listClips: later("listClips"),
    renameClip: later("renameClip"),
    deleteClip: async () => {},
    keepClip: async () => {},
    peaks: async () => new Uint8Array(),
    dragBeforeMade: async () => true,
    /** @param {unknown} ids */
    dragClips: async (ids) => (dragged.push(ids), true),
  },
  events: {
    clip: on("clip"),
    clipProgress: on("clipProgress"),
    clipRemoved: on("clipRemoved"),
    dragEnded: async (/** @type {(e: any) => void} */ cb) => (dragEnds.add(cb), () => dragEnds.delete(cb)),
    dragMaking: async () => () => {},
  },
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
/** @param {number} id @param {string} status */
const clip = (id, status, title = "Clip") => ({
  id,
  vod_id: 1,
  bar: null,
  start: 1,
  end: 2,
  title,
  status,
  shown: true,
});
const pills = () => clips.list.map((c) => [c.id, c.status]);

const { clips } = await import("./clips.svelte.ts");
const { carry } = await import("./dragout.ts");

test("news of a clip that came before the bar's list answers isn't undone by the list", async () => {
  clips.open(1, false);
  await settle();
  // Listed while it was cut; its "ready" reaches the page first.
  heard.clip(clip(1, "ready"));
  answer("listClips", [clip(1, "cutting"), clip(2, "ready")]);
  await settle();
  assert.deepEqual(pills(), [
    [1, "ready"],
    [2, "ready"],
  ]);
});

test("an answer that comes after the clip's deletion doesn't bring its pill back", async () => {
  const renamed = clips.rename(2, "Renamed");
  heard.clipRemoved(2);
  answer("renameClip", clip(2, "ready", "Renamed"));
  await renamed;
  assert.deepEqual(pills(), [[1, "ready"]], "a deleted clip's pill is back");
});

test("a list that comes after a clip's deletion doesn't bring its pill back", async () => {
  clips.reload({ kind: "vod", id: 1 });
  heard.clipRemoved(1);
  answer("listClips", [clip(1, "ready")]);
  await settle();
  assert.deepEqual(pills(), []);
});

test("an answer with no news since is shown", async () => {
  heard.clip(clip(3, "ready"));
  const renamed = clips.rename(3, "Renamed");
  answer("renameClip", clip(3, "ready", "Renamed"));
  await renamed;
  assert.deepEqual(
    clips.list.map((c) => c.title),
    ["Renamed"],
  );
});

test("C on the stretch a changed clip is listed at asks the app for that clip, with no note", async () => {
  // A site's VOD, the note on downloading not hidden.
  globalThis.fake.noteHidden = false;
  clips.open(2, true);
  await settle();
  // Listed as the app lists a clip changed in the editor: at its first piece's
  // stretch (app clips.rs `make_clips` gives that clip: Decided 1; the
  // clips-fixes review, M1).
  const changed = { ...clip(7, "ready"), vod_id: 2, start: 10, end: 20 };
  answer("listClips", [changed]);
  await settle();
  added.length = 0;
  const made = clips.make(10, 20);
  await settle();
  assert.equal(clips.asking, null, "the note asked");
  assert.deepEqual(added, [[2, 10, 20, true]]);
  answer("addClip", changed);
  assert.equal((await made)?.id, 7);
  assert.deepEqual(pills(), [[7, "ready"]]);
  // A stretch with no clip asks the note first.
  const fresh = clips.make(30, 40);
  await settle();
  assert.notEqual(clips.asking, null, "no note asked for a new clip");
  clips.answer(false);
  assert.equal(await fresh, null);
  globalThis.fake.noteHidden = true;
});

/** Carries `start`..`end` of VOD `vodId` (the one shown) from (100, 100), as an
 *  Alt-drag on its timeline does, `ownPlayer` if it plays from a file here. */
const carried = (/** @type {number} */ vodId, start = 10, end = 20, ownPlayer = true) =>
  carry(
    /** @type {any} */ ({
      vod: { id: vodId, audio_only: true, thumb: null },
      start,
      end,
      ownPlayer,
      from: { left: 0, top: 0, width: 10, height: 10 },
      x: 100,
      y: 100,
    }),
  );

test(
  "a stretch of the VOD shown dragged out is its footage, not a clip changed in the editor listed there",
  { timeout: 5000 },
  async () => {
    // A VOD played from a file here: the clip is made as the carry begins.
    clips.open(3, false);
    await settle();
    // Listed at 10..20, its first piece's stretch, with a piece added after it
    // in the editor: it plays more than the stretch.
    const changed = { ...clip(7, "ready"), vod_id: 3, start: 10, end: 20, file_path: "/7.mp4", plain: false };
    answer("listClips", [changed]);
    await settle();
    added.length = dragged.length = 0;
    const done = carried(3);
    await settle();
    // The app answers as it does (app clips.rs `make_clips`): add_clip with the
    // clip listed at the stretch, add_piece_clip with a new one of just it.
    const fresh = {
      ...clip(20, "ready"),
      vod_id: 3,
      start: 10,
      end: 20,
      shown: false,
      file_path: "/20.mp4",
      plain: true,
    };
    answer("addClip", changed);
    answer("addPieceClip", fresh);
    send({ type: "pointermove", clientX: 0, clientY: 300, buttons: 1 });
    for (let i = 0; i < 10 && !dragged.length; i++) await settle();
    assert.deepEqual(dragged, [[20]], "the changed clip handed over");
    assert.deepEqual(added, [["piece", 3, 10, 20, false]]);
    for (const cb of [...dragEnds])
      cb({ drag: 1, delivered: true, failed: null, finished_only: false, receiver: null });
    send({ type: "pointerup", clientX: 0, clientY: 300, buttons: 0 });
    await done;
  },
);

test(
  "a stretch of a site's VOD dragged out where a changed clip is listed asks the note first; an unchanged one is given",
  { timeout: 5000 },
  async () => {
    // A site's VOD, the note on downloading not hidden: made at the edge.
    globalThis.fake.noteHidden = false;
    clips.open(4, true);
    await settle();
    const changed = { ...clip(8, "ready"), vod_id: 4, start: 10, end: 20, file_path: "/8.mp4", plain: false };
    const kept = { ...clip(9, "ready"), vod_id: 4, start: 30, end: 40, file_path: "/9.mp4", plain: true };
    answer("listClips", [changed, kept]);
    await settle();
    added.length = dragged.length = 0;
    // Over a changed clip's stretch: footage to download, so the note asks
    // at the edge, and no drag begins.
    const asked = carried(4, 10, 20, false);
    send({ type: "pointermove", clientX: 0, clientY: 300, buttons: 1 });
    for (let i = 0; i < 10 && !clips.asking; i++) await settle();
    assert.notEqual(clips.asking, null, "no note asked before downloading the stretch");
    await asked;
    assert.deepEqual(dragged, []);
    clips.answer(false);
    await settle();
    // An unchanged clip of the stretch is the app's to give: no note.
    const given = carried(4, 30, 40, false);
    send({ type: "pointermove", clientX: 0, clientY: 300, buttons: 1 });
    await settle();
    assert.equal(clips.asking, null, "the note asked for a clip made already");
    assert.deepEqual(added, [["piece", 4, 30, 40, false]]);
    answer("addPieceClip", kept);
    for (let i = 0; i < 10 && !dragged.length; i++) await settle();
    assert.deepEqual(dragged, [[9]]);
    for (const cb of [...dragEnds])
      cb({ drag: 2, delivered: true, failed: null, finished_only: false, receiver: null });
    send({ type: "pointerup", clientX: 0, clientY: 300, buttons: 0 });
    await given;
    globalThis.fake.noteHidden = true;
  },
);

test("stretches dragged from the VOD onto its clip bar are its footage too, C's are the clips listed there", async () => {
  clips.open(5, false);
  await settle();
  const changed = { ...clip(10, "ready"), vod_id: 5, start: 10, end: 20, plain: false };
  answer("listClips", [changed]);
  await settle();
  added.length = 0;
  // Several picked and dropped on the clip bar (dragout.ts `carrySet`).
  const dropped = clips.makeAll([{ start: 10, end: 20 }], true);
  await settle();
  assert.deepEqual(added, [["pieces", 5, [[10, 20]]]]);
  answer("addPieceClips", [{ ...changed, id: 11, plain: true }]);
  await dropped;
  // C on the same stretches (bulk.ts `clipSegments`) gives the clip listed there.
  added.length = 0;
  const pressed = clips.makeAll([{ start: 10, end: 20 }]);
  await settle();
  assert.deepEqual(added, [[5, 10, 20, true]]);
  answer("addClip", changed);
  await pressed;
});
