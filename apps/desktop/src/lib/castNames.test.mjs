// What a cast member is called on screen (castNames.ts): node --test apps/desktop/src/lib/castNames.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { displayName, sameName, speakerName } from "./castNames.ts";

test("a character played by a streamer reads as the streamer, then the character", () => {
  assert.equal(displayName({ name: "Gigi", player: "GEEGA" }), "GEEGA (Gigi)");
  assert.equal(displayName({ name: "Marlowe", player: "Marlowe Vance" }), "Marlowe Vance (Marlowe)");
  // The player's name as the file has it, spaces at the ends dropped.
  assert.equal(displayName({ name: "Gigi", player: "  GEEGA " }), "GEEGA (Gigi)");
});

test("a streamer under their own name, or no player, is the name alone", () => {
  assert.equal(displayName({ name: "Marlowe" }), "Marlowe");
  assert.equal(displayName({ name: "Marlowe", player: "" }), "Marlowe");
  assert.equal(displayName({ name: "Marlowe", player: "   " }), "Marlowe");
  assert.equal(displayName({ name: "Geega", player: "GEEGA" }), "Geega", "case aside, the same name");
  assert.equal(displayName({ name: "Geega", player: " geega " }), "Geega", "spacing aside, the same name");
});

test("a speaker not in the cast is named as the line has them", () => {
  assert.equal(speakerName(undefined, "Brigid"), "Brigid");
  assert.equal(speakerName({ name: "Gigi", player: "GEEGA" }, "gi"), "GEEGA (Gigi)");
  assert.equal(speakerName({ name: "Ada" }, "ada"), "Ada");
});

test("names are compared as the cast index compares them", () => {
  assert.ok(sameName(" Ada", "ADA "));
  assert.ok(!sameName("Ada", "Adam"));
});
