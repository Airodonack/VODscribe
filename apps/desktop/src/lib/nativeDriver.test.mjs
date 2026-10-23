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
