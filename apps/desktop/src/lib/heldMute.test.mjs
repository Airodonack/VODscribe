// A mute set on the bar before a site's player takes one (heldMute.ts).
// node --test apps/desktop/src/lib/heldMute.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";

import { HeldMute } from "./heldMute.ts";

test("muted before ready, the bar shows it till the player reports it", () => {
  const mute = new HeldMute();
  mute.set(true);
  mute.heard(false);
  assert.equal(mute.ready(), true, "the mute isn't sent again at ready");
  mute.heard(false);
  assert.equal(mute.muted, true, "the player's word before it took the mute");
  mute.heard(true);
  mute.heard(false);
  assert.equal(mute.muted, false, "once it took it, its own controls count again");
});

test("a player that never takes the mute sent at ready has its own word again soon after", () => {
  let now = 0;
  const mute = new HeldMute(() => now);
  mute.set(true);
  mute.ready();
  for (now = 0; now < 10_000; now += 250) mute.heard(false);
  assert.equal(mute.muted, false, "the bar still shows muted while it plays with sound");
});

test("muted again while waiting, the wait starts over", () => {
  let now = 0;
  const mute = new HeldMute(() => now);
  mute.set(true);
  mute.ready();
  now = 2500;
  mute.set(false);
  now = 4000;
  mute.heard(true);
  assert.equal(mute.muted, false, "the player's report before it took the new one");
  mute.heard(false);
  mute.heard(true);
  assert.equal(mute.muted, true, "its own controls once it took it");
});
