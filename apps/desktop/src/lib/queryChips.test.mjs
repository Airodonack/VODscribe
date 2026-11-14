// The search's grouping and the Within control (queryChips.ts): node --test apps/desktop/src/lib/queryChips.test.mjs
// Sources: CHANGELOG.md 0.6.0 ("Searches show how they were read.", "Search learns from, in, within, after,
// before.", "The search filter gains Within and From whom."), queryChips.ts's doc comments, tests/e2e/scenarios/search.mjs's header.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  DEFAULT_WITHIN,
  MOST_WITHIN,
  UNITS,
  chipsOf,
  filterText,
  lengthText,
  namesTyped,
  removeFilter,
  shows,
  splitLength,
  withName,
} from "./queryChips.ts";

/** @typedef {import("./api").QueryNode} Node */
/** @param {string} text @returns {Node} */
const phrase = (text, strict = false) => ({ kind: "phrase", text, strict, prefix: false });
/** @param {"from" | "in" | "within" | "after" | "before"} key @param {string} value @returns {Node} */
const filter = (key, value) => ({ kind: "filter", key, value });
/** The chips as text: [quoted] phrases, -not, key:value, OR, brackets. @param {Node} tree */
const read = (tree) =>
  chipsOf(tree)
    .map((c) =>
      c.kind === "phrase"
        ? c.strict
          ? `"${c.text}"`
          : c.text
        : c.kind === "not"
          ? `-${c.text}`
          : c.kind === "filter"
            ? c.text
            : c.kind === "or"
              ? "OR"
              : c.kind === "open"
                ? "("
                : ")",
    )
    .join(" ");

/** The tree's nodes as the core gives them. @param {Node[]} items @returns {Node} */
const and = (...items) => ({ kind: "and", items });
/** @param {Node[]} items @returns {Node} */
const or = (...items) => ({ kind: "or", items });
/** @param {Node} item @returns {Node} */
const not = (item) => ({ kind: "not", item });

// shows (queryChips.ts shows, "Whether the grouping is worth showing: more than one piece, an operator or a filter")

/**
 * queryChips.ts shows and CHANGELOG 0.6.0 "Searches show how they were read" ("a search with more than one part shows
 * as chips"): no search, or one phrase (quoted or not), shows no chips.
 */
test("no search and a single phrase show no chips", () => {
  assert.equal(shows(null), false);
  assert.equal(shows(phrase("clutch play")), false);
  assert.equal(shows(phrase("clutch", true)), false);
});

/** queryChips.ts shows ("more than one piece"): a search of several pieces shows its chips. */
test("a search of several pieces shows its chips", () => {
  assert.equal(shows(and(phrase("nice"), phrase("clutch", true))), true);
});

/** queryChips.ts shows ("an operator"): a minus or an OR is worth showing. */
test("a minus or an OR shows its chips", () => {
  assert.equal(shows(not(phrase("frick"))), true);
  assert.equal(shows(or(phrase("zebra"), phrase("landed"))), true);
});

/** queryChips.ts shows ("a filter"): a filter alone is worth showing. */
test("a filter alone shows its chip", () => {
  assert.equal(shows(filter("in", "chat")), true);
});

// chipsOf

/**
 * CHANGELOG 0.6.0 "Searches show how they were read" ("a search with more than one part shows as chips");
 * queryChips.ts the header comment ("each piece a chip"); queryChips.ts Chip (a phrase to find; strict ones were quoted).
 */
test("each piece of a search is a chip, in order, the quoted ones strict", () => {
  assert.deepEqual(chipsOf(and(phrase("i did it"), phrase("clutch", true), filter("in", "chat"))), [
    { kind: "phrase", text: "i did it", strict: false },
    { kind: "phrase", text: "clutch", strict: true },
    { kind: "filter", key: "in", value: "chat", text: "in:chat" },
  ]);
});

/**
 * CHANGELOG 0.6.0 "Searches show how they were read" ("OR between the alternatives"); e2e search.mjs ("zebra OR
 * landed" shown as two word chips with OR between them).
 */
test("OR is shown between the alternatives", () => {
  assert.equal(read(or(phrase("zebra"), phrase("landed"))), "zebra OR landed");
  assert.equal(read(or(phrase("well that was"), phrase("damn"), phrase("wow"))), "well that was OR damn OR wow");
});

/**
 * CHANGELOG 0.6.0 "Searches show how they were read" ("brackets round groups"); queryChips.ts the header comment ("brackets around a
 * group inside another"): an AND inside an OR.
 */
test("brackets go round an AND inside an OR", () => {
  // "a" "b" OR "c"
  assert.equal(read(or(and(phrase("a", true), phrase("b", true)), phrase("c", true))), '( "a" "b" ) OR "c"');
  // (from:a "x") OR (from:b "y")
  const from = (/** @type {string} */ who, /** @type {string} */ what) => and(filter("from", who), phrase(what, true));
  assert.equal(read(or(from("a", "x"), from("b", "y"))), '( from:a "x" ) OR ( from:b "y" )');
});

/** queryChips.ts Chip (the chips' kinds) and the header comment ("brackets around a group inside another"): an open and a close chip. */
test("a group's brackets are an open chip and a close chip", () => {
  assert.deepEqual(
    chipsOf(or(and(phrase("a"), phrase("b")), phrase("c"))).map((c) => c.kind),
    ["open", "phrase", "phrase", "close", "or", "phrase"],
  );
});

/** queryChips.ts the header comment ("brackets around a group inside another"): an OR inside an AND. */
test("brackets go round an OR inside an AND", () => {
  // from:x ("a" OR "b")
  assert.equal(read(and(filter("from", "x"), or(phrase("a", true), phrase("b", true)))), 'from:x ( "a" OR "b" )');
});

/** queryChips.ts the header comment ("brackets around a group inside another"): the whole search has none round it. */
test("the whole search has no brackets round it", () => {
  assert.equal(read(and(phrase("nice"), phrase("clutch"))), "nice clutch");
  assert.equal(read(or(phrase("a"), phrase("b"))), "a OR b");
});

/**
 * CHANGELOG 0.6.0 "Searches show how they were read" ("a crossed-out chip for a minus"); queryChips.ts Chip ("A phrase that mustn't be near (`-word`)").
 */
test("a minus is a chip of its own kind, for the page to cross out", () => {
  assert.deepEqual(chipsOf(and(phrase("i did it"), not(phrase("frick")))), [
    { kind: "phrase", text: "i did it", strict: false },
    { kind: "not", text: "frick", strict: false },
  ]);
  assert.deepEqual(chipsOf(and(phrase("nice"), not(phrase("frick", true)))).at(-1), {
    kind: "not",
    text: "frick",
    strict: true,
  });
});

/**
 * CHANGELOG 0.6.0 "Search learns from, in, within, after, before" ("Each one shows as a chip"); queryChips.ts Chip
 * ("`text` is the filter as the core writes it, to find it in the box again"); e2e search.mjs (landed
 * within:5: two chips, landed and within:5).
 */
test("a filter is a chip carrying its text as the core writes it", () => {
  assert.deepEqual(chipsOf(and(phrase("landed"), filter("within", "5"))), [
    { kind: "phrase", text: "landed", strict: false },
    { kind: "filter", key: "within", value: "5", text: "within:5" },
  ]);
  assert.deepEqual(chipsOf(filter("from", "Some One")), [
    { kind: "filter", key: "from", value: "Some One", text: 'from:"Some One"' },
  ]);
});

// filterText

/** queryChips.ts filterText ("`key:value`"): a filter's text is its key, a colon and its value. */
test("a filter's text is its key, a colon and its value", () => {
  assert.equal(filterText("in", "chat"), "in:chat");
  assert.equal(filterText("from", "a,b"), "from:a,b");
});

/** queryChips.ts filterText ("the value quoted if it has a space"). */
test("a filter's value with a space is quoted", () => {
  assert.equal(filterText("from", "Some One"), 'from:"Some One"');
});

/** queryChips.ts filterText ("unless it's a list of names with quotes of its own (`from:a,\"Some One\"`)"). */
test("a list of names with quotes of its own is not quoted again", () => {
  assert.equal(filterText("from", 'a,"Some One"'), 'from:a,"Some One"');
});

// removeFilter

/**
 * CHANGELOG 0.6.0 "Search learns from, in, within, after, before" ("an × to take it out of the search");
 * queryChips.ts removeFilter ("`query` with the filter `text` taken out ... and the spaces round it tidied"); e2e
 * search.mjs (the chip's × takes its text out of the box).
 */
test("removing a filter takes its text out of the box and tidies the spaces", () => {
  assert.equal(removeFilter("landed in:chat", "in:chat"), "landed");
  assert.equal(removeFilter("in:chat landed", "in:chat"), "landed");
  assert.equal(removeFilter("a in:chat b", "in:chat"), "a b");
  assert.equal(removeFilter("in:chat", "in:chat"), "");
});

/** queryChips.ts removeFilter ("any case"). */
test("a filter is taken out in any case", () => {
  assert.equal(removeFilter("a IN:Chat b", "in:chat"), "a b");
});

/** queryChips.ts removeFilter ("the first time it's there"). */
test("only the first time a filter is there is taken out", () => {
  assert.equal(removeFilter("a in:chat b in:chat", "in:chat"), "a b in:chat");
});

/** queryChips.ts removeFilter ("as a whole word"). */
test("a filter is taken out only as a whole word", () => {
  assert.equal(removeFilter("join:chat in:chat", "in:chat"), "join:chat");
  assert.equal(removeFilter("in:chatty", "in:chat"), "in:chatty");
});

/** queryChips.ts removeFilter ("the first time it's there ... quoted or not"): the first of the two spellings, wherever it is. */
test("the first time a filter is there is taken out, whichever way it was spelled", () => {
  assert.equal(removeFilter('from:bob hi from:"bob"', "from:bob"), 'hi from:"bob"');
});

/** queryChips.ts removeFilter ("its value quoted or not: `from:"bob"` is the core's from:bob"). */
test("a filter is taken out whether its value was quoted or not", () => {
  assert.equal(removeFilter('hi from:"bob"', "from:bob"), "hi");
  assert.equal(removeFilter('hi from:"Some One"', 'from:"Some One"'), "hi");
  assert.equal(removeFilter('hi from:a,"Some One" x', 'from:a,"Some One"'), "hi x");
  assert.equal(removeFilter('from:"bob" hi from:bob', "from:bob"), "hi from:bob", "the first, either way");
});

/** queryChips.ts removeFilter ("the spaces round it tidied"): beside a bracket no space is left or added. */
test("a filter taken out beside a bracket leaves no stray space", () => {
  assert.equal(removeFilter('(from:a "x") OR "y"', "from:a"), '("x") OR "y"');
  assert.equal(removeFilter('("x" from:a) OR "y"', "from:a"), '("x") OR "y"');
});

/** queryChips.ts removeFilter ("the spaces round it tidied"): however many spaces were round it, one is left between the rest. */
test("a filter taken out leaves one space between what was round it", () => {
  assert.equal(removeFilter("x y   in:chat   z", "in:chat"), "x y z");
  assert.equal(removeFilter('("a" in:chat b)', "in:chat"), '("a" b)');
  assert.equal(removeFilter("in:chat   z", "in:chat"), "z");
  assert.equal(removeFilter("z   in:chat", "in:chat"), "z");
});

/** queryChips.ts removeFilter ("as it was if it isn't there"). */
test("a query without the filter is left as it was", () => {
  assert.equal(removeFilter("landed", "in:chat"), "landed");
  assert.equal(removeFilter("  landed  in:audio ", "in:chat"), "  landed  in:audio ");
});

// The From whom control

/**
 * CHANGELOG 0.6.0 "The search filter gains Within and From whom" ("takes several separated by commas");
 * queryChips.ts namesTyped ("`done` are those before the last comma, as typed; `typing` the one after it").
 */
test("the From whom control takes several names, separated by commas", () => {
  assert.deepEqual(namesTyped("Ad"), { done: [], typing: "Ad" });
  assert.deepEqual(namesTyped("Ada, So"), { done: ["Ada"], typing: "So" });
  assert.deepEqual(namesTyped("Ada, Bob, Cy"), { done: ["Ada", "Bob"], typing: "Cy" });
  assert.deepEqual(namesTyped("Ada,"), { done: ["Ada"], typing: "" });
});

/** queryChips.ts namesTyped ("one with a comma in quotes"; `done` "as typed", so with its quotes). */
test("a name with a comma in quotes is one name", () => {
  assert.deepEqual(namesTyped('"Last, First", Ada,'), { done: ['"Last, First"', "Ada"], typing: "" });
});

/** queryChips.ts namesTyped ("`typing` the one after it, without its quotes, for the suggestions"). */
test("the name being typed is read without its quotes", () => {
  assert.deepEqual(namesTyped('Ada, "Last, Fi'), { done: ["Ada"], typing: "Last, Fi" });
  assert.deepEqual(namesTyped('Ada, "Bo"'), { done: ["Ada"], typing: "Bo" });
});

/** queryChips.ts withName ("The control's `text` with the name being typed replaced by `name`"). */
test("choosing a suggestion replaces the name being typed", () => {
  assert.equal(withName("", "Ada"), "Ada");
  assert.equal(withName("Ada, so", "Some One"), "Ada, Some One");
  assert.equal(withName("Ada, Bob, c", "Cy"), "Ada, Bob, Cy");
});

/** queryChips.ts withName ("The name in the control's text, quoted if it has a comma"). */
test("a chosen name with a comma is quoted", () => {
  assert.equal(withName("Ada,", "Last, First"), 'Ada, "Last, First"');
  assert.equal(withName("", "Last, First"), '"Last, First"');
});

// Within

/**
 * queryChips.ts DEFAULT_WITHIN ("The window when neither the filter nor the text sets another, in seconds (the core's
 * `DEFAULT_WITHIN`, query.rs)": crates/core/src/query.rs DEFAULT_WITHIN, 20 s); CHANGELOG 0.6.0 "Search learns ..." ("20
 * seconds unless you say"); queryChips.ts MOST_WITHIN (the widest, an hour: query.rs MOST_WITHIN).
 */
test("Within is 20 seconds unless said and at most an hour, as in the core", () => {
  assert.equal(DEFAULT_WITHIN, 20);
  assert.equal(MOST_WITHIN, 3600);
});

/** queryChips.ts UNITS ("The Within control's units, and how many seconds each is"); CHANGELOG 0.6.0 ("seconds, minutes or hours"). */
test("Within's units are seconds, minutes and hours", () => {
  assert.deepEqual(UNITS, { s: 1, m: 60, h: 3600 });
});

/** queryChips.ts splitLength ("Seconds as the control shows them: in the largest unit they're a whole number of"). */
test("Within's length is shown in the largest unit it is a whole number of", () => {
  assert.deepEqual(splitLength(20), { n: 20, unit: "s" });
  assert.deepEqual(splitLength(90), { n: 90, unit: "s" });
  assert.deepEqual(splitLength(60), { n: 1, unit: "m" });
  assert.deepEqual(splitLength(120), { n: 2, unit: "m" });
  assert.deepEqual(splitLength(3600), { n: 1, unit: "h" });
  assert.deepEqual(splitLength(5400), { n: 90, unit: "m" });
  assert.deepEqual(splitLength(7200), { n: 2, unit: "h" });
});

/** queryChips.ts lengthText ("\"20 s\", \"2 min\", \"1 h\", for the filter's summary"); e2e search.mjs (reading "within 5 s"). */
test("the filter's summary writes Within as 20 s, 2 min or 1 h", () => {
  assert.equal(lengthText(20), "20 s");
  assert.equal(lengthText(5), "5 s");
  assert.equal(lengthText(120), "2 min");
  assert.equal(lengthText(300), "5 min");
  assert.equal(lengthText(3600), "1 h");
});
