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
      return Promise.resolve(() => {});
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
  /** @param {string} name @param {unknown[]} args */
  command(name, args) {
    if (name === "playerSetVolume") return Promise.resolve();
    return new Promise((answer, refuse) => this.asked.push({ name, args, answer, refuse }));
  },
  /** @param {Status} status */
  event(status) {
    for (const cb of this.listeners) cb(status);
  },
};
Object.assign(globalThis, { fakeApp });
const { NativeDriver } = await import("./nativeDriver.ts");

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
  const driver = new NativeDriver(0);
  driver.opened({ fps: 30, duration: 60 });
  fakeApp.event(status(6));
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
