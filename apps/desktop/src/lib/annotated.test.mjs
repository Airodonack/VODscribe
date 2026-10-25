// What an imported annotations file shows (annotated.ts): node --test apps/desktop/src/lib/annotated.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CAST_COLORS,
  HIGHLIGHT_COLORS,
  activityColor,
  activityLabel,
  castDetail,
  castIndex,
  chapterColor,
  highlightColor,
  speakerColor,
  withAlpha,
} from "./annotated.ts";

const cast = [
  { name: "Marlowe", aliases: ["Mar", "M"], role: "streamer" },
  { name: "Ada", aliases: [], role: "friend" },
];

test("a speaker is found by name or alias, any case", () => {
  assert.equal(castIndex(cast, "Marlowe"), 0);
  assert.equal(castIndex(cast, "mar"), 0);
  assert.equal(castIndex(cast, " ADA "), 1);
  assert.equal(castIndex(cast, "Brigid"), -1);
  assert.equal(castIndex(cast, null), -1);
  assert.equal(castIndex(cast, ""), -1);
});

test("each cast member has their own colour, by their place in the cast", () => {
  assert.equal(speakerColor(cast, "M"), CAST_COLORS[0]);
  assert.equal(speakerColor(cast, "Ada"), CAST_COLORS[1]);
  assert.notEqual(CAST_COLORS[0], CAST_COLORS[1]);
  assert.equal(speakerColor(cast, "Brigid"), null, "someone not in the cast has none");
  // A cast longer than the palette starts it again.
  const many = Array.from({ length: CAST_COLORS.length + 1 }, (_, i) => ({
    name: `p${i}`,
    aliases: [],
    role: "chatter",
  }));
  assert.equal(speakerColor(many, `p${CAST_COLORS.length}`), CAST_COLORS[0]);
});

test("a highlight's kind picks its colour; another kind has hype's", () => {
  assert.equal(highlightColor("funny"), HIGHLIGHT_COLORS.funny);
  assert.equal(highlightColor("Drama"), HIGHLIGHT_COLORS.drama);
  assert.equal(highlightColor("weird"), HIGHLIGHT_COLORS.hype);
  assert.equal(highlightColor(null), HIGHLIGHT_COLORS.hype);
  assert.notEqual(highlightColor("funny"), highlightColor("lore"));
});

test("chapters take turns in the palette, neighbours apart", () => {
  assert.notEqual(chapterColor(0), chapterColor(1));
  assert.equal(chapterColor(0), chapterColor(6));
  assert.equal(chapterColor(-1), chapterColor(5));
});

test("activity reads as its kind, and its name when it has one", () => {
  assert.equal(activityLabel({ start: 0, end: 1, kind: "just chatting" }), "Just chatting");
  assert.equal(activityLabel({ start: 0, end: 1, kind: "game", name: "Chess" }), "Game: Chess");
  assert.equal(activityLabel({ start: 0, end: 1, kind: "game", name: " " }), "Game");
  assert.notEqual(activityColor("game"), activityColor("Just Chatting"));
  assert.equal(activityColor("something else"), activityColor("another"));
});

test("a cast member's line has their role and other names", () => {
  assert.equal(castDetail(cast[0]), "Streamer · also Mar, M");
  assert.equal(castDetail(cast[1]), "Friend");
  assert.equal(castDetail({ name: "Bo", aliases: ["bo", ""], role: "in-game character" }), "In-game character");
});

test("a colour at an alpha, without color-mix", () => {
  assert.equal(withAlpha("#2f7de1", 0.25), "rgb(47 125 225 / 0.25)");
});
