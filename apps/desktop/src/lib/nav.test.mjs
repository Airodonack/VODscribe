// Back and forward between the app's views (nav.svelte.ts), the page's
// stores loaded as the app's build does, with the shell behind them faked.
// node --test apps/desktop/src/lib/nav.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { loadStoresWithShell } from "./shellFake.mjs";

loadStoresWithShell();
/** @type {any} */ (globalThis).shell = { invoke: async () => {}, listen: async () => () => {} };

let copies = 0;
/** A copy of the store of its own, and the window it steps: `open` is what App.svelte does when the
 *  user opens a view or changes the search, and a step taken goes where `go` says, as App's effect
 *  follows it (a change to what's already shown changes nothing to follow).
 *  @returns {Promise<{ nav: any, here: () => { doc: any, search: any }, open: (doc: any, search?: any) => void, taken: any[] }>} */
async function window() {
  const { nav } = await import(`./nav.svelte.ts?copy=${copies++}`);
  let shown = { doc: null, search: results("") };
  /** @type {any[]} */
  const taken = [];
  nav.attach((/** @type {any} */ step) => {
    taken.push(step);
    const moved =
      JSON.stringify(shown.doc) !== JSON.stringify(step.doc) || searching(shown.search) !== searching(step.search);
    shown = { doc: step.doc, search: step.search };
    if (moved) nav.arrived(step.doc, step.search);
  });
  return {
    nav,
    taken,
    here: () => shown,
    open(doc, search = shown.search) {
      shown = { doc, search };
      nav.arrived(doc, search);
    },
  };
}

/** Whether a search has its results up, as App.svelte shows them. @param {any} s */
const searching = (s) => (s.query.trim() !== "" || s.from.trim() !== "") && !s.aside;
/** @param {string} query @param {object} [more] */
const results = (query, more = {}) => ({
  query,
  all: false,
  audio: true,
  chat: true,
  within: 20,
  from: "",
  aside: false,
  ...more,
});
/** A session storage of its own. */
function memoryStorage() {
  const stored = new Map();
  globalThis.sessionStorage = /** @type {any} */ ({
    getItem: (/** @type {string} */ k) => stored.get(k) ?? null,
    setItem: (/** @type {string} */ k, /** @type {string} */ v) => void stored.set(k, v),
    removeItem: (/** @type {string} */ k) => void stored.delete(k),
  });
}
/** @param {number} id */
const vod = (id) => ({ kind: "vod", id });
/** @param {number} id */
const edit = (id) => ({ kind: "edit", id });
const docs = (/** @type {any[]} */ steps) => steps.map((s) => s.doc && `${s.doc.kind}:${s.doc.id}`);

// 0.4.0: "Back and forward (Alt+← and Alt+→, or ⌘[ and ⌘]) between the VODs and edits you've opened, each as you left it."
test("back and forward step between the VODs and edits opened", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(edit(1));
  w.open(vod(2));
  w.nav.goBack();
  w.nav.goBack();
  w.nav.goForward();
  assert.deepEqual(docs(w.taken), ["edit:1", "vod:1", "edit:1"]);
  w.nav.goForward();
  assert.deepEqual(docs(w.taken).slice(-1), ["vod:2"]);
});

// nav.svelte.ts `Nav.goForward`: a step forward is one back again.
test("after going forward, back goes back again", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(2));
  w.nav.goBack();
  w.nav.goForward();
  assert.equal(w.nav.back.length, 2);
  w.nav.goBack();
  w.nav.goBack();
  assert.deepEqual(docs(w.taken), ["vod:1", "vod:2", "vod:1", null]);
});

// nav.svelte.ts `Nav.goBack`, `goForward`: with no step that way, nothing is taken.
test("back with nothing before it, and forward with nothing after it, go nowhere", async () => {
  const w = await window();
  w.nav.goBack();
  w.nav.goForward();
  w.open(vod(1));
  w.nav.goForward();
  assert.equal(w.taken.length, 0);
});

// nav.svelte.ts header: a step is taken "when what's open changes"; the opening of a view from the start has the start behind it.
test("the first VOD opened has nothing open behind it", async () => {
  const w = await window();
  w.open(vod(1));
  w.nav.goBack();
  assert.deepEqual(w.taken, [{ doc: null, search: results(""), left: {} }]);
});

// nav.svelte.ts `Nav.arrived`: a view opened after going back drops what was forward.
test("opening a view after going back drops the steps forward", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(2));
  w.nav.goBack();
  w.open(vod(3));
  w.nav.goForward();
  assert.equal(w.taken.length, 1, "only the step back was taken");
  w.nav.goBack();
  assert.deepEqual(docs(w.taken).slice(-1), ["vod:1"]);
});

// nav.svelte.ts `Nav.arrived`: "A step is taken when what's open changes ... not when the playhead moves".
test("the playhead moving is no step", async () => {
  const w = await window();
  w.open(vod(1));
  w.nav.note(vod(1), { time: 5 });
  w.open(vod(1));
  w.nav.note(vod(1), { time: 9 });
  w.open(vod(1));
  assert.equal(w.nav.back.length, 1);
});

// nav.svelte.ts `key`: a VOD and an edit of the same number are different views.
test("a VOD and an edit with the same number are different views", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(edit(1));
  assert.equal(w.nav.back.length, 2);
  w.nav.goBack();
  assert.deepEqual(docs(w.taken), ["vod:1"]);
});

// 0.5.1: "Back and forward know your searches: leave a search's results (double-click a result, open a VOD, clear the search box) and Back brings them back as they were, with the same words and filter, scrolled where you left them."
test("back brings a search's results back with the same words and filter, scrolled where they were", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(1), results("landed", { all: true, chat: false, within: 45, from: "ann" }));
  w.nav.note(vod(1), { results: 150, scroll: 30 });
  // A result double-clicked: the VOD, the words kept in the box.
  w.open(vod(1), results("landed", { all: true, chat: false, within: 45, from: "ann", aside: true }));
  w.nav.goBack();
  const step = w.taken.at(-1);
  assert.deepEqual(step.search, results("landed", { all: true, chat: false, within: 45, from: "ann" }));
  assert.equal(step.left.results, 150);
  assert.deepEqual(w.nav.bringBack(vod(1)), { results: 150, scroll: 30 });
  w.nav.goForward();
  assert.equal(w.taken.at(-1).search.aside, true, "forward is the VOD, the words kept");
});

// nav.svelte.ts `Nav.arrived`: "A search retyped or refiltered is the same results, newer."
test("retyping or refiltering a search is no step of its own", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(1), results("la"));
  w.open(vod(1), results("lan"));
  w.open(vod(1), results("lan", { within: 60, chat: false }));
  assert.equal(w.nav.back.length, 2, "the start, and the VOD before its results came up");
  w.open(vod(2));
  w.nav.goBack();
  assert.deepEqual(
    w.taken.at(-1).search,
    results("lan", { within: 60, chat: false }),
    "back to the newest words and filter",
  );
});

// 0.5.1: "(... clear the search box) and Back brings them back": the results going is a step as well as their coming up.
test("the results coming up, and going, are each a step", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(1), results("landed"));
  w.open(vod(1), results(""));
  assert.equal(w.nav.back.length, 3);
  w.nav.goBack();
  assert.equal(w.taken.at(-1).search.query, "landed");
  w.nav.goBack();
  assert.equal(w.taken.at(-1).search.query, "");
});

// nav.svelte.ts `searching`: "words typed, or a name in From whom, which lists what it passes alone".
test("a name in From whom alone brings results up, and spaces alone don't", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(1), results("   "));
  assert.equal(w.nav.back.length, 1, "spaces are not a search");
  w.open(vod(1), results("", { from: "ann" }));
  assert.equal(w.nav.back.length, 2);
  w.open(vod(1), results("  ", { from: " ann " }));
  assert.equal(w.nav.back.length, 2, "still the same results");
  w.open(vod(1), results("", { from: "  " }));
  assert.equal(w.nav.back.length, 3, "a From of spaces is no name, the results go");
});

// nav.svelte.ts `Search.aside`: "Its results set aside for the view under them (a match double-clicked), the words kept in the box."
test("results set aside are the view itself, not results", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(1), results("landed", { aside: true }));
  assert.equal(w.nav.back.length, 1, "setting them aside before they were up is no step");
  w.open(vod(1), results("landed"));
  assert.equal(w.nav.back.length, 2, "bringing them up is");
});

// nav.svelte.ts `Nav.arrived`, `Nav.#take`: a step taken is not one of its own.
test("arriving by a step is not a step", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(2));
  w.nav.goBack();
  assert.equal(w.nav.back.length, 1);
  assert.equal(w.nav.forward.length, 1);
  w.nav.goBack();
  w.nav.goForward();
  w.nav.goForward();
  assert.deepEqual(docs(w.taken), ["vod:1", null, "vod:1", "vod:2"]);
});

// nav.svelte.ts `Nav.#take`: "Arriving where it already is changes nothing to wait for."
test("a step to where the window already is doesn't swallow the next change", async () => {
  memoryStorage();
  const w = await window();
  w.open(vod(1));
  w.nav.keep();
  assert.equal(w.nav.restore(), true);
  assert.equal(w.taken.length, 1);
  assert.equal(w.nav.back.length, 1);
  w.open(vod(2));
  assert.equal(w.nav.back.length, 2, "VOD 2 opened is a step");
});

// nav.svelte.ts header: "how its view was left (the playhead, the timeline's zoom, how far the transcript or the results were scrolled), which the views write here as they go (`note`) and read when brought back (`bringBack`)."
test("a view is brought back as it was left, and only while it is being brought back", async () => {
  const w = await window();
  w.open(vod(1));
  w.nav.note(vod(1), { time: 61 });
  w.nav.note(vod(1), { zoom: { start: 10, span: 30 }, scroll: 200 });
  w.open(vod(2));
  w.nav.goBack();
  assert.deepEqual(w.nav.bringBack(vod(1)), { time: 61, zoom: { start: 10, span: 30 }, scroll: 200 });
  assert.equal(w.nav.bringBack(vod(2)), null, "not the one being brought");
  w.open(vod(3));
  assert.equal(w.nav.bringBack(vod(1)), null, "a view opened another way isn't brought back");
});

// nav.svelte.ts header: "What they noted last also has a VOD or an edit opened any other way start where its playhead was left this session (`lastSeen`)."
test("a view opened another way is told where it was left this session", async () => {
  const w = await window();
  assert.equal(w.nav.lastSeen(vod(1)), null);
  w.nav.note(vod(1), { time: 12 });
  w.nav.note(vod(1), { scroll: 40 });
  w.nav.note(edit(1), { time: 99 });
  assert.deepEqual(w.nav.lastSeen(vod(1)), { time: 12, scroll: 40 });
  assert.deepEqual(w.nav.lastSeen(edit(1)), { time: 99 });
  w.nav.note(null, { scroll: 7 });
  assert.deepEqual(w.nav.lastSeen(null), { scroll: 7 }, "nothing open has its own place too");
});

// nav.svelte.ts `KEEP`: "The most steps kept each way."
test("the last hundred steps back are kept", async () => {
  const w = await window();
  for (let id = 1; id <= 105; id++) w.open(vod(id));
  assert.equal(w.nav.back.length, 100);
  for (let i = 0; i < 100; i++) w.nav.goBack();
  assert.deepEqual(docs(w.taken).at(-1), "vod:5");
  w.nav.goBack();
  assert.equal(w.taken.length, 100, "nothing is further back");
});

// nav.svelte.ts `Nav.forget`: "Drops the steps to `doc` (it's deleted), and then any to where they'd be already".
test("a removed VOD is out of back and forward, and steps that then meet are one", async () => {
  const w = await window();
  w.open(vod(3));
  w.open(vod(1));
  w.open(vod(2));
  w.open(vod(1));
  w.nav.forget(vod(2));
  // Back was: nothing open, VOD 3, VOD 1, VOD 2; here VOD 1.
  w.nav.goBack();
  assert.deepEqual(docs(w.taken), ["vod:3"], "VOD 1 behind VOD 1 is where it already is");
  w.nav.goForward();
  assert.deepEqual(docs(w.taken).slice(-1), ["vod:1"]);
});

// nav.svelte.ts `Nav.forget`: "open now, it's nowhere, so leaving it is no step back to it."
test("a removed VOD open now is nowhere: back goes to before it, forward to nothing open", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(2));
  w.nav.forget(vod(2));
  w.nav.goBack();
  assert.deepEqual(docs(w.taken), ["vod:1"]);
  w.nav.goForward();
  assert.deepEqual(docs(w.taken), ["vod:1", null]);
  w.nav.goForward();
  assert.equal(w.taken.length, 2, "never to the VOD removed");
});

// nav.svelte.ts `Nav.forget`: "A search's results over it that looked through every VOD stay, over nothing open ...; those of it alone go."
test("a removed VOD's results that looked through every VOD stay over nothing open; those of it alone go", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(1), results("landed", { all: true }));
  w.open(vod(2));
  w.open(vod(2), results("again"));
  w.open(vod(3));
  w.nav.forget(vod(1));
  w.nav.forget(vod(2));
  const left = w.nav.back.map((/** @type {any} */ s) => [s.doc && s.doc.id, s.search.query]);
  assert.deepEqual(left, [
    [null, ""],
    [null, "landed"],
  ]);
});

// nav.svelte.ts `Nav.forget`: the steps forward lose the removed VOD, and those left meeting the open view too.
test("a removed VOD is out of the steps forward, and steps forward that then meet the open view are one", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(2));
  w.open(vod(1));
  w.nav.goBack();
  w.nav.goBack();
  // Here VOD 1; forward: VOD 1, VOD 2.
  w.nav.forget(vod(2));
  w.nav.goForward();
  assert.deepEqual(docs(w.taken), ["vod:2", "vod:1"], "no step forward to VOD 1 where it already is");
});

// nav.svelte.ts `searchKey`: "What makes a step of its own: the words, whatever the spaces round them, or with none, the names".
test("results over nothing open after a removal are one step only where they are the same results", async () => {
  const w = await window();
  w.open(vod(1));
  w.open(vod(1), results("x", { all: true }));
  w.open(vod(2));
  w.open(vod(2), results("y", { all: true }));
  w.open(vod(3));
  w.open(vod(3), results(" y ", { all: true }));
  w.open(vod(4));
  w.open(vod(4), results("", { from: "ann", all: true }));
  w.open(vod(5));
  w.open(vod(5), results("", { from: " ann ", all: true }));
  w.open(vod(6));
  w.open(vod(6), results("", { from: "bob", all: true }));
  w.open(vod(7));
  for (const id of [1, 2, 3, 4, 5, 6]) w.nav.forget(vod(id));
  const left = w.nav.back.map((/** @type {any} */ s) => [
    s.doc && s.doc.id,
    s.search.query + "|" + s.search.from.trim(),
  ]);
  assert.deepEqual(left, [
    [null, "|"],
    [null, "x|"],
    [null, "y|"],
    [null, "|ann"],
    [null, "|bob"],
  ]);
});

// 0.7.1 Fixes: "Reload brings back what was open. Reload, in the right-click menu, reopens what was open where it was, instead of leaving nothing open while the video's player carried on."
test("a reloaded page gets back what was open as it was left, with its steps back and forward", async () => {
  memoryStorage();
  const before = await window();
  before.open(vod(1));
  before.open(vod(2));
  before.nav.goBack();
  before.nav.note(vod(1), { time: 33 });
  before.nav.keep();

  const after = await window();
  assert.equal(after.nav.restore(), true);
  assert.deepEqual(docs(after.taken), ["vod:1"]);
  assert.equal(after.taken[0].left.time, 33);
  assert.deepEqual(after.nav.bringBack(vod(1)), { time: 33 });
  assert.equal(after.nav.back.length, 1);
  assert.equal(after.nav.forward.length, 1);
  assert.equal(after.nav.restore(), false, "kept for the page loaded again, once");
  after.nav.goForward();
  assert.deepEqual(docs(after.taken), ["vod:1", "vod:2"]);
});

// nav.svelte.ts `Nav.restore`: steps back and forward are `kept.back ?? []`, whatever the kept page lacked.
test("a kept page with no steps back or forward comes back with none", async () => {
  const w = await window();
  w.open(vod(1));
  globalThis.sessionStorage = /** @type {any} */ ({
    getItem: () => JSON.stringify({ here: { doc: vod(9), search: results(""), left: {} } }),
    removeItem() {},
  });
  assert.equal(w.nav.restore(), true);
  assert.equal(w.nav.back.length, 0);
  assert.equal(w.nav.forward.length, 0);
  assert.deepEqual(docs(w.taken), ["vod:9"]);
});

// nav.svelte.ts `Nav.keep`, `Nav.restore`: "False if nothing was kept"; "Nothing kept: the page starts at the library".
test("nothing kept, a storage that fails, or what's kept unreadable, leaves the page at the library", async () => {
  const w = await window();
  globalThis.sessionStorage = /** @type {any} */ ({ getItem: () => null, setItem() {}, removeItem() {} });
  assert.equal(w.nav.restore(), false);
  globalThis.sessionStorage = /** @type {any} */ ({
    getItem() {
      throw new Error("blocked");
    },
    setItem() {
      throw new Error("blocked");
    },
    removeItem() {},
  });
  w.open(vod(1));
  assert.doesNotThrow(() => w.nav.keep());
  assert.equal(w.nav.restore(), false);
  globalThis.sessionStorage = /** @type {any} */ ({ getItem: () => "{not json", setItem() {}, removeItem() {} });
  assert.equal(w.nav.restore(), false);
  assert.equal(w.taken.length, 0);
});
