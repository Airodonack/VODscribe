import assert from "node:assert/strict";
import { test } from "node:test";
import { colorMixFallbacks } from "./colorMix.js";

const SUPPORTS = "@supports not (color: color-mix(in srgb, red, red))";

test("colour tokens get rgb triplets beside them", () => {
  const out = colorMixFallbacks(":root { --accent: #4f46e5; --white: #fff; --gap: 4px; }");
  assert.match(out, /--accent-rgb: 79 70 229;/);
  assert.match(out, /--white-rgb: 255 255 255;/);
  assert.doesNotMatch(out, /--gap-rgb/);
});

test("a mix with transparent falls back to the same colour at that alpha", () => {
  const out = colorMixFallbacks(".a { background: color-mix(in srgb, var(--accent) 18%, transparent); }");
  assert.ok(out.includes(`${SUPPORTS} {`), out);
  assert.match(out, /\.a \{ background: rgb\(var\(--accent-rgb\) \/ 0\.18\); \}/);
  // The original stays first, for web views that have color-mix.
  assert.ok(out.indexOf("color-mix(in srgb, var(--accent)") < out.indexOf(SUPPORTS));
});

test("a hex colour mixed with transparent becomes rgb at that alpha", () => {
  const out = colorMixFallbacks(".a { box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 38%, transparent); }");
  assert.match(out, /box-shadow: inset 0 0 0 1px rgb\(0 0 0 \/ 0\.38\)/);
});

test("a mix of two colours falls back to the larger share", () => {
  const out = colorMixFallbacks(
    ".a { border-color: color-mix(in srgb, var(--accent) 55%, var(--border)); color: color-mix(in srgb, var(--event) 18%, var(--text)); }",
  );
  assert.match(out, /border-color: var\(--accent\);/);
  assert.match(out, /color: var\(--text\);/);
});

test("custom properties get their fallback in the @supports block too", () => {
  const out = colorMixFallbacks(".bar { --unplayed: color-mix(in srgb, var(--muted) 50%, transparent); }");
  assert.match(out, /\.bar \{ --unplayed: rgb\(var\(--muted-rgb\) \/ 0\.5\); \}/);
});

test("rules inside @media keep their fallback inside it", () => {
  const out = colorMixFallbacks(
    "@media (max-width: 9px) { .b { color: color-mix(in srgb, var(--accent) 60%, transparent); } }",
  );
  const media = out.indexOf("@media");
  const supports = out.indexOf(SUPPORTS);
  assert.ok(media >= 0 && supports > media, out);
  assert.ok(out.lastIndexOf("}") > supports);
});

test("a colour that can't be faded falls back to transparent below half", () => {
  const out = colorMixFallbacks(".s { border: 2px solid color-mix(in srgb, currentColor 22%, transparent); }");
  assert.match(out, /border: 2px solid transparent;/);
});

test("stylesheets without color-mix or tokens come back unchanged", () => {
  const css = ".a { color: red; }";
  assert.equal(colorMixFallbacks(css), css);
});
