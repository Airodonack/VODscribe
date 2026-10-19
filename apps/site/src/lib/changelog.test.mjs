// The releases page's changelog reading (changelog.ts): node --test apps/site/src/lib/changelog.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { longDate, releases, runs } from "./changelog.ts";

test("reads versions, dates and changes, and leaves out an unreleased one", () => {
  const md = [
    "# Changelog",
    "Intro text.",
    "## 0.3.0 (unreleased)",
    "- Not yet.",
    "## 0.2.1 — 2026-10-20",
    "- One.",
    "- Two, wrapped",
    "  onto a second line.",
    "## 0.2.0 — 2026-10-13",
    "- Three.",
    "## 0.1.0 — 2026-10-12",
    "",
    "First release.",
  ].join("\n");
  assert.deepEqual(releases(md), [
    {
      version: "0.2.1",
      date: "2026-10-20",
      intro: [],
      changes: [
        { text: "One.", items: [] },
        { text: "Two, wrapped onto a second line.", items: [] },
      ],
    },
    { version: "0.2.0", date: "2026-10-13", intro: [], changes: [{ text: "Three.", items: [] }] },
    { version: "0.1.0", date: "2026-10-12", intro: ["First release."], changes: [] },
  ]);
});

test("items under a change, each of which can wrap", () => {
  const md = ["## 0.2.1 — 2026-10-20", "- New.", "- Fixes:", "  - One fix,", "    wrapped.", "  - Another."].join("\n");
  assert.deepEqual(releases(md)[0].changes, [
    { text: "New.", items: [] },
    { text: "Fixes:", items: ["One fix, wrapped.", "Another."] },
  ]);
});

test("the real changelog: released versions, newest first, each with something to say", () => {
  const list = releases(readFileSync(new URL("../../../../CHANGELOG.md", import.meta.url), "utf8"));
  assert.ok(list.length >= 4);
  const key = (/** @type {string} */ v) =>
    v
      .split(".")
      .map((n) => n.padStart(4, "0"))
      .join(".");
  for (let i = 1; i < list.length; i++) assert.ok(key(list[i - 1].version) > key(list[i].version), list[i].version);
  for (const r of list) assert.ok(r.changes.length + r.intro.length > 0, r.version);
});

test("code runs and dates", () => {
  assert.deepEqual(runs("Names like `VODscribe-0.1.3.exe` now."), [
    { code: false, text: "Names like " },
    { code: true, text: "VODscribe-0.1.3.exe" },
    { code: false, text: " now." },
  ]);
  assert.equal(longDate("2026-10-13"), "October 13, 2026");
});
