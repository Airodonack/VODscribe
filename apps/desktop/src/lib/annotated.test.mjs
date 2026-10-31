// What an imported annotations file shows (annotated.ts): node --test apps/desktop/src/lib/annotated.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CAST_COLORS,
  activityColor,
  activityLabel,
  castDetail,
  castIndex,
  channelMember,
  crowds,
  isVoice,
  speakerColor,
} from "./annotated.ts";
import {
  HIGHLIGHT_GLYPHS,
  UNKNOWN_GLYPH,
  highlightGlyph,
  oneEmoji,
  LIMITS,
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
    // Only dots: a path's own or its parent.
    { platform: "twitch", login: "." },
    { platform: "kick", login: ".." },
    { platform: "youtube", login: "@..." },
  ])
    assert.equal(readAccount(odd), null, JSON.stringify(odd));
  assert.deepEqual(readAccount({ platform: "twitch", login: "i.o" }), { platform: "twitch", login: "i.o" });
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

/**
 * A small file as crates/core annotations.rs's tests have it, with `path` (its fields) set to `value`.
 * @param {(string | number)[]} path
 * @param {unknown} value
 */
function fileWith(path, value) {
  const file = {
    format: "vodscribe-annotations",
    version: 1,
    source: { platform: "twitch", id: "v123", channel: "Marlowe", title: "A stream", duration: 60 },
    produced: { at: "2026-10-02T00:00:00Z", build: "b1" },
    transcript: { segments: [{ start: 1, end: 3, text: "hello there", words: [] }] },
    chapters: [{ start: 0, title: "Start" }],
    highlights: [{ start: 5, end: 9, title: "Big laugh", kind: "funny", cast: [] }],
    cast: [{ name: "Marlowe", aliases: [], role: "streamer" }],
    activity: [],
  };
  /** @type {any} */
  let at = file;
  for (const key of path.slice(0, -1)) at = at[key];
  at[path[path.length - 1]] = value;
  return file;
}

/** @param {number} n @param {unknown} item */
const many = (n, item) => Array.from({ length: n }, () => item);

test("a file with more than the app takes is refused with a message, as the app refuses it", () => {
  // The same cases, and words, as crates/core annotations.rs's.
  const segment = { start: 1, end: 2, text: "hi" };
  /** @type {[(string | number)[], unknown, string][]} */
  const cases = [
    [["transcript", "segments"], many(LIMITS.segments + 1, segment), "it has more than 200,000 lines"],
    [
      ["transcript", "segments", 0, "words"],
      many(LIMITS.words + 1, { text: "hi", start: 1, end: 1.5 }),
      "a line has more than 2,000 words",
    ],
    [["chapters"], many(LIMITS.chapters + 1, { start: 0, title: "A" }), "it has more than 2,000 chapters"],
    [
      ["highlights"],
      many(LIMITS.highlights + 1, { start: 0, end: 1, title: "A", kind: "funny" }),
      "it has more than 5,000 highlights",
    ],
    [["highlights", 0, "cast"], many(LIMITS.names + 1, "A"), "a highlight names more than 100 people"],
    [["cast"], many(LIMITS.cast + 1, { name: "A", role: "friend" }), "it has more than 1,000 people in its cast"],
    [["cast", 0, "aliases"], many(LIMITS.names + 1, "A"), "a cast member has more than 100 other names"],
    [
      ["activity"],
      many(LIMITS.activity + 1, { start: 0, end: 1, kind: "game" }),
      "it has more than 10,000 stretches of activity",
    ],
    [["povs"], many(LIMITS.povs + 1, null), "it has more than 1,000 other points of view"],
    [["chapters", 0, "title"], "x".repeat(LIMITS.text + 1), "a text in it is longer than 10,000 characters"],
    [["produced", "build"], "é".repeat(LIMITS.text + 1), "a text in it is longer than 10,000 characters"],
  ];
  for (const [path, value, said] of cases)
    assert.throws(
      () => parseAnnotations(fileWith(path, value)),
      { message: `This annotations file is too big: ${said}.` },
      path.join("."),
    );
  // At the limits, it's read: an emoji is one character, as the app counts, though two units here.
  parseAnnotations(fileWith(["chapters", 0, "title"], "😂".repeat(LIMITS.text)));
  parseAnnotations(fileWith(["cast", 0, "aliases"], many(LIMITS.names, "A")));
  parseAnnotations(fileWith(["chapters"], many(LIMITS.chapters, { start: 0, title: "A" })));
});

test("a file of a million highlights is refused before the page draws any", () => {
  const file = fileWith(["highlights"], many(1_000_000, { start: 1, end: 2, title: "Big laugh", kind: "funny" }));
  assert.throws(() => parseAnnotations(file), {
    message: "This annotations file is too big: it has more than 5,000 highlights.",
  });
});

test("the channel's streamer is found by name, player or login, any case, a streamer first", () => {
  /** @param {string} login */
  const twitch = (login) => ({ platform: /** @type {const} */ ("twitch"), login });
  /** @type {import("./api").CastMember[]} */
  const cast = [
    { name: "Ada", aliases: [], role: "friend", account: twitch("ada_plays") },
    { name: "Mr. C", aliases: [], role: "in-game character", player: "Burn", account: twitch("burn") },
    { name: "Burn", aliases: [], role: "streamer", account: twitch("burn") },
    { name: "Jules", aliases: [], role: "friend" },
    { name: "Voice", aliases: [], role: "tts", account: twitch("voice") },
  ];
  assert.equal(channelMember(cast, " burn ")?.name, "Burn");
  assert.equal(channelMember(cast, "ADA_PLAYS")?.name, "Ada");
  assert.equal(channelMember(cast.slice(0, 2), "Burn")?.name, "Mr. C");
  // No account, a voice, no channel: no one.
  assert.equal(channelMember(cast, "Jules"), undefined);
  assert.equal(channelMember(cast, "Voice"), undefined);
  assert.equal(channelMember(cast, null), undefined);
  assert.equal(channelMember(cast, "Marlowe"), undefined);
});
