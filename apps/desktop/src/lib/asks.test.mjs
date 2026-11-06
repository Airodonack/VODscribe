// The notes that can be stopped (asks.ts); their switches in Settings are
// the settings scenario's (tests/e2e/scenarios/settings.mjs):
// node --test apps/desktop/src/lib/asks.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { ASKS, BROUGHT_OVER, OLD_KEYS, broughtOver, shows } from "./asks.ts";

/** @typedef {import("./asks.ts").Ask} Ask */
/** The notes stopped under a key of 0.7.0's. */
const OLD = /** @type {Ask[]} */ (Object.keys(OLD_KEYS));
/** @param {string} name */
const source = (name) => readFileSync(new URL(name, import.meta.url), "utf8");
/** The settings as they start, and after a reset. */
const FRESH = { ask_before_downloading: true, suggest_mov_for_resolve: true, ask_to_use_vodscription: true };

test("a note shows unless its setting is off, also before the settings are in", () => {
  for (const ask of ASKS) {
    assert.equal(shows(null, ask), true);
    assert.equal(shows({}, ask), true);
    assert.equal(shows({ [ask]: true }, ask), true);
    assert.equal(shows({ [ask]: false }, ask), false);
  }
});

test("a note stopped under its old key stays stopped, brought over once", () => {
  const kept = new Map([
    ["vodscribe.clipNoteHidden", "true"],
    ["vodscribe.resolveHintSeen", "true"],
  ]);
  /** @param {string} key */
  const load = (key) => kept.get(key) ?? null;
  const old = broughtOver(load);
  assert.deepEqual(old?.change, { ask_before_downloading: false, suggest_mov_for_resolve: false });
  assert.deepEqual(old?.keys.sort(), ["vodscribe.clipNoteHidden", "vodscribe.resolveHintSeen"]);
  // The settings saved with the change: each note stays stopped.
  const settings = { ...FRESH, ...old?.change };
  for (const ask of OLD) assert.equal(shows(settings, ask), false);
  // The VODscription offer's switch came later: nothing of 0.7.0's stops it.
  assert.equal(shows(settings, "ask_to_use_vodscription"), true);
  // Marked once saved: not brought over again, so a reset brings them back.
  for (const key of old?.keys ?? []) kept.set(key, BROUGHT_OVER);
  assert.equal(broughtOver(load), null);
  for (const ask of ASKS) assert.equal(shows(FRESH, ask), true);
});

test("only a note stopped is brought over", () => {
  assert.equal(
    broughtOver(() => null),
    null,
  );
  const one = broughtOver((key) => (key === "vodscribe.resolveHintSeen" ? "true" : null));
  assert.deepEqual(one, { change: { suggest_mov_for_resolve: false }, keys: ["vodscribe.resolveHintSeen"] });
});

test("every note that can be stopped is listed, the ones with an old key among them", () => {
  assert.deepEqual([...ASKS].sort(), Object.keys(FRESH).sort());
  for (const ask of OLD) assert.ok(ASKS.includes(ask), ask);
  assert.equal(OLD_KEYS.ask_to_use_vodscription, undefined);
});

test("the old keys are read only to bring them over", () => {
  for (const name of ["./permission.ts", "./resolveDrop.svelte.ts", "./appSettings.svelte.ts"])
    for (const key of Object.values(OLD_KEYS)) assert.ok(!source(name).includes(key), `${name} reads ${key}`);
});
