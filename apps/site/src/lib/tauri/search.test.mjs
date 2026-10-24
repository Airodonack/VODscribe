// The demo's search (search.ts) against the app's: its phrase search and
// highlighting against the Rust's cases (crates/core/tests/phrase-search.json),
// and the library search against search.rs's tests, below.
// node --test apps/site/src/lib/tauri/search.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { MOST_WITHIN, TOO_WIDE } from "./query.ts";
import { authorSuggestions, findPhrase, highlight, lastIsPrefix, searchLibrary, terms } from "./search.ts";

/** @type {{ about: string, words: string[], query: string, ranges: [number, number][], marked: string }[]} */
const cases = JSON.parse(
  readFileSync(new URL("../../../../../crates/core/tests/phrase-search.json", import.meta.url), "utf8"),
);

for (const c of cases) {
  test(c.about, () => {
    const words = c.words.map((text) => ({ text, terms: terms(text) }));
    const q = terms(c.query);
    assert.deepEqual(findPhrase(words, q, lastIsPrefix(q)), c.ranges);
    // How the result shows it: matched runs in [brackets].
    const marked = highlight(words, c.ranges)
      .map((p) => (p.hit ? `[${p.text}]` : p.text))
      .join("");
    assert.equal(marked, c.marked);
  });
}

// The search over a library (`searchLibrary`, `authorSuggestions`) against
// the app's own tests of it, case for case: crates/core search.rs `mod tests`
// (all but the ones on what the app does only to be fast). A test added or
// changed there is added or changed here.

/** A library to search: VODs newest first, as the app's library lists them. */
/** @typedef {import("./search.ts").LibraryVod} LibraryVod */
/** @typedef {import("./search.ts").Results} Results */
function library() {
  /** @type {LibraryVod[]} */
  const vods = [];
  let ids = 0;
  /** @param {number} id */
  const vodOf = (id) => {
    const v = vods.find((v) => v.id === id);
    assert.ok(v, `VOD ${id}`);
    return v;
  };
  return {
    vods,
    /** @param {string} channel */
    vod(channel) {
      const v = { id: ++ids, channel, lines: [], chat: [] };
      vods.unshift(v);
      return v.id;
    },
    /** A line of `text` from `start`, a word every 0.3 s, each 0.25 s long.
     *  @param {number} id @param {number} start @param {string} text */
    say(id, start, text) {
      const words = text.split(" ").map((t, i) => ({
        text: t,
        start: start + i * 0.3,
        end: start + i * 0.3 + 0.25,
        terms: terms(t),
      }));
      vodOf(id).lines.push({ id: ++ids, start, end: words[words.length - 1].end, words });
    },
    /** @param {number} id @param {number} time @param {string} author @param {string} text */
    chat(id, time, author, text) {
      const words = text.split(/\s+/).map((w) => ({ text: w, terms: terms(w) }));
      vodOf(id).chat.push({ line: { id: ++ids, time, author, color: null }, words });
    },
    /** @param {string} query @param {import("./search.ts").Filter} filter */
    find(query, filter = {}, limit = 500) {
      // A fresh array: the search works out what it needs once per library.
      return searchLibrary([...vods], query, filter, limit);
    },
  };
}

/** Each hit as its text with the marked runs in [brackets]. @param {Results} r */
const marked = (r) => r.hits.map((h) => h.parts.map((p) => (p.hit ? `[${p.text}]` : p.text)).join(""));

test("a match runs across lines", () => {
  const db = library();
  const v = db.vod("streamer");
  db.say(v, 10, "damn");
  db.say(v, 12, "well that was");
  db.say(v, 14, "a good shot");
  db.say(v, 100, "well that was a good shot");
  let r = db.find('well that was a good shot "damn"');
  assert.deepEqual(marked(r), ["[damn well that was a good shot]"], "three lines, every piece marked");
  assert.deepEqual([r.hits[0].time, r.hits[0].lines], [10, 3]);
  assert.ok(Math.abs(r.hits[0].end - 14.85) < 1e-9, `the last word's end: ${r.hits[0].end}`);
  assert.deepEqual(marked(db.find('(well that was a good shot "damn")')), marked(r));
  r = db.find("well that was a good shot");
  assert.deepEqual(marked(r), ["[well that was a good shot]", "[well that was a good shot]"]);
  assert.deepEqual(
    r.hits.map((h) => h.lines),
    [2, 1],
  );
});

test("a phrase over many lines", () => {
  const db = library();
  const v = db.vod("s");
  ["you", "know", "what", "i", "really", "mean"].forEach((w, i) => db.say(v, i, w));
  let r = db.find("you know what i really mean");
  assert.deepEqual(
    r.hits.map((h) => h.lines),
    [6],
  );
  assert.deepEqual(marked(r), ["[you know what i really mean]"]);
  assert.equal(db.find("what i real").hits[0].lines, 3, "the last word a prefix, while typing");
  const w = db.vod("s");
  db.say(w, 0, "sure you know");
  db.say(w, 1, "…");
  db.say(w, 2, "what");
  r = db.find("you know what");
  assert.deepEqual(
    r.hits.map((h) => [h.vod_id, h.lines]),
    [
      [w, 3],
      [v, 3],
    ],
    "past a line of no words",
  );
  assert.equal(db.find('"what i real"').hits.length, 0, "quoted: whole words");
});

test("a minus leaves out matches with the word near", () => {
  const db = library();
  // The layouts, each frick within about 10 s of "i did it": all out.
  /** @type {[number, string][][]} */
  const layouts = [
    [
      [0, "frick"],
      [3, "i did it"],
    ],
    [
      [0, "i did it"],
      [3, "frick"],
    ],
    [[0, "frick i did it"]],
    [[0, "i did it frick"]],
    [
      [0, "frick"],
      [9, "i did it"],
    ],
  ];
  for (const lines of layouts) {
    const v = db.vod("s");
    for (const [start, text] of lines) db.say(v, start, text);
  }
  assert.equal(db.find("i did it").hits.length, 5);
  assert.deepEqual(marked(db.find("i did it -frick")), []);
  // Far enough away it doesn't count: 30 s, or 12 s (past the centred window's 9.6 s).
  const far = db.vod("s");
  db.say(far, 0, "frick");
  db.say(far, 30, "i did it");
  const edge = db.vod("s");
  db.say(edge, 0, "frick");
  db.say(edge, 12, "i did it");
  assert.deepEqual(
    db.find("i did it -frick").hits.map((h) => h.vod_id),
    [edge, far],
  );
  assert.equal(db.find("i did it -frick within:4").hits.length, 5, "only same-line fricks are near");
  assert.equal(db.find('frick -"i did it"').hits.length, 2, "the far and edge ones");
  assert.equal(db.find('frick -"did i"').hits.length, 7);
});

test("quoted words in any order within the window", () => {
  const db = library();
  const v = db.vod("s");
  db.say(v, 0, "order matters any words at all");
  db.say(v, 100, "all");
  db.say(v, 105, "the words");
  db.say(v, 110, "in any");
  db.say(v, 119, "order");
  db.say(v, 200, "all the words in any");
  db.say(v, 225, "order");
  const q = '"all" "words" "any" "order"';
  const r = db.find(q);
  assert.deepEqual(
    r.hits.map((h) => h.time),
    [0, 100],
    "225 s is too far from 200 s",
  );
  assert.equal(marked(r)[0], "[order] matters [any words] at [all]");
  assert.equal(r.hits[1].lines, 4);
  assert.equal(db.find(`${q} within:10`).hits.length, 1);
  assert.equal(db.find(`${q} within:1m`).hits.length, 3, "200 s to 225 s is in a minute");
  assert.equal(r.query.within, null);
  assert.equal(db.find(`${q} within:1m`).query.within, 60);
  // The filter's window is the default the typed one overrides.
  assert.equal(db.find(q, { within: 60 }).hits.length, 3);
  assert.equal(db.find(`${q} within:10`, { within: 60 }).hits.length, 1);
});

test("the tightest set, and no match twice", () => {
  const db = library();
  const v = db.vod("s");
  db.say(v, 0, "damn");
  db.say(v, 8, "damn");
  db.say(v, 10, "nice shot");
  const times = (/** @type {Results} */ r) => r.hits.map((h) => [h.time, h.lines]);
  assert.deepEqual(times(db.find('nice shot "damn"')), [[8, 2]], "the nearer damn");
  const u = db.vod("s");
  db.say(u, 0, "damn");
  db.say(u, 8, "damn");
  db.say(u, 10, "nice shot");
  for (const t of [500, 600, 700, 800]) db.say(u, t, "nice shot");
  assert.deepEqual(times(db.find('nice shot "damn"', { vodId: u }, 10)), [[8, 2]]);
  const w = db.vod("s");
  db.say(w, 0, "ping pong ping pong");
  assert.deepEqual(
    marked(db.find('"ping" "pong"')),
    ["[ping pong ping pong]"],
    "two matches of one line: one hit, both marked",
  );
  const x = db.vod("s");
  db.say(x, 0, "clutch play and another clutch play");
  assert.deepEqual(marked(db.find("clutch play")), ["[clutch play] and another [clutch play]"]);
});

test("pieces never share a word", () => {
  const db = library();
  const v = db.vod("s");
  db.say(v, 0, "nice shot");
  db.say(v, 100, "nice");
  db.say(v, 105, "very nice");
  db.say(v, 200, "good shot");
  const times = (/** @type {string} */ q) => db.find(q).hits.map((h) => [h.time, h.lines]);
  assert.deepEqual(times('"nice" "nice"'), [[100, 2]], "not the nice at 0 s, alone");
  assert.deepEqual(marked(db.find('"nice" "nice"')), ["[nice] very [nice]"]);
  assert.equal(db.find('"nice" "nice" within:3').hits.length, 0, "the two are 5 s apart");
  assert.equal(db.find('"good shot" "shot"').hits.length, 0, "one shot can't be both");
  assert.deepEqual(times('"good" "shot"'), [[200, 1]], "two pieces, two words");
});

test("a word twice in a line is one hit toward the limit", () => {
  const db = library();
  for (const v of [db.vod("s"), db.vod("s")]) for (const t of [0, 30, 60]) db.say(v, t, "hype hype");
  let r = db.find("hype", {}, 4);
  assert.deepEqual([r.hits.length, r.truncated], [4, true], "on into the second VOD, and more there");
  r = db.find("hype", {}, 6);
  assert.deepEqual([r.hits.length, r.truncated], [6, false]);
});

test("OR, AND and brackets", () => {
  const db = library();
  const v = db.vod("s");
  db.say(v, 0, "well that was");
  db.say(v, 50, "oh damn");
  db.say(v, 100, "nice clutch");
  db.say(v, 150, "good play");
  const times = (/** @type {string} */ q) => db.find(q).hits.map((h) => h.time);
  assert.deepEqual(times("well that was OR damn"), [0, 50.3]);
  assert.deepEqual(times('nice ("clutch" OR "play")'), [100]);
  assert.deepEqual(times('"good" AND "play" OR "oh"'), [50, 150]);
  assert.deepEqual(times('"oh" OR "oh"'), [50], "the same match once");
  assert.deepEqual(times('"nice" OR "clutch"'), [100], "matches in the same line are one hit");
});

test("who said it", () => {
  const db = library();
  const mine = db.vod("Streamer");
  const theirs = db.vod("other");
  db.say(mine, 5, "hello chat");
  db.say(theirs, 5, "hello chat");
  db.chat(mine, 6, "streamer", "hello everyone");
  db.chat(mine, 7, "Bob", "hello streamer");
  db.chat(mine, 8, "bob", "gg");
  db.chat(theirs, 9, "BOB", "hello again");
  db.chat(theirs, 10, "alice", "hi bob");
  const hits = (/** @type {string} */ q, /** @type {import("./search.ts").Filter} */ filter = {}) => {
    const r = db.find(q, filter, 10);
    assert.equal(r.query.error, null, q);
    return r.hits.map((h) => [h.vod_id, h.source, h.time]);
  };
  const [s, c] = ["speech", "chat"];
  assert.deepEqual(
    hits("from:streamer hello"),
    [
      [mine, s, 5],
      [mine, c, 6],
    ],
    "their speech and their chat",
  );
  assert.deepEqual(
    hits("from:bob hello"),
    [
      [theirs, c, 9],
      [mine, c, 7],
    ],
    "any case",
  );
  assert.deepEqual(hits("@bob"), [[theirs, c, 10]], "@name is words");
  assert.deepEqual(
    hits("from:bob"),
    [
      [theirs, c, 9],
      [mine, c, 7],
      [mine, c, 8],
    ],
    "a filter alone lists all",
  );
  assert.deepEqual(hits("from:bob -hello"), [[mine, c, 8]]);
  assert.deepEqual(hits('(from:alice "hi") OR (from:bob "gg")'), [
    [theirs, c, 10],
    [mine, c, 8],
  ]);
  assert.deepEqual(hits("hello in:audio"), [
    [theirs, s, 5],
    [mine, s, 5],
  ]);
  assert.deepEqual(
    hits("from:bob from:alice -hello"),
    [
      [theirs, c, 10],
      [mine, c, 8],
    ],
    "either name",
  );
  // The typed in: wins over the filter's sources.
  const r = db.find("hello in:chat", { speech: true, chat: false, vodId: mine }, 10);
  assert.deepEqual(
    r.hits.map((h) => h.time),
    [6, 7],
  );
  assert.deepEqual(r.query.sources, { audio: false, chat: true });
  assert.equal(db.find("from:bob in:chat").query.from, "bob");
  // The filter's who, for alternatives that don't type their own.
  assert.deepEqual(hits("hello", { from: "bob" }), [
    [theirs, c, 9],
    [mine, c, 7],
  ]);
  assert.deepEqual(hits("hello", { from: "streamer" }), [
    [mine, s, 5],
    [mine, c, 6],
  ]);
  assert.deepEqual(
    hits("hello from:streamer", { from: "bob" }),
    [
      [mine, s, 5],
      [mine, c, 6],
    ],
    "typed wins",
  );
  assert.deepEqual(hits("-hello", { from: "bob" }), [[mine, c, 8]], "a filter, so a minus alone lists");
  assert.deepEqual(hits('(from:alice "hi") OR "gg"', { from: "bob" }), [
    [theirs, c, 10],
    [mine, c, 8],
  ]);
  /** @param {string} prefix @param {number | null} vodId */
  const names = (prefix, vodId = null) =>
    authorSuggestions([...db.vods], prefix, vodId).map((s) => [s.name.toLowerCase(), s.streamer, s.messages]);
  assert.deepEqual(names("B"), [["bob", false, 3]]);
  assert.deepEqual(names("st"), [["streamer", true, 1]], "the channel, and its own chat");
  assert.equal(names("", theirs).length, 3, "other, bob and alice");
  assert.deepEqual(names("zz"), []);
});

// A name alone lists everything it passes: a channel's speech (every line of
// its VODs) and its own chat, a chatter's chat; `in:` narrows it, and several
// names mean any of them.
test("a name alone lists what they said and wrote", () => {
  const db = library();
  const mine = db.vod("Streamer");
  const theirs = db.vod("other");
  db.say(mine, 5, "hello chat");
  db.say(mine, 30, "what a play");
  db.say(theirs, 5, "hello chat");
  db.chat(mine, 6, "streamer", "hello everyone");
  db.chat(mine, 7, "Bob", "hello streamer");
  db.chat(theirs, 9, "alice", "hi bob");
  const hits = (/** @type {string} */ q) => {
    const r = db.find(q);
    assert.equal(r.query.error, null, q);
    return r.hits.map((h) => [h.vod_id, h.source, h.time]);
  };
  const [s, c] = ["speech", "chat"];
  assert.deepEqual(hits("from:streamer"), [
    [mine, s, 5],
    [mine, c, 6],
    [mine, s, 30],
  ]);
  assert.deepEqual(hits("from:streamer in:audio"), [
    [mine, s, 5],
    [mine, s, 30],
  ]);
  assert.deepEqual(hits("from:streamer in:chat"), [[mine, c, 6]]);
  assert.deepEqual(hits("from:streamer after:10"), [[mine, s, 30]]);
  assert.deepEqual(hits("from:streamer -play"), [
    [mine, s, 5],
    [mine, c, 6],
  ]);
  assert.deepEqual(
    hits("from:streamer within:1m -hello"),
    [],
    "the window, centred on each line, that the minus clears",
  );
  assert.deepEqual(hits("from:streamer -hello"), [[mine, s, 30]]);
  const either = [
    [theirs, c, 9],
    [mine, s, 5],
    [mine, c, 6],
    [mine, s, 30],
  ];
  assert.deepEqual(hits("from:streamer,alice"), either);
  assert.deepEqual(hits("from:alice from:streamer"), either);
  assert.deepEqual(hits('from:Bob,"alice" in:chat'), [
    [theirs, c, 9],
    [mine, c, 7],
  ]);
  // The filter's From whom takes the same list, spaces round the commas allowed.
  /** @param {string} names @returns {import("./search.ts").Filter} */
  const from = (names) => ({ from: names });
  const by = (/** @type {string} */ q, /** @type {import("./search.ts").Filter} */ f) =>
    db.find(q, f, 10).hits.map((h) => [h.vod_id, h.source, h.time]);
  assert.deepEqual(by("in:chat", from('Bob, "alice"')), [
    [theirs, c, 9],
    [mine, c, 7],
  ]);
  assert.deepEqual(by("hello in:audio", from("streamer,alice")), [[mine, s, 5]]);
  assert.deepEqual(by("in:chat from:alice", from("Bob")), [[theirs, c, 9]], "typed wins");
  // With nothing typed, the filter's names alone list as typed ones do.
  assert.deepEqual(by("", from("streamer")), hits("from:streamer"));
  assert.deepEqual(by(" ", from("Bob, alice")), [
    [theirs, c, 9],
    [mine, c, 7],
  ]);
  const chatOnly = { ...from("streamer"), speech: false };
  assert.deepEqual(by("", chatOnly), [[mine, c, 6]]);
  assert.equal(db.find("").hits.length, 0, "nothing typed, nothing set: no search");
  assert.deepEqual(by("", from(" , ")), []);
  // Capped as every search is, VOD by VOD.
  for (let i = 0; i < 20; i++) db.say(theirs, 100 + i, "more talk");
  const r = db.find("from:other,streamer", {}, 10);
  assert.deepEqual([r.hits.length, r.truncated], [10, true]);
  assert.ok(
    r.hits.every((h) => h.vod_id === theirs && h.source === s),
    "the first VOD's lines first",
  );
});

test("times into the VOD", () => {
  const db = library();
  const v = db.vod("s");
  for (const t of [10, 4000, 8000]) {
    db.say(v, t, "gg");
    db.chat(v, t, "x", "gg");
  }
  const times = (/** @type {string} */ q) => db.find(q).hits.map((h) => h.time);
  assert.deepEqual(times("gg after:1h before:2h"), [4000, 4000]);
  assert.deepEqual(times("gg after:1:00:00 in:audio"), [4000, 8000]);
  assert.deepEqual(times("before:20 in:chat"), [10], "filters alone");
  const r = db.find("after:1:20:00 gg");
  assert.deepEqual([r.query.after, r.query.before], [4800, null]);
});

test("errors, and nothing to look for", () => {
  const db = library();
  const v = db.vod("s");
  db.say(v, 0, "a clutch play or two");
  for (const q of ['"clutch', "(clutch OR play", "clutch)", "in:video clutch"]) {
    const r = db.find(q);
    assert.ok(r.hits.length === 0 && r.query.error !== null, `${q}: ${r.query.error}`);
  }
  let r = db.find("-clutch");
  assert.equal(r.hits.length, 0);
  assert.match(r.query.error ?? "", /Add a word or a filter/);
  r = db.find("");
  assert.ok(r.hits.length === 0 && r.query.error === null && r.query.tree === null);
  for (const q of ["NEAR(clutch play)", "clutch* play", "{text}: clutch", "clutch ^play", "c:lutch"])
    assert.equal(db.find(q).query.error, null, q);
  assert.equal(db.find("clutch* play").hits.length, 1);
  assert.equal(db.find("zyzzyva clutch").hits.length, 0, "a word in no line");
});

test("the filter's window is at most an hour too", () => {
  const db = library();
  const v = db.vod("s");
  db.say(v, 0, "a clutch play");
  db.say(v, 3000, "and another play");
  let r = db.find('"clutch" "another"', { within: MOST_WITHIN });
  assert.deepEqual([r.hits.length, r.query.error], [1, null]);
  r = db.find("clutch", { within: MOST_WITHIN + 1 });
  assert.equal(r.hits.length, 0);
  assert.equal(r.query.error, TOO_WIDE);
  assert.equal(db.find("clutch within:2h").query.error, TOO_WIDE);
});

test("the rarest word is looked up", () => {
  const db = library();
  const v = db.vod("s");
  for (let i = 0; i < 20; i++) db.say(v, i * 30, i === 7 ? "the the zebra" : "the the the");
  assert.equal(db.find("the zeb").hits[0].time, 210.3);
});
