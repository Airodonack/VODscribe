// Whether a site's player refused to start (startWatch.ts).
// node --test apps/desktop/src/lib/startWatch.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";

import { StartWatch, WAIT_MS } from "./startWatch.ts";

/** A watch on a clock the test moves. */
function watch() {
  const clock = { now: 0 };
  return { clock, start: new StartWatch(() => clock.now) };
}

test("asked to play and only ready past the wait, it refused", () => {
  const { clock, start } = watch();
  start.play();
  start.heard("Idle");
  start.heard("Ready");
  clock.now = WAIT_MS - 1;
  assert.equal(start.refused(), false, "refused before the wait was out");
  clock.now = WAIT_MS;
  assert.equal(start.refused(), true);
});

test("playing at last, late, it no longer refuses", () => {
  const { clock, start } = watch();
  start.play();
  start.heard("Ready");
  clock.now = 3 * WAIT_MS;
  assert.equal(start.refused(), true);
  start.heard("Buffering");
  start.heard("Playing");
  assert.equal(start.refused(), false);
  // Paused by its own controls (Twitch says Idle when paused): nothing of ours was refused.
  start.heard("Idle");
  clock.now = 10 * WAIT_MS;
  assert.equal(start.refused(), false);
});

test("loading or buffering isn't a refusal, however long", () => {
  const { clock, start } = watch();
  start.play();
  clock.now = 5 * WAIT_MS;
  assert.equal(start.refused(), false, "said nothing yet");
  start.heard("Buffering");
  assert.equal(start.refused(), false, "buffering");
  // Back to ready after buffering, and ready for the wait: it refused.
  start.heard("Ready");
  assert.equal(start.refused(), false, "refused as it said ready");
  clock.now += WAIT_MS;
  assert.equal(start.refused(), true);
});

test("a frame slow to load is waited on from when it said it was ready", () => {
  const { clock, start } = watch();
  start.play();
  start.heard("Idle");
  clock.now = 3 * WAIT_MS;
  start.heard("Ready");
  clock.now += 100;
  assert.equal(start.refused(), false, "refused on its way from ready to playing");
  clock.now = 4 * WAIT_MS - 1;
  assert.equal(start.refused(), false);
  // It reports now and then: said again, it's been ready since the first.
  start.heard("Ready");
  clock.now = 4 * WAIT_MS;
  assert.equal(start.refused(), true);
});

test("a second Play doesn't start the wait over", () => {
  const { clock, start } = watch();
  start.play();
  start.heard("Ready");
  clock.now = WAIT_MS - 100;
  start.play();
  clock.now = WAIT_MS;
  assert.equal(start.refused(), true);
});

test("paused from the app, there's nothing to wait for", () => {
  const { clock, start } = watch();
  start.play();
  start.heard("Ready");
  clock.now = WAIT_MS;
  start.pause();
  assert.equal(start.refused(), false);
  // Asked again: the wait starts from this ask.
  start.play();
  clock.now = 2 * WAIT_MS - 1;
  assert.equal(start.refused(), false);
  clock.now = 2 * WAIT_MS;
  assert.equal(start.refused(), true);
});

test("asked to play while playing, it isn't waited on", () => {
  const { clock, start } = watch();
  start.heard("Playing");
  start.play();
  start.heard("Ready");
  clock.now = 10 * WAIT_MS;
  assert.equal(start.refused(), false);
});

test("never asked to play, it never refused", () => {
  const { clock, start } = watch();
  start.heard("Ready");
  clock.now = 10 * WAIT_MS;
  assert.equal(start.refused(), false);
});

test("loading, its first Idle isn't a refusal, however long", () => {
  const { clock, start } = watch();
  start.play();
  start.heard("Idle");
  clock.now = 10 * WAIT_MS;
  assert.equal(start.refused(), false);
});

test("a Play after a pause, left Idle for the wait, was refused", () => {
  const { clock, start } = watch();
  start.play();
  start.heard("Ready");
  start.heard("Playing");
  clock.now = 10_000;
  start.pause();
  start.heard("Idle");
  clock.now = 20_000;
  start.play();
  clock.now += WAIT_MS - 1;
  assert.equal(start.refused(), false);
  clock.now += 1;
  assert.equal(start.refused(), true, "still Idle the wait after our Play");
  start.heard("Playing");
  assert.equal(start.refused(), false);
});

test("a frame reloaded while playing (the bridge) is waited on afresh", () => {
  const { clock, start } = watch();
  start.play();
  start.heard("Playing");
  clock.now = 10_000;
  start.reloaded();
  start.play();
  start.heard("Idle");
  clock.now += 3 * WAIT_MS;
  assert.equal(start.refused(), false, "the new frame's first Idle is loading");
  start.heard("Ready");
  clock.now += WAIT_MS;
  assert.equal(start.refused(), true, "the reloaded frame refused autoplay, and nothing says so");
});
