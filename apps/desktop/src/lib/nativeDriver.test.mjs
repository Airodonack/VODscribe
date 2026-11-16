// The page's side of the app's own player (nativeDriver.ts), with the app
// faked: its commands' replies and its `player-time` events come when the
// test says. node --test apps/desktop/src/lib/nativeDriver.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// nativeDriver.ts's "./api" is the fake below (the real one needs the app);
// its other imports are found as the .ts files they are.
const hooks = `
const fake = ${JSON.stringify(`
  export const api = new Proxy({}, { get: (_, name) => (...args) => globalThis.fakeApp.command(name, args) });
  export const events = {
    playerTime: (cb) => {
      globalThis.fakeApp.listeners.push(cb);
      return Promise.resolve(() => globalThis.fakeApp.unlistened++);
    },
  };`)};
export async function resolve(specifier, context, next) {
  if (specifier === "./api" && context.parentURL?.endsWith("/nativeDriver.ts"))
    return { url: "data:text/javascript," + encodeURIComponent(fake), shortCircuit: true };
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith(".ts"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`;
register("data:text/javascript," + encodeURIComponent(hooks));

/** @typedef {import("./api").PlayerStatus} Status */
/** @typedef {{ name: string, args: unknown[], answer: (s: Status) => void, refuse: (e: Error) => void }} Asked */
/** The app: each command waits for the test to answer it; events go out when told. */
const fakeApp = {
  /** @type {((s: Status) => void)[]} */
  listeners: [],
  /** @type {Asked[]} */
  asked: [],
  /** Every command sent, as `name(args)`. @type {string[]} */
  log: [],
  /** How many listeners were let go of. */
  unlistened: 0,
  /** @param {string} name @param {unknown[]} args */
  command(name, args) {
    this.log.push(name + "(" + args.join(", ") + ")");
    if (name === "playerSetVolume") return Promise.resolve();
    return new Promise((answer, refuse) => this.asked.push({ name, args, answer, refuse }));
  },
  /** @param {Status} status */
  event(status) {
    for (const cb of this.listeners) cb(status);
  },
};
Object.assign(globalThis, { fakeApp });
const { afterClose, NativeDriver } = await import("./nativeDriver.ts");

let seq = 0;
/** A status as the app makes them, each with the next `seq`. @param {number} time */
const status = (time, playing = true) => ({
  seq: ++seq,
  time,
  playing,
  running: playing,
  speed: 1,
  shown: time,
  fps: 30,
});

/** A driver for an opened player, playing at 6 s. */
function playing() {
  fakeApp.listeners.length = 0;
  fakeApp.asked.length = 0;
  fakeApp.log.length = 0;
  const driver = new NativeDriver(0);
  driver.opened({ fps: 30, duration: 60 });
  fakeApp.event(status(6));
  fakeApp.log.length = 0;
  return driver;
}

test("a status made before a seek, arriving before its reply, doesn't take the time back", async () => {
  const driver = playing();
  driver.seek(51, false);
  // Made just before the seek (its seq is the newest the page has seen yet), on its way meanwhile.
  fakeApp.event(status(6.03));
  assert.ok(driver.time() >= 51, `before the reply: at ${driver.time()}`);
  assert.equal(driver.playing(), true);
  const [seek] = fakeApp.asked;
  seek.answer(status(51));
  await Promise.resolve();
  assert.ok(driver.time() >= 51, `after the reply: at ${driver.time()}`);
  // What the player says after its reply is taken again.
  fakeApp.event(status(51.5));
  assert.ok(Math.abs(driver.time() - 51.5) < 0.05, `then on with the player: ${driver.time()}`);
});

test("one made before the reply, arriving after it, is older than the reply: not taken", async () => {
  const driver = playing();
  driver.seek(51, false);
  const before = status(6.03);
  const [seek] = fakeApp.asked;
  seek.answer(status(51));
  await Promise.resolve();
  fakeApp.event(before);
  assert.ok(driver.time() >= 51, `at ${driver.time()}`);
});

test("a refused seek lets the player's word in again", async () => {
  const driver = playing();
  driver.seek(51, false);
  const [seek] = fakeApp.asked;
  seek.refuse(new Error("No player is open."));
  await new Promise((r) => setTimeout(r, 0));
  fakeApp.event(status(7));
  assert.ok(Math.abs(driver.time() - 7) < 0.05, `at ${driver.time()}`);
});

// As in CI: the reply and an event made just before the seek come in the
// same millisecond, in one turn of microtasks, either way round.
for (const order of ["the event first", "the reply first"]) {
  test(`a reply and an older event in one turn, ${order}: the reply stands`, async () => {
    const driver = playing();
    driver.seek(51, false);
    const before = status(6.57);
    const reply = status(51);
    const [seek] = fakeApp.asked;
    if (order === "the event first") {
      queueMicrotask(() => fakeApp.event(before));
      seek.answer(reply);
    } else {
      seek.answer(reply);
      queueMicrotask(() => fakeApp.event(before));
    }
    // The reply's own event, as player.rs sends it with the reply.
    queueMicrotask(() => fakeApp.event(reply));
    await new Promise((r) => setTimeout(r, 0));
    assert.ok(driver.time() >= 51, `at ${driver.time()}`);
    assert.equal(driver.playing(), true);
  });
}

test("while it opens, what the player had open before says nothing of its time; the open's reply does", async () => {
  fakeApp.listeners.length = 0;
  fakeApp.asked.length = 0;
  // Another VOD's file, playing at 4 s in the player, as this one opens at its start.
  const driver = new NativeDriver(0);
  fakeApp.event(status(4));
  fakeApp.event(status(4.1));
  assert.equal(driver.time(), 0, "the time of what was open before showed");
  assert.equal(driver.shown(), null);
  // Its open's reply says where it is; the first frame's event, made after
  // it but come before it, is taken after it.
  const reply = status(0, false);
  const frame = { ...status(0, false), shown: 0 };
  fakeApp.event(frame);
  driver.opened({ fps: 30, duration: 5, status: { ...reply, shown: null } });
  assert.equal(driver.time(), 0);
  assert.equal(driver.playing(), false);
  assert.equal(driver.shown(), 0);
  // From then on the player's word is taken as ever.
  fakeApp.event(status(1));
  assert.ok(Math.abs(driver.time() - 1) < 0.01, `at ${driver.time()}`);
});

// The edit player keeps one driver for every edit it opens (editPlayer.ts).
test("a reply for what was open before, come once another is opening, says nothing of its time", async () => {
  const driver = playing();
  driver.seek(40, false);
  const [seek] = fakeApp.asked;
  driver.opening(0);
  seek.answer(status(40, false));
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(driver.time(), 0, "the time of the edit open before showed");
});

test("while it opens, the new one's first frame is held even with a command of the last one's out", async () => {
  const driver = playing();
  driver.seek(40, false);
  const [seek] = fakeApp.asked;
  driver.opening(0);
  const reply = status(0, false);
  const frame = status(0, false);
  fakeApp.event(frame);
  seek.answer({ ...status(40, false), seq: reply.seq - 1 });
  await new Promise((r) => setTimeout(r, 0));
  driver.opened({ fps: 30, duration: 60, status: { ...reply, shown: null } });
  assert.equal(driver.shown(), 0, "its first frame was lost");
  assert.equal(driver.time(), 0);
});

test("the next view's open is sent only once the last one's close is answered", async () => {
  fakeApp.asked.length = 0;
  new NativeDriver(0).destroy();
  const opened = afterClose(() => fakeApp.command("playerOpen", [2, 3, false]));
  await new Promise((r) => setTimeout(r, 0));
  // Sent together, the two could reach the app the other way round, and the
  // close would cancel the open ("closed while opening").
  assert.deepEqual(
    fakeApp.asked.map((a) => a.name),
    ["playerClose"],
  );
  fakeApp.asked[0].answer(status(0));
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(
    fakeApp.asked.map((a) => a.name),
    ["playerClose", "playerOpen"],
  );
  const reply = { fps: 30, duration: 12 };
  fakeApp.asked[1].answer(/** @type {any} */ (reply));
  assert.equal(await opened, reply);
});

/** A driver not opened yet, with nothing sent so far. */
function closed(start = 0) {
  fakeApp.listeners.length = 0;
  fakeApp.asked.length = 0;
  fakeApp.log.length = 0;
  fakeApp.unlistened = 0;
  return new NativeDriver(start);
}

/** Decided for 0.7.4: seek never changes play state (nativeDriver.ts `seek`: "one that plays goes on playing (the player doesn't pause for a seek: player.rs)"). */
test("a seek asked not to play leaves a playing player playing", () => {
  const driver = playing();
  driver.seek(30, false);
  assert.equal(driver.playing(), true);
  assert.equal(driver.time() >= 30, true);
  assert.deepEqual(fakeApp.log, ["playerSeek(30, false)"]);
});

test("a seek of a paused player leaves it paused, or plays it if asked to", () => {
  const driver = playing();
  fakeApp.event(status(6, false));
  driver.seek(10, false);
  assert.equal(driver.playing(), false);
  driver.seek(12, true);
  assert.equal(driver.playing(), true);
});

/** nativeDriver.ts: "until the player is open, a seek, play or speed asked is kept for then." */
test("a seek, play and speed asked before it opens are done when it opens, speed first", () => {
  const driver = closed();
  driver.speed.set(2);
  driver.seek(9, true);
  assert.deepEqual(fakeApp.log, [], "nothing sent while it opens");
  assert.equal(driver.playing(), true, "shown as asked");
  assert.equal(driver.speed.get(), 2);
  assert.equal(driver.time(), 9);
  driver.opened({ fps: 30, duration: 60 });
  assert.deepEqual(fakeApp.log, ["playerSetVolume(1)", "playerSetSpeed(2)", "playerSeek(9, true)"]);
});

test("a play asked before it opens isn't cancelled by a later seek asked not to play", () => {
  const driver = closed();
  driver.seek(5, true);
  driver.seek(9, false);
  assert.equal(driver.playing(), true);
  driver.opened({ fps: 30, duration: 60 });
  assert.ok(fakeApp.log.includes("playerSeek(9, true)"), fakeApp.log.join());
});

test("play toggled before it opens is kept for then, and toggled back it's a pause", () => {
  const driver = closed(4);
  driver.togglePlay();
  assert.equal(driver.playing(), true);
  driver.togglePlay();
  assert.equal(driver.playing(), false);
  driver.opened({ fps: 30, duration: 60 });
  assert.deepEqual(fakeApp.log, ["playerSetVolume(1)", "playerSeek(4, false)"]);
});

test("nothing asked before it opens, nothing is sent when it does but the volume", () => {
  const driver = closed();
  driver.opened({ fps: 30, duration: 60 });
  assert.deepEqual(fakeApp.log, ["playerSetVolume(1)"]);
});

test("open, play and pause are sent as the player is toggled", () => {
  const driver = playing();
  driver.togglePlay();
  assert.equal(driver.playing(), false, "shown at once");
  driver.togglePlay();
  assert.equal(driver.playing(), true);
  assert.deepEqual(fakeApp.log, ["playerPause()", "playerPlay()"]);
});

test("a step is sent only once it's open", () => {
  const driver = closed();
  driver.step(1);
  assert.deepEqual(fakeApp.log, []);
  driver.opened({ fps: 30, duration: 60 });
  driver.step(-2);
  assert.deepEqual(fakeApp.log, ["playerSetVolume(1)", "playerStep(-2)"]);
});

/** nativeDriver.ts `time`: "How long what's open is, once known (the time stays within it)". */
test("the time stays within what's open", () => {
  const driver = playing();
  fakeApp.event(status(75, false));
  assert.equal(driver.time(), 60);
  const open = closed();
  open.seek(75, false);
  assert.equal(open.time(), 75, "unknown length: as it is");
});

/** nativeDriver.ts: "runs the time on by the page's clock while the player's runs (after play it stands until the sound starts)". */
test("playing and running are the player's own words, apart", () => {
  const driver = playing();
  fakeApp.event({ ...status(7), running: false });
  assert.equal(driver.playing(), true);
  assert.equal(driver.running(), false, "after play, standing until the sound starts");
  fakeApp.event(status(7.1));
  assert.equal(driver.running(), true);
});

/** nativeDriver.ts `fps`: "The frame rate at the frame shown, else at the start, once known." */
test("the frame rate is the one at the frame shown, else the start's", () => {
  const driver = closed();
  assert.equal(driver.fps, null);
  driver.opened({ fps: 25, duration: 60 });
  assert.equal(driver.fps, 25);
  fakeApp.event({ ...status(1), fps: 50 });
  assert.equal(driver.fps, 50);
  fakeApp.event({ ...status(2), fps: null });
  assert.equal(driver.fps, 25, "the player not saying: the start's");
});

test("the speed is the player's, and a change is shown at once and sent", () => {
  const driver = playing();
  assert.equal(driver.speed.get(), 1);
  driver.speed.set(1.5);
  assert.equal(driver.speed.get(), 1.5);
  assert.deepEqual(fakeApp.log, ["playerSetSpeed(1.5)"]);
  assert.equal(driver.playing(), true, "a speed change doesn't pause");
});

/** 0.7.1: "Set the volume before Play." (nativeDriver.ts `opened` sends the volume first.) */
test("the volume set before it opens is sent as it opens, and a mute as 0", () => {
  const driver = closed();
  driver.volume.set(0.4);
  assert.equal(driver.volume.get(), 0.4);
  assert.deepEqual(fakeApp.log, [], "not open: nothing sent");
  driver.opened({ fps: 30, duration: 60 });
  assert.deepEqual(fakeApp.log, ["playerSetVolume(0.4)"]);
  const muted = closed();
  muted.volume.setMuted(true);
  muted.opened({ fps: 30, duration: 60 });
  assert.deepEqual(fakeApp.log, ["playerSetVolume(0)"]);
});

test("a volume of 0 is a mute, muting keeps the volume to come back to", () => {
  const driver = playing();
  driver.volume.set(0.6);
  driver.volume.setMuted(true);
  assert.equal(driver.volume.muted(), true);
  assert.equal(driver.volume.get(), 0.6);
  driver.volume.setMuted(false);
  assert.equal(driver.volume.muted(), false);
  driver.volume.set(0);
  assert.equal(driver.volume.muted(), true);
  driver.volume.set(0.2);
  assert.equal(driver.volume.muted(), false);
  assert.deepEqual(fakeApp.log, [
    "playerSetVolume(0.6)",
    "playerSetVolume(0)",
    "playerSetVolume(0.6)",
    "playerSetVolume(0)",
    "playerSetVolume(0.2)",
  ]);
});

/** nativeDriver.ts `settled`: "When the last seek's reply came (performance.now()), null while it's out, -Infinity if it was refused or none was sent." */
test("a seek's reply is when it settled; out, none; refused or none sent, never", async () => {
  const driver = playing();
  assert.equal(driver.settled(), -Infinity, "none sent");
  driver.seek(20, false);
  assert.equal(driver.settled(), null, "out");
  fakeApp.asked[0].answer(status(20));
  await new Promise((r) => setTimeout(r, 0));
  const at = driver.settled();
  assert.ok(typeof at === "number" && Number.isFinite(at), `${at}`);
  driver.seek(30, false);
  fakeApp.asked[1].refuse(new Error("No player is open."));
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(driver.settled(), -Infinity, "refused");
});

test("an older seek's reply, after a newer seek, doesn't say the newer settled", async () => {
  const driver = playing();
  driver.seek(20, false);
  driver.seek(40, false);
  fakeApp.asked[0].answer(status(20));
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(driver.settled(), null);
});

/** nativeDriver.ts `destroy`: "Stops listening and closes the player." */
test("destroyed, it closes the player and takes no more of what it says", () => {
  const driver = playing();
  driver.destroy();
  assert.deepEqual(fakeApp.log, ["playerClose()"]);
  fakeApp.event(status(55, false));
  assert.ok(driver.time() < 50, `at ${driver.time()}`);
});

/** nativeDriver.ts `opening`: "Something else is being opened, at `time`: nothing's sent until it is." */
test("while something else opens, the time shown is the one opened at, paused, and nothing is sent", () => {
  const driver = playing();
  fakeApp.log.length = 0;
  driver.opening(12);
  assert.ok(Math.abs(driver.time() - 12) < 0.01, `${driver.time()}`);
  assert.equal(driver.playing(), false);
  driver.togglePlay();
  driver.seek(20, false);
  assert.deepEqual(fakeApp.log, []);
  assert.equal(driver.shown(), 6, "the frame on screen stays what it was");
});

/** nativeDriver.ts `tell`: "For the tests (player-seek.mjs): each status taken, in order". */
test("each status taken is recorded for the e2e harness, newest 500", () => {
  const w = /** @type {any} */ (globalThis);
  w.__e2e = {};
  const driver = playing();
  fakeApp.event(status(7, false));
  const last = w.__e2e.taken.at(-1);
  assert.equal(last.time, 7, "the status's own time");
  assert.equal(last.playing, false);
  assert.equal(typeof last.seq, "number");
  assert.ok(Math.abs(driver.time() - 7) < 0.01);
  for (let i = 0; i < 600; i++) fakeApp.event(status(8));
  assert.equal(w.__e2e.taken.length, 500);
  delete w.__e2e;
});

/** nativeDriver.ts: "until the player is open, a seek, play or speed asked is kept for then": a seek asked not to play, first, stays paused. */
test("a seek asked not to play, before it opens, of a player not playing, is paused", () => {
  const driver = closed();
  driver.seek(5, false);
  assert.equal(driver.playing(), false);
  driver.opened({ fps: 30, duration: 60 });
  assert.ok(fakeApp.log.includes("playerSeek(5, false)"), fakeApp.log.join());
});

/** nativeDriver.ts `held`: "The newest of the player's events while this opens", taken once it opens. */
test("of the events said while it opens, the newest is taken at the open, whatever order they came", () => {
  const driver = closed();
  const newest = status(9, false);
  fakeApp.event(newest);
  fakeApp.event({ ...status(2, false), seq: newest.seq - 1 });
  fakeApp.event({ ...status(3, false), seq: newest.seq });
  driver.opened({ fps: 30, duration: 60 });
  assert.ok(Math.abs(driver.time() - 9) < 0.01, `${driver.time()}`);
});

test("the open's own status is taken as it opens, with nothing said meanwhile", () => {
  const driver = closed();
  driver.opened({ fps: 30, duration: 60, status: status(33, false) });
  assert.ok(Math.abs(driver.time() - 33) < 0.01, `${driver.time()}`);
  assert.equal(driver.playing(), false);
});

/** nativeDriver.ts `destroy`: "Stops listening". */
test("destroyed, the listener is let go of, once it's known too", async () => {
  const driver = closed();
  await Promise.resolve();
  driver.destroy();
  assert.equal(fakeApp.unlistened, 1);
  const early = new NativeDriver(0);
  early.destroy();
  await Promise.resolve();
  assert.equal(fakeApp.unlistened, 2, "destroyed before the listener was set up: let go as it is");
});

/** nativeDriver.ts `tell`: "For the tests: each status taken, in order". */
test("the e2e harness's record starts empty", () => {
  const w = /** @type {any} */ (globalThis);
  w.__e2e = {};
  const driver = closed();
  driver.opened({ fps: 30, duration: 60 });
  fakeApp.event(status(2, false));
  assert.equal(w.__e2e.taken.length, 1);
  delete w.__e2e;
});

/** nativeDriver.ts `settled`: a reply to a seek sent before something else opened says nothing of what's open now. */
test("a seek's reply that comes after another open began doesn't count as settled", async () => {
  const driver = playing();
  driver.seek(20, false);
  driver.opening(0);
  fakeApp.asked[0].answer(status(20));
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(driver.settled(), -Infinity);
});

/** nativeDriver.ts `opening`: "a command's reply sent before the newest is of what was open then": its refusal too. */
test("a command of what was open before, refused after the next one opened, leaves the new one's events taken", async () => {
  const driver = playing();
  driver.seek(20, false);
  driver.opening(0);
  driver.opened({ fps: 30, duration: 60, status: status(0, false) });
  fakeApp.asked[0].refuse(new Error("closed"));
  await new Promise((r) => setTimeout(r, 0));
  fakeApp.event(status(4, false));
  assert.ok(Math.abs(driver.time() - 4) < 0.01, `${driver.time()}`);
});
