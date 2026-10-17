// Fallbacks for color-mix(), which macOS 12's WKWebView lacks (its Safari
// 17.6 has it; the system web view the app gets doesn't). For each rule that
// uses it, a copy follows inside `@supports not (color-mix)` with each mix
// replaced: a mix with transparent by the same colour at that alpha,
// `rgb(var(--x-rgb) / a)`; a mix of two colours by the larger share. The
// `--x-rgb` triplets are made here from every `--x: #hex` token, so they
// can't drift from the colours. A plain earlier declaration wouldn't do:
// a value with var() in it only fails once the variables are filled in, and
// then the property is unset rather than falling back.
//
// Used as a Svelte style preprocessor (component styles are compiled into
// the JS, so Vite's CSS pipeline never sees them) and as a Vite transform for
// the stylesheets it does load (app.css).

import postcss from "postcss";

const SUPPORTED = "(color: color-mix(in srgb, red, red))";

/** `#rgb` or `#rrggbb` as "r g b", else null.
 * @param {string} value */
function rgbOf(value) {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value.trim())?.[1];
  if (!hex) return null;
  const full = hex.length === 3 ? [...hex].map((c) => c + c).join("") : hex;
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16)).join(" ");
}

/** `text` split at its top-level commas.
 * @param {string} text */
function args(text) {
  const out = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "(") depth++;
    else if (text[i] === ")") depth--;
    else if (text[i] === "," && depth === 0) {
      out.push(text.slice(start, i).trim());
      start = i + 1;
    }
  }
  out.push(text.slice(start).trim());
  return out;
}

/** A colour and its share, from "X 40%" (no share: half).
 * @param {string} part */
function share(part) {
  const m = /^(.*?)\s+(\d+(?:\.\d+)?)%$/.exec(part);
  return m ? { color: m[1].trim(), percent: Number(m[2]) } : { color: part, percent: 50 };
}

/** `color` at `alpha` without color-mix, or null if that can't be said.
 * @param {string} color
 * @param {number} alpha */
function faded(color, alpha) {
  const token = /^var\((--[\w-]+)\)$/.exec(color)?.[1];
  if (token) return `rgb(var(${token}-rgb) / ${alpha})`;
  const rgb = rgbOf(color);
  if (rgb) return `rgb(${rgb} / ${alpha})`;
  return null;
}

/** One color-mix's arguments ("in srgb, A p%, B") as a colour without it.
 * @param {string} inner */
function unmixed(inner) {
  const [, first, second] = args(inner);
  if (!first || !second) return null;
  const a = share(first);
  const b = share(second);
  // Only one share given: the other is the rest.
  const pa = /%$/.test(first) ? a.percent : 100 - b.percent;
  if (b.color === "transparent") return faded(a.color, +(pa / 100).toFixed(3)) ?? (pa >= 50 ? a.color : "transparent");
  if (a.color === "transparent")
    return faded(b.color, +((100 - pa) / 100).toFixed(3)) ?? (pa < 50 ? b.color : "transparent");
  return pa >= 50 ? a.color : b.color;
}

/** `value` with each color-mix() replaced, or null if one can't be.
 * @param {string} value */
function fallbackOf(value) {
  let out = "";
  let at = 0;
  for (;;) {
    const start = value.indexOf("color-mix(", at);
    if (start < 0) return out + value.slice(at);
    let depth = 0;
    let end = start + "color-mix".length;
    for (; end < value.length; end++) {
      if (value[end] === "(") depth++;
      else if (value[end] === ")" && --depth === 0) break;
    }
    const color = unmixed(value.slice(start + "color-mix(".length, end));
    if (color === null) return null;
    out += value.slice(at, start) + color;
    at = end + 1;
  }
}

/** `css` with its colour tokens' rgb triplets and color-mix fallbacks added.
 * @param {string} css */
export function colorMixFallbacks(css) {
  if (!css.includes("color-mix(") && !/--[\w-]+\s*:\s*#/.test(css)) return css;
  const root = postcss.parse(css);
  /** @type {Map<import("postcss").Rule, import("postcss").Declaration[]>} */
  const fallbacks = new Map();
  root.walkDecls((decl) => {
    if (decl.prop.startsWith("--") && !decl.prop.endsWith("-rgb")) {
      const rgb = rgbOf(decl.value);
      if (rgb) decl.cloneAfter({ prop: `${decl.prop}-rgb`, value: rgb });
    }
    if (!decl.value.includes("color-mix(") || decl.parent?.type !== "rule") return;
    const value = fallbackOf(decl.value);
    if (value === null) return;
    const rule = /** @type {import("postcss").Rule} */ (decl.parent);
    fallbacks.set(rule, [...(fallbacks.get(rule) ?? []), decl.clone({ value })]);
  });
  for (const [rule, decls] of fallbacks) {
    const supports = postcss.atRule({ name: "supports", params: `not ${SUPPORTED}` });
    supports.append(postcss.rule({ selector: rule.selector }).append(decls));
    rule.after(supports);
  }
  return root.toString();
}

/** The Svelte preprocessor: each component's styles.
 * @type {import("svelte/compiler").PreprocessorGroup} */
export const colorMixStyles = { name: "color-mix", style: ({ content }) => ({ code: colorMixFallbacks(content) }) };

/** The Vite plugin: stylesheets loaded as CSS (app.css).
 * @returns {import("vite").Plugin} */
export function colorMix() {
  return {
    name: "color-mix",
    enforce: "pre",
    /** @param {string} code @param {string} id */
    transform(code, id) {
      if (!/\.css(\?|$)/.test(id) || id.includes("node_modules")) return null;
      return { code: colorMixFallbacks(code), map: null };
    },
  };
}
