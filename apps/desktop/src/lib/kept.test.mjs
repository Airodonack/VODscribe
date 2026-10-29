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
