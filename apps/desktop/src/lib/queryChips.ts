// The search's grouping as the page shows it under the box, read from the
// tree the core parsed (crates/core query.rs `Node`): each piece a chip,
// OR between alternatives, brackets around a group inside another (as the
// core writes the tree back to text), so a surprise in how a query was read
// is there to see. And what the filter's Within control reads and writes.
// Pure: node --test apps/desktop/src/lib/queryChips.test.mjs.

import type { QueryNode } from "./api";

type Key = Extract<QueryNode, { kind: "filter" }>["key"];

export type Chip =
  /** A phrase to find; strict ones were quoted (whole words). */
  | { kind: "phrase"; text: string; strict: boolean }
  /** A phrase that mustn't be near (`-word`). */
  | { kind: "not"; text: string; strict: boolean }
  /** `text` is the filter as the core writes it, to find it in the box again. */
  | { kind: "filter"; key: Key; value: string; text: string }
  | { kind: "or" }
  | { kind: "open" }
  | { kind: "close" };

/** Whether the grouping is worth showing: more than one piece, an operator or a filter. */
export function shows(tree: QueryNode | null): tree is QueryNode {
  return tree !== null && tree.kind !== "phrase";
}

/** The filter as the core writes it: `key:value`, the value quoted if it
 *  has a space, unless it's a list of names with quotes of its own
 *  (`from:a,"Some One"`). */
export function filterText(key: Key, value: string): string {
  return /\s/.test(value) && !value.includes('"') ? `${key}:"${value}"` : `${key}:${value}`;
}

export function chipsOf(tree: QueryNode): Chip[] {
  const out: Chip[] = [];
  const walk = (node: QueryNode, inside: boolean) => {
    switch (node.kind) {
      case "phrase":
        out.push({ kind: "phrase", text: node.text, strict: node.strict });
        break;
      case "not":
        // The parser only negates phrases.
        if (node.item.kind === "phrase") out.push({ kind: "not", text: node.item.text, strict: node.item.strict });
        break;
      case "filter":
        out.push({ kind: "filter", key: node.key, value: node.value, text: filterText(node.key, node.value) });
        break;
      case "or":
      case "and":
        if (inside) out.push({ kind: "open" });
        node.items.forEach((item, i) => {
          if (i > 0 && node.kind === "or") out.push({ kind: "or" });
          walk(item, true);
        });
        if (inside) out.push({ kind: "close" });
        break;
    }
  };
  walk(tree, false);
  return out;
}

/**
 * `query` with the filter `text` taken out (the first time it's there, as a
 * whole word, any case, its value quoted or not: `from:"bob"` is the core's
 * from:bob) and the spaces round it tidied; as it was if it isn't there.
 */
export function removeFilter(query: string, text: string): string {
  const lower = query.toLowerCase();
  const colon = text.indexOf(":");
  const spellings = [
    text,
    ...(colon > 0 && !text.includes('"') ? [`${text.slice(0, colon + 1)}"${text.slice(colon + 1)}"`] : []),
  ];
  const edge = (c: string | undefined) => c === undefined || /[\s()]/.test(c);
  let found: { at: number; end: number } | null = null;
  for (const want of spellings.map((t) => t.toLowerCase())) {
    for (let at = lower.indexOf(want); at !== -1; at = lower.indexOf(want, at + 1)) {
      const end = at + want.length;
      if (!edge(query[at - 1]) || !edge(query[end])) continue;
      if (!found || at < found.at) found = { at, end };
      break;
    }
  }
  if (!found) return query;
  const before = query.slice(0, found.at).replace(/\s+$/, "");
  const after = query.slice(found.end).replace(/^\s+/, "");
  const space = before && after && !before.endsWith("(") && !after.startsWith(")") ? " " : "";
  return before + space + after;
}

/**
 * The From whom control's text, which takes names as `from:` does (the
 * core's `query::names`): separated by commas, one with a comma in quotes.
 * `done` are those before the last comma, as typed; `typing` the one after
 * it, without its quotes, for the suggestions.
 */
export function namesTyped(text: string): { done: string[]; typing: string } {
  const parts: string[] = [];
  let part = "";
  let quoted = false;
  for (const c of text) {
    if (c === "," && !quoted) {
      parts.push(part);
      part = "";
      continue;
    }
    if (c === '"') quoted = !quoted;
    part += c;
  }
  const done = parts.map((p) => p.trim()).filter((p) => p.replaceAll('"', "").trim());
  return { done, typing: part.replaceAll('"', "").trim() };
}

/** The name in the control's text, quoted if it has a comma. */
const asTyped = (name: string) => (name.includes(",") ? `"${name}"` : name);

/** The control's `text` with the name being typed replaced by `name`. */
export function withName(text: string, name: string): string {
  return [...namesTyped(text).done, asTyped(name)].join(", ");
}

/** The window when neither the filter nor the text sets another, in
 *  seconds (the core's `DEFAULT_WITHIN`, query.rs). */
export const DEFAULT_WITHIN = 20;

/** The widest window a search takes, in seconds: an hour (the core's
 *  `MOST_WITHIN`, query.rs). Wider ones made a search over common words take
 *  minutes, holding up everything else the app reads meanwhile. */
export const MOST_WITHIN = 3600;

/** The Within control's units, and how many seconds each is. */
export const UNITS = { s: 1, m: 60, h: 3600 } as const;
export type Unit = keyof typeof UNITS;

/** Seconds as the control shows them: in the largest unit they're a whole number of. */
export function splitLength(seconds: number): { n: number; unit: Unit } {
  if (seconds >= 3600 && seconds % 3600 === 0) return { n: seconds / 3600, unit: "h" };
  if (seconds >= 60 && seconds % 60 === 0) return { n: seconds / 60, unit: "m" };
  return { n: seconds, unit: "s" };
}

/** "20 s", "2 min", "1 h", for the filter's summary. */
export function lengthText(seconds: number): string {
  const { n, unit } = splitLength(seconds);
  return `${n} ${unit === "m" ? "min" : unit}`;
}
