// A site's player's plays and pauses, settled (settle.ts): node --test apps/desktop/src/lib/settle.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { SETTLE, settling } from "./settle.ts";

/** A settling on made-up timers, and what it passed on.
 *  @param {import("node:test").TestContext} t */
function settled(t, ms = 30) {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  /** @type {boolean[]} */
  const sent = [];
  return {
    sent,
    s: settling((playing) => sent.push(playing), ms),
    tick: (/** @type {number} */ n) => t.mock.timers.tick(n),
  };
}

/** settle.ts: "Twitch's, on a user's play, plays its element and pauses it at once (to have the gesture count for later plays), then plays it for real". */
test("Twitch's gesture capture (play, pause at once, play later) reaches the app's player as one play", (t) => {
  const { sent, s, tick } = settled(t);
  s.want(true);
  s.want(false);
  tick(10);
  s.want(true);
  tick(60);
  assert.deepEqual(sent, [true]);
});

/** settle.ts: "it's told the pause they come to (as it was), then the real play." */
test("Twitch's real play coming late, the app's player is told the pause it was in, then the play", (t) => {
  const { sent, s, tick } = settled(t);
  s.want(true);
  s.want(false);
  tick(400);
  s.want(true);
  tick(60);
  assert.deepEqual(sent, [false, true], "never started and stopped");
});

/** settle.ts `settling`: "Passes on `forward(playing)` the last of what's wanted within `ms` of each other". */
test("a pause that stands is passed on", (t) => {
  const { sent, s, tick } = settled(t);
  s.want(true);
  tick(60);
  s.want(false);
  tick(60);
  assert.deepEqual(sent, [true, false]);
});

/** settle.ts `settling`: "always, even if the same as before (the app's player may have been paused otherwise, as for an ad)". */
test("the same again is passed on (the app's player may have been paused meanwhile, for an ad)", (t) => {
  const { sent, s, tick } = settled(t);
  s.want(true);
  tick(60);
  s.want(true);
  tick(60);
  assert.deepEqual(sent, [true, true]);
});

/** settle.ts `settling`'s `stop`. */
test("stopped, nothing waiting is passed on", (t) => {
  const { sent, s, tick } = settled(t);
  s.want(true);
  s.stop();
  tick(60);
  assert.deepEqual(sent, []);
});

/** settle.ts `SETTLE`: "How long a play or pause must stand before it's passed on (ms): Twitch's play and pause come in the same few milliseconds." */
test("a play or pause is passed on once it has stood for the settle time, not before", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  /** @type {boolean[]} */
  const sent = [];
  const s = settling((playing) => sent.push(playing));
  s.want(true);
  t.mock.timers.tick(SETTLE - 1);
  assert.deepEqual(sent, []);
  t.mock.timers.tick(1);
  assert.deepEqual(sent, [true]);
  assert.equal(SETTLE, 30);
});

/** settle.ts `settling`: each want starts the wait over. */
test("each play or pause wanted starts the wait over", (t) => {
  const { sent, s, tick } = settled(t);
  s.want(true);
  tick(20);
  s.want(false);
  tick(20);
  assert.deepEqual(sent, [], "twenty after the last: not yet");
  tick(10);
  assert.deepEqual(sent, [false]);
});

/** settle.ts `settling`'s `stop`: a wanted play after a stop is passed on as ever. */
test("after a stop, a new play or pause is passed on", (t) => {
  const { sent, s, tick } = settled(t);
  s.want(true);
  s.stop();
  s.stop();
  s.want(false);
  tick(60);
  assert.deepEqual(sent, [false]);
});
