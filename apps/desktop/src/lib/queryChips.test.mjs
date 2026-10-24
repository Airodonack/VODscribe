// The search's grouping and the Within control (queryChips.ts): node --test apps/desktop/src/lib/queryChips.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { chipsOf, filterText, lengthText, removeFilter, shows, splitLength } from "./queryChips.ts";

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

test("one phrase shows no grouping; anything more does", () => {
  assert.equal(shows(null), false);
  assert.equal(shows(phrase("clutch play")), false);
  assert.equal(shows(phrase("clutch", true)), false);
  assert.equal(shows({ kind: "not", item: phrase("frick") }), true);
  assert.equal(shows(filter("in", "chat")), true);
  assert.equal(shows({ kind: "and", items: [phrase("nice"), phrase("clutch", true)] }), true);
});

test("brackets around a group inside another, OR between alternatives", () => {
  // "a" AND "b" OR "c"
  /** @type {Node} */
  const ab = { kind: "and", items: [phrase("a", true), phrase("b", true)] };
  assert.equal(read({ kind: "or", items: [ab, phrase("c", true)] }), '( "a" "b" ) OR "c"');
  // from:x "a" OR "b"
  /** @type {Node} */
  const or = { kind: "or", items: [phrase("a", true), phrase("b", true)] };
  assert.equal(read({ kind: "and", items: [filter("from", "x"), or] }), 'from:x ( "a" OR "b" )');
  // well that was OR damn
  assert.equal(read({ kind: "or", items: [phrase("well that was"), phrase("damn")] }), "well that was OR damn");
  // i did it -frick
  assert.equal(
    read({ kind: "and", items: [phrase("i did it"), { kind: "not", item: phrase("frick") }] }),
    "i did it -frick",
  );
  // (from:a "x") OR (from:b "y")
  /** @param {string} who @param {string} what @returns {Node} */
  const from = (who, what) => ({ kind: "and", items: [filter("from", who), phrase(what, true)] });
  assert.equal(read({ kind: "or", items: [from("a", "x"), from("b", "y")] }), '( from:a "x" ) OR ( from:b "y" )');
});

test("a filter's text as the core writes it", () => {
  assert.equal(filterText("in", "chat"), "in:chat");
  assert.equal(filterText("from", "Some One"), 'from:"Some One"');
});

test("removing a filter takes its text out of the box", () => {
  assert.equal(removeFilter("landed in:chat", "in:chat"), "landed");
  assert.equal(removeFilter("in:chat landed", "in:chat"), "landed");
  assert.equal(removeFilter("a IN:Chat b", "in:chat"), "a b", "any case");
  assert.equal(removeFilter("a in:chat b in:chat", "in:chat"), "a b in:chat", "the first only");
  assert.equal(removeFilter("join:chat in:chat", "in:chat"), "join:chat", "a whole word");
  assert.equal(removeFilter('hi from:"Some One"', 'from:"Some One"'), "hi");
  assert.equal(removeFilter('(from:a "x") OR "y"', "from:a"), '("x") OR "y"');
  assert.equal(removeFilter("landed", "in:chat"), "landed", "not there: as it was");
});

test("Within's lengths", () => {
  assert.deepEqual(splitLength(20), { n: 20, unit: "s" });
  assert.deepEqual(splitLength(90), { n: 90, unit: "s" });
  assert.deepEqual(splitLength(120), { n: 2, unit: "m" });
  assert.deepEqual(splitLength(7200), { n: 2, unit: "h" });
  assert.equal(lengthText(20), "20 s");
  assert.equal(lengthText(300), "5 min");
  assert.equal(lengthText(3600), "1 h");
});
