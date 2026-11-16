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

/** heldMute.ts `ready`: "The player is ready: the mute to send it again, if one is held." */
test("ready with no mute held sends none", () => {
  const mute = new HeldMute();
  assert.equal(mute.ready(), null);
  mute.set(true);
  assert.equal(mute.muted, true, "a mute set once ready is shown as set");
});

/** heldMute.ts: "shown on the bar in place of the player's own word till the player reports it". */
test("a mute set once the player is ready isn't held: its own word counts at once", () => {
  const mute = new HeldMute();
  mute.ready();
  mute.set(true);
  mute.heard(false);
  assert.equal(mute.muted, false);
});

test("with nothing set, the bar shows the player's word", () => {
  const mute = new HeldMute();
  mute.heard(true);
  assert.equal(mute.muted, true);
  mute.heard(false);
  assert.equal(mute.muted, false);
});

/** heldMute.ts: "sent again once it's ready". */
test("a player that says it's muted before it's ready is still sent the mute at ready", () => {
  const mute = new HeldMute();
  mute.set(true);
  mute.heard(true);
  assert.equal(mute.ready(), true);
  mute.heard(false);
  assert.equal(mute.muted, true, "not yet taken after ready");
});

/** heldMute.ts: "or for a few seconds at most" (3 s from ready, or from the mute set again since). */
test("a held mute stands three seconds from ready, no longer", () => {
  let now = 1000;
  const mute = new HeldMute(() => now);
  mute.set(true);
  now = 5000;
  mute.ready();
  now = 8000;
  mute.heard(false);
  assert.equal(mute.muted, true, "at three seconds exactly: held");
  now = 8001;
  mute.heard(false);
  assert.equal(mute.muted, false, "past three seconds: the player's word");
});

/** heldMute.ts `muted`: "Whether the bar shows the sound muted": not, to begin with. */
test("nothing set, the bar shows the sound not muted", () => {
  assert.equal(new HeldMute().muted, false);
});

/** heldMute.ts `heard`: before ready, the player's word doesn't let go of the held mute. */
test("the player's reports before ready don't let go of the held mute", () => {
  const mute = new HeldMute();
  mute.set(true);
  mute.heard(false);
  assert.equal(mute.muted, true);
  mute.heard(true);
  assert.equal(mute.ready(), true, "still held at ready");
});
