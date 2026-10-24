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

/** The filter as the core writes it: `key:value`, the value quoted if it has a space. */
export function filterText(key: Key, value: string): string {
  return /\s/.test(value) ? `${key}:"${value}"` : `${key}:${value}`;
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
 * whole word, any case) and the spaces round it tidied; as it was if it
 * isn't there.
 */
export function removeFilter(query: string, text: string): string {
  const lower = query.toLowerCase();
  const want = text.toLowerCase();
  const edge = (c: string | undefined) => c === undefined || /[\s()]/.test(c);
  for (let at = lower.indexOf(want); at !== -1; at = lower.indexOf(want, at + 1)) {
    const end = at + want.length;
    if (!edge(query[at - 1]) || !edge(query[end])) continue;
    const before = query.slice(0, at).replace(/\s+$/, "");
    const after = query.slice(end).replace(/^\s+/, "");
    const space = before && after && !before.endsWith("(") && !after.startsWith(")") ? " " : "";
    return before + space + after;
  }
  return query;
}

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
