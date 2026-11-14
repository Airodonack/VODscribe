// The page's reader of a VODscription file (annotations.ts, streamers.ts), which states the same rules as
// crates/core annotations.rs and annotations_filter.rs: node --test apps/desktop/src/lib/annotations.test.mjs
// The cases are the ones crates/core/src/annotations/tests/format.rs gives the app's reader.
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";
import {
  ANNOTATIONS_FORMAT,
  HIGHLIGHT_GLYPHS,
  LIMITS,
  UNKNOWN_GLYPH,
  cardId,
  hasLongText,
  highlightGlyph,
  parseAnnotations,
  readAccount,
  readPov,
} from "./annotations.ts";

// streamers.ts imports "./annotations" as the page's bundler finds it; here it is the .ts file it is.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(specifier, context, next) {
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith(".ts"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`),
);
const { INDEX_FORMAT, INDEX_VERSION, STREAMER_FORMAT, STREAMER_VERSION, cardIdOf, readStreamer } =
  await import("./streamers.ts");

/** @param {number} n @param {unknown} item */
const many = (n, item) => Array.from({ length: n }, () => item);

/**
 * A small file as crates/core annotations/tests.rs's `file_with` has it, with `path` (its fields) set to `value`.
 * @param {(string | number)[]} path
 * @param {unknown} value
 */
function fileWith(path, value) {
  const file = {
    format: ANNOTATIONS_FORMAT,
    version: 1,
    source: { platform: "twitch", id: "v123", channel: "Marlowe", title: "A stream", duration: 60 },
    produced: { at: "2026-10-02T00:00:00Z", build: "b1" },
    transcript: { corrected: 0, segments: [{ start: 1, end: 3, text: "hello there", words: [] }] },
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

/** The message the page gives for `value`. @param {unknown} value */
function refused(value) {
  try {
    parseAnnotations(value);
  } catch (e) {
    return /** @type {Error} */ (e).message;
  }
  assert.fail("the file was read");
}

/**
 * annotations.rs `parse` ("the error's message is for the user (as `parseAnnotations` in the page words it)"):
 * a file of another format or version is refused in the words the app uses.
 */
test("the page refuses a file of another format or version in the apps words", () => {
  const format = { format: "other", version: 1 };
  assert.equal(refused(format), `This isn't a VODscribe annotations file: its format is "other".`);
  assert.equal(refused({ format: 7, version: 1 }), `This isn't a VODscribe annotations file: its format is 7.`);
  // The format is checked before the version.
  assert.equal(
    refused({ format: "other", version: 2 }),
    `This isn't a VODscribe annotations file: its format is "other".`,
  );
  const version = (/** @type {unknown} */ v) => refused({ format: ANNOTATIONS_FORMAT, version: v });
  assert.equal(version(2), "This isn't a VODscribe annotations file: version 2 isn't 1.");
  assert.equal(version("1"), `This isn't a VODscribe annotations file: version "1" isn't 1.`);
  assert.equal(version(0), "This isn't a VODscribe annotations file: version 0 isn't 1.");
});

/**
 * Decided for 0.7.4: a VODscription file with no format or version is said plainly by app and page alike ("it
 * has no format", "it has no version"), as annotations.rs `from_value` says it.
 */
test("the page words a file with no format or no version as the app does", () => {
  assert.equal(refused({ version: 1 }), "This isn't a VODscribe annotations file: it has no format.");
  assert.equal(refused({ format: ANNOTATIONS_FORMAT }), "This isn't a VODscribe annotations file: it has no version.");
});

/**
 * annotations.rs `within_limits` ("The page's `withinLimits` (annotations.ts) is the same rule"): the limits are the app's,
 * each list and text one past its limit is refused in the app's words, at its limit it is read.
 */
test("the page refuses what the app refuses by its limits", () => {
  assert.deepEqual(
    { ...LIMITS },
    {
      segments: 200_000,
      words: 2_000,
      chapters: 2_000,
      highlights: 5_000,
      cast: 1_000,
      names: 100,
      activity: 10_000,
      povs: 1_000,
      contexts: 100,
      text: 10_000,
      messages: 2_000_000,
      parts: 1_000,
    },
  );
  /** @type {[(string | number)[], number, unknown, string][]} */
  const lists = [
    [["transcript", "segments"], LIMITS.segments, { start: 1, end: 2, text: "hi" }, "it has more than 200,000 lines"],
    [
      ["transcript", "segments", 0, "words"],
      LIMITS.words,
      { text: "hi", start: 1, end: 1.5 },
      "a line has more than 2,000 words",
    ],
    [["chapters"], LIMITS.chapters, { start: 0, title: "A" }, "it has more than 2,000 chapters"],
    [
      ["highlights"],
      LIMITS.highlights,
      { start: 0, end: 1, title: "A", kind: "funny" },
      "it has more than 5,000 highlights",
    ],
    [["highlights", 0, "cast"], LIMITS.names, "A", "a highlight names more than 100 people"],
    [["cast"], LIMITS.cast, { name: "A", role: "friend" }, "it has more than 1,000 people in its cast"],
    [["cast", 0, "aliases"], LIMITS.names, "A", "a cast member has more than 100 other names"],
    [
      ["activity"],
      LIMITS.activity,
      { start: 0, end: 1, kind: "game" },
      "it has more than 10,000 stretches of activity",
    ],
    [["povs"], LIMITS.povs, null, "it has more than 1,000 other points of view"],
    [["contexts"], LIMITS.contexts, "a", "it has more than 100 contexts"],
  ];
  for (const [path, max, item, said] of lists) {
    assert.equal(
      refused(fileWith(path, many(max + 1, item))),
      `This annotations file is too big: ${said}.`,
      path.join("."),
    );
    parseAnnotations(fileWith(path, many(max, item)));
  }
  const said = "This annotations file is too big: a text in it is longer than 10,000 characters.";
  // A text counts characters, not UTF-16 units: an emoji is one; a key is a text too.
  for (const path of [
    ["chapters", 0, "title"],
    ["produced", "build"],
  ]) {
    assert.equal(refused(fileWith(path, "😂".repeat(LIMITS.text + 1))), said, path.join("."));
    parseAnnotations(fileWith(path, "😂".repeat(LIMITS.text)));
  }
  assert.equal(refused(fileWith(["produced"], { ["é".repeat(LIMITS.text + 1)]: 1 })), said, "a key");
  parseAnnotations(fileWith(["produced"], { ["é".repeat(LIMITS.text)]: 1 }));
  assert.equal(hasLongText(["a", { b: "😂".repeat(LIMITS.text) }]), false);
  assert.equal(hasLongText(["a", { b: "😂".repeat(LIMITS.text + 1) }]), true);
});

/** annotations.rs `card_id` ("The page's `cardId` (streamers.ts) is the same rule"). */
test("the pages card id is the apps", () => {
  for (const good of ["marlowe", "s-0001", "a_b", "7", "0-", "z_", "a".repeat(64)]) assert.equal(cardId(good), good);
  assert.equal(cardId("  marlowe "), "marlowe");
  assert.equal(cardId("\tmarlowe\n"), "marlowe");
  for (const bad of ["", " ", "-a", "_a", "Marlowe", "mARlowe", "a/b", "a\\b", "..", "a.b", "a b", "é", "a%2fb"])
    assert.equal(cardId(bad), null, JSON.stringify(bad));
  assert.equal(cardId("a".repeat(65)), null);
  for (const other of [3, null, undefined, {}, ["a"]]) assert.equal(cardId(other), null, String(other));
  // In a file, the same rule names a cast member's card and the VOD's contexts; what isn't an id is left out.
  const cast = [
    { name: "A", role: "friend", player: " Marlowe Plays ", streamer: " marlowe " },
    { name: "B", role: "friend", player: " ", streamer: "../marlowe" },
  ];
  const read = parseAnnotations(fileWith(["cast"], cast)).cast;
  assert.deepEqual([read[0].streamer, read[0].player, read[0].aliases], ["marlowe", "Marlowe Plays", []]);
  assert.deepEqual([read[1].streamer, read[1].player], [undefined, undefined]);
  const contexts = ["nopixel", " gta-rp ", "nopixel", "NoPixel", "../x", "", 7, null];
  assert.deepEqual(parseAnnotations(fileWith(["contexts"], contexts)).contexts, ["nopixel", "gta-rp"]);
  assert.deepEqual(parseAnnotations(fileWith(["contexts"], "nopixel")).contexts, []);
});

/** annotations.rs `Account::read` ("The page's `readAccount` (annotations.ts) is the same rule"). */
test("the pages account is the apps", () => {
  const read = (/** @type {string} */ platform, /** @type {string} */ login) => readAccount({ platform, login });
  const account = (/** @type {string} */ platform, /** @type {string} */ login) => ({ platform, login });
  assert.deepEqual(read("twitch", "marlowe_live"), account("twitch", "marlowe_live"));
  assert.deepEqual(read("kick", "a.b-c_d"), account("kick", "a.b-c_d"));
  assert.deepEqual(read(" Twitch ", " Marlowe "), account("twitch", "Marlowe"));
  assert.deepEqual(read("YOUTUBE", "@marlowe.makes"), account("youtube", "@marlowe.makes"));
  assert.deepEqual(read("youtube", "UC_x5XG1OV2P6uZZ5FSM9Ttw"), account("youtube", "UC_x5XG1OV2P6uZZ5FSM9Ttw"));
  for (const [platform, login, why] of [
    ["vimeo", "marlowe", "a site not listed"],
    ["", "marlowe", "no site"],
    ["twitch", "", "no login"],
    ["twitch", "  ", "a blank login"],
    ["twitch", ".", "a login of only dots is a path's"],
    ["kick", "..", "a login of only dots is a path's"],
    ["youtube", "@...", "a handle of only dots is a path's"],
    ["youtube", "@", "an @ and nothing"],
    ["twitch", "@marlowe", "an @ is YouTube's only"],
    ["twitch", "../marlowe", "a path"],
    ["twitch", "mar/lowe", "a slash"],
    ["twitch", "mar lowe", "a space inside"],
    ["twitch", "marlowé", "a letter outside ASCII"],
    ["youtube", "@@marlowe", "two @"],
  ])
    assert.equal(read(platform, login), null, why);
  for (const value of [
    "marlowe",
    null,
    { platform: "twitch" },
    { login: "marlowe" },
    { platform: 1, login: "x" },
    { platform: "twitch", login: 1 },
  ])
    assert.equal(readAccount(value), null, JSON.stringify(value));
  // In a file, an account that can't be read is left out and the cast member stays.
  const cast = [
    { name: "A", aliases: [], role: "friend", account: { platform: "vimeo", login: "a" } },
    { name: "B", aliases: [], role: "friend", account: { platform: "kick", login: "b" } },
  ];
  const read2 = parseAnnotations(fileWith(["cast"], cast)).cast;
  assert.deepEqual([read2[0].account, read2[1].account?.login], [undefined, "b"]);
});

/** annotations.rs `Pov::read` ("The page's `readPov` (annotations.ts) is the same rule"). */
test("the pages point of view is the apps", () => {
  const start = "2026-10-01T17:30:00Z";
  const pov = (
    /** @type {string} */ player,
    /** @type {string} */ login,
    /** @type {string} */ platform,
    /** @type {string} */ id,
    /** @type {string} */ at,
    /** @type {unknown} */ duration,
  ) => ({
    player,
    account: { platform: "twitch", login },
    video: { platform, id },
    start: at,
    duration,
  });
  assert.deepEqual(
    readPov(pov(" Burn ", " burn ", "Twitch", " v2906773722 ", " 2026-10-01T17:30:00.5+02:00 ", 7200.5)),
    {
      player: "Burn",
      account: { platform: "twitch", login: "burn" },
      video: { platform: "twitch", id: "v2906773722" },
      start: "2026-10-01T17:30:00.5+02:00",
      duration: 7200.5,
    },
  );
  for (const [why, value] of /** @type {[string, unknown][]} */ ([
    ["a blank player", pov(" ", "burn", "twitch", "v1", start, 60)],
    ["an account that can't be read", pov("Burn", "../burn", "twitch", "v1", start, 60)],
    ["a video's site not listed", pov("Burn", "burn", "vimeo", "1", start, 60)],
    ["an id that is a path", pov("Burn", "burn", "twitch", "../v1", start, 60)],
    ["an id with a dot", pov("Burn", "burn", "twitch", "v.1", start, 60)],
    ["no id", pov("Burn", "burn", "twitch", "", start, 60)],
    ["a start that is no time", pov("Burn", "burn", "twitch", "v1", "yesterday", 60)],
    ["a start with no zone", pov("Burn", "burn", "twitch", "v1", "2026-10-01T17:30:00", 60)],
    ["a length of none", pov("Burn", "burn", "twitch", "v1", start, 0)],
    ["a negative length", pov("Burn", "burn", "twitch", "v1", start, -1)],
    ["a length that is no number", pov("Burn", "burn", "twitch", "v1", start, "60")],
    ["a text", "Burn"],
    ["nothing", null],
  ]))
    assert.equal(readPov(value), null, why);
  for (const key of ["player", "account", "video", "start", "duration"]) {
    const value = /** @type {Record<string, unknown>} */ (pov("Burn", "burn", "twitch", "v1", start, 60));
    delete value[key];
    assert.equal(readPov(value), null, `without ${key}`);
  }
  // In a file, the ones that can't be read are left out and the file is read.
  const povs = [pov("Burn", "burn", "twitch", "v1", start, 60), pov("", "burn", "twitch", "v1", start, 60), "Burn"];
  assert.equal(parseAnnotations(fileWith(["povs"], povs)).povs?.length, 1);
});

/**
 * annotations.ts `HIGHLIGHT_GLYPHS` ("a file may carry a kind not listed here: it shows as `UNKNOWN_GLYPH` and keeps its
 * word"); 0.7.0: "Highlights show on the timeline." (a star for a kind the app doesn't know).
 */
test("a highlight of a kind the app does not know shows a star and keeps its word", () => {
  for (const [kind, glyph] of Object.entries(HIGHLIGHT_GLYPHS)) {
    assert.equal(highlightGlyph(kind), glyph, kind);
    assert.equal(highlightGlyph(` ${kind.toUpperCase()} `), glyph, `${kind} in capitals`);
  }
  assert.equal(UNKNOWN_GLYPH, "⭐");
  for (const kind of ["speedrun", "", " ", null, undefined, "constructor", "toString", "__proto__", "hasOwnProperty"])
    assert.equal(highlightGlyph(kind), UNKNOWN_GLYPH, String(kind));
  const read = parseAnnotations(
    fileWith(["highlights"], [{ start: 5, end: 9, title: "A", kind: "Speedrun", why: "", cast: [] }]),
  );
  assert.equal(read.highlights[0].kind, "Speedrun", "the word is the file's");
  assert.equal(highlightGlyph(read.highlights[0].kind), UNKNOWN_GLYPH);
});

/** streamers.ts (`STREAMER_FORMAT`, `INDEX_FORMAT`, versions 1): a card or an index of another shape is not read. */
test("a streamer card of another format or version is not read", () => {
  assert.deepEqual(
    [STREAMER_FORMAT, STREAMER_VERSION, INDEX_FORMAT, INDEX_VERSION],
    ["vodscribe-streamer", 1, "vodscribe-streamers", 1],
  );
  const card = (/** @type {Record<string, unknown>} */ fields) => ({
    format: "vodscribe-streamer",
    version: 1,
    id: "quill",
    ...fields,
  });
  assert.equal(readStreamer(card({}), "quill")?.id, "quill");
  assert.equal(readStreamer(card({ format: "vodscribe-annotations" }), "quill"), null);
  assert.equal(readStreamer(card({ format: undefined }), "quill"), null);
  assert.equal(readStreamer(card({ version: 2 }), "quill"), null);
  assert.equal(readStreamer(card({ version: "1" }), "quill"), null);
  assert.equal(readStreamer(card({ version: undefined }), "quill"), null);
  assert.equal(readStreamer(card({ id: "ivy" }), "quill"), null, "another person's");
  for (const other of [null, "quill", 3, undefined]) assert.equal(readStreamer(other, "quill"), null);
  const twitch = { platform: "twitch", login: "Quill_Live" };
  const index = (/** @type {Record<string, unknown>} */ fields) => ({
    format: "vodscribe-streamers",
    version: 1,
    accounts: { "twitch/quill_live": "quill" },
    ...fields,
  });
  assert.equal(cardIdOf(index({}), /** @type {any} */ (twitch)), "quill");
  assert.equal(cardIdOf(index({ format: "vodscribe-streamer" }), /** @type {any} */ (twitch)), null);
  assert.equal(cardIdOf(index({ version: 2 }), /** @type {any} */ (twitch)), null);
  assert.equal(cardIdOf(index({ version: undefined }), /** @type {any} */ (twitch)), null);
  assert.equal(cardIdOf(index({ accounts: null }), /** @type {any} */ (twitch)), null);
});
