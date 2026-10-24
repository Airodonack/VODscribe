import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { findResults, summarize } from "./merge-results.mjs";

const ok = (name, seconds = 60) => ({ name, ok: true, seconds });

test("a run with failures and a flaky pass", () => {
  const shards = [
    { label: "e2e-a-1", results: [ok("one"), { name: "two", ok: false, error: "boom", seconds: 5 }] },
    { label: "e2e-a-2", results: [{ ...ok("three"), flaky: true, attempts: 2, firstError: "focus" }, ok("four")] },
  ];
  const { lines, ok: passed } = summarize(shards, 2);
  assert.equal(passed, false);
  assert.equal(lines[0], "e2e: 2 of 4 passed, 1 flaky, in 2 shards");
  assert.ok(lines.includes("  FAIL two [e2e-a-1] (5 s): boom"));
  assert.ok(lines.includes("  flaky three [e2e-a-2] (60 s): failed first: focus"));
});

test("all passing, with every shard there", () => {
  const { lines, ok: passed } = summarize([{ label: "s", results: [ok("one")] }], 1);
  assert.equal(passed, true);
  assert.equal(lines[0], "e2e: 1 of 1 passed, in 1 shard");
});

test("a missing shard fails the run, and so does none at all", () => {
  assert.equal(summarize([{ label: "s", results: [ok("one")] }], 2).ok, false);
  assert.equal(summarize([], 0).ok, false);
});

test("findResults reads a folder of shard folders", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "merge-"));
  for (const name of ["b-2", "a-1"]) {
    mkdirSync(path.join(dir, name));
    writeFileSync(path.join(dir, name, "results.json"), "[]");
  }
  mkdirSync(path.join(dir, "empty"));
  assert.deepEqual(
    findResults([dir]).map((f) => f.label),
    ["a-1", "b-2"],
  );
});
