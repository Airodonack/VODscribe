// The carry engine (dragout.ts), with the page and the app faked: pointer
// events come when the test says. node --test apps/desktop/src/lib/dragout.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// dragout.ts's imports from the app are the fakes below, each kept on
// globalThis.fake for the test to read; pressToDrag is the .ts file it is.
const fakes = {
  "./api": `export const api = globalThis.fake.api;
    export const events = { dragEnded: async () => () => {} };
    export const thumbUrl = async () => null;`,
  "./appSettings.svelte": `export const settings = { load: async () => {}, value: { clip_format: "mp4" } };`,
  "./resolveHint.svelte": `export const formatHint = { dragged() {} };`,
  "./ask": `export const ask = () => new Promise((answer) => (globalThis.fake.answer = answer));`,
  "./clips.svelte": `export const clips = globalThis.fake.clips;`,
  "./cuts.svelte": `export const cuts = { list: [] };`,
  "./edits.svelte": `export const edits = globalThis.fake.edits;
    export const putOf = (s) => ({ kind: "range", vodId: s.vodId, start: s.start, end: s.end, title: s.title });`,
  "./media": `export const PEAKS_RATE = 1; export const columns = () => []; export const stretchIn = () => undefined;`,
  "./platform": `export const formatTime = (s) => String(s); export const playsFromFile = () => true;`,
};
const hooks = `
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (context.parentURL?.endsWith("/dragout.ts") && fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier === "./pressToDrag" && context.parentURL?.endsWith("/dragout.ts"))
    return next("./pressToDrag.ts", context);
  return next(specifier, context);
}`;
register("data:text/javascript," + encodeURIComponent(hooks));

/** @typedef {{ type: string, clientX?: number, clientY?: number, buttons?: number }} Sent */
/** The window, as far as dragout.ts listens to it. */
const listening = new Map();
/** @param {Sent} e */
const send = (e) => {
  for (const cb of [...(listening.get(e.type) ?? [])]) cb({ preventDefault() {}, ...e });
};
const element = () => ({
  className: "",
  style: {},
  dataset: {},
  width: 0,
  height: 0,
  classList: { add() {}, remove() {} },
  append() {},
  remove() {},
  animate() {},
  getContext: () => null,
  decode: async () => {},
  /** @param {(b: null) => void} r */
  toBlob: (r) => r(null),
});
/** What's under the pointer: an edit's row, or nothing. @type {unknown} */
let under = null;
/** @param {number} id */
const editRow = (id) => ({
  /** @param {string} s */
  closest: (s) => (s === "[data-edit-drop]" ? { dataset: { editDrop: String(id) } } : null),
});
const fake = {
  /** @type {((made: boolean) => void) | null} */
  answer: null,
  /** @type {unknown[][]} */
  added: [],
  /** @type {unknown[]} */
  dragged: [],
  api: {
    frame: async () => new Uint8Array(),
    peaks: async () => new Uint8Array(),
    /** @param {unknown} ids */
    dragClips: async (ids) => void fake.dragged.push(ids),
    deleteClip: async () => {},
    addClip: async () => null,
    setClipRange: async () => null,
    listVods: async () => [],
    listClips: async () => [],
  },
  clips: {
    /** @type {unknown} */
    carrying: null,
    carried: 0,
    overBar: false,
    doc: { kind: "vod", id: 1 },
    vodId: 1,
    fromSite: true,
    error: null,
    list: [],
    find: () => undefined,
    /** @param {number} start @param {number} end @param {boolean} shown */
    make: async (start, end, shown) => ({ id: 9, start, end, shown, status: "ready", file_path: "/c.mp4" }),
    /** @param {number} id @param {() => boolean} held */
    ready: async (id, held) => (held() ? { id, start: 0, end: 900, status: "ready", file_path: "/c.mp4" } : null),
    move: async () => {},
    makeAll: async () => {},
  },
  edits: {
    openId: null,
    dropTarget: null,
    inserting: null,
    /** @param {unknown[]} a */
    add: (...a) => fake.added.push(["add", ...a]),
    /** @param {unknown[]} a */
    put: (...a) => fake.added.push(["put", ...a]),
    /** @param {unknown[]} a */
    addRange: (...a) => fake.added.push(["addRange", ...a]),
    /** @param {unknown[]} a */
    createWith: (...a) => fake.added.push(["createWith", ...a]),
  },
};
Object.assign(globalThis, {
  fake,
  window: {
    /** @param {string} type @param {(e: unknown) => void} cb */
    addEventListener: (type, cb) => listening.set(type, new Set([...(listening.get(type) ?? []), cb])),
    /** @param {string} type @param {(e: unknown) => void} cb */
    removeEventListener: (type, cb) => listening.get(type)?.delete(cb),
    getSelection: () => null,
  },
  innerWidth: 1000,
  innerHeight: 800,
  getComputedStyle: () => ({ getPropertyValue: () => "" }),
  document: {
    createElement: element,
    body: { appendChild() {}, classList: { add() {}, remove() {} } },
    documentElement: {},
    elementFromPoint: () => under,
    querySelector: () => null,
  },
});
const { carry } = await import("./dragout.ts");

/** @param {number} x @param {number} y @param {number} [buttons] */
const move = (x, y, buttons = 1) => send({ type: "pointermove", clientX: x, clientY: y, buttons });
/** @param {number} x @param {number} y */
const up = (x, y) => send({ type: "pointerup", clientX: x, clientY: y, buttons: 0 });
/** A stretch of a site's VOD, carried from (100, 100). @param {number} end */
const carried = (end) =>
  carry(
    /** @type {Parameters<typeof carry>[0]} */ ({
      vod: { id: 1, audio_only: true, thumb: null },
      start: 0,
      end,
      ownPlayer: false,
      from: { left: 0, top: 0, width: 10, height: 10 },
      x: 100,
      y: 100,
    }),
  );
function reset() {
  fake.added.length = 0;
  fake.dragged.length = 0;
  fake.answer = null;
  under = null;
}

test("let go where the page didn't see it, back with no button held: the carry is over, a later click drops nothing", async () => {
  reset();
  const done = carried(5);
  move(120, 120);
  under = editRow(7);
  move(200, 200, 0);
  assert.equal(fake.clips.carrying, null, "still carried with no button held");
  up(200, 200);
  await done;
  assert.deepEqual(fake.added, []);
});

test("a pointer cancelled mid-carry ends it, as a press ends", async () => {
  reset();
  const done = carried(5);
  move(120, 120);
  send({ type: "pointercancel", buttons: 0 });
  assert.equal(fake.clips.carrying, null);
  await done;
});

test("at the window's edge, a question the system asks takes the release: no drag out begins", async () => {
  reset();
  // A site VOD's 15 minutes: made at the edge, asked about first.
  const done = carried(900);
  move(0, 300);
  await new Promise((r) => setTimeout(r, 20));
  assert.ok(fake.answer, "the long download was asked about");
  // The question had the release; the pointer moves on with no button held.
  move(300, 300, 0);
  fake.answer(true);
  await done;
  assert.deepEqual(fake.dragged, [], "the system's drag was asked for with no button held");
});
