// Streamer cards (streamers.ts) and the fields annotations give them (annotations.ts): node --test apps/desktop/src/lib/streamers.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";
import { cardId, parseAnnotations } from "./annotations.ts";

// streamers.ts imports "./annotations" as the page's bundler finds it; here it
// is the .ts file it is.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(specifier, context, next) {
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith(".ts"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`),
);
const { CardCache, accountKey, cardIdOf, cardView, httpsUrl, readStreamer } = await import("./streamers.ts");

/** A card file of Quill's, with `fields`. @param {Record<string, unknown>} fields */
const card = (fields) => ({ format: "vodscribe-streamer", version: 1, id: "quill", ...fields });
/** An index of `accounts`. @param {Record<string, unknown>} accounts */
const index = (accounts) => ({ format: "vodscribe-streamers", version: 1, accounts });
/** An account, typed as one. @param {import("./annotations").Account["platform"]} platform @param {string} login */
const A = (platform, login) => ({ platform, login });
const twitch = A("twitch", "Quill_Live");

test("a card with only its id reads as one with nothing in it", () => {
  assert.deepEqual(readStreamer(card({}), "quill"), {
    id: "quill",
    channels: [],
    aliases: [],
    socials: [],
    contexts: {},
  });
});

test("a card of another format, version or person isn't one", () => {
  assert.equal(readStreamer(card({ format: "vodscribe-annotations" }), "quill"), null);
  assert.equal(readStreamer(card({ version: 2 }), "quill"), null);
  assert.equal(readStreamer(card({}), "other"), null);
  assert.equal(readStreamer(null, "quill"), null);
  assert.equal(readStreamer("quill", "quill"), null);
});

test("what can't be read in a card is left out, the rest kept", () => {
  const got = readStreamer(
    card({
      name: "  Quill  ",
      avatar: { platform: "vimeo", login: "q" },
      channels: [twitch, A("twitch", "quill_live"), A("kick", "../x"), A("youtube", "@quill"), "quill"],
      aliases: ["Q", " Q ", "", 3],
      socials: [
        { label: "Bluesky", url: "https://bsky.app/profile/quill.example" },
        { url: "http://quill.example" },
        { url: "javascript:alert(1)" },
        { url: "https://quill.example/" },
      ],
      contexts: {
        nopixel: {
          characters: [{ name: "Ivy Quill", wiki: "https://nopixel.fandom.com/wiki/Ivy_Quill" }, { name: "" }],
        },
        "Bad Key": { characters: [{ name: "x" }] },
        empty: { characters: [] },
        other: { title: "Other server", characters: [{ name: "Ivy", wiki: "http://wiki.example" }] },
      },
    }),
    "quill",
  );
  assert.deepEqual(got, {
    id: "quill",
    name: "Quill",
    channels: [twitch, A("youtube", "@quill")],
    aliases: ["Q"],
    socials: [{ label: "Bluesky", url: "https://bsky.app/profile/quill.example" }, { url: "https://quill.example/" }],
    contexts: {
      nopixel: {
        title: "NoPixel",
        characters: [{ name: "Ivy Quill", wiki: "https://nopixel.fandom.com/wiki/Ivy_Quill" }],
      },
      other: { title: "Other server", characters: [{ name: "Ivy" }] },
    },
  });
});

test("only https addresses are links", () => {
  assert.equal(httpsUrl("https://a.example/x"), "https://a.example/x");
  assert.equal(httpsUrl(" https://a.example "), "https://a.example/");
  for (const bad of ["http://a.example", "file:///etc/passwd", "javascript:x", "a.example", "", 3, null])
    assert.equal(httpsUrl(bad), null, String(bad));
});

test("an account's card id is in the index by its site and login in lower case", () => {
  assert.equal(accountKey(twitch), "twitch/quill_live");
  const idx = index({ "twitch/quill_live": "quill", "kick/bad": "Not An Id" });
  assert.equal(cardIdOf(idx, twitch), "quill");
  assert.equal(cardIdOf(idx, A("kick", "bad")), null, "an id that can't be one");
  assert.equal(cardIdOf(idx, A("kick", "quill")), null, "not there");
  assert.equal(cardIdOf(idx, A("twitch", "constructor")), null, "only the index's own keys");
  assert.equal(cardIdOf(null, twitch), null, "no index");
  assert.equal(cardIdOf({ ...idx, version: 2 }, twitch), null, "an index of another shape");
});

test("a card shows what its file has, else what the cast says, and no section the VOD doesn't name", () => {
  const member = { aliases: ["Quilly"], account: A("kick", "quill") };
  // Nothing from the file host: the cast's name, aliases and account.
  assert.deepEqual(cardView(member, "Ivy (Quill)", null, ["nopixel"]), {
    name: "Ivy (Quill)",
    avatar: A("kick", "quill"),
    aliases: ["Quilly"],
    channels: [A("kick", "quill")],
    socials: [],
    sections: [],
  });
  // No account either: a name, and nothing else.
  assert.deepEqual(cardView({ aliases: [] }, "Jules", null, []), {
    name: "Jules",
    avatar: null,
    aliases: [],
    channels: [],
    socials: [],
    sections: [],
  });
  const streamer = readStreamer(
    card({
      name: "Quill",
      channels: [A("youtube", "@quill"), twitch],
      aliases: ["Quill", "Q"],
      socials: [{ url: "https://www.quill.example/" }],
      contexts: { nopixel: { characters: [{ name: "Ivy Quill" }] } },
    }),
    "quill",
  );
  const full = cardView(member, "Ivy (Quill)", streamer, ["nopixel"]);
  assert.equal(full.name, "Quill");
  assert.deepEqual(full.aliases, ["Q"], "not the name it's headed by");
  assert.deepEqual(
    full.channels.map((c) => c.platform),
    ["youtube", "twitch", "kick"],
    "the card's, then the cast's",
  );
  assert.deepEqual(full.avatar, A("kick", "quill"), "the cast's account, not YouTube's");
  assert.deepEqual(full.socials, [{ label: "quill.example", href: "https://www.quill.example/" }]);
  assert.deepEqual(full.sections, [{ id: "nopixel", title: "NoPixel", characters: [{ name: "Ivy Quill" }] }]);
  assert.deepEqual(cardView(member, "Ivy (Quill)", streamer, []).sections, [], "the VOD names no context");
  assert.deepEqual(cardView(member, "Ivy (Quill)", streamer, ["gta"]).sections, [], "nor this one");
});

test("cards are asked for once a session; offline, asked again next time", async () => {
  /** @type {string[]} */
  const asked = [];
  let online = false;
  const cache = new CardCache(
    async (id) => {
      asked.push(id);
      if (!online) throw new Error("offline");
      return id === "quill" ? card({ name: "Quill" }) : null;
    },
    async () => {
      asked.push("index");
      if (!online) throw new Error("offline");
      return index({ "twitch/quill_live": "quill" });
    },
  );
  // Offline: none, and nothing kept.
  assert.equal(await cache.card("quill"), null);
  assert.equal(await cache.of({ account: twitch }), null);
  online = true;
  assert.equal((await cache.card("quill"))?.name, "Quill");
  assert.equal((await cache.of({ streamer: "quill" }))?.name, "Quill", "by the id the cast names");
  assert.equal((await cache.of({ account: twitch }))?.name, "Quill", "by the account, through the index");
  assert.equal((await cache.of({}, twitch))?.name, "Quill", "by the channel's account");
  assert.equal(await cache.card("nobody"), null, "not there");
  assert.equal(await cache.card("nobody"), null);
  assert.equal(await cache.of({ streamer: "../x" }), null, "an id that can't be one asks nothing");
  assert.deepEqual(asked, ["quill", "index", "quill", "index", "nobody"]);
});

test("a card asked for fresh is asked for past what's kept, so one published since shows", async () => {
  /** @type {string[]} */
  const asked = [];
  let published = false;
  const cache = new CardCache(
    async (id) => (asked.push(id), published ? card({ name: "Quill" }) : null),
    async () => (asked.push("index"), index(published ? { "twitch/quill_live": "quill" } : {})),
  );
  assert.equal(await cache.of({}, twitch), null, "not in the index yet");
  assert.equal(await cache.of({ streamer: "quill" }), null, "nor the card");
  published = true;
  assert.equal(await cache.of({}, twitch), null, "kept for the session");
  assert.equal((await cache.fresh({}, twitch))?.name, "Quill", "by the index asked again");
  assert.equal((await cache.of({ streamer: "quill" }))?.name, "Quill", "and kept so");
  assert.equal(await cache.fresh({}), null, "no account, no card: nothing to ask for");
  assert.deepEqual(asked, ["index", "quill", "index", "quill"]);
});

test("annotations name a cast member's card and the VOD's contexts, and drop what can't be one", () => {
  const file = {
    format: "vodscribe-annotations",
    version: 1,
    source: { platform: "twitch", id: "v1", channel: "Quill", title: "", duration: 60, recorded_at: null },
    produced: { at: "2026-11-05T00:00:00Z", build: "b" },
    transcript: { segments: [], corrected: 0 },
    chapters: [],
    highlights: [],
    cast: [
      { name: "Ivy", aliases: [], role: "in-game character", player: "Quill", streamer: " quill " },
      { name: "Bo", aliases: [], role: "friend", streamer: "Bo/../x" },
    ],
    activity: [],
    contexts: ["nopixel", "nopixel", "No Pixel", 7],
  };
  const read = parseAnnotations(file);
  assert.equal(read.cast[0].streamer, "quill");
  assert.equal("streamer" in read.cast[1], false);
  assert.deepEqual(read.contexts, ["nopixel"]);
  assert.deepEqual(parseAnnotations({ ...file, contexts: undefined }).contexts, [], "a file with none has none");
  assert.throws(
    () => parseAnnotations({ ...file, contexts: Array.from({ length: 101 }, (_, i) => `c${i}`) }),
    /more than 100 contexts/,
  );
  // As crates/core annotations.rs `card_id` reads one.
  for (const good of ["quill", "s-0001", "a_b", "7"]) assert.equal(cardId(good), good);
  for (const bad of ["", "-a", "_a", "Quill", "a/b", "..", "a.b", "a b", "é", "a".repeat(65), 3, null])
    assert.equal(cardId(bad), null, String(bad));
});
