// A player's position on the page (playerClock.ts): node --test apps/desktop/src/lib/playerClock.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as wait } from "node:timers/promises";
import { Clock } from "./playerClock.ts";

test("before the player says anything, there's no shown position", () => {
  assert.equal(new Clock(5, true).shown(), null);
});

test("shown runs on from a report only while the player said it was moving", async () => {
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  await wait(100);
  const moving = clock.shown() ?? 0;
  assert.ok(moving > 10.05 && moving < 10.3, `${moving}`);
  clock.heardAt(12, false);
  await wait(50);
  assert.equal(clock.shown(), 12, "said paused: it stands");
});

test("a player that goes quiet stops showing as moving half a second on", async () => {
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  await wait(700);
  const shown = clock.shown() ?? 0;
  assert.ok(Math.abs(shown - 10.5) < 0.01, `${shown}`);
  // The page's own clock runs on regardless: that's the faint line.
  assert.ok(clock.now() > 10.6);
});

test("a seek moves the page's clock at once, and shown only when the player says", () => {
  const clock = new Clock(0, false);
  clock.heardAt(3, false);
  clock.set(40);
  assert.equal(clock.now(), 40);
  assert.equal(clock.shown(), 3);
  clock.heardAt(40, false);
  assert.equal(clock.shown(), 40);
});

test("a report of moving without a position keeps where it was shown", async () => {
  const clock = new Clock(0, true);
  clock.heardAt(8, true);
  await wait(100);
  clock.heardMoving(false);
  const at = clock.shown();
  await wait(100);
  assert.equal(clock.shown(), at);
});
