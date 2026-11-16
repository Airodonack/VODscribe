// The demo's parser (query.ts) against the app's own tests of it, case for
// case: crates/core query/tests.rs, test by test, in its order and by its
// names. A test added or changed there is added or changed here, so the two
// can't drift apart unseen. The tests of `terms` itself are search.test.mjs's.
// node --test apps/site/src/lib/tauri/query.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  conjunctions,
  filtered,
  MOST_BRACKETS,
  MOST_WITHIN,
  names,
  noInfo,
  parse,
  queryInfo,
  seconds,
  show,
  TOO_WIDE,
} from "./query.ts";

/** @typedef {import("./query.ts").Node} Node */
/** @typedef {import("./query.ts").Conjunction} Conjunction */
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
/** The grouping `text` parses to. @param {string} text */
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
/** The alternatives `text` is run as. @param {string} text */
const alternatives = (text) => conjunctions(parse(text));
/** The message running `text` is refused with. @param {string} text */
const alternativesError = (text) => {
  try {
    alternatives(text);
  } catch (e) {
    return /** @type {Error} */ (e).message;
  }
  assert.fail(`${text} runs`);
};
/** `text`'s one alternative. @param {string} text */
const only = (text) => {
  const c = alternatives(text);
  assert.equal(c.length, 1, `${text} is one alternative`);
  return c[0];
};
/** What the page shows of `text`. @param {string} text */
const info = (text) => queryInfo(parse(text));
/** @param {string[]} terms @param {boolean} prefix */
const piece = (terms, prefix = false) => ({ terms, prefix });
/** @param {Partial<Conjunction>} c @returns {Conjunction} */
const conjunction = (c) => ({ pieces: [], not: [], from: [], sources: null, after: null, before: null, ...c });

// Pieces and phrases.

test("words typed one after another are one loose phrase in order", () => {
  assert.deepEqual(tree("clutch play"), typing("clutch play"));
  assert.deepEqual(tree("don't stop"), typing("don't stop"));
  assert.deepEqual(only("clutch play now").pieces, [piece(["clutch", "play", "now"], true)]);
});

test("the last word is a prefix while it is being typed", () => {
  assert.deepEqual(tree("clutch pl"), typing("clutch pl"));
  assert.deepEqual(tree("clutch pl "), typing("clutch pl"), "a space after doesn't end the typing");
  assert.ok(only("clutch pl").pieces[0].prefix);
});

test("only the phrase the query ends with is being typed", () => {
  assert.deepEqual(tree('clutch pl "good"'), and([loose("clutch pl"), strict("good")]));
  assert.deepEqual(tree("clutch pl from:bob"), and([loose("clutch pl"), filter("from", "bob")]));
  assert.deepEqual(tree("clutch OR play"), or([loose("clutch"), typing("play")]));
  assert.ok(!only("clutch pl in:chat").pieces[0].prefix);
});

test("a one-letter last word is that word, not a prefix", () => {
  assert.deepEqual(tree("a"), loose("a"));
  assert.deepEqual(tree("clutch p"), loose("clutch p"));
  assert.ok(!only("clutch p").pieces[0].prefix);
});

test("a quoted string is a strict phrase of whole words", () => {
  assert.deepEqual(tree('"a good shot"'), strict("a good shot"));
  assert.deepEqual(tree('hi "clutch"'), and([loose("hi"), strict("clutch")]), "never a prefix, even last");
  assert.deepEqual(only('hi "clutch"').pieces[1], piece(["clutch"]));
});

test("words form a phrase until a quote, an operator, a bracket or a filter", () => {
  assert.deepEqual(tree('nice "clutch"'), and([loose("nice"), strict("clutch")]));
  assert.deepEqual(tree("well that was OR damn"), or([loose("well that was"), typing("damn")]));
  assert.deepEqual(tree("a b AND c"), and([loose("a b"), loose("c")]));
  assert.deepEqual(tree("nice (clutch) play"), and([loose("nice"), loose("clutch"), typing("play")]));
  assert.deepEqual(tree("nice from:x play"), and([loose("nice"), filter("from", "x"), typing("play")]));
  assert.deepEqual(tree("nice -b play"), and([loose("nice"), not(loose("b")), typing("play")]));
});

test("quoted words are pieces of their own", () => {
  assert.deepEqual(
    tree('"all" "words" "any" "order"'),
    and([strict("all"), strict("words"), strict("any"), strict("order")]),
  );
  assert.deepEqual(
    only('"all" "words" "any" "order"').pieces.map((p) => p.terms),
    [["all"], ["words"], ["any"], ["order"]],
  );
});

test("the worked examples read as decided", () => {
  assert.deepEqual(tree('well that was a good shot "damn"'), and([loose("well that was a good shot"), strict("damn")]));
  assert.deepEqual(tree('(well that was a good shot "damn")'), tree('well that was a good shot "damn"'));
  assert.deepEqual(tree("i did it -frick"), and([loose("i did it"), not(loose("frick"))]));
  assert.deepEqual(tree("well that was OR damn"), or([loose("well that was"), typing("damn")]));
});

test("a query's words are folded as the index folds them", () => {
  const c = only("Café don't");
  assert.deepEqual(c.pieces, [piece(["cafe", "don", "t"])]);
  assert.ok(
    c.pieces[0].terms.every((t) => /^[\p{L}\p{N}]+$/u.test(t)),
    "nothing FTS reads",
  );
});

test("a query with nothing in it has no tree", () => {
  assert.equal(parse("").tree, null);
  assert.equal(parse("  !! - ").tree, null);
  assert.equal(parse("()").tree, null);
  assert.equal(parse("-!!").tree, null);
  assert.deepEqual(tree('"" hello'), typing("hello"), "an empty quote is nothing");
  assert.deepEqual(alternatives(""), []);
});

test("anything else is words", () => {
  assert.deepEqual(tree("re:zero"), typing("re:zero"), "an unknown key");
  assert.deepEqual(tree("cats or dogs"), typing("cats or dogs"), "or in lower case");
  assert.deepEqual(tree("cats and dogs"), typing("cats and dogs"), "and in lower case");
  assert.deepEqual(tree("at 12:30"), typing("at 12:30"));
  assert.deepEqual(tree("@Ada hi"), typing("@Ada hi"));
  const c = only("@Ada hi");
  assert.ok(c.from.length === 0 && !filtered(c));
  assert.deepEqual(c.pieces[0].terms, ["ada", "hi"]);
});

// Minus.

test("a minus leaves out a word or a quoted phrase", () => {
  assert.deepEqual(tree("i did it -frick"), and([loose("i did it"), not(loose("frick"))]));
  assert.deepEqual(tree('-"a phrase" bb'), and([not(strict("a phrase")), typing("bb")]));
  const c = only('i did it -frick -"a phrase"');
  assert.deepEqual(c.pieces, [piece(["i", "did", "it"])]);
  assert.deepEqual(c.not, [piece(["frick"]), piece(["a", "phrase"])]);
});

test("a minus goes only before a word or a quoted phrase", () => {
  assert.match(error("-(a b)"), /minus/);
  assert.match(error("-from:bob"), /minus/);
});

test("a dash inside a word is the word's, and one on its own is punctuation", () => {
  assert.deepEqual(tree("well-known"), typing("well-known"));
  assert.deepEqual(tree("hello -"), typing("hello"));
  assert.deepEqual(tree("hello - there"), typing("hello there"));
});

test("exclusions alone look for nothing", () => {
  const c = only("-frick");
  assert.ok(c.pieces.length === 0 && !filtered(c));
});

// OR, AND and brackets.

test("OR offers alternatives", () => {
  assert.deepEqual(tree('"a" OR "b"'), or([strict("a"), strict("b")]));
  assert.deepEqual(
    alternatives('"a" OR "b"').map((c) => c.pieces[0].terms),
    [["a"], ["b"]],
  );
});

test("AND binds tighter than OR", () => {
  assert.deepEqual(tree('"a" AND "b" OR "c"'), or([and([strict("a"), strict("b")]), strict("c")]));
  assert.deepEqual(tree('"a" OR "b" AND "c"'), or([strict("a"), and([strict("b"), strict("c")])]));
  assert.deepEqual(
    alternatives('"a" AND "b" OR "c"').map((c) => c.pieces.length),
    [2, 1],
  );
});

test("brackets group", () => {
  assert.deepEqual(tree('("a" OR "b") "c"'), and([or([strict("a"), strict("b")]), strict("c")]));
  assert.deepEqual(tree('"a" AND ("b" OR "c")'), and([strict("a"), or([strict("b"), strict("c")])]));
  const c = alternatives('("a" OR "b") "c"');
  assert.equal(c.length, 2);
  assert.ok(c.every((c) => c.pieces.length === 2 && c.pieces[1].terms.join() === "c"));
});

test("pieces and filters side by side bind looser than OR", () => {
  assert.deepEqual(tree('from:x "a" OR "b"'), and([filter("from", "x"), or([strict("a"), strict("b")])]));
  assert.deepEqual(
    tree('nice "clutch" OR "play" "good"'),
    and([loose("nice"), or([strict("clutch"), strict("play")]), strict("good")]),
  );
  assert.ok(alternatives('from:x "a" OR "b"').every((c) => c.from.join() === "x"));
});

test("a group inside one of its own kind is part of it", () => {
  assert.deepEqual(tree('("a" AND "b") AND "c"'), and([strict("a"), strict("b"), strict("c")]));
  assert.deepEqual(tree('("a" OR "b") OR "c"'), or([strict("a"), strict("b"), strict("c")]));
  assert.deepEqual(tree('(("a"))'), strict("a"));
});

test("an OR or AND with nothing on one side is left out", () => {
  assert.deepEqual(tree("well that was OR"), loose("well that was"));
  assert.deepEqual(tree("OR damn"), typing("damn"));
  assert.deepEqual(tree("a OR OR bb"), or([loose("a"), typing("bb")]));
  assert.deepEqual(tree("a AND"), loose("a"));
  assert.deepEqual(tree("AND a"), loose("a"));
  assert.deepEqual(tree('"a" AND OR "b"'), or([strict("a"), strict("b")]));
});

test("more than 64 alternatives are refused", () => {
  const groups = (/** @type {number} */ n) => Array.from({ length: n }, (_, i) => `("a${i}" OR "b${i}")`).join(" ");
  assert.equal(alternatives(groups(6)).length, 64);
  assert.match(alternativesError(groups(7)), /alternatives/);
  // `n` words, each an alternative, from `a` on.
  const ors = (/** @type {string} */ a, /** @type {number} */ n) =>
    Array.from({ length: n }, (_, i) => `"${a}${i}"`).join(" OR ");
  assert.equal(alternatives(ors("a", 64)).length, 64);
  assert.match(alternativesError(ors("a", 65)), /alternatives/);
  // 8 by 9 is 72 of them.
  assert.match(alternativesError(`(${ors("a", 8)}) (${ors("b", 9)})`), /alternatives/);
});

test("brackets deep inside each other are refused", () => {
  const nested = (/** @type {number} */ n) => `${"(".repeat(n)}a${")".repeat(n)}`;
  assert.equal(alternatives(nested(MOST_BRACKETS)).length, 1);
  assert.match(error(nested(MOST_BRACKETS + 1)), /brackets/);
  assert.match(error(nested(100_000)), /brackets/);
  assert.match(error("(".repeat(100_000)), /brackets/);
  // Side by side, not inside each other: any number.
  const sideBySide = Array.from({ length: 100 }, (_, i) => `(w${i})`).join(" ");
  assert.equal(alternatives(sideBySide).length, 1);
});

// Filters.

test("from names who said or wrote it", () => {
  assert.deepEqual(tree("from:bob"), filter("from", "bob"));
  assert.deepEqual(only("from:bob hi").from, ["bob"]);
  assert.deepEqual(tree('from:"Some One" hi'), and([filter("from", "Some One"), typing("hi")]));
  assert.deepEqual(only('from:"Some One" hi').from, ["Some One"]);
});

test("from takes several names separated by commas, any in quotes", () => {
  assert.deepEqual(tree("from:a,b"), filter("from", "a,b"));
  assert.deepEqual(only("from:ada,marlowe").from, ["ada", "marlowe"]);
  assert.deepEqual(only('from:a,"Some One",c').from, ["a", "Some One", "c"]);
  assert.deepEqual(only('from:"Some One",a').from, ["Some One", "a"]);
  assert.deepEqual(tree('from:a,"Some One" hi'), and([filter("from", 'a,"Some One"'), typing("hi")]));
});

test("a comma inside quotes is the name's", () => {
  assert.deepEqual(only('from:"Last, First"').from, ["Last, First"]);
  assert.deepEqual(only('from:"Last, First",bob').from, ["Last, First", "bob"]);
  assert.deepEqual(names('"Last, First", b'), ["Last, First", "b"]);
});

test("from typed twice is any of their names", () => {
  assert.deepEqual(only("from:a from:b,c hi").from, ["a", "b", "c"]);
});

test("names reads the From whom control's text", () => {
  assert.deepEqual(names("Ada"), ["Ada"]);
  assert.deepEqual(names(" Ada ,  Some One"), ["Ada", "Some One"]);
  assert.deepEqual(names('"Some One",bob'), ["Some One", "bob"]);
});

test("an empty name between commas is no one", () => {
  assert.deepEqual(only("from:a, hi").from, ["a"]);
  assert.deepEqual(only("from:Ada,,Bob").from, ["Ada", "Bob"]);
  assert.equal(only("from:,").from.length, 0);
  assert.deepEqual(names(" Ada ,  Some One,"), ["Ada", "Some One"]);
});

test("in:chat or in:audio chooses the sources", () => {
  assert.deepEqual(only("in:chat hi").sources, { audio: false, chat: true });
  assert.deepEqual(only("in:audio hi").sources, { audio: true, chat: false });
  assert.deepEqual(only("in:chat in:audio hi").sources, { audio: true, chat: true }, "either in:");
  assert.equal(only("hi").sources, null, "the filter's own");
});

test("in takes only audio or chat", () => {
  assert.match(error("in:video"), /in:audio/);
});

test("keys and a time's units are read in any case", () => {
  assert.deepEqual(tree("FROM:Bob"), filter("from", "Bob"));
  assert.equal(seconds("1H"), 3600);
});

test("within takes seconds, minutes or hours, a bare number seconds", () => {
  const within = (/** @type {string} */ text) => parse(text).within;
  assert.equal(within("gg within:20"), 20);
  assert.equal(within("gg within:20s"), 20);
  assert.equal(within("gg within:2m"), 120);
  assert.equal(within("gg within:1h"), 3600);
});

test("the last within anywhere in the query is the window", () => {
  assert.equal(parse("within:5 a within:2m").within, 120);
  assert.equal(parse("(a within:2m) OR b").within, 120, "inside an alternative too");
  assert.equal(info("(a within:2m) OR b").within, 120, "shown in the filter's control");
});

test("within not typed leaves the window to the filter", () => {
  assert.equal(parse("gg").within, null);
  assert.equal(info("gg from:bob").within, null);
});

test("within is at most an hour", () => {
  assert.equal(parse("chat within:1h").within, MOST_WITHIN);
  assert.equal(parse("chat within:60m").within, MOST_WITHIN);
  assert.equal(error("chat within:1h0m1s"), TOO_WIDE);
  assert.equal(error("chat within:3h"), TOO_WIDE);
  assert.equal(error("(a within:2h) OR b"), TOO_WIDE);
});

test("a within that is not a length of time says so", () => {
  assert.match(error("within:soon"), /within:/);
});

test("a window of no time is a window", () => {
  for (const text of ["gg within:0", "gg within:0s", "gg within:0:00", "gg within:0.0m"]) {
    assert.equal(parse(text).within, 0, text);
  }
  assert.equal(info("gg within:0").within, 0, "shown in the filter's control");
});

test("after and before take a time into the VOD", () => {
  const c = only("gg after:1:20:00 before:2h");
  assert.deepEqual([c.after, c.before], [4800, 7200]);
  assert.equal(only("gg after:90m").after, 5400);
  const shown = info("gg after:1:20:00 before:2h");
  assert.deepEqual([shown.after, shown.before], [4800, 7200]);
});

test("after or before typed twice both hold", () => {
  const c = only("in:chat after:10 after:20 before:50 before:40");
  assert.deepEqual([c.after, c.before], [20, 40]);
});

test("a time that is not one says so", () => {
  assert.match(error("after:yesterday"), /after:/);
  assert.match(error("before:soon"), /before:/);
});

test("a filter being typed does nothing yet", () => {
  assert.deepEqual(tree("from:"), filter("from", ""));
  for (const text of ["hi from:", "hi in:", "hi within:", "hi after:", "hi before:"]) {
    const query = parse(text);
    assert.equal(query.within, null, text);
    assert.deepEqual(conjunctions(query), [conjunction({ pieces: [piece(["hi"])] })], text);
    assert.deepEqual(queryInfo(query), { ...noInfo(), tree: query.tree }, text);
  }
  assert.deepEqual(only("from: hi").from, []);
});

test("a filter's value may be quoted", () => {
  assert.equal(parse('gg within:"2m"').within, 120);
  assert.equal(only('gg after:"1:20:00"').after, 4800);
  assert.deepEqual(only('gg in:"chat"').sources, { audio: false, chat: true });
});

test("a filter alone lists everything it passes", () => {
  for (const text of ["from:bob", "from:bob -hello", "in:chat", "after:10", "before:1h"]) {
    const c = only(text);
    assert.ok(c.pieces.length === 0 && filtered(c), text);
  }
  assert.deepEqual(only("from:bob -hello").not, [piece(["hello"])]);
});

test("alternatives carry their own filters", () => {
  assert.deepEqual(
    tree('(from:a "x") OR (from:b "y")'),
    or([and([filter("from", "a"), strict("x")]), and([filter("from", "b"), strict("y")])]),
  );
  let c = alternatives('(from:a "x") OR (from:b "y")');
  assert.deepEqual(
    c.map((c) => [c.from, c.pieces[0].terms]),
    [
      [["a"], ["x"]],
      [["b"], ["y"]],
    ],
  );
  c = alternatives('(in:chat after:10 "x") OR "y"');
  assert.deepEqual([c[0].sources, c[0].after], [{ audio: false, chat: true }, 10]);
  assert.deepEqual([c[1].sources, c[1].after], [null, null]);
});

test("a filter outside brackets holds for every alternative", () => {
  const c = alternatives('from:x ("a" OR "b") "c" -d');
  assert.equal(c.length, 2);
  assert.ok(c.every((c) => c.from.join() === "x" && c.pieces.length === 2 && c.not.length === 1));
});

// Errors.

test("a quote left open says to close it", () => {
  for (const text of ['"unclosed', '-"unclosed', '" OR *', 'from:a,"Some', 'within:"2m']) {
    assert.match(error(text), /quote/, text);
  }
});

test("a bracket left open or closed unopened says so", () => {
  assert.match(error("(a OR b"), /bracket isn't closed/);
  assert.match(error("((a) b"), /bracket isn't closed/);
  assert.match(error("a OR b)"), /wasn't opened/);
});

// What the page shows.

test("the filters shown are those of the whole query", () => {
  let shown = info('from:x in:chat after:10 before:20 ("a" OR "b")');
  assert.equal(shown.from, "x");
  assert.deepEqual(shown.sources, { audio: false, chat: true });
  assert.deepEqual([shown.after, shown.before], [10, 20]);
  shown = info('(from:a in:chat after:10 "x") OR (from:b before:20 "y")');
  assert.deepEqual([shown.from, shown.sources, shown.after, shown.before], [null, null, null, null]);
});

test("the filters shown are the ones applied", () => {
  const query = parse("gg after:20 after:10 before:40 before:50 in:chat in:audio");
  const shown = queryInfo(query);
  const c = conjunctions(query)[0];
  assert.deepEqual([shown.after, shown.before, shown.sources], [c.after, c.before, c.sources]);
  assert.deepEqual([shown.after, shown.before], [20, 40]);
});

test("the From whom control shows every name once", () => {
  assert.equal(info('from:a,"Some One" hi').from, "a, Some One");
  assert.equal(info("from:Ada from:bob,ada").from, "Ada, bob");
  assert.equal(info('from:"Some One"').from, "Some One");
  assert.equal(info('from:"Last, First",b').from, '"Last, First", b');
  assert.equal(info("hi").from, null);
  // The control's text, read back, is the same names.
  assert.deepEqual(names(info('from:"Last, First",b').from ?? ""), ["Last, First", "b"]);
});

test("a query that cannot be read shows its message in place of the grouping", () => {
  const message = error('"unclosed');
  assert.deepEqual(noInfo(message), {
    tree: null,
    within: null,
    from: null,
    sources: null,
    after: null,
    before: null,
    error: message,
  });
});

test("the query as the page reads it", () => {
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
  const json = JSON.parse(JSON.stringify(info("from:x in:chat within:2m after:10 before:20 hi")));
  delete json.tree;
  assert.deepEqual(json, {
    within: 120,
    from: "x",
    sources: { audio: false, chat: true },
    after: 10,
    before: 20,
    error: null,
  });
  for (const key of ["from", "in", "within", "after", "before"]) {
    assert.deepEqual(tree(`${key}:`), filter(/** @type {import("./query.ts").Key} */ (key), ""));
  }
});

/** `node` with no phrase being typed. @param {Node} node @returns {Node} */
const notTyping = (node) =>
  node.kind === "phrase"
    ? { ...node, prefix: false }
    : node.kind === "or" || node.kind === "and"
      ? { kind: node.kind, items: node.items.map(notTyping) }
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
    assert.deepEqual(notTyping(tree(`${shown} ()`)), notTyping(parsed), `${text} → ${shown}`);
  }
  assert.equal(show(tree('from:x "a" OR "b"')), 'from:x ("a" OR "b")');
});

// Times.

test("seconds reads each form it names", () => {
  assert.equal(seconds("1:20:00"), 4800);
  assert.equal(seconds("20:00"), 1200);
  assert.equal(seconds("90"), 90);
  assert.equal(seconds("90s"), 90);
  assert.equal(seconds("2m"), 120);
  assert.equal(seconds("1h"), 3600);
  assert.equal(seconds("1h30m"), 5400);
  assert.equal(seconds("1h30m15s"), 5415);
});

test("seconds takes larger units first", () => {
  assert.equal(seconds("30m1h"), null);
  assert.equal(seconds("1m1m"), null);
});

test("seconds reads nothing else as a time", () => {
  for (const text of ["", " ", "2x", "h", "m30", "1:2:3:4", "1::2", "1h 30m", "-5", "soon"]) {
    assert.equal(seconds(text), null, JSON.stringify(text));
  }
});

test("a time may have decimals", () => {
  assert.equal(seconds("1.5m"), 90);
  assert.equal(seconds("0.5"), 0.5);
});

test("a time too large to be finite is no time", () => {
  const huge = `1${"0".repeat(305)}`;
  assert.equal(seconds(`${huge}h`), null);
  assert.equal(seconds(`${huge}:00:00`), null);
  assert.match(error(`within:${huge}h`), /within:/);
});
