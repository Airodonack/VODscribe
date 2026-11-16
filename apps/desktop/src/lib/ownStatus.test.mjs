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

/** ownStatus.ts `take`: "Takes `status` unless an older one than the last"; one as old as the last is no newer. */
test("a status with the seq of the last is left", () => {
  const own = new OwnStatus();
  assert.equal(own.take(status(5, 2.5)), true);
  assert.equal(own.take(status(5, 9)), false);
  assert.equal(own.shown(), 2.5);
});

/** ownStatus.ts `shown`: "Where the frame on screen is (null: none shows, or nothing said yet)"; `fps`: "once said". */
test("a status saying no frame shows, or no frame rate, reads as none", () => {
  const own = new OwnStatus();
  own.take(status(1, 4, 25));
  own.take({ ...status(2, null), fps: null });
  assert.equal(own.shown(), null);
  assert.equal(own.fps(), null);
});

/** ownStatus.ts `take`: "statuses without `seq` (the page's own guesses) always", whatever the newest. */
test("a guess taken doesn't lower what counts as the newest", () => {
  const own = new OwnStatus();
  own.take(status(7, 1));
  own.take({ ...status(undefined, 2), seq: undefined });
  assert.equal(own.take(status(6, 3)), false);
  assert.equal(own.shown(), 2);
});
