// What the app's own player last said (ownStatus.ts): node --test apps/desktop/src/lib/ownStatus.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { OwnStatus } from "./ownStatus.ts";

/** @param {number | undefined} seq @param {number | null} shown */
const status = (seq, shown, fps = 30) => ({
  seq,
  time: shown ?? 0,
  playing: true,
  running: true,
  speed: 1,
  shown,
  fps,
});

test("nothing said yet: no frame shown, no frame rate", () => {
  const own = new OwnStatus();
  assert.equal(own.shown(), null);
  assert.equal(own.fps(), null);
});

test("the newest status counts: an older one arriving late is left", () => {
  const own = new OwnStatus();
  assert.equal(own.take(status(5, 2.5, 60)), true);
  assert.equal(own.take(status(4, 1.0)), false, "older");
  assert.equal(own.shown(), 2.5);
  assert.equal(own.fps(), 60);
});

test("the page's own guesses (no seq) are taken, and a later status still counts", () => {
  const own = new OwnStatus();
  own.take(status(5, 2.5));
  assert.equal(own.take({ ...status(undefined, 3.0), seq: undefined }), true);
  assert.equal(own.shown(), 3.0);
  assert.equal(own.take(status(6, 3.1)), true);
  assert.equal(own.shown(), 3.1);
});
