// The reads kept per VOD (kept.ts): node --test apps/desktop/src/lib/kept.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { KEEP, forget, kept, keptEach } from "./kept.ts";

test("a read is made once and kept, until forgotten", async () => {
  let reads = 0;
  const read = () => Promise.resolve(++reads);
  assert.equal(await kept(1, "lines", read), 1);
  assert.equal(await kept(1, "lines", read), 1);
  assert.equal(await kept(2, "lines", read), 2);
  forget(1, (key) => key === "lines");
  assert.equal(await kept(1, "lines", read), 3);
  assert.equal(await kept(2, "lines", read), 2);
  forget(null);
  assert.equal(await kept(2, "lines", read), 4);
});

test("a read that fails isn't kept", async () => {
  let reads = 0;
  /** @returns {Promise<number>} */
  const read = () => (++reads === 1 ? Promise.reject(new Error("no")) : Promise.resolve(reads));
  await assert.rejects(kept(10, "marks", read));
  assert.equal(await kept(10, "marks", read), 2);
});

test("only the last few VODs are kept", async () => {
  let reads = 0;
  const read = () => Promise.resolve(++reads);
  for (let id = 100; id < 100 + KEEP + 1; id++) await kept(id, "lines", read);
  // The first was let go; the second still kept.
  assert.equal(await kept(101, "lines", read), 2);
  assert.equal(await kept(100, "lines", read), KEEP + 2);
});

test("of several, only those not kept are read, together", async () => {
  /** @type {number[][]} */
  const asked = [];
  /** @param {number[]} missing */
  const read = (missing) => (asked.push(missing), Promise.resolve(missing.map((i) => `w${i}`)));
  assert.deepEqual(await keptEach(200, ["a", "b"], read), ["w0", "w1"]);
  assert.deepEqual(await keptEach(200, ["b", "c", "a"], read), ["w1", "w1", "w0"]);
  assert.deepEqual(asked, [[0, 1], [1]]);
});

// kept.ts `readsOf`: "VOD `vodId`'s reads, made the most recent (the oldest beyond KEEP let go)."
test("a VOD read again is the most recent, so it isn't the one let go", async () => {
  let reads = 0;
  const read = () => Promise.resolve(++reads);
  for (let id = 300; id < 300 + KEEP; id++) await kept(id, "lines", read);
  await kept(300, "lines", read);
  await kept(400, "lines", read);
  const before = reads;
  assert.equal(await kept(300, "lines", read), 1, "300 was read again, so it stayed");
  assert.equal(reads, before);
  await kept(301, "lines", read);
  assert.equal(reads, before + 1, "301, the oldest, was let go");
});

// kept.ts header: "A read that fails isn't kept." keptEach: the batch's failure is each key's.
test("of several read together, a failure isn't kept for any of them", async () => {
  let reads = 0;
  /** @param {number[]} missing @returns {Promise<string[]>} */
  const read = (missing) =>
    ++reads === 1 ? Promise.reject(new Error("no")) : Promise.resolve(missing.map((i) => `v${i}`));
  await assert.rejects(keptEach(500, ["a", "b"], read));
  assert.deepEqual(await keptEach(500, ["a", "b"], read), ["v0", "v1"]);
});

// kept.ts `forget`: "whose key `which` picks"; a VOD's other reads stay.
test("forgetting picks the reads by key and leaves the VOD's others kept", async () => {
  let reads = 0;
  const read = () => Promise.resolve(++reads);
  await kept(600, "lines", read);
  await kept(600, "marks", read);
  forget(600, (key) => key === "marks");
  assert.equal(await kept(600, "lines", read), 1);
  assert.equal(await kept(600, "marks", read), 3);
});

// kept.ts `keptEach`: of several, "those not kept read together": none missing, none read.
test("of several all kept, nothing is read", async () => {
  /** @type {number[][]} */
  const asked = [];
  /** @param {number[]} missing */
  const read = (missing) => (asked.push(missing), Promise.resolve(missing.map((i) => `w${i}`)));
  await keptEach(700, ["a"], read);
  await keptEach(700, ["a"], read);
  assert.deepEqual(asked, [[0]]);
});

// kept.ts header: "A read that fails isn't kept": a failure of a read let go of meanwhile leaves the newer read kept.
test("a read that fails after a newer one was kept doesn't let the newer go", async () => {
  /** @type {(e: Error) => void} */
  let fail = () => {};
  const old = new Promise((_, reject) => (fail = reject));
  const first = kept(800, "lines", () => old);
  forget(800);
  let reads = 0;
  assert.equal(await kept(800, "lines", () => Promise.resolve(++reads)), 1);
  fail(new Error("late"));
  await assert.rejects(first);
  assert.equal(await kept(800, "lines", () => Promise.resolve(++reads)), 1, "kept still");
  assert.equal(reads, 1);
});
