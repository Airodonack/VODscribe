// One selection at a time (selection.svelte.ts), compiled as the app's build
// does (types stripped, then Svelte's runes), with a stretch and a mark
// handed over as cuts.svelte.ts hands its own.
// node --test apps/desktop/src/lib/selection.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
export async function resolve(specifier, context, next) {
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

const selection = await import("./selection.svelte.ts");
const { snippets, library } = selection;

// The stretch selected (I and O) and the mark selected, as cuts.svelte.ts keeps them.
/** @type {{ stretch: { start: number, end: number } | null, mark: number | null }} */
const cuts = { stretch: null, mark: null };
selection.clearsStretch(() => {
  cuts.stretch = null;
  cuts.mark = null;
});
selection.clearsMark?.(() => (cuts.mark = null));

test("a plain click on background lets go of what's picked and the mark selected", () => {
  // An Alt-click picked a stretch between cuts, a clip of the bar beside it.
  snippets.set([{ kind: "segment", start: 1, end: 2 }]);
  snippets.click({ kind: "clip", id: 7 }, "toggle", []);
  assert.equal(snippets.size, 2);
  // A plain click on the timeline (or empty space in the clip bar).
  selection.background();
  assert.equal(snippets.size, 0);
  // The library's rows picked go the same way.
  library.set([
    { kind: "vod", id: 1 },
    { kind: "vod", id: 2 },
  ]);
  selection.background();
  assert.equal(library.size, 0);
  // A mark selected goes too.
  cuts.mark = 4;
  selection.background();
  assert.equal(cuts.mark, null);
});

test("a plain click on background keeps the stretch selected (I and O)", () => {
  cuts.stretch = { start: 1, end: 3 };
  selection.background();
  assert.deepEqual(cuts.stretch, { start: 1, end: 3 });
});

// The library's open row, as App.svelte hands it over: library.follow(() => open).
/** @type {{ kind: "vod" | "edit", id: number } | null} */
let open = null;
library.follow(() => open);
const vod = (/** @type {number} */ id) => /** @type {const} */ ({ kind: "vod", id });
const edit = (/** @type {number} */ id) => /** @type {const} */ ({ kind: "edit", id });
const clip = (/** @type {number} */ id) => /** @type {const} */ ({ kind: "clip", id });
const segment = (/** @type {number} */ start, /** @type {number} */ end) =>
  /** @type {const} */ ({ kind: "segment", start, end });
const rows = [vod(1), vod(2), edit(1), vod(3)];

/** Everything let go, nothing open, nothing selected. */
function fresh() {
  open = null;
  library.opened(null, null);
  snippets.set([]);
  library.set([]);
  cuts.stretch = null;
  cuts.mark = null;
}

// pick.ts: "A plain click: that item alone, and the anchor." (selection.svelte.ts `Selection.click`)
test("a click on a row picks it alone; Ctrl-click then adds another, numbered in the order picked", () => {
  fresh();
  library.click(vod(2), "plain", rows);
  library.click(vod(1), "toggle", rows);
  assert.deepEqual(library.items, [vod(2), vod(1)]);
  assert.equal(library.number(vod(2)), 1);
  assert.equal(library.number(vod(1)), 2);
  assert.equal(library.number(vod(3)), null);
});

// CHANGELOG 0.6.0: "With two or more picked, each shows its number in the order you picked them".
test("a number shows only while two or more are picked", () => {
  fresh();
  library.set([vod(1)]);
  assert.equal(library.number(vod(1)), null);
  library.add([vod(3)]);
  assert.equal(library.number(vod(1)), 1);
  assert.equal(library.number(vod(3)), 2);
});

// CHANGELOG 0.7.0: "In the library a click opens a row and picks it alone".
test("opening a row picks it alone, and it isn't several picked", () => {
  fresh();
  open = vod(2);
  library.opened(null, vod(2));
  assert.deepEqual(library.items, [vod(2)]);
  assert.equal(library.picking, false);
  // Opening another takes its place.
  open = vod(3);
  library.opened(vod(2), vod(3));
  assert.deepEqual(library.items, [vod(3)]);
});

// CHANGELOG 0.7.0 Fixes: "Click then Ctrl-click picks both."
test("a click on a row and then a Ctrl-click on another picks both", () => {
  fresh();
  open = vod(1);
  library.opened(null, vod(1));
  library.click(vod(3), "toggle", rows);
  assert.deepEqual(library.items, [vod(1), vod(3)]);
  assert.equal(library.picking, true);
});

// selection.svelte.ts `opened`: "closing it (nothing open) lets it go, unless it was picked with others."
test("closing the open row lets it go, unless it was picked with others", () => {
  fresh();
  open = vod(1);
  library.opened(null, vod(1));
  open = null;
  library.opened(vod(1), null);
  assert.deepEqual(library.items, []);
  library.set([vod(1), vod(2)]);
  library.opened(vod(1), null);
  assert.deepEqual(library.items, [vod(1), vod(2)]);
});

// selection.svelte.ts `opened`: opening the row that's open already changes nothing.
test("opening the row that's open already leaves what's picked", () => {
  fresh();
  open = vod(1);
  library.set([vod(1), vod(2)]);
  library.opened(vod(1), vod(1));
  assert.deepEqual(library.items, [vod(1), vod(2)]);
});

// CHANGELOG 0.7.0: "Escape, or a click below the list, lets go of all but the open row."
test("letting go leaves the open row picked, or nothing if none is open", () => {
  fresh();
  open = vod(2);
  library.set([vod(1), vod(2), vod(3)]);
  library.clear();
  assert.deepEqual(library.items, [vod(2)]);
  assert.equal(library.picking, false);
  open = null;
  library.clear();
  assert.deepEqual(library.items, []);
});

// CHANGELOG 0.7.0 Fixes: "Clicking elsewhere clears picks. A plain click on the timeline, or on empty space
// in the clip bar or the library, lets go of the stretches, clips and rows picked".
test("a plain click on background leaves the open row picked", () => {
  fresh();
  open = vod(2);
  library.set([vod(1), vod(2)]);
  selection.background();
  assert.deepEqual(library.items, [vod(2)]);
});

// CHANGELOG 0.6.0: "One kind of thing is picked at once. Picking clips or stretches lets go of library rows and the other way round".
test("picking clips or stretches lets go of the library's rows, and the other way round", () => {
  fresh();
  library.set([vod(1), vod(2)]);
  snippets.click(clip(7), "toggle", []);
  snippets.click(segment(1, 2), "toggle", []);
  assert.equal(library.size, 0);
  assert.equal(snippets.size, 2);
  library.set([vod(1), vod(2)]);
  assert.equal(snippets.size, 0);
  assert.equal(library.size, 2);
});

// CHANGELOG 0.6.0: "selecting a cut, chapter or highlight lets go of both"; selection.svelte.ts "starting a set lets the stretch or the mark go".
test("picking a set lets the stretch selected and the mark selected go", () => {
  fresh();
  cuts.stretch = { start: 1, end: 3 };
  cuts.mark = 4;
  snippets.set([clip(7), clip(8)]);
  assert.equal(cuts.stretch, null);
  assert.equal(cuts.mark, null);
});

// selection.svelte.ts `Selection.#to`: only a set of several, not the open row alone, starts one.
test("opening a row alone lets go of nothing else", () => {
  fresh();
  cuts.stretch = { start: 1, end: 3 };
  open = vod(1);
  library.opened(null, vod(1));
  assert.deepEqual(cuts.stretch, { start: 1, end: 3 });
});

// selection.svelte.ts `stretchMade`: "A stretch selection was made, or a mark selected: the sets go."
test("making a stretch selection or selecting a mark lets go of both sets", () => {
  fresh();
  snippets.set([clip(7), clip(8)]);
  selection.stretchMade();
  assert.equal(snippets.size, 0);
  library.set([vod(1), vod(2)]);
  selection.stretchMade();
  assert.equal(library.size, 0);
});

// selection.svelte.ts `only`: "`item` and only it, or, if it's picked alone already, nothing."
// CHANGELOG 0.7.0: "clicking another selects that one instead, and clicking the selected one again lets it go".
test("a plain click on a clip picks it alone; on the one picked alone, lets it go", () => {
  fresh();
  snippets.set([clip(7), clip(8)]);
  snippets.only(clip(8));
  assert.deepEqual(snippets.items, [clip(8)]);
  snippets.only(clip(7));
  assert.deepEqual(snippets.items, [clip(7)]);
  snippets.only(clip(7));
  assert.deepEqual(snippets.items, []);
});

// CHANGELOG 0.7.0: "Alt-click (Option-click on a Mac) picks the stretch between cuts there by itself".
test("a stretch picked alone, picked again with only, is let go; stretches are the same when their times are", () => {
  fresh();
  snippets.only(segment(1, 2));
  snippets.only(segment(1, 2 + 1e-9));
  assert.deepEqual(snippets.items, []);
  snippets.only(segment(1, 2));
  snippets.only(segment(1, 2.5));
  assert.deepEqual(snippets.items, [segment(1, 2.5)]);
});

// CHANGELOG 0.7.0: "Ctrl+Alt-click (⌘⌥-click) adds it to the others".
test("a stretch and a clip with the same number are two different things", () => {
  fresh();
  snippets.click(clip(1), "toggle", []);
  snippets.click(segment(1, 1), "toggle", []);
  assert.equal(snippets.size, 2);
  assert.equal(snippets.has(clip(1)), true);
  assert.equal(snippets.has(segment(1, 1)), true);
  assert.equal(snippets.has(clip(2)), false);
});

// CHANGELOG 0.6.0: "drag a box over them from empty space in the clip bar or below the library's list".
test("a box picks what it holds; with Shift held it adds to what was picked before it", () => {
  fresh();
  library.set([vod(1)]);
  library.beginBox();
  library.boxed([vod(2), vod(3)], true);
  assert.deepEqual(library.items, [vod(1), vod(2), vod(3)]);
  // The box shrinks: what it let go of goes, what was before it stays, nothing twice.
  library.boxed([vod(1), vod(2)], true);
  assert.deepEqual(library.items, [vod(1), vod(2)]);
  library.boxed([vod(3)], false);
  assert.deepEqual(library.items, [vod(3)]);
});

// selection.svelte.ts `add`: "Picks `list` too ... after what's picked already."
test("adding what's picked already adds nothing", () => {
  fresh();
  library.set([vod(1), vod(2)]);
  const before = library.items;
  library.add([vod(2)]);
  assert.equal(library.items, before);
});

// selection.svelte.ts `keep`: "Keeps only what `still` says is there yet (a cut moved, a clip deleted another way)."
test("what's gone from the screen goes from the set, and so does the anchor if it was it", () => {
  fresh();
  snippets.set([clip(7), clip(8), clip(9)]);
  snippets.keep((i) => i.kind === "clip" && i.id !== 9);
  assert.deepEqual(snippets.items, [clip(7), clip(8)]);
  assert.equal(snippets.anchor, null);
  snippets.set([clip(7), clip(8)]);
  assert.deepEqual(snippets.anchor, clip(8));
  snippets.keep((i) => i.kind === "clip" && i.id === 7);
  assert.deepEqual(snippets.items, [clip(7)]);
  assert.equal(snippets.anchor, null);
});

// CHANGELOG 0.7.0: "Press ↑ or ↓ to open the row before or after, or with Shift held to pick the run to it".
test("an arrow key from a row goes to the next, or with Shift picks the run; at an end it goes nowhere", () => {
  fresh();
  open = vod(2);
  library.opened(null, vod(2));
  assert.deepEqual(library.step(vod(2), 1, false, rows), edit(1));
  assert.deepEqual(library.items, [edit(1)]);
  assert.deepEqual(library.step(edit(1), 1, true, rows), vod(3));
  assert.deepEqual(library.items, [edit(1), vod(3)]);
  assert.equal(library.step(vod(3), 1, true, rows), null);
  assert.deepEqual(library.items, [edit(1), vod(3)]);
});

// selection.svelte.ts: "Ctrl-click (⌘ on a Mac)"; on this machine the platform is not a Mac.
test("a Ctrl-click toggles and a Shift-click ranges; Ctrl+Shift adds the run; a Meta-click isn't a toggle here", () => {
  const key = (/** @type {object} */ props) =>
    /** @type {MouseEvent} */ (/** @type {unknown} */ ({ ctrlKey: false, metaKey: false, shiftKey: false, ...props }));
  assert.equal(selection.howOf(key({})), "plain");
  assert.equal(selection.howOf(key({ ctrlKey: true })), "toggle");
  assert.equal(selection.howOf(key({ shiftKey: true })), "range");
  assert.equal(selection.howOf(key({ ctrlKey: true, shiftKey: true })), "addRange");
  assert.equal(selection.howOf(key({ metaKey: true })), "plain");
  assert.equal(selection.picks(key({})), false);
  assert.equal(selection.picks(key({ ctrlKey: true })), true);
  assert.equal(selection.picks(key({ shiftKey: true })), true);
});

// selection.svelte.ts `count`: "What to say a set holds: "3 clips", "2 VODs and 1 edit"."
test("a count says what's held, leaving out what's none", () => {
  assert.equal(selection.count([[3, "clip", "clips"]]), "3 clips");
  assert.equal(selection.count([[1, "clip", "clips"]]), "1 clip");
  assert.equal(
    selection.count([
      [2, "VOD", "VODs"],
      [1, "edit", "edits"],
    ]),
    "2 VODs and 1 edit",
  );
  assert.equal(
    selection.count([
      [0, "VOD", "VODs"],
      [2, "edit", "edits"],
    ]),
    "2 edits",
  );
});

// selection.svelte.ts `overlaps`: whether two boxes on screen overlap.
test("two boxes overlap when they share area, not when they only touch", () => {
  const box = (/** @type {number} */ l, /** @type {number} */ t, /** @type {number} */ r, /** @type {number} */ b) =>
    /** @type {DOMRect} */ (/** @type {unknown} */ ({ left: l, top: t, right: r, bottom: b }));
  assert.equal(selection.overlaps(box(0, 0, 10, 10), box(5, 5, 15, 15)), true);
  assert.equal(selection.overlaps(box(0, 0, 10, 10), box(10, 0, 20, 10)), false);
  assert.equal(selection.overlaps(box(0, 0, 10, 10), box(0, 10, 10, 20)), false);
  assert.equal(selection.overlaps(box(0, 0, 10, 10), box(20, 20, 30, 30)), false);
});

// selection.svelte.ts `dragBox`: "A box dragged out from empty space in `node` (a scrolling list), drawn as it goes".
// CHANGELOG 0.6.0: "drag a box over them from empty space in the clip bar or below the library's list".
/** The window, the page's body and a list in it, as far as dragBox uses them. */
function boxPage() {
  const win = new EventTarget();
  /** @type {string[]} */
  const removed = [];
  const remove = win.removeEventListener.bind(win);
  win.removeEventListener = (/** @type {string} */ type, /** @type {any} */ fn) => (
    removed.push(type),
    remove(type, fn)
  );
  globalThis.window = /** @type {any} */ (win);
  /** @type {any[]} */
  const drawn = [];
  globalThis.DOMRect = /** @type {any} */ (
    class {
      constructor(
        /** @type {number} */ left = 0,
        /** @type {number} */ top = 0,
        /** @type {number} */ width = 0,
        /** @type {number} */ height = 0,
      ) {
        Object.assign(this, { left, top, width, height, right: left + width, bottom: top + height });
      }
    }
  );
  globalThis.document = /** @type {any} */ ({
    createElement: (/** @type {string} */ tag) => ({
      tag,
      className: "",
      style: {},
      gone: false,
      remove() {
        this.gone = true;
      },
    }),
    body: { append: (/** @type {any} */ el) => drawn.push(el) },
  });
  const node = /** @type {any} */ (new EventTarget());
  node.clientWidth = 280;
  node.clientHeight = 500;
  return { win, node, drawn, removed };
}
/** @param {EventTarget} to @param {object} props */
const pointer = (/** @type {EventTarget} */ to, /** @type {string} */ type, /** @type {any} */ props = {}) => {
  const { target, ...rest } = props;
  const e = Object.assign(new Event(type, { cancelable: true }), {
    button: 0,
    buttons: 1,
    offsetX: 5,
    offsetY: 5,
    ...rest,
  });
  if (target) Object.defineProperty(e, "target", { value: target });
  to.dispatchEvent(e);
  return e;
};

/** A box action on a fresh page, over the library's rows picked with `hits`. */
function boxing(/** @type {(box: DOMRect) => any[]} */ hits = () => []) {
  fresh();
  const p = boxPage();
  const seen = { empty: 0, boxes: /** @type {any[]} */ ([]) };
  const action = selection.dragBox(p.node, {
    set: library,
    hits: (box) => (seen.boxes.push(box), hits(box)),
    onempty: () => seen.empty++,
  });
  return {
    ...p,
    seen,
    action,
    press: (/** @type {object} */ props = {}) => pointer(p.node, "pointerdown", props),
  };
}

test("a box dragged from empty space is drawn as it goes and picks what it holds, and goes at the release", () => {
  const b = boxing(() => [vod(1), vod(2)]);
  const down = b.press({ clientX: 100, clientY: 200 });
  assert.equal(down.defaultPrevented, true);
  pointer(b.win, "pointermove", { clientX: 140, clientY: 260 });
  assert.equal(b.drawn.length, 1);
  assert.equal(b.drawn[0].tag, "div");
  assert.equal(b.drawn[0].className, "select-box");
  assert.deepEqual(b.drawn[0].style, { left: "100px", top: "200px", width: "40px", height: "60px" });
  assert.deepEqual(library.items, [vod(1), vod(2)]);
  pointer(b.win, "pointerup");
  assert.equal(b.drawn[0].gone, true);
  assert.equal(b.seen.empty, 0);
  assert.deepEqual(library.items, [vod(1), vod(2)]);
});

test("a box dragged up and to the left is the same box", () => {
  const b = boxing();
  b.press({ clientX: 100, clientY: 200 });
  pointer(b.win, "pointermove", { clientX: 60, clientY: 150 });
  assert.deepEqual(b.drawn[0].style, { left: "60px", top: "150px", width: "40px", height: "50px" });
  const box = b.seen.boxes.at(-1);
  assert.deepEqual([box.left, box.top, box.right, box.bottom], [60, 150, 100, 200]);
});

test("a press moved less than four pixels draws no box; let go, it's a click on empty space", () => {
  const b = boxing();
  b.press({ clientX: 100, clientY: 200 });
  pointer(b.win, "pointermove", { clientX: 102, clientY: 202 });
  assert.equal(b.drawn.length, 0);
  assert.equal(b.seen.boxes.length, 0);
  pointer(b.win, "pointerup");
  assert.equal(b.seen.empty, 1);
  // A move of four draws it.
  b.press({ clientX: 100, clientY: 200 });
  pointer(b.win, "pointermove", { clientX: 104, clientY: 200 });
  assert.equal(b.drawn.length, 1);
});

test("a press on a pill or row, of another button, or on the scrollbar is left alone", () => {
  const b = boxing();
  /** @type {object[]} */
  const ignored = [{ target: new EventTarget() }, { button: 2 }, { offsetX: 280 }, { offsetY: 500 }];
  for (const props of ignored) {
    const e = b.press({ clientX: 0, clientY: 0, ...props });
    assert.equal(e.defaultPrevented, false, JSON.stringify(props));
    pointer(b.win, "pointermove", { clientX: 50, clientY: 50 });
    assert.equal(b.drawn.length, 0, JSON.stringify(props));
  }
  // Just inside the list's edge is a press on it.
  b.press({ clientX: 0, clientY: 0, offsetX: 279, offsetY: 499 });
  pointer(b.win, "pointermove", { clientX: 50, clientY: 50 });
  assert.equal(b.drawn.length, 1);
});

test("a box drawn with Shift held adds to what was picked before it", () => {
  const b = boxing(() => [vod(3)]);
  library.set([vod(2)]);
  b.press({ clientX: 0, clientY: 0, shiftKey: true });
  pointer(b.win, "pointermove", { clientX: 50, clientY: 50 });
  assert.deepEqual(library.items, [vod(2), vod(3)]);
  pointer(b.win, "pointerup");
  const plain = boxing(() => [vod(3)]);
  library.set([vod(1)]);
  plain.press({ clientX: 0, clientY: 0 });
  pointer(plain.win, "pointermove", { clientX: 50, clientY: 50 });
  assert.deepEqual(library.items, [vod(3)]);
});

test("a release that happened elsewhere ends the box at the next move, and is no click", () => {
  const b = boxing();
  b.press({ clientX: 0, clientY: 0 });
  pointer(b.win, "pointermove", { clientX: 50, clientY: 50 });
  pointer(b.win, "pointermove", { clientX: 80, clientY: 80, buttons: 0 });
  assert.equal(b.drawn[0].gone, true);
  assert.equal(b.seen.boxes.length, 1);
  pointer(b.win, "pointermove", { clientX: 90, clientY: 90 });
  assert.equal(b.seen.boxes.length, 1);
  assert.equal(b.seen.empty, 0);
  // Before a box was drawn, the same.
  b.press({ clientX: 0, clientY: 0 });
  pointer(b.win, "pointermove", { clientX: 1, clientY: 1, buttons: 0 });
  pointer(b.win, "pointerup");
  assert.equal(b.seen.empty, 0);
});

test("a cancelled press ends the box and is no click; moves after it are no one's", () => {
  const b = boxing();
  b.press({ clientX: 0, clientY: 0 });
  pointer(b.win, "pointermove", { clientX: 50, clientY: 50 });
  pointer(b.win, "pointercancel");
  assert.equal(b.drawn[0].gone, true);
  pointer(b.win, "pointermove", { clientX: 90, clientY: 90 });
  pointer(b.win, "pointerup");
  assert.equal(b.drawn.length, 1);
  assert.equal(b.seen.empty, 0);
});

test("a move or a release with no press does nothing", () => {
  const b = boxing();
  pointer(b.win, "pointermove", { clientX: 90, clientY: 90 });
  pointer(b.win, "pointerup");
  assert.equal(b.drawn.length, 0);
  assert.equal(b.seen.empty, 0);
});

test("the box's options can be changed, and a destroyed list takes no more presses", () => {
  const b = boxing();
  /** @type {number[]} */
  const empties = [];
  b.action.update({ set: library, hits: () => [], onempty: () => empties.push(1) });
  b.press({ clientX: 0, clientY: 0 });
  pointer(b.win, "pointerup");
  assert.deepEqual(empties, [1]);
  assert.equal(b.seen.empty, 0);
  b.press({ clientX: 0, clientY: 0 });
  b.action.destroy();
  pointer(b.win, "pointermove", { clientX: 50, clientY: 50 });
  assert.equal(b.drawn.length, 0);
  b.press({ clientX: 0, clientY: 0 });
  pointer(b.win, "pointermove", { clientX: 50, clientY: 50 });
  assert.equal(b.drawn.length, 0);
});

// selection.svelte.ts `clearsStretch`: "`letGo` lets go of a stretch selection: called whenever a set starts."
test("a stretch selection's owner is told whenever a set of several starts, in either set", () => {
  fresh();
  let told = 0;
  selection.clearsStretch(() => told++);
  snippets.set([clip(1), clip(2)]);
  library.set([vod(1), vod(2)]);
  assert.equal(told, 2);
});

// selection.svelte.ts `clearsMark`: "called on a click on background".
test("a mark selected is let go by background, by a click on it and by nothing else", () => {
  fresh();
  let told = 0;
  selection.clearsMark(() => told++);
  snippets.set([clip(1), clip(2)]);
  assert.equal(told, 0);
  selection.background();
  assert.equal(told, 1);
});

test("a press's window listeners are all taken off when it's over", () => {
  const b = boxing();
  b.press({ clientX: 0, clientY: 0 });
  pointer(b.win, "pointerup");
  assert.deepEqual(b.removed.sort(), ["pointercancel", "pointermove", "pointerup"]);
});

test("moving on draws the one box, not another each time", () => {
  const b = boxing();
  b.press({ clientX: 0, clientY: 0 });
  pointer(b.win, "pointermove", { clientX: 50, clientY: 50 });
  pointer(b.win, "pointermove", { clientX: 60, clientY: 60 });
  assert.equal(b.drawn.length, 1);
  assert.deepEqual(b.drawn[0].style, { left: "0px", top: "0px", width: "60px", height: "60px" });
});

// selection.svelte.ts `opened`: "opening one picks it alone".
test("opening another row while several are picked picks it alone", () => {
  fresh();
  open = vod(1);
  library.set([vod(1), vod(2)]);
  open = vod(3);
  library.opened(vod(1), vod(3));
  assert.deepEqual(library.items, [vod(3)]);
});

// selection.svelte.ts `keep`: a set with nothing gone is left as it is, its anchor too.
test("keeping a set with all still there leaves it, and an anchor still there stays", () => {
  fresh();
  snippets.set([clip(7), clip(8)]);
  const before = snippets.items;
  snippets.keep(() => true);
  assert.equal(snippets.items, before);
  snippets.keep((i) => i.kind === "clip" && i.id !== 7);
  assert.deepEqual(snippets.items, [clip(8)]);
  assert.deepEqual(snippets.anchor, clip(8));
});

test("boxes that touch at an edge, or lie to one side, don't overlap", () => {
  const rect = (/** @type {number} */ l, /** @type {number} */ t, /** @type {number} */ r, /** @type {number} */ b) =>
    /** @type {DOMRect} */ (/** @type {unknown} */ ({ left: l, top: t, right: r, bottom: b }));
  const at = rect(10, 0, 20, 10);
  assert.equal(selection.overlaps(at, rect(0, 0, 10, 10)), false);
  assert.equal(selection.overlaps(at, rect(0, 10, 30, 20)), false);
  assert.equal(selection.overlaps(rect(0, 10, 30, 20), at), false);
  assert.equal(selection.overlaps(rect(20, 0, 30, 10), rect(0, 0, 10, 10)), false);
  assert.equal(selection.overlaps(rect(0, 20, 10, 30), rect(0, 0, 10, 10)), false);
});
