// The demo's parser (query.ts) against the app's own tests of it, case for
// case: crates/core query.rs `mod tests`. A test added or changed there is
// added or changed here, so the two can't drift apart unseen.
// node --test apps/site/src/lib/tauri/query.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  conjunctions,
  filtered,
  MOST_BRACKETS,
  MOST_WITHIN,
  names,
  parse,
  queryInfo,
  seconds,
  show,
  TOO_WIDE,
  topFilters,
} from "./query.ts";

/** @typedef {import("./query.ts").Node} Node */
/** @param {string} text @returns {Node} */
const loose = (text) => ({ kind: "phrase", text, strict: false, prefix: false });
/** @param {string} text @returns {Node} */
const typing = (text) => ({ kind: "phrase", text, strict: false, prefix: true });
/** @param {string} text @returns {Node} */
const strict = (text) => ({ kind: "phrase", text, strict: true, prefix: false });
/** @param {Node} item @returns {Node} */
const not = (item) => ({ kind: "not", item });
/** @param {Node[]} items @returns {Node} */
const and = (items) => ({ kind: "and", items });
/** @param {Node[]} items @returns {Node} */
const or = (items) => ({ kind: "or", items });
/** @param {import("./query.ts").Key} key @param {string} value @returns {Node} */
const filter = (key, value) => ({ kind: "filter", key, value });
/** @param {string} text */
const tree = (text) => {
  const t = parse(text).tree;
  assert.ok(t, `${text} has a tree`);
  return t;
};
/** The message `text` is refused with. @param {string} text */
const error = (text) => {
  try {
    parse(text);
  } catch (e) {
    return /** @type {Error} */ (e).message;
  }
  assert.fail(`${text} parses`);
};
/** The message running `text` is refused with. @param {string} text */
const alternativesError = (text) => {
  try {
    conjunctions(parse(text));
  } catch (e) {
    return /** @type {Error} */ (e).message;
  }
  assert.fail(`${text} runs`);
};

test("words are one phrase, the last word being typed", () => {
  assert.deepEqual(tree("clutch play"), typing("clutch play"));
  assert.deepEqual(tree("clutch play "), typing("clutch play"), "a space after doesn't end the typing");
  assert.deepEqual(tree("a"), loose("a"), "one letter is a word, not a prefix");
  assert.deepEqual(tree("don't stop"), typing("don't stop"));
  assert.equal(parse("").tree, null);
  assert.equal(parse("  !! - ").tree, null, "nothing to search for");
  assert.throws(() => parse('" OR *'));
});

test("quotes make strict pieces", () => {
  assert.deepEqual(tree('nice "clutch"'), and([loose("nice"), strict("clutch")]));
  assert.deepEqual(
    tree('"all" "words" "any" "order"'),
    and([strict("all"), strict("words"), strict("any"), strict("order")]),
  );
  assert.deepEqual(tree('"a good shot"'), strict("a good shot"));
  assert.deepEqual(tree('"" hello'), typing("hello"), "an empty quote is nothing");
});

test("the owner's examples", () => {
  assert.deepEqual(tree('well that was a good shot "damn"'), and([loose("well that was a good shot"), strict("damn")]));
  assert.deepEqual(tree('(well that was a good shot "damn")'), tree('well that was a good shot "damn"'));
  assert.deepEqual(tree("i did it -frick"), and([loose("i did it"), not(loose("frick"))]));
  assert.deepEqual(tree("well that was OR damn"), or([loose("well that was"), typing("damn")]));
});

test("AND binds tighter than OR, and side by side looser", () => {
  assert.deepEqual(tree('"a" AND "b" OR "c"'), or([and([strict("a"), strict("b")]), strict("c")]), "AND before OR");
  assert.deepEqual(tree('"a" OR "b" AND "c"'), or([strict("a"), and([strict("b"), strict("c")])]));
  assert.deepEqual(
    tree('from:x "a" OR "b"'),
    and([filter("from", "x"), or([strict("a"), strict("b")])]),
    "typed filters apply to the whole query",
  );
  assert.deepEqual(
    tree('nice "clutch" OR "play" "good"'),
    and([loose("nice"), or([strict("clutch"), strict("play")]), strict("good")]),
  );
  assert.deepEqual(tree('("a" OR "b") "c"'), and([or([strict("a"), strict("b")]), strict("c")]));
  assert.deepEqual(
    tree('(from:a "x") OR (from:b "y")'),
    or([and([filter("from", "a"), strict("x")]), and([filter("from", "b"), strict("y")])]),
  );
  assert.deepEqual(tree("a b AND c"), and([loose("a b"), loose("c")]), "AND splits a phrase");
  assert.deepEqual(tree("cats or dogs"), typing("cats or dogs"), "lowercase or is a word");
});

test("half-typed operators are left out", () => {
  assert.deepEqual(tree("well that was OR"), loose("well that was"));
  assert.deepEqual(tree("OR damn"), typing("damn"));
  assert.deepEqual(tree("a OR OR bb"), or([loose("a"), typing("bb")]));
  assert.deepEqual(tree("a AND"), loose("a"));
  assert.deepEqual(tree('"a" AND OR "b"'), or([strict("a"), strict("b")]));
  assert.deepEqual(tree("hello -"), typing("hello"), "a dash on its own is punctuation, as before");
  assert.deepEqual(tree("from:"), filter("from", ""), "a filter being typed");
  assert.equal(parse("()").tree, null);
});

test("negation and filters", () => {
  assert.deepEqual(tree('-"a phrase" bb'), and([not(strict("a phrase")), typing("bb")]));
  assert.deepEqual(tree("well-known"), typing("well-known"), "a dash inside a word is the word's");
  assert.deepEqual(
    tree('from:"Some One" in:chat hi'),
    and([filter("from", "Some One"), filter("in", "chat"), typing("hi")]),
  );
  assert.deepEqual(tree("re:zero"), typing("re:zero"), "an unknown key is words");
  assert.deepEqual(tree("FROM:Bob"), filter("from", "Bob"), "keys in any case");
  const q = parse("within:5 a within:2m after:1:20:00 before:2h in:audio from:bob");
  assert.equal(q.within, 120, "the last within: wins");
  const c = conjunctions(q);
  assert.equal(c.length, 1);
  assert.deepEqual([c[0].after, c[0].before], [4800, 7200]);
  assert.deepEqual(c[0].sources, { audio: true, chat: false });
  assert.deepEqual(c[0].from, ["bob"]);
  assert.equal(topFilters(q).length, 6);
});

// `from:a,b` is either name, as `from:a from:b` is; a name with a space or a
// comma (a YouTube display name can have one) goes in quotes.
test("several names separated by commas", () => {
  /** @param {string} text */
  const from = (text) => conjunctions(parse(text))[0].from;
  assert.deepEqual(tree("from:a,b"), filter("from", "a,b"));
  assert.deepEqual(from("from:a,b"), ["a", "b"]);
  assert.deepEqual(from('from:a,"Some One",c'), ["a", "Some One", "c"]);
  assert.deepEqual(tree('from:a,"Some One" hi'), and([filter("from", 'a,"Some One"'), typing("hi")]));
  assert.deepEqual(from('from:"Some One",a'), ["Some One", "a"]);
  assert.deepEqual(from('from:"Last, First"'), ["Last, First"], "quoted, a comma is the name's");
  assert.deepEqual(from('from:"Last, First",bob'), ["Last, First", "bob"]);
  assert.deepEqual(from("from:a, hi"), ["a"], "a comma left at the end, while typing");
  assert.deepEqual(from("from:a from:b,c"), ["a", "b", "c"]);
  assert.equal(conjunctions(parse("from:,"))[0].from.length, 0);
  assert.match(error('from:a,"Some'), /quote/);
  // The filter shows them all, each once.
  /** @param {string} text */
  const shown = (text) => queryInfo(parse(text)).from;
  assert.equal(shown('from:a,"Some One" hi'), "a, Some One");
  assert.equal(shown("from:Ada from:bob,ada"), "Ada, bob");
  assert.equal(shown('from:"Some One"'), "Some One");
  assert.equal(shown('from:"Last, First",b'), '"Last, First", b');
  // The control's text, read back, is the same names.
  assert.deepEqual(names('"Last, First", b'), ["Last, First", "b"]);
  assert.deepEqual(names(" Ada ,  Some One,"), ["Ada", "Some One"]);
  assert.equal(shown("hi"), null);
});

test("errors say what to fix", () => {
  assert.match(error('"unclosed'), /quote/);
  assert.match(error('-"unclosed'), /quote/);
  assert.match(error("(a OR b"), /bracket isn't closed/);
  assert.match(error("a OR b)"), /wasn't opened/);
  assert.match(error("((a) b"), /bracket/);
  assert.match(error("in:video"), /in:audio/);
  assert.match(error("within:soon"), /within:/);
  assert.match(error("within:0"), /within:/);
  assert.match(error("after:yesterday"), /after:/);
  assert.match(error("-(a b)"), /minus/);
  assert.match(error("-from:bob"), /minus/);
  const many = Array.from({ length: 7 }, (_, i) => `("a${i}" OR "b${i}")`).join(" ");
  assert.match(alternativesError(many), /alternatives/);
});

// Brackets pasted thousands deep ran the Rust parser's stack out.
test("brackets deep inside each other are refused", () => {
  const nested = (/** @type {number} */ n) => `${"(".repeat(n)}a${")".repeat(n)}`;
  assert.equal(conjunctions(parse(nested(MOST_BRACKETS))).length, 1);
  assert.match(error(nested(MOST_BRACKETS + 1)), /brackets/);
  assert.match(error(nested(100_000)), /brackets/);
  assert.match(error("(".repeat(100_000)), /brackets/);
});

test("alternatives carry their own filters", () => {
  let c = conjunctions(parse('(from:a "x") OR (from:b "y")'));
  assert.deepEqual(
    c.map((c) => [c.from, c.pieces[0].terms]),
    [
      [["a"], ["x"]],
      [["b"], ["y"]],
    ],
  );
  c = conjunctions(parse('from:x ("a" OR "b") "c" -d'));
  assert.equal(c.length, 2);
  assert.ok(c.every((c) => c.from.length === 1 && c.from[0] === "x" && c.pieces.length === 2 && c.not.length === 1));
  c = conjunctions(parse("in:chat in:audio after:10 after:20 before:50 before:40"));
  assert.deepEqual(c[0].sources, { audio: true, chat: true }, "either in:");
  assert.deepEqual([c[0].after, c[0].before], [20, 40], "all times hold");
  c = conjunctions(parse("Café don't"));
  assert.deepEqual(c[0].pieces, [{ terms: ["cafe", "don", "t"], prefix: false }]);
  assert.ok(
    c[0].pieces[0].terms.every((t) => /^[\p{L}\p{N}]+$/u.test(t)),
    "nothing FTS reads",
  );
  c = conjunctions(parse("-frick"));
  assert.ok(c[0].pieces.length === 0 && !filtered(c[0]), "nothing to look for");
});

test("times", () => {
  assert.equal(seconds("20"), 20);
  assert.equal(seconds("20s"), 20);
  assert.equal(seconds("2m"), 120);
  assert.equal(seconds("1h"), 3600);
  assert.equal(seconds("1h30m"), 5400);
  assert.equal(seconds("1.5m"), 90);
  assert.equal(seconds("1:20:00"), 4800);
  assert.equal(seconds("20:00"), 1200);
  assert.equal(seconds("30m1h"), null);
  assert.equal(seconds("2x"), null);
  assert.equal(seconds("h"), null);
  assert.equal(seconds("1:2:3:4"), null);
  // Each part finite, the total too large to be a time.
  const huge = `1${"0".repeat(305)}`;
  assert.equal(seconds(`${huge}h`), null);
  assert.equal(seconds(`${huge}:00:00`), null);
  assert.match(error(`within:${huge}h`), /within:/);
});

test("the window is at most an hour", () => {
  assert.equal(parse("chat within:1h").within, MOST_WITHIN);
  assert.equal(parse("chat within:60m").within, MOST_WITHIN);
  assert.equal(error("chat within:1h0m1s"), TOO_WIDE);
  assert.equal(error("chat within:3h"), TOO_WIDE);
  assert.equal(error("(a within:2h) OR b"), TOO_WIDE);
});

/** @param {Node} node @returns {Node} */
const stripPrefix = (node) =>
  node.kind === "phrase"
    ? { ...node, prefix: false }
    : node.kind === "or" || node.kind === "and"
      ? { kind: node.kind, items: node.items.map(stripPrefix) }
      : node;

test("shown as text, it parses the same", () => {
  for (const text of [
    'well that was a good shot "damn"',
    "i did it -frick",
    '"all" "words" "any" "order"',
    'from:x "a" OR "b"',
    '"a" AND "b" OR "c"',
    '(from:a "x") OR (from:b "y")',
    "a b AND c",
    'nice "clutch" OR "play" good',
    'from:"Some One" -"a phrase" within:2m',
    'from:a,"Some One" hi',
    'from:"Last, First" hi',
    'from:"Last, First",b hi',
  ]) {
    const parsed = tree(text);
    const shown = show(parsed);
    // Shown, the last phrase isn't being typed any more; else the same.
    assert.deepEqual(stripPrefix(tree(`${shown} ()`)), stripPrefix(parsed), `${text} → ${shown}`);
  }
  assert.equal(show(tree('from:x "a" OR "b"')), 'from:x ("a" OR "b")');
});

// The filter's controls show what the search applies.
test("the filters shown are the ones applied", () => {
  const query = parse("gg after:20 after:10 before:40 before:50");
  const info = queryInfo(query);
  const c = conjunctions(query)[0];
  assert.deepEqual([info.after, info.before], [c.after, c.before]);
  assert.deepEqual([info.after, info.before], [20, 40]);
});

test("the tree as the page reads it", () => {
  // Compared as JSON, as the app's serde output is.
  assert.deepEqual(JSON.parse(JSON.stringify(tree('from:x ("a" OR -b) c'))), {
    kind: "and",
    items: [
      { kind: "filter", key: "from", value: "x" },
      {
        kind: "or",
        items: [
          { kind: "phrase", text: "a", strict: true, prefix: false },
          { kind: "not", item: { kind: "phrase", text: "b", strict: false, prefix: false } },
        ],
      },
      { kind: "phrase", text: "c", strict: false, prefix: false },
    ],
  });
});
