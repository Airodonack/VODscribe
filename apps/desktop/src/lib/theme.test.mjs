// The dark colors are listed twice in app.css, for Dark and for System on a
// dark system: node --test apps/desktop/src/lib/theme.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const css = readFileSync(new URL("../app.css", import.meta.url), "utf8");
/** @param {string} selector */
const colors = (selector) => {
  const start = css.indexOf(`${selector} {`);
  assert.ok(start >= 0, `${selector} is in app.css`);
  const body = css.slice(start, css.indexOf("}", start));
  return [...body.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((m) => `${m[1]}: ${m[2].trim()}`);
};

test("Dark and System's dark have the same colors", () => {
  const dark = colors(':root[data-theme="dark"]');
  assert.ok(dark.length > 10);
  assert.deepEqual(colors(':root:not([data-theme="light"])'), dark);
});
