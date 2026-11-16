// Streamer cards (streamers.ts): node --test apps/desktop/src/lib/streamers.test.mjs
// Sources: streamers.ts's doc comments; CHANGELOG.md 0.7.2 ("Names open a streamer card.", "Refresh a VOD's details.").
// A card's format and version, and the card id's rule, are tested in annotations.test.mjs.
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

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
const { CONTEXT_TITLES, CardCache, accountKey, cardIdOf, cardView, httpsUrl, readStreamer } =
  await import("./streamers.ts");

/** A card file of Quill's, with `fields`. @param {Record<string, unknown>} fields */
const card = (fields) => ({ format: "vodscribe-streamer", version: 1, id: "quill", ...fields });
/** An index of `accounts`. @param {Record<string, unknown>} accounts */
const index = (accounts) => ({ format: "vodscribe-streamers", version: 1, accounts });
/** An account, typed as one. @param {import("./annotations").Account["platform"]} platform @param {string} login */
const A = (platform, login) => ({ platform, login });
const twitch = A("twitch", "Quill_Live");
/** A card as read, from `fields`. @param {Record<string, unknown>} fields */
const read = (fields) => readStreamer(card(fields), "quill");

// Reading a card.

/** streamers.ts `Streamer` ("Everything in a card but its id may be missing, and the card shows only what's there"). */
test("a card with only its id reads as one with nothing in it", () => {
  assert.deepEqual(read({}), { id: "quill", channels: [], aliases: [], socials: [], contexts: {} });
});

/** streamers.ts `readStreamer` ("What's in it that can't be read is left out"); `text` (trimmed, 200 characters at most). */
test("a card's name is read trimmed, and left out when it isn't text or is too long", () => {
  assert.equal(read({ name: "  Quill  " })?.name, "Quill");
  for (const bad of ["", "   ", 3, null, "q".repeat(201)])
    assert.equal(read({ name: bad })?.name, undefined, String(bad));
  assert.equal(read({ name: "q".repeat(200) })?.name, "q".repeat(200));
});

/** streamers.ts `StreamerFile` (`avatar`: "one of their accounts ... Never a picture's address"). */
test("a card's picture is one of their accounts, or none", () => {
  assert.deepEqual(read({ avatar: twitch })?.avatar, twitch);
  assert.equal(read({ avatar: { platform: "vimeo", login: "q" } })?.avatar, undefined);
  assert.equal(read({ avatar: "https://img.example/q.png" })?.avatar, undefined);
});

/** streamers.ts `readStreamer` ("a channel on a site not listed" is left out); `StreamerFile` (`channels`). */
test("a card's channels are the accounts that can be read, each once", () => {
  const got = read({
    channels: [
      twitch,
      A("twitch", "quill_live"),
      A("kick", "../x"),
      { platform: "vimeo", login: "q" },
      A("youtube", "@quill"),
      "quill",
    ],
  });
  assert.deepEqual(got?.channels, [twitch, A("youtube", "@quill")]);
});

/** streamers.ts `readStreamer` (aliases: text, once each). */
test("a card's other names are read once each, the blank and what isn't text left out", () => {
  assert.deepEqual(read({ aliases: ["Q", " Q ", "", 3, "Quilly"] })?.aliases, ["Q", "Quilly"]);
});

/** streamers.ts `readStreamer` ("a link that isn't `https`" is left out); `httpsUrl`. */
test("a card's other pages are links only when they are https, with their label when they have one", () => {
  const got = read({
    socials: [
      { label: "Bluesky", url: "https://bsky.app/profile/quill.example" },
      { url: "http://quill.example" },
      { url: "javascript:alert(1)" },
      { label: "No link" },
      null,
      { url: "https://quill.example/", label: " " },
    ],
  });
  assert.deepEqual(got?.socials, [
    { label: "Bluesky", url: "https://bsky.app/profile/quill.example" },
    { url: "https://quill.example/" },
  ]);
});

/** streamers.ts `readStreamer` ("a section with no characters" is left out); `ContextSection` ("its heading (else `CONTEXT_TITLES`', else its id)"). */
test("a card's contexts keep the sections with characters, headed by their title, else the known one, else their id", () => {
  assert.equal(CONTEXT_TITLES.nopixel, "NoPixel");
  const got = read({
    contexts: {
      nopixel: { characters: [{ name: "Ivy Quill" }] },
      other: { title: "Other server", characters: [{ name: "Ivy" }] },
      plain: { characters: [{ name: "Jo" }] },
      empty: { characters: [] },
      noneat: {},
      "Bad Key": { characters: [{ name: "x" }] },
      text: "nopixel",
    },
  });
  assert.deepEqual(got?.contexts, {
    nopixel: { title: "NoPixel", characters: [{ name: "Ivy Quill" }] },
    other: { title: "Other server", characters: [{ name: "Ivy" }] },
    plain: { title: "plain", characters: [{ name: "Jo" }] },
  });
});

/** streamers.ts `Character` ("its name, and its wiki page (`https`)"). */
test("a character keeps its name, and its wiki page only when it is https", () => {
  const got = read({
    contexts: {
      nopixel: {
        characters: [
          { name: "Ivy Quill", wiki: "https://nopixel.fandom.com/wiki/Ivy_Quill" },
          { name: "Ivy", wiki: "http://wiki.example" },
          { name: "" },
          { wiki: "https://wiki.example" },
        ],
      },
    },
  });
  assert.deepEqual(got?.contexts.nopixel.characters, [
    { name: "Ivy Quill", wiki: "https://nopixel.fandom.com/wiki/Ivy_Quill" },
    { name: "Ivy" },
  ]);
});

/** streamers.ts `MOST` ("The most a card brings of each, short of what would make it a page"). */
test("a card brings at most fifty of each list", () => {
  const got = read({
    channels: Array.from({ length: 80 }, (_, i) => A("twitch", `q${i}`)),
    aliases: Array.from({ length: 80 }, (_, i) => `a${i}`),
    socials: Array.from({ length: 80 }, (_, i) => ({ url: `https://q${i}.example/` })),
  });
  assert.deepEqual([got?.channels.length, got?.aliases.length, got?.socials.length], [50, 50, 50]);
});

/** streamers.ts `httpsUrl` ("`value` as an address a card may link to: `https`, and nothing that isn't one"). */
test("only https addresses are links", () => {
  assert.equal(httpsUrl("https://a.example/x"), "https://a.example/x");
  assert.equal(httpsUrl(" https://a.example "), "https://a.example/");
  for (const bad of ["http://a.example", "file:///etc/passwd", "javascript:x", "a.example", "", 3, null])
    assert.equal(httpsUrl(bad), null, String(bad));
});

// The index.

/** streamers.ts `accountKey`, `StreamerIndex` ("each account's card id, by \"<platform>/<login in lower case>\""). */
test("an account's key in the index is its site and login in lower case", () => {
  assert.equal(accountKey(twitch), "twitch/quill_live");
  assert.equal(accountKey(A("youtube", "@Quill")), "youtube/@quill");
});

/** streamers.ts `cardIdOf` ("The card id of `account` in `index` ..., or null: none there, or no index"). */
test("an account's card id is found in the index by its key, or is none", () => {
  const idx = index({ "twitch/quill_live": "quill", "kick/bad": "Not An Id" });
  assert.equal(cardIdOf(idx, twitch), "quill");
  assert.equal(cardIdOf(idx, A("twitch", "QUILL_LIVE")), "quill", "any case");
  assert.equal(cardIdOf(idx, A("kick", "bad")), null, "an id that can't be one");
  assert.equal(cardIdOf(idx, A("kick", "quill")), null, "not there");
  assert.equal(cardIdOf(idx, A("twitch", "constructor")), null, "only the index's own keys");
  assert.equal(cardIdOf(null, twitch), null, "no index");
  assert.equal(cardIdOf({ ...index({}), accounts: null }, twitch), null, "an index with no accounts");
});

// What a card shows.

const member = { aliases: ["Quilly"], account: A("kick", "quill") };
const streamer = read({
  name: "Quill",
  channels: [A("youtube", "@quill"), twitch],
  aliases: ["Quill", "Q"],
  socials: [{ url: "https://www.quill.example/" }],
  contexts: { nopixel: { characters: [{ name: "Ivy Quill" }] }, gta: { characters: [{ name: "Jo" }] } },
});

/** streamers.ts `cardView` ("where it has it, else what the VOD's cast says (`named`)"). */
test("with no card from the file host, a card shows what the cast says", () => {
  assert.deepEqual(cardView(member, "Ivy (Quill)", null, ["nopixel"]), {
    name: "Ivy (Quill)",
    avatar: A("kick", "quill"),
    aliases: ["Quilly"],
    channels: [A("kick", "quill")],
    socials: [],
    sections: [],
  });
});

/** streamers.ts `CardView` ("each list empty when there's nothing for it, and then not drawn at all"). */
test("a cast member with no account and no card has a name and nothing else", () => {
  assert.deepEqual(cardView({ aliases: [] }, "Jules", null, []), {
    name: "Jules",
    avatar: null,
    aliases: [],
    channels: [],
    socials: [],
    sections: [],
  });
});

/** streamers.ts `cardView` (the card's name where it has one; aliases: not the name it's headed by). */
test("a card is headed by its file's name, and doesn't list that name among its other names", () => {
  const full = cardView(member, "Ivy (Quill)", streamer, []);
  assert.equal(full.name, "Quill");
  assert.deepEqual(full.aliases, ["Q"]);
  assert.deepEqual(cardView(member, "Ivy", read({ aliases: ["Q"] }), []).aliases, ["Q"]);
  assert.deepEqual(cardView(member, "Ivy", read({}), []).aliases, ["Quilly"], "a card with none: the cast's");
  assert.deepEqual(
    cardView({ aliases: ["ivy", " "] }, "Ivy", null, []).aliases,
    [],
    "the cast's too, but for the name",
  );
});

/** streamers.ts `cardView` ("Their account in the cast is one of their channels either way"). */
test("a card's channels are the file's, then the cast's account unless it is among them", () => {
  assert.deepEqual(
    cardView(member, "Ivy", streamer, []).channels.map((c) => c.platform),
    ["youtube", "twitch", "kick"],
  );
  const same = cardView({ aliases: [], account: A("twitch", "QUILL_LIVE") }, "Ivy", streamer, []);
  assert.equal(same.channels.length, 2, "the same account in another case isn't added again");
});

/** streamers.ts `cardView` ("The image server has no YouTube pictures to show: another site's first"; `avatar`: the card's own first). */
test("a card's picture is the file's, else the cast's account, else a channel on a site with pictures", () => {
  assert.deepEqual(cardView(member, "Ivy", read({ avatar: A("twitch", "pic") }), []).avatar, A("twitch", "pic"));
  assert.deepEqual(cardView(member, "Ivy", streamer, []).avatar, A("kick", "quill"), "the cast's, not YouTube's");
  assert.deepEqual(cardView({ aliases: [] }, "Ivy", streamer, []).avatar, twitch, "a channel's, not YouTube's");
  const onlyYoutube = read({ channels: [A("youtube", "@quill")] });
  assert.deepEqual(
    cardView({ aliases: [] }, "Ivy", onlyYoutube, []).avatar,
    A("youtube", "@quill"),
    "YouTube's, when it's all there is",
  );
});

/** streamers.ts `socialLabel` ("the one given, else the address without its scheme and trailing slash"). */
test("a card's other pages are labelled as given, else by their address", () => {
  const got = cardView(
    member,
    "Ivy",
    read({ socials: [{ label: "Bluesky", url: "https://bsky.app/q" }, { url: "https://www.quill.example/" }] }),
    [],
  );
  assert.deepEqual(got.socials, [
    { label: "Bluesky", href: "https://bsky.app/q" },
    { label: "quill.example", href: "https://www.quill.example/" },
  ]);
});

/** streamers.ts `cardView` ("A section shows only for a context the VOD names, in the VOD's order"). */
test("a card shows a section only for a context the VOD names, in the VOD's order", () => {
  /** @param {string[]} contexts */
  const ids = (contexts) => cardView(member, "Ivy", streamer, contexts).sections.map((s) => s.id);
  assert.deepEqual(ids(["nopixel"]), ["nopixel"]);
  assert.deepEqual(ids(["gta", "nopixel"]), ["gta", "nopixel"]);
  assert.deepEqual(ids(["nopixel", "gta", "nopixel"]), ["nopixel", "gta"], "each once");
  assert.deepEqual(ids([]), [], "the VOD names none");
  assert.deepEqual(ids(["other"]), [], "nor this one");
  assert.deepEqual(cardView(member, "Ivy", streamer, ["nopixel"]).sections[0], {
    id: "nopixel",
    title: "NoPixel",
    characters: [{ name: "Ivy Quill" }],
  });
});

/** streamers.ts `cardView` (sections come from the card: none without one). */
test("without a card from the file host there are no sections, whatever the VOD names", () => {
  assert.deepEqual(cardView(member, "Ivy", null, ["nopixel"]).sections, []);
});

// Asking for cards.

/** @param {{ online: boolean, asked: string[], published?: boolean }} world */
function cacheIn(world) {
  return new CardCache(
    async (id) => {
      world.asked.push(id);
      if (!world.online) throw new Error("offline");
      return id === "quill" && world.published !== false ? card({ name: "Quill" }) : null;
    },
    async () => {
      world.asked.push("index");
      if (!world.online) throw new Error("offline");
      return index(world.published === false ? {} : { "twitch/quill_live": "quill" });
    },
  );
}

/** streamers.ts `CardCache` ("each asked for once a session"). */
test("a card and the index are asked for once a session", async () => {
  const world = { online: true, asked: /** @type {string[]} */ ([]) };
  const cache = cacheIn(world);
  assert.equal((await cache.card("quill"))?.name, "Quill");
  assert.equal((await cache.card("quill"))?.name, "Quill");
  assert.equal((await cache.of({ account: twitch }))?.name, "Quill");
  assert.equal((await cache.of({ account: twitch }))?.name, "Quill");
  assert.equal(await cache.card("nobody"), null, "one that isn't there");
  assert.equal(await cache.card("nobody"), null);
  assert.deepEqual(world.asked, ["quill", "index", "nobody"]);
});

/** streamers.ts `CardCache` ("A failure to ask (offline) is kept for nothing, so the next card opened asks again"). */
test("offline, a card and the index are none and asked again the next time", async () => {
  const world = { online: false, asked: /** @type {string[]} */ ([]) };
  const cache = cacheIn(world);
  assert.equal(await cache.card("quill"), null);
  assert.equal(await cache.of({ account: twitch }), null);
  world.online = true;
  assert.equal((await cache.card("quill"))?.name, "Quill");
  assert.equal((await cache.of({ account: twitch }))?.name, "Quill");
  assert.deepEqual(world.asked, ["quill", "index", "quill", "index"]);
});

/** streamers.ts `CardCache.of` ("the one the VOD's annotations name, else the one the index has for their account (`account`, the VOD's channel's when the cast gives none), else none"). */
test("a cast member's card is the one the VOD names, else the index's for their account or the channel's", async () => {
  const world = { online: true, asked: /** @type {string[]} */ ([]) };
  const cache = cacheIn(world);
  assert.equal((await cache.of({ streamer: "quill" }))?.name, "Quill", "by the id the cast names");
  assert.deepEqual(world.asked, ["quill"], "the index isn't asked when the cast names the card");
  assert.equal((await cache.of({ account: twitch }))?.name, "Quill", "by the account, through the index");
  assert.equal((await cache.of({}, twitch))?.name, "Quill", "by the channel's account");
  assert.equal(await cache.of({}), null, "no card, no account");
});

/** streamers.ts `CardCache.of` (a name that can't be a card id asks for nothing). */
test("an id that can't be a card's asks for nothing", async () => {
  const world = { online: true, asked: /** @type {string[]} */ ([]) };
  assert.equal(await cacheIn(world).of({ streamer: "../x" }), null);
  assert.deepEqual(world.asked, []);
});

/** streamers.ts `CardCache.fresh` ("`of`, past what's kept this session: the index asked for again, and the card it then names"); CHANGELOG 0.7.2 "Refresh a VOD's details." ("any VODscription or streamer card published since"). */
test("a card asked for fresh is asked for past what's kept, so one published since shows", async () => {
  const world = { online: true, asked: /** @type {string[]} */ ([]), published: false };
  const cache = cacheIn(world);
  assert.equal(await cache.of({}, twitch), null, "not in the index yet");
  assert.equal(await cache.of({ streamer: "quill" }), null, "nor the card");
  world.published = true;
  assert.equal(await cache.of({}, twitch), null, "kept for the session");
  assert.equal((await cache.fresh({}, twitch))?.name, "Quill", "by the index asked again");
  assert.equal((await cache.of({ streamer: "quill" }))?.name, "Quill", "and kept so");
  assert.deepEqual(world.asked, ["index", "quill", "index", "quill"]);
});

/** streamers.ts `CardCache.fresh` (no named card and no account: nothing to ask for). */
test("a card asked for fresh with no id and no account asks for nothing", async () => {
  const world = { online: true, asked: /** @type {string[]} */ ([]) };
  assert.equal(await cacheIn(world).fresh({}), null);
  assert.deepEqual(world.asked, []);
});

/** streamers.ts `CardCache.fresh` ("A failure keeps nothing, as ever"). */
test("a card asked for fresh while offline is none, and kept for nothing", async () => {
  const world = { online: false, asked: /** @type {string[]} */ ([]) };
  const cache = cacheIn(world);
  assert.equal(await cache.fresh({ streamer: "quill" }), null);
  world.online = true;
  assert.equal((await cache.of({ streamer: "quill" }))?.name, "Quill");
});

/** streamers.ts `MOST` ("The most a card brings of each"): contexts too. */
test("a card brings at most fifty contexts", () => {
  const contexts = Object.fromEntries(Array.from({ length: 80 }, (_, i) => [`c${i}`, { characters: [{ name: "x" }] }]));
  assert.equal(Object.keys(read({ contexts })?.contexts ?? {}).length, 50);
});

/** streamers.ts `readStreamer` ("What's in it that can't be read is left out"): contexts, sections and characters that are nothing. */
test("a card whose contexts, a section or a character is nothing is read without it", () => {
  assert.deepEqual(read({ contexts: null })?.contexts, {});
  assert.deepEqual(read({ contexts: ["nopixel"] })?.contexts, {});
  const got = read({ contexts: { nopixel: null, gta: "x", other: { characters: [null, 3, { name: "Jo" }] } } });
  assert.deepEqual(got?.contexts, { other: { title: "other", characters: [{ name: "Jo" }] } });
});

/** streamers.ts `socialLabel` ("the address without its scheme and trailing slash"). */
test("a page with no label is read out without its scheme at the start, www or not, and its last slash", () => {
  const got = cardView(
    member,
    "Ivy",
    read({ socials: [{ url: "https://bsky.app/q/r/" }, { url: "https://a.example/go?to=https://b.example/" }] }),
    [],
  );
  assert.deepEqual(
    got.socials.map((s) => s.label),
    ["bsky.app/q/r", "a.example/go?to=https://b.example"],
  );
});

/** streamers.ts `cardView` (aliases: "not the name it's headed by"): whatever the spaces around either. */
test("an alias that is the name it's headed by isn't repeated whatever the spaces around it", () => {
  assert.deepEqual(cardView({ aliases: [" ivy ", "Iv"] }, " Ivy ", null, []).aliases, ["Iv"]);
  assert.deepEqual(cardView(member, "Ivy", read({ name: "Quill", aliases: [" quill ", "Q"] }), []).aliases, ["Q"]);
});
