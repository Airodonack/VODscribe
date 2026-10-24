import assert from "node:assert/strict";
import { test } from "node:test";

import { deal, parseShard, refreshed } from "./lib/durations.mjs";

const durations = { a: 150, b: 90, c: 80, d: 70, e: 20, f: 10, g: 5 };

test("deals longest first, each to the least loaded", () => {
  const lists = deal(Object.keys(durations), durations, 3);
  assert.deepEqual(
    lists.map((l) => l.names),
    [["a"], ["b", "e", "f", "g"], ["c", "d"]],
  );
  assert.deepEqual(
    lists.map((l) => l.seconds),
    [150, 125, 150],
  );
});

test("the same input deals the same way, whatever order the names come in", () => {
  const names = Object.keys(durations);
  assert.deepEqual(deal(names, durations, 2), deal([...names].reverse(), durations, 2));
});

test("unknown scenarios come last, each counted as the median, spread out", () => {
  const lists = deal(["z", "y", "a", "b"], { a: 10, b: 4 }, 2);
  // a (10) to the first, b (4) to the second, then y and z at the median (4) each.
  assert.deepEqual(
    lists.map((l) => l.names),
    [["a"], ["b", "y", "z"]],
  );
});

test("one list holds everything; more lists than scenarios leave some empty", () => {
  assert.deepEqual(deal(["b", "a"], {}, 1)[0].names, ["a", "b"]);
  assert.equal(deal(["a"], durations, 3).filter((l) => l.names.length).length, 1);
  assert.throws(() => deal(["a"], durations, 0));
});

test("shards are i/N, counted from 1", () => {
  assert.deepEqual(parseShard("2/3"), { index: 2, count: 3 });
  for (const bad of ["0/3", "4/3", "1/0", "1", "a/b", undefined]) assert.throws(() => parseShard(bad));
});

test("refreshing keeps a failure's and an instant skip's old time, drops removed scenarios", () => {
  const results = [
    { name: "a", ok: true, seconds: 100 },
    { name: "a", ok: true, seconds: 120 },
    { name: "a", ok: true, seconds: 200 },
    { name: "b", ok: false, seconds: 600 },
    { name: "c", ok: true, skipped: "no network", seconds: 0 },
    { name: "d", ok: true, skipped: "the window manager took it", seconds: 39 },
    { name: "new", ok: true, seconds: 7 },
  ];
  assert.deepEqual(refreshed({ a: 1, b: 2, c: 3, gone: 4 }, results, ["a", "b", "c", "d", "new"]), {
    a: 120,
    b: 2,
    c: 3,
    d: 39,
    new: 7,
  });
});
