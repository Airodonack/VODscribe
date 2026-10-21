// A player's position on the page (playerClock.ts): node --test apps/desktop/src/lib/playerClock.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as wait } from "node:timers/promises";
import { Clock } from "./playerClock.ts";

test("before the player says anything, there's no shown position", () => {
  assert.equal(new Clock(5, true).shown(), null);
});

test("a clock that waits for its player stands until the player says it's moving", async () => {
  // A site's player asked to play as it loads: nothing heard for a while.
  const clock = new Clock(0, true, { waitsForPlayer: true });
  await wait(150);
  assert.equal(clock.now(), 0, "no word yet: it stands");
  clock.heardAt(0, false);
  await wait(100);
  assert.equal(clock.now(), 0, "there, not moving yet: it stands");
  clock.heardAt(0.2, true);
  await wait(100);
  assert.ok(clock.now() > 0.25, `moving: it runs (${clock.now()})`);
  // One that doesn't wait runs from the start, as before.
  const eager = new Clock(0, true);
  await wait(100);
  assert.ok(eager.now() > 0.05);
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

test("a player that goes quiet stops showing as moving a second and a half on", async () => {
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  await wait(1700);
  const shown = clock.shown() ?? 0;
  assert.ok(Math.abs(shown - 11.5) < 0.01, `${shown}`);
  // The page's own clock runs on regardless: that's the faint line.
  assert.ok(clock.now() > 11.6);
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

test("while a seek is under way, positions from elsewhere count as the target", () => {
  const clock = new Clock(0, true);
  clock.heardAt(100, true);
  clock.asked(5000, true);
  // A long VOD reloading for the seek: the old place, or 0.
  clock.heardAt(100, false);
  clock.heardAt(0, false);
  assert.ok(Math.abs(clock.now() - 5000) < 0.05, "the page's clock stays at the target");
  assert.equal(clock.shown(), 5000, "the solid line at the target, standing");
  // There, still buffering: shown where it says.
  clock.heardAt(5000.2, false);
  assert.equal(clock.shown(), 5000.2);
  clock.heardAt(0, false);
  assert.equal(clock.shown(), 5000.2, "not back to the target");
  // Playing there: through, and reports are taken as they come again.
  clock.heardAt(5000.5, true);
  clock.heardAt(10, true);
  assert.ok(Math.abs(clock.now() - 10) < 0.05, "taken again");
});

test("Twitch saying it plays, at the old place, after a seek: playing from the target", async () => {
  const clock = new Clock(7500, true);
  clock.heardAt(7500.7, true);
  clock.asked(20849.4, true);
  clock.heardAt(7500.7, false); // Buffering
  clock.heardAt(7501.7, true); // Playing, the old place still
  await wait(100);
  const shown = clock.shown() ?? 0;
  assert.ok(shown > 20849.45 && shown < 20849.7, `${shown}`);
});

test("a seek while paused is through once the player says it's there", () => {
  const clock = new Clock(0, false);
  clock.heardAt(30, false);
  clock.asked(200, false);
  // Twitch: the old place once, Idle, then nothing till played.
  clock.heardAt(30, false);
  assert.equal(clock.shown(), 200, "at the target, standing");
  clock.heardAt(200, false);
  clock.heardAt(31, false);
  assert.equal(clock.shown(), 31, "through: taken as it comes");
});

test("a speed change runs both positions on at the new speed from where they are", async () => {
  const clock = new Clock(10, true);
  clock.heardAt(10, true);
  await wait(100);
  const before = clock.now();
  clock.setSpeed(2);
  assert.ok(Math.abs(clock.now() - before) < 0.02, "no jump");
  await wait(200);
  const now = clock.now() - before;
  const shown = (clock.shown() ?? 0) - before;
  assert.ok(now > 0.35 && now < 0.5, `ran ${now} s in 0.2 s`);
  assert.ok(shown > 0.35 && shown < 0.5, `shown ran ${shown} s in 0.2 s`);
});

// Recorded from Twitch's embed (win-11, WebView2 155, October 17): while
// playing it says where it is twice a second, the second time repeating
// the first's position; after a seek, it says the place before it once more.

test("Twitch repeating its last position while playing doesn't pull the solid line back", async () => {
  const clock = new Clock(18.51, true);
  clock.heardAt(18.51, true);
  await wait(500);
  const before = clock.shown() ?? 0;
  clock.heardAt(18.51, true); // said again, half a second on
  const after = clock.shown() ?? 0;
  assert.ok(after >= before, `${after} after ${before}`);
  clock.heardAt(19.57, true);
  assert.ok(Math.abs((clock.shown() ?? 0) - 19.57) < 0.01, "a new position is taken");
});

test("a seek of a second while playing isn't undone by the place before it, said again", () => {
  const clock = new Clock(15.63, true);
  clock.heardAt(15.63, true);
  clock.asked(17.22, true);
  clock.heardAt(15.63, true); // the old place, Playing
  assert.ok(Math.abs((clock.shown() ?? 0) - 17.22) < 0.05, `${clock.shown()}`);
  assert.ok(Math.abs(clock.now() - 17.22) < 0.05, `${clock.now()}`);
  clock.heardAt(17.44, true);
  assert.ok(Math.abs((clock.shown() ?? 0) - 17.44) < 0.01, "there: through");
});

test("a seek of a second while paused stays at the target when the old place is said again", () => {
  const clock = new Clock(32.77, false);
  clock.heardAt(32.77, false);
  clock.asked(33.77, false);
  clock.heardAt(32.77, false); // the old place once, Idle
  assert.equal(clock.shown(), 33.77);
  assert.equal(clock.now(), 33.77);
});
