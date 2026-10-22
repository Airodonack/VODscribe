// The keys both players share: node --test apps/desktop/src/lib/playerKeys.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { editShortcuts, inOut, stepOf, transportKey, vodShortcuts } from "./playerKeys.ts";

test("arrows step 5 s, 1 s with Shift; J and L 10 s whatever Shift", () => {
  assert.equal(stepOf("ArrowLeft", false), -5);
  assert.equal(stepOf("ArrowRight", true), 1);
  assert.equal(stepOf("j", true), -10);
  assert.equal(stepOf("l", false), 10);
  assert.equal(stepOf("x", false), undefined);
});

test("transport keys do what they say, and nothing else", () => {
  /** @type {string[]} */
  const did = [];
  /** @type {import("./playerKeys.ts").Transport} */
  const t = {
    togglePlay: () => did.push("play"),
    seekBy: (s) => did.push(`seek ${s}`),
    step: (n) => did.push(`step ${n}`),
    fullscreen: true,
    setFullscreen: (on) => did.push(`full ${on}`),
  };
  for (const key of [" ", "k", ",", ".", "ArrowLeft", "f", "Escape"]) assert.ok(transportKey(key, false, t), key);
  assert.deepEqual(did, ["play", "play", "step -1", "step 1", "seek -5", "full false", "full false"]);
  assert.equal(transportKey("q", false, t), false);
  // Without frame steps (a site's player), , and . mean nothing; out of full
  // screen, neither does Escape.
  assert.equal(transportKey(",", false, { ...t, step: undefined }), false);
  assert.equal(transportKey("Escape", false, { ...t, fullscreen: false }), false);
});

test("each legend lists a key once for each thing it does", () => {
  for (const list of [vodShortcuts(true, false), editShortcuts(true, false), vodShortcuts(true, true)]) {
    const does = list.map((s) => s.does);
    assert.equal(new Set(does).size, does.length, does.join(" | "));
    assert.ok(does.includes("undo or redo"));
  }
  assert.ok(
    !vodShortcuts(false, false).some((s) => s.does === "a frame back or on"),
    "no frame steps on a site's player",
  );
});

test("I and O move the selection's ends, swapping past the other end", () => {
  const picked = { start: 10, end: 20 };
  assert.deepEqual(inOut("i", 12, picked, [], 100), { start: 12, end: 20 });
  assert.deepEqual(inOut("o", 15, picked, [], 100), { start: 10, end: 15 });
  // O before the in point: the selection runs from O to I; I after the out
  // point likewise.
  assert.deepEqual(inOut("o", 4, picked, [], 100), { start: 4, end: 10 });
  assert.deepEqual(inOut("i", 30, picked, [], 100), { start: 20, end: 30 });
  // Onto the other end it would be empty: nothing changes.
  assert.equal(inOut("i", 20, picked, [], 100), null);
  assert.equal(inOut("o", 10, picked, [], 100), null);
});

test("with no selection, I runs one on to the next cut, O back to the one before", () => {
  const cuts = [30, 10, 50];
  assert.deepEqual(inOut("i", 12, null, cuts, 100), { start: 12, end: 30 });
  assert.deepEqual(inOut("o", 12, null, cuts, 100), { start: 10, end: 12 });
  // On a cut, it's the stretch on from it, or back from it.
  assert.deepEqual(inOut("i", 30, null, cuts, 100), { start: 30, end: 50 });
  assert.deepEqual(inOut("o", 30, null, cuts, 100), { start: 10, end: 30 });
  // No cuts: to the end, or from the start.
  assert.deepEqual(inOut("i", 60, null, [], 100), { start: 60, end: 100 });
  assert.deepEqual(inOut("o", 60, null, [], 100), { start: 0, end: 60 });
  // At the end (I) or the start (O) there's nothing to select.
  assert.equal(inOut("i", 100, null, [], 100), null);
  assert.equal(inOut("o", 0, null, [], 100), null);
});
