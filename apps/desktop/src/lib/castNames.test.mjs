// What a cast member is called on screen (castNames.ts): node --test apps/desktop/src/lib/castNames.test.mjs
// Sources: castNames.ts's doc comments; CHANGELOG.md 0.7.0 ("Characters link to their streamer.": "the heading reads the
// character's name with the streamer in parentheses, \"Mr. C (Burn)\", and so does Cast above the transcript").
import assert from "node:assert/strict";
import { test } from "node:test";
import { displayName, sameName, speakerName } from "./castNames.ts";

/** castNames.ts `displayName` ("\"<name> (<player>)\" where a streamer plays them under another name"). */
test("a character played by a streamer reads as the character, then the streamer", () => {
  assert.equal(displayName({ name: "Gigi", player: "GEEGA" }), "Gigi (GEEGA)");
  assert.equal(displayName({ name: "Mr. C", player: "Burn" }), "Mr. C (Burn)");
  assert.equal(displayName({ name: "Marlowe", player: "Marlowe Vance" }), "Marlowe (Marlowe Vance)");
});

/** castNames.ts `displayName` (the player's name with its spaces at the ends dropped). */
test("the player's name is read with the spaces at its ends dropped", () => {
  assert.equal(displayName({ name: "Gigi", player: "  GEEGA " }), "Gigi (GEEGA)");
});

/** castNames.ts (header: "A streamer whose entry is their own name, or a cast without roster data, is their name alone"). */
test("a member with no player is their name alone", () => {
  assert.equal(displayName({ name: "Marlowe" }), "Marlowe");
  assert.equal(displayName({ name: "Marlowe", player: "" }), "Marlowe");
  assert.equal(displayName({ name: "Marlowe", player: "   " }), "Marlowe");
});

/** castNames.ts `displayName` ("where a streamer plays them under another name": the same name, case and spacing aside, is not another). */
test("a streamer under their own name is the name alone, case and spacing aside", () => {
  assert.equal(displayName({ name: "Geega", player: "GEEGA" }), "Geega");
  assert.equal(displayName({ name: "Geega", player: " geega " }), "Geega");
});

/** castNames.ts `speakerName` ("the cast member's display name"). */
test("a line's speaker in the cast is named as their cast member is", () => {
  assert.equal(speakerName({ name: "Gigi", player: "GEEGA" }, "gi"), "Gigi (GEEGA)");
  assert.equal(speakerName({ name: "Ada" }, "ada"), "Ada");
});

/** castNames.ts `speakerName` ("`said` as it is for someone not in the cast"). */
test("a speaker not in the cast is named as the line has them", () => {
  assert.equal(speakerName(undefined, "Brigid"), "Brigid");
  assert.equal(speakerName(undefined, " brigid "), " brigid ");
});

/** castNames.ts `sameName` ("Whether two names are one, case and spacing at the ends aside (as annotated.ts `castIndex` matches a speaker)"). */
test("two names are one when they differ only in case and the spaces at their ends", () => {
  assert.ok(sameName(" Ada", "ADA "));
  assert.ok(!sameName("Ada", "Adam"));
  assert.ok(!sameName("A da", "Ada"), "spaces inside count");
  assert.ok(sameName("", "  "));
});
