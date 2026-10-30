// "One request at a time, the newest wins" (latest.ts): node --test apps/desktop/src/lib/latest.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { latest } from "./latest.ts";

/** Requests answered by hand: `asked` lists what was asked, `answer(i, v)` answers the ith. */
function requests() {
  /** @type {{ arg: string, resolve: (v: string) => void, reject: (e: Error) => void }[]} */
  const asked = [];
  /** @param {string} arg */
  const fetch = (arg) =>
    /** @type {Promise<string>} */ (new Promise((resolve, reject) => asked.push({ arg, resolve, reject })));
  /** @param {number} i @param {string} v */
  const answer = async (i, v) => {
    asked[i].resolve(v);
    await new Promise((r) => setTimeout(r));
  };
  return { asked, fetch, answer };
}

/** A `latest` whose shown and freed values are kept. @param {boolean} [between] */
function watched(between) {
  const r = requests();
  /** @type {string[]} */
  const shown = [];
  /** @type {string[]} */
  const freed = [];
  const l = latest({ fetch: r.fetch, got: (v) => shown.push(v), free: (v) => freed.push(v), between });
  return { ...r, l, shown, freed };
}

test("one at a time: those wanted meanwhile wait, and only the newest is asked next", async () => {
  const w = watched();
  w.l.want("a");
  w.l.want("b");
  w.l.want("c");
  assert.deepEqual(
    w.asked.map((a) => a.arg),
    ["a"],
  );
  await w.answer(0, "A");
  assert.deepEqual(
    w.asked.map((a) => a.arg),
    ["a", "c"],
  );
  await w.answer(1, "C");
  assert.deepEqual(w.shown, ["C"]);
});

test("an older answer coming after a newer one is wanted is dropped and freed", async () => {
  const w = watched();
  w.l.want("a");
  w.l.want("b");
  await w.answer(0, "A");
  assert.deepEqual(w.shown, []);
  assert.deepEqual(w.freed, ["A"]);
  await w.answer(1, "B");
  assert.deepEqual(w.shown, ["B"]);
});

test("the one shown is freed once the next is shown, and on clear", async () => {
  const w = watched();
  w.l.want("a");
  await w.answer(0, "A");
  w.l.want("b");
  assert.deepEqual(w.freed, []);
  await w.answer(1, "B");
  assert.deepEqual(w.freed, ["A"]);
  w.l.clear();
  assert.deepEqual(w.freed, ["A", "B"]);
});

test("an answer come after clear is freed, not shown", async () => {
  const w = watched();
  w.l.want("a");
  w.l.clear();
  await w.answer(0, "A");
  assert.deepEqual(w.shown, []);
  assert.deepEqual(w.freed, ["A"]);
  assert.equal(w.asked.length, 1);
});

test("between: what comes for a moment moved on from still shows while the newest is asked", async () => {
  const w = watched(true);
  w.l.want("a");
  w.l.want("b");
  await w.answer(0, "A");
  assert.deepEqual(w.shown, ["A"]);
  await w.answer(1, "B");
  assert.deepEqual(w.shown, ["A", "B"]);
  assert.deepEqual(w.freed, ["A"]);
});

test("a failure is told only for the one still wanted, and the newest is asked after it", async () => {
  const r = requests();
  /** @type {string[]} */
  const failed = [];
  const l = latest({ fetch: r.fetch, got() {}, failed: (_e, arg) => failed.push(arg) });
  l.want("a");
  l.want("b");
  r.asked[0].reject(new Error("no"));
  await new Promise((done) => setTimeout(done));
  assert.deepEqual(failed, []);
  r.asked[1].reject(new Error("no"));
  await new Promise((done) => setTimeout(done));
  assert.deepEqual(failed, ["b"]);
});

test("between: what was asked before a clear isn't shown for what's wanted after it", async () => {
  // An edge of an edit's piece let go of (clear), and another dragged
  // before the first one's frame came: that frame is the old edge's.
  const w = watched(true);
  w.l.want("first edge");
  w.l.clear();
  w.l.want("second edge");
  await w.answer(0, "first edge's frame");
  assert.deepEqual(w.shown, []);
  assert.deepEqual(w.freed, ["first edge's frame"]);
  await w.answer(1, "second edge's frame");
  assert.deepEqual(w.shown, ["second edge's frame"]);
});

test("between: a moment moved on from that fails says nothing; the newest is asked", async () => {
  const r = requests();
  /** @type {string[]} */
  const failed = [];
  const l = latest({ fetch: r.fetch, got() {}, between: true, failed: (_e, arg) => failed.push(arg) });
  l.want("a");
  l.want("b");
  r.asked[0].reject(new Error("no"));
  await new Promise((done) => setTimeout(done));
  assert.deepEqual(failed, []);
  assert.deepEqual(
    r.asked.map((a) => a.arg),
    ["a", "b"],
  );
});
