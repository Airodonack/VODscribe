// What a stylesheet uses, for compare.mjs: which of it each engine takes.

/**
 * What a stylesheet's text uses: its declarations (property, value; the
 * custom properties left out), its selectors (each of a list) and its media
 * queries, each once. A plain scanner, enough for the app's built CSS:
 * strings, parentheses and nesting kept track of.
 */
export function cssUsed(text) {
  text = text.replace(/\/\*[\s\S]*?\*\//g, "");
  const decls = new Map();
  const selectors = new Set();
  const medias = new Set();
  let i = 0;
  // Reads up to the first of `stops` at depth 0 (outside strings and parentheses).
  const scan = (stops) => {
    let depth = 0;
    let from = i;
    for (; i < text.length; i++) {
      const c = text[i];
      if (c === '"' || c === "'") {
        const q = c;
        for (i++; i < text.length && text[i] !== q; i++) if (text[i] === "\\") i++;
      } else if (c === "(" || c === "[") depth++;
      else if (c === ")" || c === "]") depth--;
      else if (depth === 0 && stops.includes(c)) break;
    }
    return text.slice(from, i);
  };
  const splitTop = (s) => {
    const parts = [];
    let depth = 0;
    let from = 0;
    for (let k = 0; k < s.length; k++) {
      if (s[k] === "(" || s[k] === "[") depth++;
      else if (s[k] === ")" || s[k] === "]") depth--;
      else if (s[k] === "," && depth === 0) {
        parts.push(s.slice(from, k));
        from = k + 1;
      }
    }
    parts.push(s.slice(from));
    return parts.map((p) => p.trim()).filter(Boolean);
  };
  // A block's contents: declarations and rules, to its closing brace.
  const body = (declarations) => {
    while (i < text.length) {
      const item = scan(declarations ? "{};" : "{}").trim();
      const c = text[i];
      if (c === "{") {
        i++;
        rule(item);
      } else {
        i++;
        if (item && declarations) {
          const colon = item.indexOf(":");
          const prop = item.slice(0, colon).trim().toLowerCase();
          const value = item.slice(colon + 1).trim();
          if (colon > 0 && !prop.startsWith("--") && !decls.has(`${prop}: ${value}`))
            decls.set(`${prop}: ${value}`, [prop, value]);
        }
        if (c === "}") return;
      }
    }
  };
  const rule = (prelude) => {
    if (prelude.startsWith("@")) {
      const [, name, cond] = prelude.match(/^@([\w-]+)\s*([\s\S]*)$/) ?? [];
      if (name === "media") medias.add(cond.trim());
      // @keyframes' "from", "50%": their declarations count; their names aren't selectors.
      if (name === "keyframes" || name === "-webkit-keyframes") {
        while (i < text.length) {
          scan("{}");
          if (text[i] === "}") return void i++;
          i++;
          body(true);
        }
        return;
      }
      return body(["font-face", "page", "property", "counter-style"].includes(name) || false);
    }
    for (const s of splitTop(prelude)) selectors.add(s);
    body(true);
  };
  while (i < text.length) {
    const prelude = scan("{;").trim();
    if (i >= text.length) break;
    if (text[i] === ";") {
      i++;
      continue;
    }
    i++;
    rule(prelude);
  }
  return { decls: [...decls.values()], selectors: [...selectors], medias: [...medias] };
}
