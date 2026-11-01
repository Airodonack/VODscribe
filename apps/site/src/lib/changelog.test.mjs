// The releases page's changelog reading (changelog.ts): node --test apps/site/src/lib/changelog.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { counts, countsText, longDate, releases, runs, sentences } from "./changelog.ts";
import { DOWNLOADS, onHost, versionedUrl } from "./downloads.ts";

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

test("the text before the changes is the lead, a line of it at a time, and the unreleased one's is left out", () => {
  const md = [
    "## 0.3.0 (unreleased)",
    "",
    "Not out yet.",
    "",
    "- Not yet.",
    "## 0.2.1 — 2026-10-20",
    "",
    "What's new, as you'd tell a friend.",
    "",
    "- One.",
  ].join("\n");
  assert.deepEqual(releases(md), [
    {
      version: "0.2.1",
      date: "2026-10-20",
      intro: ["What's new, as you'd tell a friend."],
      changes: [{ text: "One.", items: [] }],
    },
  ]);
});

test("a change splits after its first sentence: a full stop, a space and a capital", () => {
  assert.deepEqual(sentences("One thing. And another. A third."), {
    first: "One thing.",
    rest: "And another. A third.",
  });
  assert.deepEqual(sentences("Just one sentence."), { first: "Just one sentence.", rest: "" });
  // Not before a small letter, a quote's full stop, or inside a code run.
  assert.deepEqual(sentences('Says "Press Play." under it. Then more.'), {
    first: 'Says "Press Play." under it.',
    rest: "Then more.",
  });
  assert.deepEqual(sentences("Version 0.5.0. Turning it off."), { first: "Version 0.5.0.", rest: "Turning it off." });
  assert.deepEqual(sentences("Type `a. B` to see. It works."), { first: "Type `a. B` to see.", rest: "It works." });
  assert.deepEqual(sentences("No full stop at all"), { first: "No full stop at all", rest: "" });
  // The halves still carry their code runs.
  assert.deepEqual(runs(sentences("Search for `x`. Then `y`.").rest), [
    { code: false, text: "Then " },
    { code: true, text: "y" },
    { code: false, text: "." },
  ]);
});

test("counts: the changes besides Fixes:, and the fixes under it", () => {
  const md = ["## 0.2.1 — 2026-10-20", "- New.", "- Newer.", "- Fixes:", "  - One.", "  - Two.", "  - Three."].join(
    "\n",
  );
  const [r] = releases(md);
  assert.deepEqual(counts(r), { changes: 2, fixes: 3 });
  assert.equal(countsText(r), "2 changes, 3 fixes");
  assert.equal(countsText(releases("## 0.1.1 — 2026-10-12\n- Fixes:\n  - One.")[0]), "1 fix");
  assert.equal(countsText(releases("## 0.1.0 — 2026-10-12\n- First.")[0]), "1 change");
  assert.equal(countsText(releases("## 0.1.0 — 2026-10-12\nFirst release.")[0]), "");
});

test("each version's own downloads, from the versions on the host", () => {
  assert.equal(
    versionedUrl(DOWNLOADS[0], "0.7.0"),
    "https://dl.vodscribe.com/releases/0.7.0/VODscribe-0.7.0-windows-x64-setup.exe",
  );
  assert.deepEqual(
    DOWNLOADS.map((d) => versionedUrl(d, "0.4.0").split("/").pop()),
    [
      "VODscribe-0.4.0-windows-x64-setup.exe",
      "VODscribe-0.4.0-windows-x64.msi",
      "VODscribe-0.4.0-macos-arm64.dmg",
      "VODscribe-0.4.0-macos-x64.dmg",
      "VODscribe-0.4.0-linux-x64.deb",
      "VODscribe-0.4.0-linux-x64.rpm",
    ],
  );
  assert.deepEqual(["0.1.0", "0.1.2", "0.1.3", "0.2.0", "0.10.0", "1.0.0"].map(onHost), [
    false,
    false,
    true,
    true,
    true,
    true,
  ]);
});

test("the real changelog's 0.7.0 and 0.6.0 open with a lead", () => {
  const list = releases(readFileSync(new URL("../../../../CHANGELOG.md", import.meta.url), "utf8"));
  for (const v of ["0.7.0", "0.6.0"]) assert.ok(list.find((r) => r.version === v)?.intro.length, v);
});
