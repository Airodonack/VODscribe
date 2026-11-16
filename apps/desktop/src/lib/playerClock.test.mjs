// A player's position on the page (playerClock.ts): node --test apps/desktop/src/lib/playerClock.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { Clock } from "./playerClock.ts";

test("before the player says anything, there's no shown position", () => {
  assert.equal(new Clock(5, true).shown(), null);
});

test("a clock that waits for its player stands until the player says it's moving", (t) => {
  const time = madeUpTime(t);
  // A site's player asked to play as it loads: nothing heard for a while.
  const clock = new Clock(0, true, { waitsForPlayer: true });
  time.after(150);
  assert.equal(clock.now(), 0, "no word yet: it stands");
  clock.heardAt(0, false);
  time.after(100);
  assert.equal(clock.now(), 0, "there, not moving yet: it stands");
  clock.heardAt(0.2, true);
  time.after(100);
  assert.ok(clock.now() > 0.25, `moving: it runs (${clock.now()})`);
  // One that doesn't wait runs from the start, as before.
  const eager = new Clock(0, true);
  time.after(100);
  assert.ok(eager.now() > 0.05);
});

test("shown runs on from a report only while the player said it was moving", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.after(100);
  const moving = clock.shown() ?? 0;
  assert.ok(moving > 10.05 && moving < 10.3, `${moving}`);
  clock.heardAt(12, false);
  time.after(50);
  assert.equal(clock.shown(), 12, "said paused: it stands");
});

test("a player that goes quiet stops showing as moving a second and a half on", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.after(1700);
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

test("a report of moving without a position keeps where it was shown", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(8, true);
  time.after(100);
  clock.heardMoving(false);
  const at = clock.shown();
  time.after(100);
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

test("Twitch saying it plays, at the old place, after a seek: playing from the target", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(7500, true);
  clock.heardAt(7500.7, true);
  clock.asked(20849.4, true);
  clock.heardAt(7500.7, false); // Buffering
  clock.heardAt(7501.7, true); // Playing, the old place still
  time.after(100);
  const shown = clock.shown() ?? 0;
  assert.ok(shown > 20849.45 && shown < 20849.7, `${shown}`);
});

test("a site's player opened at a time, saying 0 as it loads: the time stays where it was opened", () => {
  // As embed.ts opens Twitch's: at 26.8 s, paused, as a click on the timeline does.
  const clock = new Clock(26.8, false, { waitsForPlayer: true });
  clock.asked(26.8, false);
  clock.heardAt(0, false);
  assert.ok(Math.abs(clock.now() - 26.8) < 1e-9, `the page's clock at 26.8, not ${clock.now()}`);
  // There (its time takes whole seconds): through, and taken as it comes.
  clock.heardAt(26, false);
  assert.equal(clock.now(), 26);
  clock.heardAt(30, false);
  assert.equal(clock.now(), 30);
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

test("a speed change runs both positions on at the new speed from where they are", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(10, true);
  clock.heardAt(10, true);
  time.after(100);
  const before = clock.now();
  clock.setSpeed(2);
  assert.ok(Math.abs(clock.now() - before) < 0.02, "no jump");
  time.after(200);
  const now = clock.now() - before;
  const shown = (clock.shown() ?? 0) - before;
  assert.ok(now > 0.35 && now < 0.5, `ran ${now} s in 0.2 s`);
  assert.ok(shown > 0.35 && shown < 0.5, `shown ran ${shown} s in 0.2 s`);
});

// Recorded from Twitch's embed (win-11, WebView2 155, October 17): while
// playing it says where it is twice a second, the second time repeating
// the first's position; after a seek, it says the place before it once more.

test("Twitch repeating its last position while playing doesn't pull the solid line back", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(18.51, true);
  clock.heardAt(18.51, true);
  time.after(500);
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

test("a clock that waits doesn't leap on by the time it stood, after a seek", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true, { waitsForPlayer: true });
  clock.heardAt(10, true);
  clock.asked(100, true);
  // Twitch: the old place, buffering at the new one; then playing there.
  clock.heardAt(10, false);
  time.after(300);
  assert.ok(Math.abs(clock.now() - 100) < 0.01, `stands at the target while buffering: ${clock.now()}`);
  clock.heardAt(10, true);
  assert.ok(clock.now() < 100.05, `runs on from the target, not 0.3 s ahead: ${clock.now()}`);
});

test("a clock that waits doesn't leap when told it's moving without a position", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true, { waitsForPlayer: true });
  clock.heardAt(0, false);
  time.after(300);
  clock.heardMoving(true);
  assert.ok(clock.now() < 0.05, `from 0, not 0.3: ${clock.now()}`);
});

test("a clock that waits doesn't jump back when told it stopped without a position", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true, { waitsForPlayer: true });
  clock.heardAt(5, true);
  time.after(300);
  clock.heardMoving(false);
  assert.ok(clock.now() > 5.25, `stays where it ran to, not back to 5: ${clock.now()}`);
});

test("pausing, the last position said again doesn't pull it back", (t) => {
  const time = madeUpTime(t);
  // Twitch, as it pauses: Idle, with the position it last said while playing.
  const clock = new Clock(0, true);
  clock.heardAt(4.4, true);
  time.after(300);
  clock.heardAt(4.4, false);
  assert.ok(clock.now() > 4.65, `the time where it ran to: ${clock.now()}`);
  assert.ok((clock.shown() ?? 0) > 4.65, `shown where it ran to: ${clock.shown()}`);
  // Its true place, said after, counts.
  clock.heardAt(4.77, false);
  assert.equal(clock.shown(), 4.77);
});

test("a seek while playing, the player buffering at the old place: the time stands at the target, not past it", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true, { waitsForPlayer: true });
  clock.heardAt(10, true);
  clock.asked(100, true);
  time.after(300);
  clock.heardAt(10, false);
  assert.ok(Math.abs(clock.now() - 100) < 0.02, `at the target: ${clock.now()}`);
  clock.heardAt(10, true);
  assert.ok(clock.now() < 100.05, `runs on from the target: ${clock.now()}`);
});

test("after a seek, where the picture is stands at its old place till the player says (it's seeking)", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.after(100);
  clock.asked(100, true);
  const at = clock.shown() ?? 0;
  time.after(300);
  assert.ok(Math.abs((clock.shown() ?? 0) - at) < 0.02, `stands at ${at}, not run on: ${clock.shown()}`);
  clock.heardAt(100.1, true);
  assert.ok(Math.abs((clock.shown() ?? 0) - 100.1) < 0.02, `there once said: ${clock.shown()}`);
});

/** The page's time made up for the test (`t.after` puts it back): `at(ms)` sets it.
 *  @param {import("node:test").TestContext} t */
function madeUpTime(t) {
  const real = /** @type {PropertyDescriptor} */ (Object.getOwnPropertyDescriptor(globalThis, "performance"));
  let ms = 0;
  Object.defineProperty(globalThis, "performance", { value: { now: () => ms }, configurable: true });
  t.after(() => Object.defineProperty(globalThis, "performance", real));
  return { at: (/** @type {number} */ to) => (ms = to), after: (/** @type {number} */ by) => (ms += by) };
}

// Recorded from Twitch's embed (WebKitGTK 2.54, October 22): its position
// moves on in quarter seconds, so one report came 0.25 s short and the next
// made up for it; taken as exact, the time by frame went 7 back, then 9 on.
test("Twitch's reports a quarter second off don't move the time by frame back or on in leaps", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(4, true, { waitsForPlayer: true });
  const said = [
    [0, 4.338],
    [188, 4.338],
    [1001, 5.091],
    [1211, 5.091],
    [2004, 6.341],
    [2231, 6.341],
  ];
  let last = null;
  for (let ms = 0; ms <= 3000; ms += 50) {
    time.at(ms);
    for (; said.length && said[0][0] <= ms; said.shift()) clock.heardAt(said[0][1], true);
    const frame = Math.floor((clock.shown() ?? 0) * 30);
    if (last !== null) assert.ok(frame >= last && frame - last <= 3, `at ${ms} ms: frame ${last} -> ${frame}`);
    last = frame;
  }
});

test("a report further off than that is taken as it comes", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(4, true, { waitsForPlayer: true });
  clock.heardAt(10, true);
  time.at(1000);
  clock.heardAt(10.6, true);
  assert.equal(clock.shown(), 10.6);
});

test("after a pause, play or speed change, a report near the time is taken exactly", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(4, true, { waitsForPlayer: true });
  clock.heardAt(10, true);
  time.at(500);
  clock.playing = false;
  clock.playing = true;
  time.at(600);
  clock.heardAt(10.5, true);
  assert.equal(clock.shown(), 10.5, "after play: exact");
  time.at(700);
  clock.setSpeed(2);
  time.at(800);
  clock.heardAt(10.8, true);
  assert.equal(clock.shown(), 10.8, "after a speed change: exact");
  time.at(900);
  clock.heardMoving(false);
  clock.heardMoving(true);
  time.at(1000);
  clock.heardAt(11.1, true);
  assert.equal(clock.shown(), 11.1, "moving again after a stop: exact");
});

test("reports further apart than STALE, the player losing a little each time: the time stays near them", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(10, true, { waitsForPlayer: true });
  // Each 1.7 s on, the player 1.5 s on: it lost 0.2 s in stalls it didn't say.
  for (let i = 0; i <= 5; i++) {
    time.at(i * 1700);
    const said = 10 + i * 1.5;
    clock.heardAt(said, true);
    assert.ok(Math.abs(clock.now() - said) <= 0.3, `report ${i}: the time ${clock.now()}, the player ${said}`);
  }
});

/**
 * playerClock.ts `playing`: "Whether it plays (or is about to: buffering).
 * The time goes on from where it is as this changes."
 */
test("paused, the time stands; played again, it goes on from there, not leaping by the time it stood", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  time.at(1000);
  clock.playing = false;
  assert.equal(clock.now(), 1);
  time.at(5000);
  assert.equal(clock.now(), 1, "stands while paused");
  clock.playing = true;
  assert.equal(clock.now(), 1, "not leaping by the time it stood");
  time.at(6000);
  assert.equal(clock.now(), 2);
});

/** playerClock.ts `speed`: "Seconds of the media a second of play is." */
test("at twice the speed the time runs twice as fast, and shown with it", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  assert.equal(clock.speed, 1);
  clock.heardAt(10, true);
  clock.setSpeed(2);
  time.at(500);
  assert.equal(clock.now(), 11);
  assert.equal(clock.shown(), 11);
  clock.setSpeed(2);
  assert.equal(clock.shown(), 11, "the same speed again changes nothing");
});

/**
 * playerClock.ts `asked`: "until the player is seen there, the positions it
 * reports from anywhere else are taken as the target"; the seek is given up
 * on after SEEK_GIVE_UP (15 s).
 */
test("a seek the player never reaches is given up on after fifteen seconds", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.asked(100, true);
  time.at(14_000);
  clock.heardAt(5, true);
  assert.equal(clock.shown(), 100, "set aside: the target");
  time.at(16_000);
  clock.heardAt(6, true);
  assert.equal(clock.shown(), 6, "given up: the player's word");
});

/** playerClock.ts `heardAt`: "The player said it's at `seconds`, moving or not (unknown: as it last said)." */
test("a report that doesn't say whether it moves keeps what the player said before", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.at(1000);
  clock.heardAt(12);
  time.at(1500);
  assert.equal(clock.shown(), 12.5, "still moving");
  clock.heardAt(14, false);
  clock.heardAt(15);
  time.at(2500);
  assert.equal(clock.shown(), 15, "still stopped");
});

/** playerClock.ts `heardMoving`: "The player said whether it's moving, without a position." */
test("a word of moving before the player has said where it is shows nothing", () => {
  const clock = new Clock(3, true);
  clock.heardMoving(true);
  assert.equal(clock.shown(), null);
});

/** playerClock.ts `heardMoving` and `shown`: a player that stops says so, and stands where it was. */
test("a player that says it stopped stands where it was shown", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.at(800);
  clock.heardMoving(false);
  assert.equal(clock.shown(), 10.8);
  time.at(5000);
  assert.equal(clock.shown(), 10.8);
});

/** playerClock.ts `set`: "where it was last told to be". */
test("told where it is, the page's clock runs on from there, only while it plays", (t) => {
  const time = madeUpTime(t);
  const paused = new Clock(0, false);
  const playing = new Clock(0, true);
  time.at(1000);
  paused.set(7);
  playing.set(7);
  time.at(2000);
  assert.equal(paused.now(), 7);
  assert.equal(playing.now(), 8);
});

/** Decided for 0.7.4: seek never changes play state (playerClock.ts `asked`: "`play`: playing there"). */
test("a seek doesn't change whether the clock plays", (t) => {
  const time = madeUpTime(t);
  const playing = new Clock(0, true);
  playing.asked(50, false);
  assert.equal(playing.playing, true, "asked not to play: still playing");
  time.at(1000);
  assert.equal(playing.now(), 51, "runs on from the target");
  const paused = new Clock(0, false);
  paused.asked(50, true);
  assert.equal(paused.playing, false, "asked to play: the clock is told by the player, not the seek");
  time.at(2000);
  assert.equal(paused.now(), 50);
});

/** playerClock.ts `changed`: "Played, paused, sought or sped up since the player last said where it is: its next report is where it is now, exactly, however near." Unchanged, a near report agrees. */
test("a play, a speed or a moving word that changes nothing leaves the next near report agreeing", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.at(500);
  clock.playing = true;
  clock.setSpeed(1);
  clock.heardMoving(true);
  clock.heardAt(10.55, true);
  assert.equal(clock.shown(), 10.5, "agreed, not taken exactly");
});

/** playerClock.ts `shown`: "Null before it has said anything." */
test("a speed change or a seek before the player has said anything shows nothing", () => {
  const sped = new Clock(0, true);
  sped.setSpeed(2);
  assert.equal(sped.shown(), null);
  const sought = new Clock(0, true);
  sought.asked(5, true);
  assert.equal(sought.shown(), null);
});

/** playerClock.ts `asked`: given up at `until`, 15 s on; not before, nor at that moment. */
test("a seek is still waited on at fifteen seconds exactly", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.asked(100, true);
  time.at(15_000);
  clock.heardAt(5, true);
  assert.equal(clock.shown(), 100);
});

/** playerClock.ts `NEAR`: "How near a seek's target a report must be to count as there": two seconds is. */
test("a report two seconds from a seek's target is there", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.asked(100, true);
  time.at(100);
  clock.heardAt(102, true);
  assert.equal(clock.shown(), 102);
});

/** playerClock.ts `heardAt`: "moving or not as it says"; saying nothing of moving, as it last said (nothing said: not moving). */
test("while a seek is waited on, a report that says nothing of moving is not moving the first time, as before after", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.asked(100, true);
  clock.heardAt(5);
  time.at(1000);
  assert.equal(clock.shown(), 100, "not moving");
  const moving = new Clock(0, true);
  moving.heardAt(10, true);
  moving.asked(100, true);
  moving.heardAt(5, true);
  time.at(1500);
  moving.heardAt(5);
  time.at(2000);
  assert.ok((moving.shown() ?? 0) > 100, `still moving: ${moving.shown()}`);
});

/** playerClock.ts `heardAt` ("Not moving there yet: the time stands at the target, not where it had run on to"). */
test("a seek's target is where the time stands while not moving, and runs on from while moving", (t) => {
  const time = madeUpTime(t);
  const stands = new Clock(0, true);
  stands.heardAt(10, true);
  stands.asked(100, true);
  time.at(500);
  stands.heardAt(10, false);
  assert.equal(stands.now(), 100);
  const runs = new Clock(0, true);
  time.at(0);
  runs.heardAt(10, true);
  runs.asked(100, true);
  time.at(500);
  runs.heardAt(10, true);
  assert.equal(runs.now(), 100.5);
});

/** playerClock.ts `heardAt` ("Twitch says its last position again"): said again with no word of moving, while moving, it's still run on. */
test("the last position said again, with no word of moving, leaves the line running on", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.at(500);
  clock.heardAt(10);
  assert.equal(clock.shown(), 10.5);
});

/** playerClock.ts `heardAt`: "Stopped, said with the place it last said while moving: it stopped where it had run on to". */
test("the last position said again as it stops leaves it standing where it ran to", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.at(500);
  clock.heardAt(10, false);
  time.at(5000);
  assert.equal(clock.shown(), 10.5);
});

/** playerClock.ts `heardAt` (`AGREES`): a near report that says it stopped is taken as it is. */
test("a near report that says it stopped is taken exactly, and stands", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(10, true);
  time.at(500);
  clock.heardAt(10.55, false);
  time.at(5000);
  assert.equal(clock.shown(), 10.55);
});

/** playerClock.ts `heardAt`: a first report with no word of moving is not moving. */
test("a first report that says nothing of moving stands", (t) => {
  const time = madeUpTime(t);
  const clock = new Clock(0, true);
  clock.heardAt(5);
  time.at(1000);
  assert.equal(clock.shown(), 5);
});
