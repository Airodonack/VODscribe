// The keys both players share: node --test apps/desktop/src/lib/playerKeys.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { editShortcuts, stepOf, transportKey, vodShortcuts } from "./playerKeys.ts";

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
