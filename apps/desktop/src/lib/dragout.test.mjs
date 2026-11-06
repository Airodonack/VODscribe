// The carry engine (dragout.ts), with the page and the app faked: pointer
// events come when the test says. node --test apps/desktop/src/lib/dragout.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// dragout.ts's imports from the app are the fakes below, each kept on
// globalThis.fake for the test to read; pressToDrag is the .ts file it is.
const fakes = {
  "./api": `export const api = globalThis.fake.api;
    export const events = {
      dragEnded: async (cb) => (globalThis.fake.listeners.add(cb), () => globalThis.fake.listeners.delete(cb)),
      dragMaking: async (cb) => (globalThis.fake.makingListeners.add(cb), () => globalThis.fake.makingListeners.delete(cb)),
    };
    export const thumbUrl = async () => null;`,
  "./appSettings.svelte": `export const settings = globalThis.fake.settings;`,
  "./resolveDrop.svelte": `export const resolveNote = {
    show: (how, count, remake, what = "clip") => (
      globalThis.fake.resolve.push(what === "clip" ? [how, count] : [how, count, what]),
      (globalThis.fake.remake = remake)
    ),
  };`,
  "./toasts.svelte": `export const toast = globalThis.fake.toast;`,
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
  /** How the note before a site's first download is answered, while it's up. @type {((made: boolean) => void) | null} */
  answer: null,
  /** Told how the note is answered next (`clips.nextAnswer`). @type {((yes: boolean) => void)[]} */
  heard: [],
  /** Whether that note comes up. */
  noted: false,
  /** What the page listens with for the system's drags to end ("drag-ended"). @type {Set<(e: unknown) => void>} */
  listeners: new Set(),
  /** The number each system's drag was asked for with, in order. @type {unknown[]} */
  numbers: [],
  /**
   * The system's drag numbered `drag` (the last asked for, unless said) ends
   * as `e` says: each listener hears it, as each hears Tauri's events.
   * @param {{ delivered: boolean, failed: string | null, finished_only: boolean, receiver: string | null, resolve?: "refused" | "maybe" }} e
   * @param {unknown} [drag]
   */
  ended: (e, drag = fake.numbers.at(-1)) => {
    for (const cb of [...fake.listeners]) cb({ drag, ...e });
  },
  /** What the page listens with for drags let go on another app while still being made ("drag-making"). @type {Set<(e: unknown) => void>} */
  makingListeners: new Set(),
  /**
   * The system's drag numbered `drag` (the last asked for, unless said) was
   * let go on another app with `making` of its clips still being made.
   * @param {number} making
   * @param {unknown} [drag]
   */
  making: (making, drag = fake.numbers.at(-1)) => {
    for (const cb of [...fake.makingListeners]) cb({ drag, making });
  },
  /** Whether the system's drag can begin before the files are made. */
  beforeMade: true,
  /** How the app answers a drag asked for: begun, not (the button came up first), or refused. @type {() => boolean} */
  begins: () => true,
  /** @type {number[]} */
  deleted: [],
  /** @type {number[]} */
  waited: [],
  /** @type {unknown[][]} */
  added: [],
  /** @type {unknown[]} */
  dragged: [],
  /** The clips made, in order. @type {{ shown: boolean }[]} */
  made: [],
  /** Clips kept in the clip bar. @type {number[]} */
  kept: [],
  /** The Resolve notes shown: how it went, and how many clips. @type {[string, number][]} */
  resolve: [],
  /** What the last note's Switch to MOV does, once clips are MOV. @type {(() => void) | undefined} */
  remake: undefined,
  /** The format clips are made in. */
  settings: { load: async () => {}, value: { clip_format: "mp4" } },
  /** Clips made again (`clips.move`): id, start and end. @type {number[][]} */
  moved: [],
  /** What the toasts said. @type {string[]} */
  toasts: [],
  /** The toasts hidden as no longer so, by their place in `toasts`. @type {number[]} */
  dropped: [],
  toast: {
    /** @param {string} message */
    show: (message) => fake.toasts.push(message) - 1,
    /** @param {string} message */
    showUnlessUp: (message) =>
      fake.toasts.at(-1) === message ? fake.toasts.length - 1 : fake.toasts.push(message) - 1,
    /** @param {number} seq */
    drop: (seq) => void fake.dropped.push(seq),
  },
  api: {
    frame: async () => new Uint8Array(),
    peaks: async () => new Uint8Array(),
    /** @param {unknown} ids @param {unknown} _image @param {unknown} drag */
    dragClips: async (ids, _image, drag) => (fake.dragged.push(ids), fake.numbers.push(drag), fake.begins()),
    dragBeforeMade: async () => fake.beforeMade,
    /** @param {number} id @param {unknown} drag */
    dragEdit: async (id, drag) => (fake.dragged.push(["edit", id]), fake.numbers.push(drag), fake.begins()),
    /** @param {number} id */
    deleteClip: async (id) => void fake.deleted.push(id),
    /** @param {number} id */
    keepClip: async (id) => void fake.kept.push(id),
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
    /** The clips of the bar shown, as the store has them now. @type {{ id: number, shown: boolean }[]} */
    list: [],
    find: () => undefined,
    /** @param {number} start @param {number} end @param {boolean} shown */
    make: async (start, end, shown) => {
      // The note before a site's first download, if the test asks for it.
      const asked = () =>
        new Promise((answer) => {
          fake.answer = (yes) => {
            answer(yes);
            for (const told of fake.heard.splice(0)) told(yes);
          };
        });
      if (fake.noted && !(await asked())) return null;
      const clip = { id: 9, start, end, shown, status: "ready", file_path: "/c.mp4" };
      fake.made.push(clip);
      return clip;
    },
    /** Whether making a clip asks the note first. */
    asksFor: () => fake.noted,
    nextAnswer: () => new Promise((told) => fake.heard.push(told)),
    /** @param {number} id @param {() => boolean} held */
    ready: async (id, held) => (
      fake.waited.push(id),
      held() ? { id, start: 0, end: 900, status: "ready", file_path: "/c.mp4" } : null
    ),
    /** @param {number} id @param {number} start @param {number} end */
    move: async (id, start, end) => void fake.moved.push([id, start, end]),
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
    /** Edits rendered again. @type {number[]} */
    rendered: [],
    /** @param {number} id */
    render: async (id) => void fake.edits.rendered.push(id),
    error: null,
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
const { carry, carrySet, backOver, backLeft, dragEditOut } = await import("./dragout.ts");

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
  fake.made.length = 0;
  fake.deleted.length = 0;
  fake.waited.length = 0;
  fake.answer = null;
  fake.heard.length = 0;
  fake.noted = false;
  fake.kept.length = 0;
  fake.resolve.length = 0;
  fake.remake = undefined;
  fake.settings.value.clip_format = "mp4";
  fake.moved.length = 0;
  fake.edits.rendered.length = 0;
  fake.toasts.length = 0;
  fake.dropped.length = 0;
  fake.listeners.clear();
  fake.makingListeners.clear();
  fake.numbers.length = 0;
  fake.beforeMade = true;
  fake.begins = () => true;
  fake.clips.error = null;
  fake.clips.list = [];
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

test("a long stretch of a site's VOD is made with no question at the edge", async () => {
  reset();
  // 15 minutes, downloaded at the edge: once a question (the long download's).
  const done = carried(900);
  move(0, 300);
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(fake.answer, null, "asked about first");
  assert.equal(fake.dragged.length, 1, "the system's drag didn't begin at once");
  fake.ended?.({ delivered: true, failed: null, finished_only: false, receiver: null });
  await done;
});

test("at the window's edge, the note before a site's first download is asked instead of the system's drag", async () => {
  reset();
  // A site VOD's first download: a question can't be answered in the
  // middle of the system's drag, so none begins; the carry ends at the edge.
  fake.noted = true;
  const done = carried(900);
  move(0, 300);
  const over = await Promise.race([done.then(() => true), new Promise((r) => setTimeout(() => r(false), 200))]);
  assert.ok(over, "the carry went on at the edge, waiting for the note's answer");
  await new Promise((r) => setTimeout(r, 20));
  assert.ok(fake.answer, "the note wasn't shown");
  assert.equal(fake.clips.carrying, null, "still carried while the note asks");
  // Let go after, wherever: nothing more happens, and the note waits.
  up(300, 300);
  fake.answer(true);
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(fake.dragged, [], "the system's drag was asked for");
  assert.deepEqual(
    fake.made.map((c) => c.shown),
    [true],
    "said yes to, the clip isn't made in the clip bar",
  );
  assert.deepEqual(fake.deleted, [], "the clip made was forgotten");
  assert.deepEqual(fake.toasts, ["Downloading the clip. Drag it from the clip bar when it's ready."]);
});

test("the note at the window's edge said no to: nothing made, nothing said", async () => {
  reset();
  fake.noted = true;
  const done = carried(900);
  move(0, 300);
  await done;
  await new Promise((r) => setTimeout(r, 20));
  up(300, 300);
  fake.answer?.(false);
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(fake.made, []);
  assert.deepEqual(fake.toasts, []);
});

/** Two stretches picked, carried from (100, 100); `files` makes their clips. @param {boolean} early @param {() => Promise<unknown[]>} files */
const carriedSet = (early, files) =>
  carrySet(
    /** @type {Parameters<typeof carrySet>[0]} */ (
      /** @type {unknown} */ ({
        items: [],
        label: "2 stretches",
        stretches: [
          { start: 0, end: 5 },
          { start: 5, end: 10 },
        ],
        files,
        early,
        vodId: 1,
        from: { left: 0, top: 0, width: 10, height: 10 },
        x: 100,
        y: 100,
      })
    ),
  );
/** Two clips only for a drag, made. */
const pair = () => [
  { id: 3, start: 0, end: 5, shown: false, status: "waiting", file_path: null },
  { id: 4, start: 5, end: 10, shown: false, status: "waiting", file_path: null },
];
const tick = () => new Promise((r) => setTimeout(r, 20));

// (Let go just past the edge, before their files are made: no system drag,
// whose minute-long timers would hold the test up.)
test("several picked from a file here: their files are made as the carry begins, not again at the edge", async () => {
  reset();
  let asked = 0;
  const done = carriedSet(true, async () => (asked++, pair()));
  assert.equal(asked, 1, "not made as the carry began");
  move(120, 120);
  move(0, 300);
  up(-5, 300);
  await done;
  assert.equal(asked, 1, "made again at the edge");
  assert.deepEqual(fake.dragged, []);
});

test("several picked from a site: their files are made only at the edge", async () => {
  reset();
  let asked = 0;
  const done = carriedSet(false, async () => (asked++, pair()));
  move(120, 120);
  await tick();
  assert.equal(asked, 0, "made before the edge");
  move(0, 300);
  up(-5, 300);
  await done;
  assert.equal(asked, 1);
});

test("several made ahead, dropped on a VOD's clip bar: kept once they're made, not made twice", async () => {
  reset();
  /** @type {(list: unknown[]) => void} */
  let finish = () => {};
  const makeAll = fake.clips.makeAll;
  /** @type {unknown[]} */
  const kept = [];
  fake.clips.makeAll = /** @type {typeof makeAll} */ (
    /** @type {unknown} */ (async (/** @type {unknown} */ ranges) => void kept.push(ranges))
  );
  try {
    const done = carriedSet(true, () => new Promise((r) => (finish = r)));
    move(120, 120);
    under = { closest: (/** @type {string} */ s) => (s === ".clip-bar" ? {} : null) };
    move(300, 300);
    up(300, 300);
    await done;
    await tick();
    assert.deepEqual(kept, [], "kept before those made ahead were there");
    finish(pair());
    await tick();
    assert.equal(kept.length, 1);
  } finally {
    fake.clips.makeAll = makeAll;
  }
});

test("past the edge before its files are made: the system's drag begins at once, and they're forgotten only once it's over", async () => {
  reset();
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  assert.deepEqual(fake.dragged, [[3, 4]], "the drag didn't begin with the clips being made");
  assert.deepEqual(fake.waited, [], "waited for the files at the edge");
  await tick();
  assert.deepEqual(fake.deleted, [], "forgotten before the drag was over");
  fake.ended?.({ delivered: true, failed: null, finished_only: false, receiver: null });
  await tick();
  assert.deepEqual(fake.deleted, [3, 4]);
  assert.equal(fake.clips.error, null);
});

test("let go on another app before they're made: it's said once, and unsaid when the drag is over", async () => {
  reset();
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  fake.making(2, -1);
  assert.deepEqual(fake.toasts, [], "said of another drag");
  fake.making(2);
  fake.making(1);
  assert.deepEqual(fake.toasts, ["Making 2 clips. They'll be where you dropped them once they're made."]);
  fake.ended?.({ delivered: true, failed: null, finished_only: false, receiver: null });
  await tick();
  assert.deepEqual(fake.dropped, [0], "still said once they went");
  assert.equal(fake.makingListeners.size, 0, "still listened for after the drag");
});

test("one clip still being made is said as the clip", async () => {
  reset();
  const done = carried(5);
  move(120, 120);
  move(0, 300);
  await new Promise((r) => setTimeout(r, 20));
  await done;
  fake.making(1);
  assert.deepEqual(fake.toasts, ["Making the clip. It'll be where you dropped it once it's made."]);
});

test("let go on an app that takes finished files only: it's said, by its name, and the clips are kept to drag again", async () => {
  reset();
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  fake.ended?.({ delivered: false, failed: null, finished_only: true, receiver: "Kdenlive" });
  await tick();
  assert.deepEqual(fake.toasts, [
    "Kdenlive takes finished files only. The clips are being made; drag them from the clip bar when they're ready.",
  ]);
  assert.deepEqual(fake.kept, [3, 4], "not kept in the clip bar");
  assert.deepEqual(fake.deleted, [], "forgotten, with nothing to drag again");
  assert.equal(fake.clips.error, null);
});

test("an app that takes finished files only, its name unknown, is said as that app", async () => {
  reset();
  const done = carried(5);
  move(120, 120);
  move(0, 300);
  await new Promise((r) => setTimeout(r, 20));
  fake.ended?.({ delivered: false, failed: null, finished_only: true, receiver: null });
  await done;
  await tick();
  assert.deepEqual(fake.toasts, [
    "That app takes finished files only. The clip is being made; drag it from the clip bar when it's ready.",
  ]);
  assert.deepEqual(fake.kept, [9]);
});

test("refused to DaVinci Resolve: its note asks, and the clips are kept to drag again", async () => {
  reset();
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  fake.ended?.({ delivered: false, failed: null, finished_only: false, receiver: null, resolve: "refused" });
  await tick();
  assert.deepEqual(fake.resolve, [["refused", 2]]);
  assert.deepEqual(fake.kept, [3, 4], "not kept in the clip bar");
  assert.deepEqual(fake.deleted, []);
  assert.deepEqual(fake.toasts, [], "said in the note alone");
});

test("refused to Resolve while still being made, then switched to MOV: made again as MOV once made as MP4", async () => {
  reset();
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  fake.ended?.({ delivered: false, failed: null, finished_only: false, receiver: null, resolve: "refused" });
  await tick();
  // Made meanwhile, in the format their making began in.
  const ready = fake.clips.ready;
  fake.clips.ready = async (id) => ({ id, start: 0, end: 5, shown: true, status: "ready", file_path: `/${id}.mp4` });
  try {
    fake.settings.value.clip_format = "mov";
    fake.remake?.();
    await tick();
    assert.deepEqual(
      fake.moved,
      [
        [3, 0, 5],
        [4, 0, 5],
      ],
      "left as MP4",
    );
  } finally {
    fake.clips.ready = ready;
  }
});

// (The races review's case: a stretch carried out, its clip still being cut
// as MP4 when Resolve refuses it; and trimmed while the note is up.)
test("one clip refused to Resolve while being cut, trimmed meanwhile: Switch makes it again as MOV, as trimmed", async () => {
  reset();
  const make = fake.clips.make;
  const ready = fake.clips.ready;
  /** The clip as the store has it now. */
  let now = {
    id: 9,
    start: 0,
    end: 5,
    shown: false,
    status: "cutting",
    file_path: "",
  };
  fake.clips.make = async (start, end, shown) => (fake.made.push(now), { ...now, start, end, shown });
  fake.clips.ready = async (id) => ({ ...now, id });
  try {
    const done = carried(5);
    move(120, 120);
    move(0, 300);
    await new Promise((r) => setTimeout(r, 20));
    // Resolve refused it at its first ask (no hold), the clip still being cut as MP4.
    fake.ended?.({ delivered: false, failed: null, finished_only: false, receiver: null, resolve: "refused" });
    await done;
    await tick();
    assert.deepEqual(fake.resolve, [["refused", 1]]);
    // Meanwhile its cut finished as MP4, and it was trimmed; then Switch to MOV.
    now = { ...now, start: 1, end: 4, shown: true, status: "ready", file_path: "/9.mp4" };
    fake.settings.value.clip_format = "mov";
    fake.remake?.();
    await tick();
    assert.deepEqual(fake.moved, [[9, 1, 4]], "the clip from the drop isn't made again as MOV, as trimmed");
  } finally {
    fake.clips.make = make;
    fake.clips.ready = ready;
  }
});

test("an MP4 edit refused to Resolve: the note asks of the edit, and Switch to MOV renders it again", async () => {
  reset();
  const edit = /** @type {import("./api").Edit} */ (/** @type {unknown} */ ({ id: 7, file_path: "/e.mp4" }));
  await dragEditOut(edit);
  assert.deepEqual(fake.dragged, [["edit", 7]]);
  // Another drag's end isn't the edit's.
  fake.ended({ delivered: false, failed: null, finished_only: false, receiver: null, resolve: "refused" }, -1);
  assert.deepEqual(fake.resolve, []);
  fake.ended({ delivered: false, failed: null, finished_only: false, receiver: null, resolve: "refused" });
  assert.deepEqual(fake.resolve, [["refused", 1, "edit"]]);
  assert.equal(fake.listeners.size, 0, "still listened for after the drag");
  fake.settings.value.clip_format = "mov";
  fake.remake?.();
  await tick();
  assert.deepEqual(fake.edits.rendered, [7], "left as MP4");
});

test("an edit let go where Resolve may be is rendered again too once switched; one already MOV isn't", async () => {
  reset();
  await dragEditOut(/** @type {import("./api").Edit} */ (/** @type {unknown} */ ({ id: 7, file_path: "/e.mp4" })));
  fake.ended({ delivered: true, failed: null, finished_only: false, receiver: null, resolve: "maybe" });
  assert.deepEqual(fake.resolve, [["maybe", 1, "edit"]]);
  fake.settings.value.clip_format = "mov";
  fake.remake?.();
  await tick();
  assert.deepEqual(fake.edits.rendered, [7]);
  reset();
  await dragEditOut(/** @type {import("./api").Edit} */ (/** @type {unknown} */ ({ id: 8, file_path: "/e.mov" })));
  fake.ended({ delivered: true, failed: null, finished_only: false, receiver: null, resolve: "maybe" });
  fake.settings.value.clip_format = "mov";
  fake.remake?.();
  await tick();
  assert.deepEqual(fake.edits.rendered, []);
});

test("an edit's drag that never began listens no more", async () => {
  reset();
  fake.begins = () => false;
  await dragEditOut(/** @type {import("./api").Edit} */ (/** @type {unknown} */ ({ id: 7, file_path: "/e.mp4" })));
  assert.equal(fake.listeners.size, 0);
});

test("let go where Resolve may be: its note asks, and the clips go as any drop's", async () => {
  reset();
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  fake.ended?.({ delivered: true, failed: null, finished_only: false, receiver: null, resolve: "maybe" });
  await tick();
  assert.deepEqual(fake.resolve, [["maybe", 2]]);
  assert.deepEqual(fake.kept, []);
  assert.deepEqual(fake.deleted, [3, 4]);
});

test("a drag whose file couldn't be made says so once it's over", async () => {
  reset();
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  fake.ended?.({ delivered: false, failed: "no footage", finished_only: false, receiver: null });
  await tick();
  assert.equal(fake.clips.error, "Couldn't make the clip: no footage");
  assert.deepEqual(fake.deleted, [3, 4]);
});

test("a drag out begun while the last one's files are still being handed over: each hears its own end", async () => {
  reset();
  const ended = { delivered: true, failed: null, finished_only: false, receiver: null };
  // Let go on a file manager that copies the files as they're made (Dolphin,
  // Explorer, Finder): the system's drag is over, its end still to come.
  const first = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await first;
  const second = carriedSet(true, async () => pair().map((c) => ({ ...c, id: c.id + 2 })));
  move(120, 120);
  move(0, 300);
  await second;
  assert.deepEqual(fake.dragged, [
    [3, 4],
    [5, 6],
  ]);
  fake.ended(ended, fake.numbers[0]);
  await tick();
  assert.deepEqual(fake.deleted, [3, 4], "the second's clips were forgotten as the first ended");
  // An edit's drag out (edits.rs drag_edit) has no number: neither's.
  fake.ended(ended, null);
  await tick();
  assert.deepEqual(fake.deleted, [3, 4], "forgotten as an edit's drag ended");
  // The second, brought back over the window, still lands where a carry would.
  under = { closest: (/** @type {string} */ s) => (s === ".clip-bar" ? {} : null) };
  backOver(300, 300);
  assert.equal(fake.clips.overBar, true, "brought back over the clip bar, nothing lit up");
  backLeft();
  fake.ended(ended, fake.numbers[1]);
  await tick();
  assert.deepEqual(fake.deleted, [3, 4, 5, 6], "the second's own end wasn't heard");
});

test("the same clips dragged out again while the first drag's are still being handed over: kept till the last ends", async () => {
  reset();
  const ended = { delivered: true, failed: null, finished_only: false, receiver: null };
  // Let go on a file manager that copies them as they're made; the same
  // stretches pulled out again meanwhile are the same clips (clips.make).
  const first = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await first;
  const second = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await second;
  assert.deepEqual(fake.dragged, [
    [3, 4],
    [3, 4],
  ]);
  fake.ended(ended, fake.numbers[0]);
  await tick();
  assert.deepEqual(fake.deleted, [], "forgotten as the first ended, while the second still hands them over");
  fake.ended(ended, fake.numbers[1]);
  await tick();
  assert.deepEqual(fake.deleted, [3, 4], "not forgotten once the last was over");
});

test("the same clips pulled out again but let go before that drag began: the first's still hands them over", async () => {
  reset();
  const first = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await first;
  // Let go just past the edge, before the second's system drag could begin.
  const second = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  up(-5, 300);
  await second;
  await tick();
  assert.deepEqual(fake.deleted, [], "forgotten while the first drag still hands them over");
  fake.ended({ delivered: true, failed: null, finished_only: false, receiver: null }, fake.numbers[0]);
  await tick();
  assert.deepEqual(fake.deleted, [3, 4]);
});

test("a clip kept in the clip bar while its drag out is still being handed over stays", async () => {
  reset();
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  // C pressed on the same stretch meanwhile (clips.make with shown): a pill.
  fake.clips.list = [{ ...pair()[0], shown: true }, pair()[1]];
  fake.ended({ delivered: true, failed: null, finished_only: false, receiver: null });
  await tick();
  assert.deepEqual(fake.deleted, [4], "the clip kept meanwhile was deleted as the drag ended");
});

test("where the system can't wait for the files, the drag begins once they're made", async () => {
  reset();
  fake.beforeMade = false;
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  assert.deepEqual(fake.waited, [3, 4], "didn't wait for the files");
  assert.deepEqual(fake.dragged, [[3, 4]]);
  fake.ended?.({ delivered: true, failed: null, finished_only: false, receiver: null });
  await tick();
});

test("the button up before the system's drag could begin: nothing said, the clips forgotten, whatever the words", async () => {
  reset();
  fake.begins = () => false;
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  await tick();
  assert.deepEqual(fake.dragged, [[3, 4]]);
  assert.equal(fake.clips.error, null);
  assert.deepEqual(fake.deleted, [3, 4], "kept, for a drag that never began");
});

test("a drag the app refused is said, even when its words say released", async () => {
  reset();
  // (Tauri rejects with the command's error text.)
  fake.begins = () =>
    /** @type {boolean} */ (/** @type {unknown} */ (Promise.reject("The clip was released from the library.")));
  const done = carriedSet(true, async () => pair());
  move(120, 120);
  move(0, 300);
  await done;
  await tick();
  assert.equal(fake.clips.error, "The clip was released from the library.");
  assert.deepEqual(fake.deleted, [3, 4]);
});
