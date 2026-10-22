// A site's player's plays and pauses, settled (settle.ts): node --test apps/desktop/src/lib/settle.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as wait } from "node:timers/promises";
import { settling } from "./settle.ts";

test("Twitch's gesture capture (play, pause at once, play later) reaches the app's player as one play", async () => {
  /** @type {boolean[]} */
  const sent = [];
  const s = settling((playing) => sent.push(playing), 30);
  s.want(true);
  s.want(false);
  await wait(10);
  s.want(true);
  await wait(60);
  assert.deepEqual(sent, [true]);
});

test("a pause that stands is passed on", async () => {
  /** @type {boolean[]} */
  const sent = [];
  const s = settling((playing) => sent.push(playing), 30);
  s.want(true);
  await wait(60);
  s.want(false);
  await wait(60);
  assert.deepEqual(sent, [true, false]);
});

test("the same again is passed on (the app's player may have been paused meanwhile, for an ad)", async () => {
  /** @type {boolean[]} */
  const sent = [];
  const s = settling((playing) => sent.push(playing), 30);
  s.want(true);
  await wait(60);
  s.want(true);
  await wait(60);
  assert.deepEqual(sent, [true, true]);
});

test("stopped, nothing waiting is passed on", async () => {
  /** @type {boolean[]} */
  const sent = [];
  const s = settling((playing) => sent.push(playing), 30);
  s.want(true);
  s.stop();
  await wait(60);
  assert.deepEqual(sent, []);
});
