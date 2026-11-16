// What an imported annotations file shows (annotated.ts): node --test apps/desktop/src/lib/annotated.test.mjs
// Sources: annotated.ts's doc comments; CHANGELOG.md 0.7.0 ("Cast lists who's speaking.", "Text-to-speech voices read
// as TTS.", "Highlights show on the timeline.", "Chapters show above the timeline."), 0.7.1 ("Streamer pictures show by
// channels."), 0.7.2 ("The channel over the player links to its page on every site.", Fixes "Every Twitch channel
// shows its picture."). The reader's own rules (annotations.ts) are tested in annotations.test.mjs.
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";
import { oneEmoji, parseAnnotations } from "./annotations.ts";

// annotated.ts imports "./accounts" as the page's bundler finds it; here it is the
// .ts file it is.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(specifier, context, next) {
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith(".ts"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`),
);
const {
  ACTIVITY_COLORS,
  CAST_COLORS,
  activityColor,
  activityLabel,
  castDetail,
  castIndex,
  channelAccount,
  channelMember,
  channelPage,
  channelPerson,
  crowds,
  isVoice,
  speakerColor,
} = await import("./annotated.ts");

/** @typedef {import("./api").CastMember} CastMember */
/** @param {string} login */
const twitch = (login) => ({ platform: /** @type {const} */ ("twitch"), login });

/** @type {CastMember[]} */
const cast = [
  { name: "Marlowe", aliases: ["Mar", "M"], role: "streamer" },
  { name: "Ada", aliases: [], role: "friend" },
];

// The cast: who a speaker is, and their colour.

/** annotated.ts `castIndex` ("by their name or one of their aliases, any case"). */
test("a speaker is found in the cast by name or alias, any case", () => {
  assert.equal(castIndex(cast, "Marlowe"), 0);
  assert.equal(castIndex(cast, "mar"), 0);
  assert.equal(castIndex(cast, "M"), 0);
  assert.equal(castIndex(cast, " ADA "), 1);
});

/** annotated.ts `castIndex` ("-1 if none"). */
test("a speaker who is no one in the cast, or is named by nothing, has no place in it", () => {
  assert.equal(castIndex(cast, "Brigid"), -1);
  assert.equal(castIndex(cast, null), -1);
  assert.equal(castIndex(cast, undefined), -1);
  assert.equal(castIndex(cast, ""), -1);
  assert.equal(castIndex(cast, "   "), -1);
});

/** CHANGELOG 0.7.0 "Cast lists who's speaking." ("Transcript lines name their speaker in that person's colour"); `speakerColor` ("by their place in the cast"). */
test("each cast member has their own colour, by their place in the cast", () => {
  assert.equal(speakerColor(cast, "M"), CAST_COLORS[0], "by an alias");
  assert.equal(speakerColor(cast, "Ada"), CAST_COLORS[1]);
  assert.equal(new Set(CAST_COLORS).size, CAST_COLORS.length, "no two palette colours alike");
});

/** annotated.ts `speakerColor` ("null for someone not in it"). */
test("someone not in the cast has no colour", () => {
  assert.equal(speakerColor(cast, "Brigid"), null);
  assert.equal(speakerColor(cast, null), null);
});

/** annotated.ts `speakerColor` ("by their place in the cast"), `CAST_COLORS` (a palette of fixed length). */
test("a cast longer than the palette starts the palette again", () => {
  /** @type {CastMember[]} */
  const many = Array.from({ length: CAST_COLORS.length * 2 + 1 }, (_, i) => ({
    name: `p${i}`,
    aliases: [],
    role: "chatter",
  }));
  assert.equal(speakerColor(many, `p${CAST_COLORS.length - 1}`), CAST_COLORS.at(-1));
  assert.equal(speakerColor(many, `p${CAST_COLORS.length}`), CAST_COLORS[0]);
  assert.equal(speakerColor(many, `p${CAST_COLORS.length * 2}`), CAST_COLORS[0]);
});

// What the cast list says of a member.

/** CHANGELOG 0.7.0 "Cast lists who's speaking." ("Cast above the transcript lists everyone with their role and other names"). */
test("a cast member's line has their role and other names", () => {
  assert.equal(castDetail(cast[0]), "Streamer · also Mar, M");
  assert.equal(castDetail(cast[1]), "Friend");
});

/** annotated.ts `castDetail` ("A role as the cast list says it: \"Streamer\", \"In-game character\""). */
test("a role is said with its first letter a capital", () => {
  assert.equal(castDetail({ name: "Bo", aliases: [], role: "in-game character" }), "In-game character");
});

/** annotated.ts `castDetail` ("An alias that is their name, or their player's ... isn't repeated"). */
test("an alias that is the member's name or their player's isn't repeated, nor is an empty one", () => {
  assert.equal(castDetail({ name: "Bo", aliases: ["bo", "", "  "], role: "friend" }), "Friend");
  assert.equal(
    castDetail({ name: "Gigi", aliases: ["geega", "G"], role: "streamer", player: "GEEGA" }),
    "Streamer · also G",
  );
});

/** CHANGELOG 0.7.0 "Text-to-speech voices read as TTS."; `castDetail` ("a text-to-speech voice's \"TTS\""). */
test("a text-to-speech voice's role is said as TTS", () => {
  assert.equal(castDetail({ name: "TTS", aliases: [], role: "tts" }), "TTS");
  assert.equal(castDetail({ name: "Voice", aliases: ["Robot"], role: " TTS " }), "TTS · also Robot");
});

/** annotated.ts `isVoice` ("a text-to-speech voice (role \"tts\"): no one's channel, so never a link"). */
test("a cast member whose role is tts, any case, is a voice; no one else is", () => {
  assert.ok(isVoice({ role: "tts" }) && isVoice({ role: "TTS" }) && isVoice({ role: " Tts " }));
  assert.ok(!isVoice({ role: "streamer" }));
  assert.ok(!isVoice(undefined));
});

// The activity band and the highlights on the timeline.

/** CHANGELOG 0.7.0 "Chapters show above the timeline." ("a thin band under them shows when it's a game, just chatting or an event"). */
test("each kind of activity has a colour of its own", () => {
  const kinds = ["game", "just chatting", "event", "break"];
  assert.deepEqual(Object.keys(ACTIVITY_COLORS), kinds);
  assert.equal(new Set(kinds.map(activityColor)).size, kinds.length);
});

/** annotated.ts `ACTIVITY_COLORS` ("by kind; another kind is gray"). */
test("an activity's colour is by its kind, any case; another kind is gray", () => {
  assert.equal(activityColor("Just Chatting"), activityColor("just chatting"));
  assert.equal(activityColor("GAME"), ACTIVITY_COLORS.game);
  assert.equal(activityColor("something else"), "#8c8c86");
  assert.equal(activityColor("another"), activityColor("something else"));
  assert.equal(activityColor("toString"), "#8c8c86", "nothing but the table's own kinds");
});

/** annotated.ts `activityLabel` ("\"Game: Chess\", \"Just chatting\""). */
test("an activity reads as its kind, and its name when it has one", () => {
  assert.equal(activityLabel({ start: 0, end: 1, kind: "just chatting" }), "Just chatting");
  assert.equal(activityLabel({ start: 0, end: 1, kind: "game", name: "Chess" }), "Game: Chess");
  assert.equal(activityLabel({ start: 0, end: 1, kind: "game", name: " Chess " }), "Game: Chess");
  assert.equal(activityLabel({ start: 0, end: 1, kind: "game", name: " " }), "Game");
});

/** CHANGELOG 0.7.0 "Highlights show on the timeline." ("several close together show as one with how many"); `crowds`. */
test("highlights closer than a glyph's width on the timeline are one glyph with their count", () => {
  /** @param {number[]} xs */
  const at = (xs) => crowds(xs, (x) => x, 14).map((g) => [g.first, g.count]);
  assert.deepEqual(at([100, 0, 13, 20, 40]), [
    [0, 2],
    [20, 1],
    [40, 1],
    [100, 1],
  ]);
  assert.deepEqual(at([5, 5, 5]), [[5, 3]]);
  assert.deepEqual(at([]), []);
});

/** annotated.ts `crowds` ("one less than `within` from the first of the group before joins it"). */
test("a glyph's crowd is measured from its first, so a row of close ones doesn't chain into one", () => {
  /** @param {number[]} xs @param {number} within */
  const at = (xs, within) => crowds(xs, (x) => x, within).map((g) => [g.first, g.count]);
  assert.deepEqual(at([0, 10, 20, 30], 14), [
    [0, 2],
    [20, 2],
  ]);
  assert.deepEqual(
    at([0, 14], 14),
    [
      [0, 1],
      [14, 1],
    ],
    "exactly `within` away is another glyph",
  );
});

/** annotated.ts `crowds` ("`members` which (the first among them)"; "Zoomed in, they come apart"). */
test("a crowd names the highlights it stands for, the first among them, and comes apart zoomed in", () => {
  assert.deepEqual(
    crowds([30, 0, 10], (x) => x, 14).map((g) => g.members),
    [[0, 10], [30]],
  );
  assert.equal(crowds([0, 10], (x) => x, 14).length, 1);
  assert.equal(crowds([0, 10], (x) => x * 10, 14).length, 2, "drawn farther apart: two");
});

// Whose channel a VOD is on.

/** @type {CastMember[]} */
const channels = [
  { name: "Ada", aliases: [], role: "friend", account: twitch("ada_plays") },
  { name: "Mr. C", aliases: [], role: "in-game character", player: "Burn", account: twitch("burn") },
  { name: "Burn", aliases: [], role: "streamer", account: twitch("burn") },
  { name: "Jules", aliases: [], role: "friend" },
  { name: "Voice", aliases: [], role: "tts", account: twitch("voice") },
];

/** annotated.ts `channelMember` ("the one with an account whose name, player or login is `channel`", "a streamer first"). */
test("the channel's cast member is found by name, player or login, any case, a streamer first", () => {
  assert.equal(channelMember(channels, " burn ")?.name, "Burn");
  assert.equal(channelMember(channels, "ADA_PLAYS")?.name, "Ada", "by login");
  assert.equal(channelMember(channels.slice(0, 2), "Burn")?.name, "Mr. C", "by player");
});

/** annotated.ts `channelMember` ("or whose login is the channel's `login` as its site gave it (any case)"). */
test("the channel's cast member is found by the login its site gave, whatever the channel's name", () => {
  assert.equal(channelMember(channels, "마를로", "BURN")?.name, "Burn");
  assert.equal(channelMember(channels, null, "ada_plays")?.name, "Ada");
});

/** annotated.ts `channelMember` ("Undefined when none has an account"; a voice is no one's channel). */
test("no one is the channel's who has no account, is a voice, or isn't named", () => {
  assert.equal(channelMember(channels, "Jules"), undefined, "no account");
  assert.equal(channelMember(channels, "Voice"), undefined, "a voice");
  assert.equal(channelMember(channels, null), undefined);
  assert.equal(channelMember(channels, " ", " "), undefined);
  assert.equal(channelMember(channels, "Marlowe"), undefined, "not in the cast");
});

/** @param {string} platform @param {string | null} channel */
const vod = (platform, channel, url = "") => ({ platform, channel, url });

/** annotated.ts `channelAccount` ("on Twitch the login the site gave (`channel_login`), else ... the channel's name in lower case"). */
test("a Twitch VOD's channel is an account by the login its site gave, else its name in lower case", () => {
  assert.deepEqual(channelAccount(vod("twitch", " Marlowe ")), twitch("marlowe"));
  const korean = { ...vod("twitch", "마를로"), channel_login: "marlowe" };
  assert.deepEqual(channelAccount(korean), twitch("marlowe"));
  // The login the site gave wins over one guessed from the name.
  assert.deepEqual(
    channelAccount({ ...korean, channel: "Marlowe Plays", channel_login: "marlowe_tv" }),
    twitch("marlowe_tv"),
  );
});

/** CHANGELOG 0.7.2 Fixes "Every Twitch channel shows its picture."; `channelAccount` ("when that name can be a login"). */
test("a Twitch channel whose name can't be a login, and gave none, has no account", () => {
  assert.equal(channelAccount(vod("twitch", "마를로")), undefined);
  assert.equal(channelAccount(vod("twitch", "Marlowe Plays")), undefined, "a space");
  assert.equal(channelAccount(vod("twitch", "a".repeat(26))), undefined, "too long");
  assert.equal(channelAccount(vod("twitch", null)), undefined);
});

/** annotated.ts `channelAccount` ("on Kick it's in the VOD's address (kick.com/<login>/videos/…)"). */
test("a Kick VOD's channel is the login in its address", () => {
  assert.deepEqual(channelAccount(vod("kick", "Ada Plays", "https://kick.com/Ada_Plays/videos/0b7f")), {
    platform: "kick",
    login: "ada_plays",
  });
  assert.deepEqual(channelAccount(vod("kick", null, "https://www.kick.com/ada-plays/videos/0b7f")), {
    platform: "kick",
    login: "ada-plays",
  });
});

/** annotated.ts `channelAccount` ("None for a file, or when nothing says"). */
test("a Kick address without the channel, or a file, has no account", () => {
  assert.equal(channelAccount(vod("kick", "Ada", "https://kick.com/video/0b7f")), undefined);
  assert.equal(channelAccount(vod("kick", "Ada", "http://kick.com/ada/videos/0b7f")), undefined, "not https");
  assert.equal(channelAccount(vod("local", null, "file:///a.mp4")), undefined);
  assert.equal(channelAccount(vod("dailymotion", "Quill", "https://www.dailymotion.com/video/x9")), undefined);
});

const ytUrl = "https://www.youtube.com/watch?v=noa29qsTc4g";
/** @param {Partial<import("./api").Vod>} fields */
const yt = (fields) => ({ platform: "youtube", channel: "Quill", url: ytUrl, ...fields });
const ytId = "UCabcdefghijklmnopqrstuv";

/** annotated.ts `channelAccount` ("on YouTube the handle the site gave (\"@name\") or the channel's id"). */
test("a YouTube VOD's channel is the handle or id its site gave", () => {
  assert.deepEqual(channelAccount(yt({ channel_login: "@quill" })), { platform: "youtube", login: "@quill" });
  assert.deepEqual(channelAccount(yt({ channel_login: ytId })), { platform: "youtube", login: ytId });
  assert.deepEqual(channelAccount(yt({ channel_login: "@마를로" })), { platform: "youtube", login: "@마를로" });
});

/** annotated.ts `channelAccount` ("else either as its channel's page (`channel_url`) has it"). */
test("a YouTube VOD with no handle kept has the one its channel's page has", () => {
  assert.deepEqual(channelAccount(yt({ channel_url: "https://www.youtube.com/@quill" })), {
    platform: "youtube",
    login: "@quill",
  });
  assert.deepEqual(channelAccount(yt({ channel_url: `https://www.youtube.com/channel/${ytId}/` })), {
    platform: "youtube",
    login: ytId,
  });
});

/** annotated.ts `channelAccount` ("never a guess from YouTube's name"). */
test("a YouTube VOD whose site gave nothing usable has no account, never one guessed from its name", () => {
  for (const fields of [
    {},
    { channel_login: null, channel_url: null },
    { channel_login: "quill" },
    { channel_url: "https://www.youtube.com.example/@quill" },
    { channel_url: "https://evil.example/@quill" },
    { channel_url: "http://www.youtube.com/@quill" },
    { channel_url: "https://user@www.youtube.com/@quill" },
    { channel_url: "https://www.youtube.com:8443/@quill" },
    { channel_url: "https://www.youtube.com/watch?v=noa29qsTc4g" },
    { channel_url: "https://www.youtube.com/channel/UC1/videos" },
    { channel_url: "not a link" },
  ])
    assert.equal(channelAccount(yt(fields)), undefined, JSON.stringify(fields));
});

/** annotated.ts `channelPage` ("on YouTube its account's (`channelAccount`)"); CHANGELOG 0.7.2 "The channel over the player links to its page on every site." */
test("a YouTube VOD's channel links to its account's page", () => {
  assert.deepEqual(channelPage(yt({ channel_login: "@quill" })), {
    label: "youtube.com/@quill",
    href: "https://www.youtube.com/@quill",
  });
  assert.deepEqual(channelPage(yt({ channel_url: `https://www.youtube.com/channel/${ytId}` })), {
    label: `youtube.com/channel/${ytId}`,
    href: `https://www.youtube.com/channel/${ytId}`,
  });
  assert.deepEqual(channelPage(yt({ channel_login: "@마를로" })), {
    label: "youtube.com/@%EB%A7%88%EB%A5%BC%EB%A1%9C",
    href: "https://www.youtube.com/@%EB%A7%88%EB%A5%BC%EB%A1%9C",
  });
  assert.equal(channelPage(yt({})), undefined, "nothing known");
});

/** annotated.ts `channelPage` ("on another site the channel's page as the site gave it (`channel_url`), only when that is https on the VOD's own site (its host or one under it)"). */
test("another site's VOD links to its channel's page, on that site only", () => {
  const own = "https://www.dailymotion.com/video/x9";
  /** @param {Partial<import("./api").Vod>} fields */
  const dm = (fields) => ({ platform: "dailymotion", channel: "Quill", url: own, ...fields });
  assert.deepEqual(channelPage(dm({ channel_url: "https://www.dailymotion.com/quill" })), {
    label: "dailymotion.com/quill",
    href: "https://www.dailymotion.com/quill",
  });
  assert.deepEqual(channelPage(dm({ channel_url: "https://dailymotion.com/quill/" })), {
    label: "dailymotion.com/quill",
    href: "https://dailymotion.com/quill/",
  });
  assert.deepEqual(
    channelPage(dm({ channel_url: "https://quill.dailymotion.com/" }))?.href,
    "https://quill.dailymotion.com/",
    "a host under it",
  );
  for (const fields of [
    {},
    { channel_login: "x2t8mt3" },
    { channel_url: "https://www.dailymotion.com.example/quill" },
    { channel_url: "https://evil.example/www.dailymotion.com/quill" },
    { channel_url: "http://www.dailymotion.com/quill" },
    { channel_url: "javascript:alert(1)" },
    { channel_url: "https://www.dailymotion.com:8443/quill" },
    { channel_url: "https://www.dailymotion.com/quill", url: "file:///a.mp4" },
  ])
    assert.equal(channelPage(dm(fields)), undefined, JSON.stringify(fields));
});

/** annotated.ts `channelPage` ("None on Twitch and Kick, whose card goes by the account as before, nor for a file"). */
test("a Twitch or Kick VOD, or a file, has no channel page of its own", () => {
  const elsewhere = { channel: "Marlowe", channel_url: "https://www.twitch.tv/marlowe" };
  assert.equal(channelPage({ platform: "twitch", url: "https://www.twitch.tv/videos/1", ...elsewhere }), undefined);
  assert.equal(channelPage({ platform: "kick", url: "https://kick.com/m/videos/1", ...elsewhere }), undefined);
  assert.equal(channelPage({ platform: "local", url: "file:///a.mp4", ...elsewhere }), undefined);
});

/** @type {CastMember[]} */
const people = [
  { name: "Marlowe", aliases: [], role: "streamer", account: twitch("marlowe") },
  { name: "마를로", aliases: [], role: "friend" },
  { name: "Ada", aliases: [], role: "friend" },
  { name: "Voice", aliases: [], role: "tts" },
];
const twitchVod = { platform: "twitch", url: "https://www.twitch.tv/videos/1" };

/** annotated.ts `channelPerson` ("the cast member the channel is by an account (`channelMember`, by the channel's name or the login its site gave)"). */
test("the channel's person is the cast member with its login, not one who shares its name", () => {
  assert.equal(channelPerson({ ...twitchVod, channel: "마를로", channel_login: "marlowe" }, people)?.name, "Marlowe");
  assert.equal(channelPerson({ ...twitchVod, channel: "marlowe" }, people)?.name, "Marlowe");
});

/** annotated.ts `channelPerson` ("else one named as it (by name or player) with no account, given the channel's own where the VOD has one"). */
test("a cast member named as the channel with no account is given the channel's own", () => {
  assert.deepEqual(channelPerson({ ...twitchVod, channel: "Ada" }, people), {
    name: "Ada",
    aliases: [],
    role: "friend",
    account: twitch("ada"),
  });
  assert.deepEqual(
    channelPerson({ platform: "youtube", url: ytUrl, channel: "Ada" }, people),
    { name: "Ada", aliases: [], role: "friend" },
    "none known: none given",
  );
});

/** annotated.ts `channelPerson` ("else the channel itself, with that account"; "Undefined for a VOD with no channel"). */
test("with no one in the cast, the channel is a streamer of its own, with its account where it has one", () => {
  assert.deepEqual(channelPerson({ ...twitchVod, channel: "Pell" }, people), {
    name: "Pell",
    aliases: [],
    role: "streamer",
    account: twitch("pell"),
  });
  assert.equal(channelPerson({ ...twitchVod, channel: null }, people), undefined);
  assert.equal(channelPerson({ ...twitchVod, channel: " " }, people), undefined);
});

/** annotated.ts `isVoice` ("no one's channel"), used by `channelPerson`. */
test("a voice in the cast isn't the channel's person", () => {
  assert.deepEqual(channelPerson({ platform: "youtube", url: ytUrl, channel: "Voice" }, people), {
    name: "Voice",
    aliases: [],
    role: "streamer",
  });
});

// Reading, in the page's own words (annotations.ts), not covered by annotations.test.mjs.

/** CHANGELOG 0.7.0 "Chapters show above the timeline." ("each with its emoji when the file gives one"); annotations.ts `oneEmoji`. */
test("a chapter's icon is one emoji, or none", () => {
  for (const icon of ["🎮", "💬", " 🍳 ", "👩‍🍳", "🏳️‍🌈", "❤️", "🇫🇷"]) assert.equal(oneEmoji(icon), icon.trim(), icon);
  for (const icon of ["", " ", "🎮🎮", "a", "🎮 game", "Game", "7", "?", ":)", "日", null, 3, ["🎮"]])
    assert.equal(oneEmoji(icon), null, String(icon));
});

/** annotations.ts `parseAnnotations` (a chapter's icon that isn't one emoji is left out, its chapter kept). */
test("a file with a chapter icon that isn't one emoji keeps the chapter without it", () => {
  const read = parseAnnotations({
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
  });
  assert.deepEqual(read.chapters, [
    { start: 0, title: "Hello", icon: "💬" },
    { start: 10, title: "Play" },
    { start: 20, title: "Bye" },
  ]);
});

/** crates/core annotations.rs (a segment's words, a highlight's why and cast, a cast member's aliases are filled in when the file leaves them out). */
test("a file that leaves out what the app's reader fills in is read as the app reads it", () => {
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
  assert.equal(castIndex(read.cast, "Marlowe"), 0, "and the views take the cast as read");
  assert.equal(castDetail(read.cast[0]), "Streamer");
});

/** annotated.ts `castIndex` ("-1 if none"): a blank speaker is no one, though a member has a blank name or alias. */
test("a blank speaker is no one, even where a cast member's name or alias is blank", () => {
  assert.equal(castIndex([{ name: "", aliases: [], role: "friend" }], ""), -1);
  assert.equal(castIndex([{ name: "Ada", aliases: [""], role: "friend" }], " "), -1);
});

/** annotated.ts `CAST_COLORS` ("each reads on the light and the dark theme alike"): each is a colour. */
test("each cast colour, and each activity colour, is a six-digit hex colour", () => {
  for (const color of [...CAST_COLORS, ...Object.values(ACTIVITY_COLORS)]) assert.match(color, /^#[0-9a-f]{6}$/, color);
});

/** annotated.ts `castDetail` (an alias is their name, or their player's, whatever the spaces around it or the member's own). */
test("an alias is not repeated whatever the spaces around it or around the name", () => {
  assert.equal(castDetail({ name: " Bo ", aliases: [" bo "], role: "friend" }), "Friend");
  assert.equal(
    castDetail({ name: "Gigi", aliases: [" geega ", "G"], role: "streamer", player: " GEEGA " }),
    "Streamer · also G",
  );
});

/** annotated.ts `channelMember` ("a streamer first"): a role's spaces and case don't matter. */
test("a streamer is the channel's cast member first, whatever the spaces or case of their role", () => {
  /** @type {CastMember[]} */
  const two = [
    { name: "Burn", aliases: [], role: "friend", account: twitch("burn") },
    { name: "Burn", aliases: [], role: " Streamer ", account: twitch("burn") },
  ];
  assert.equal(channelMember(two, "burn"), two[1]);
});

/** annotated.ts `channelAccount` ("on Kick it's in the VOD's address (kick.com/<login>/videos/…)"): the address itself, not a link inside another. */
test("a Kick login is found at the start of the VOD's address only", () => {
  assert.equal(
    channelAccount(vod("kick", "Ada", "https://evil.example/?next=https://kick.com/ada/videos/0b7f")),
    undefined,
  );
});

/** annotated.ts `channelAccount` (the handle or id the site gave is a YouTube channel's, not another site's). */
test("a handle or id on a VOD of another site is no YouTube account", () => {
  assert.equal(channelAccount({ ...vod("dailymotion", "Quill"), channel_login: "@quill" }), undefined);
  assert.equal(channelAccount({ ...vod("local", "Quill"), channel_login: ytId }), undefined);
});

/** annotated.ts `channelAccount` ("on YouTube the handle the site gave (\"@name\") or the channel's id, else either as its channel's page has it"): what isn't one is no account. */
test("a YouTube handle or id with anything around it, or a page that isn't the channel's own, is no account", () => {
  for (const fields of [
    { channel_login: "not@quill" },
    { channel_login: "@quill!" },
    { channel_login: "@" },
    { channel_login: `x${ytId}` },
    { channel_login: `${ytId}x` },
    { channel_url: "https://www.youtube.com/@quill/videos" },
    { channel_url: "https://www.youtube.com/@quill!" },
    { channel_url: `https://www.youtube.com/channel/${ytId}/videos` },
    { channel_url: `https://www.youtube.com/user/${ytId}` },
    { channel_url: `https://www.youtube.com/c/${ytId}` },
  ])
    assert.equal(channelAccount(yt(fields)), undefined, JSON.stringify(fields));
});

/** annotated.ts `channelPage` ("its host or one under it"; the label is the address without its scheme). */
test("a host under the VOD's site is its own whatever its name ends in, and a label drops the scheme at its start only", () => {
  const dm = (/** @type {Partial<import("./api").Vod>} */ fields) => ({
    platform: "dailymotion",
    channel: "Quill",
    url: "https://www.dailymotion.com/video/x9",
    ...fields,
  });
  assert.equal(
    channelPage(dm({ channel_url: "https://farm.dailymotion.com/q" }))?.href,
    "https://farm.dailymotion.com/q",
  );
  assert.equal(
    channelPage(dm({ channel_url: "https://www.dailymotion.com/go?to=https://a.example/" }))?.label,
    "dailymotion.com/go?to=https://a.example",
  );
});

/** annotated.ts `channelPage` ("None on Twitch and Kick, whose card goes by the account as before, nor for a file"): even where the channel's page is on the VOD's own site. */
test("a Twitch or Kick VOD, or a file, has no channel page though its channel's page is on its own site", () => {
  const own = (/** @type {string} */ platform, /** @type {string} */ url, /** @type {string} */ page) =>
    channelPage({ platform, url, channel: "M", channel_url: page });
  assert.equal(own("kick", "https://kick.com/m/videos/1", "https://kick.com/m"), undefined);
  assert.equal(own("twitch", "https://www.twitch.tv/videos/1", "https://www.twitch.tv/m"), undefined);
  assert.equal(own("local", "https://a.example/a.mp4", "https://a.example/m"), undefined);
});

/** annotated.ts `channelPerson` ("one named as it (by name or player) with no account"). */
test("a cast member whose player is the channel, with no account, is given the channel's own", () => {
  /** @type {CastMember[]} */
  const played = [{ name: "Mr. C", aliases: [], role: "in-game character", player: "Burn" }];
  assert.deepEqual(channelPerson({ ...twitchVod, channel: "burn" }, played), {
    name: "Mr. C",
    aliases: [],
    role: "in-game character",
    player: "Burn",
    account: twitch("burn"),
  });
});

/** annotated.ts `castDetail` (an alias that is blank isn't listed), also where the member has a player. */
test("a blank alias isn't listed, though the member has a player", () => {
  assert.equal(
    castDetail({ name: "Gigi", aliases: ["  ", "G"], role: "streamer", player: "GEEGA" }),
    "Streamer · also G",
  );
});
