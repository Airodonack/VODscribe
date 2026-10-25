// A VOD page's review and its verdicts file (verdicts.ts): node --test apps/site/src/lib/verdicts.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { fileName, parseReviewed, parseTime, rowKey, storageKey, verdictLines } from "./verdicts.ts";

/** @typedef {import("./verdicts.ts").ReviewRow} Row */
/** @typedef {import("./verdicts.ts").Reviewed} Reviewed */

/** @type {Row} */
const chapter = { kind: "chapter", start: 0, end: 15, title: "The old tutorial" };
/** @type {Row} */
const door = { kind: "chapter", start: 15, end: null, title: "The door" };
/** @type {Row} */
const pelican = { kind: "highlight", start: 7, end: 14, title: "The pelican skip" };
/** @type {Row} */
const quiet = { kind: "highlight", start: 30, end: null, title: "Quiet" };

/** @type {Reviewed} */
const reviewed = {
  rows: {
    [rowKey(chapter)]: { verdict: "keep", corrected_start: null, note: "" },
    [rowKey(pelican)]: { verdict: "misplaced", corrected_start: 6.5, note: " Starts on the jump. " },
    [rowKey(door)]: { verdict: null, corrected_start: null, note: "Title?" },
    [rowKey(quiet)]: { verdict: null, corrected_start: null, note: "  " },
  },
  note: "Good overall.",
};

test("one line per row judged, in the rows' order, then the VOD's", () => {
  const lines = verdictLines("twitch", "v1", [chapter, door, pelican, quiet], reviewed).map((l) => JSON.parse(l));
  assert.deepEqual(lines, [
    { platform: "twitch", id: "v1", kind: "chapter", start: 0, title: "The old tutorial", verdict: "keep", note: "" },
    { platform: "twitch", id: "v1", kind: "chapter", start: 15, title: "The door", verdict: null, note: "Title?" },
    {
      platform: "twitch",
      id: "v1",
      kind: "highlight",
      start: 7,
      end: 14,
      title: "The pelican skip",
      verdict: "misplaced",
      corrected_start: 6.5,
      note: "Starts on the jump.",
    },
    { platform: "twitch", id: "v1", kind: "vod", note: "Good overall." },
  ]);
});

test("a corrected start goes only with a misplaced verdict; nothing judged leaves the VOD's line", () => {
  /** @type {Reviewed} */
  const kept = { rows: { [rowKey(pelican)]: { verdict: "keep", corrected_start: 6.5, note: "" } }, note: "" };
  assert.equal(JSON.parse(verdictLines("twitch", "v1", [pelican], kept)[0]).corrected_start, undefined);
  assert.deepEqual(verdictLines("twitch", "v1", [chapter, pelican], { rows: {}, note: "" }), [
    '{"platform":"twitch","id":"v1","kind":"vod","note":""}',
  ]);
});

test("what's kept reads back, and anything else reads as nothing said", () => {
  assert.deepEqual(parseReviewed(JSON.stringify(reviewed)), reviewed);
  assert.deepEqual(parseReviewed(null), { rows: {}, note: "" });
  assert.deepEqual(parseReviewed("{not json"), { rows: {}, note: "" });
  assert.deepEqual(parseReviewed('{"rows":{"a":{"verdict":"maybe","corrected_start":-1,"note":3}},"note":7}'), {
    rows: { a: { verdict: null, corrected_start: null, note: "" } },
    note: "",
  });
});

test("times are typed as h:mm:ss, m:ss or seconds", () => {
  assert.equal(parseTime("1:02:05"), 3725);
  assert.equal(parseTime(" 2:05 "), 125);
  assert.equal(parseTime("0:07.5"), 7.5);
  assert.equal(parseTime("125"), 125);
  assert.equal(parseTime(""), null);
  assert.equal(parseTime("1:2:3:4"), null);
  assert.equal(parseTime("1.5:00"), null);
  assert.equal(parseTime("soon"), null);
});

test("each VOD is kept apart, and its file is named for it", () => {
  assert.equal(storageKey("twitch", "v1"), "vodscribe.review.twitch.v1");
  assert.notEqual(storageKey("twitch", "v1"), storageKey("youtube", "v1"));
  assert.equal(fileName("twitch", "v2901646294"), "verdicts-twitch-v2901646294.jsonl");
  assert.equal(fileName("x", "a/b"), "verdicts-x-a_b.jsonl");
});
