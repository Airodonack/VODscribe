// The e2e file host's filter of published annotations against the vectors
// the app's reader is tested by (crates/core/tests/data).

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { annotationsFilter, positions } from "./fileHost.mjs";

const vectors = JSON.parse(
  readFileSync(new URL("../../../crates/core/tests/data/annotations-filter-vectors.json", import.meta.url), "utf8"),
);

test("a key's bits are the vectors'", () => {
  for (const { key, bits, hashes, positions: want } of vectors.keys)
    assert.deepEqual(positions(key, bits, hashes), want, key);
});

test("a filter is the vectors' file, byte for byte", () => {
  for (const { name, present, file } of vectors.filters)
    assert.equal(annotationsFilter(present).toString("hex"), file, name);
  const { present_prefix, present_count, file_sha256 } = vectors.large;
  const keys = Array.from({ length: present_count }, (_, i) => `${present_prefix}${i}`);
  assert.equal(createHash("sha256").update(annotationsFilter(keys)).digest("hex"), file_sha256);
});
