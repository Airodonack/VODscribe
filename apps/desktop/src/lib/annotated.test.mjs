// What an imported annotations file shows (annotated.ts): node --test apps/desktop/src/lib/annotated.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CAST_COLORS,
  activityColor,
  activityLabel,
  castDetail,
  castIndex,
  crowds,
  isVoice,
  speakerColor,
} from "./annotated.ts";
import {
  HIGHLIGHT_GLYPHS,
  UNKNOWN_GLYPH,
  highlightGlyph,
  oneEmoji,
  parseAnnotations,
  readAccount,
} from "./annotations.ts";
import { AVATAR_BASE, avatarUrl, channelLabel, channelUrl } from "./accounts.ts";

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
  // A text-to-speech voice's role is an abbreviation, said as one; and it is no one to link to.
  assert.equal(castDetail({ name: "TTS", aliases: [], role: "tts" }), "TTS");
  assert.equal(castDetail({ name: "Voice", aliases: ["Robot"], role: " TTS " }), "TTS · also Robot");
  assert.ok(isVoice({ role: "tts" }) && isVoice({ role: "TTS" }));
  assert.ok(!isVoice({ role: "streamer" }) && !isVoice(undefined));
  // The streamer who plays them is named in the heading already.
  assert.equal(
    castDetail({ name: "Gigi", aliases: ["geega", "G"], role: "streamer", player: "GEEGA" }),
    "Streamer · also G",
  );
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
  // Each group names the ones it stands for, the first among them.
  assert.deepEqual(
    crowds([30, 0, 10], (x) => x, 14).map((g) => g.members),
    [[0, 10], [30]],
  );
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

test("a cast member's account leads to their channel and their picture", () => {
  assert.equal(channelUrl({ platform: "twitch", login: "marlowe" }), "https://www.twitch.tv/marlowe");
  assert.equal(channelUrl({ platform: "kick", login: "ada_plays" }), "https://kick.com/ada_plays");
  // A handle has its own page; anything else is a channel id.
  assert.equal(channelUrl({ platform: "youtube", login: "@ada.makes" }), "https://www.youtube.com/@ada.makes");
  assert.equal(channelUrl({ platform: "youtube", login: "UCabc-123" }), "https://www.youtube.com/channel/UCabc-123");
  assert.equal(channelLabel({ platform: "twitch", login: "marlowe" }), "twitch.tv/marlowe");
  assert.equal(channelLabel({ platform: "kick", login: "ada" }), "kick.com/ada");
  assert.equal(AVATAR_BASE, "https://img.vodscribe.com");
  assert.equal(
    avatarUrl(AVATAR_BASE, { platform: "twitch", login: "marlowe" }),
    "https://img.vodscribe.com/avatar/twitch/marlowe",
  );
  assert.equal(
    avatarUrl("http://127.0.0.1:9/", { platform: "youtube", login: "@ada" }),
    "http://127.0.0.1:9/avatar/youtube/@ada",
  );
  // Escaped all the same, should a login that can't be one reach it.
  assert.equal(avatarUrl("http://h", { platform: "kick", login: "a/../b" }), "http://h/avatar/kick/a%2F..%2Fb");
});

test("a cast member's player and account are read when they can be, left out when they can't", () => {
  assert.deepEqual(readAccount({ platform: " Twitch", login: " marlowe " }), { platform: "twitch", login: "marlowe" });
  assert.deepEqual(readAccount({ platform: "youtube", login: "@ada.makes" }), {
    platform: "youtube",
    login: "@ada.makes",
  });
  for (const odd of [
    null,
    "marlowe",
    { platform: "vimeo", login: "bo" },
    { platform: "kick", login: "../cy" },
    { platform: "kick", login: "@di" },
    { platform: "twitch", login: "" },
    { platform: "twitch" },
    { platform: 1, login: "x" },
  ])
    assert.equal(readAccount(odd), null, JSON.stringify(odd));
  const file = {
    format: "vodscribe-annotations",
    version: 1,
    source: { platform: "twitch", id: "v1", channel: "", title: "", duration: 60, recorded_at: null },
    produced: { at: "", build: "" },
    transcript: { segments: [], corrected: 0 },
    chapters: [],
    highlights: [],
    cast: [
      {
        name: "Marlowe",
        aliases: [],
        role: "in-game character",
        player: " Marlowe Plays ",
        account: { platform: "twitch", login: "marlowe_plays" },
      },
      { name: "Bo", aliases: [], role: "friend", player: 3, account: { platform: "vimeo", login: "bo" } },
      { name: "Cy", aliases: [], role: "friend" },
    ],
    activity: [],
  };
  assert.deepEqual(parseAnnotations(file).cast, [
    {
      name: "Marlowe",
      aliases: [],
      role: "in-game character",
      player: "Marlowe Plays",
      account: { platform: "twitch", login: "marlowe_plays" },
    },
    { name: "Bo", aliases: [], role: "friend" },
    { name: "Cy", aliases: [], role: "friend" },
  ]);
});

test("a file that leaves out what the app's reader fills in is read as the app reads it", () => {
  // crates/core annotations.rs: a segment's words, a highlight's why and cast, a cast member's aliases.
  const read = parseAnnotations({
    format: "vodscribe-annotations",
    version: 1,
    source: { platform: "twitch", id: "v1", duration: 60 },
    transcript: { segments: [{ id: 1, start: 0, end: 2, text: "hello there", speaker: "Mar" }] },
    chapters: [],
    highlights: [{ start: 1, end: 5, title: "A moment", kind: "funny" }],
    cast: [{ name: "Marlowe", role: "streamer" }],
    activity: [],
  });
  assert.deepEqual(read.transcript.segments[0].words, []);
  assert.equal(read.highlights[0].why, "");
  assert.deepEqual(read.highlights[0].cast, []);
  assert.deepEqual(read.cast[0].aliases, []);
  // What the site's page does with the cast.
  assert.equal(castIndex(read.cast, "Marlowe"), 0);
  assert.equal(castIndex(read.cast, "Brigid"), -1);
  assert.equal(castDetail(read.cast[0]), "Streamer");
});
