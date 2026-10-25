// What an imported annotations file shows (annotated.ts): node --test apps/desktop/src/lib/annotated.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { CAST_COLORS, activityColor, activityLabel, castDetail, castIndex, crowds, speakerColor } from "./annotated.ts";
import { HIGHLIGHT_GLYPHS, UNKNOWN_GLYPH, highlightGlyph, oneEmoji, parseAnnotations } from "./annotations.ts";

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

test("a highlight's kind picks its emoji, any case; another kind has the star", () => {
  assert.equal(highlightGlyph("funny"), "😂");
  assert.equal(highlightGlyph(" Drama "), "🍿");
  assert.equal(highlightGlyph("visual"), "👀");
  assert.equal(highlightGlyph("other"), UNKNOWN_GLYPH);
  assert.equal(highlightGlyph("weird"), UNKNOWN_GLYPH);
  assert.equal(highlightGlyph("constructor"), UNKNOWN_GLYPH, "nothing but the table's own kinds");
  assert.equal(highlightGlyph(null), UNKNOWN_GLYPH);
  // Each kind its own emoji, none the star.
  const glyphs = Object.values(HIGHLIGHT_GLYPHS);
  assert.equal(glyphs.length, 15);
  assert.equal(new Set([...glyphs, UNKNOWN_GLYPH]).size, glyphs.length + 1);
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

test("highlights closer than a glyph's width on the timeline are one glyph with their count", () => {
  /** @param {number[]} xs */
  const at = (xs) => crowds(xs, (x) => x, 14).map((g) => [g.first, g.count]);
  assert.deepEqual(at([100, 0, 13, 20, 40]), [
    [0, 2],
    [20, 1],
    [40, 1],
    [100, 1],
  ]);
  // Measured from a group's first, so a row of close ones doesn't chain into one.
  assert.deepEqual(at([0, 10, 20, 30]), [
    [0, 2],
    [20, 2],
  ]);
  assert.deepEqual(at([5, 5, 5]), [[5, 3]]);
  assert.deepEqual(at([]), []);
});

test("a chapter's icon is one emoji, or none", () => {
  for (const icon of ["🎮", "💬", " 🍳 ", "👩‍🍳", "🏳️‍🌈", "❤️", "🇫🇷"]) assert.equal(oneEmoji(icon), icon.trim(), icon);
  for (const icon of ["", " ", "🎮🎮", "a", "🎮 game", "Game", "7", "?", ":)", "日", null, 3, ["🎮"]])
    assert.equal(oneEmoji(icon), null, String(icon));
  // A file with an icon that isn't one keeps its chapter, without the icon.
  const file = {
    format: "vodscribe-annotations",
    version: 1,
    source: { platform: "twitch", id: "v1", channel: "", title: "", duration: 60, recorded_at: null },
    produced: { at: "", build: "" },
    transcript: { segments: [], corrected: 0 },
    chapters: [
      { start: 0, title: "Hello", icon: "💬" },
      { start: 10, title: "Play", icon: "a game" },
      { start: 20, title: "Bye" },
    ],
    highlights: [],
    cast: [],
    activity: [],
  };
  assert.deepEqual(parseAnnotations(file).chapters, [
    { start: 0, title: "Hello", icon: "💬" },
    { start: 10, title: "Play" },
    { start: 20, title: "Bye" },
  ]);
});
