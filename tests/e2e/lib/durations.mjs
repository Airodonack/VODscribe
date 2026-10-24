// How long each scenario took when it last ran (tests/e2e/durations.json,
// seconds by scenario), and the dealing of scenarios between workers or
// shards by it: the longest first, each to whichever has the least to do so
// far, so they all end at about the same time. The file is refreshed from a
// run's results by durations.mjs.

import { readFileSync } from "node:fs";

/** The durations file as { scenario: seconds }; empty if there's none. */
export function readDurations(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (e) {
    if (e.code === "ENOENT") return {};
    throw e;
  }
}

/**
 * Deals `names` into `n` lists, longest first by `durations` (ties by
 * name), each to the list with the least seconds so far (ties to the first).
 * Scenarios with no recorded time come last, in name order, each counted as
 * the median of the known ones (or one second) so they spread out too. The
 * same input always deals the same way: CI's shards rely on that.
 */
export function deal(names, durations, n) {
  if (!Number.isInteger(n) || n < 1) throw new Error(`can't deal into ${n}`);
  const known = names.filter((name) => Number.isFinite(durations[name]));
  const unknown = names.filter((name) => !Number.isFinite(durations[name])).sort((a, b) => a.localeCompare(b));
  known.sort((a, b) => durations[b] - durations[a] || a.localeCompare(b));
  const sorted = known.map((name) => durations[name]).sort((a, b) => a - b);
  const guess = sorted.length ? sorted[Math.floor((sorted.length - 1) / 2)] : 1;
  const lists = Array.from({ length: n }, () => ({ names: [], seconds: 0 }));
  for (const name of [...known, ...unknown]) {
    const least = lists.reduce((best, list) => (list.seconds < best.seconds ? list : best));
    least.names.push(name);
    least.seconds += Number.isFinite(durations[name]) ? durations[name] : guess;
  }
  return lists;
}

/** "--shard 2/3" as { index: 2, count: 3 }. */
export function parseShard(text) {
  const match = /^(\d+)\/(\d+)$/.exec(text ?? "");
  const index = match && Number(match[1]);
  const count = match && Number(match[2]);
  if (!match || count < 1 || index < 1 || index > count) throw new Error(`--shard ${text}: i/N, 1 ≤ i ≤ N`);
  return { index, count };
}

/**
 * `durations` refreshed from runs' results (results.json's entries): each
 * scenario that passed gets its seconds (the median, if it ran more than
 * once), and so does one skipped part way (drag-out, when the window manager
 * takes its gesture); a failed one keeps what it had (a failure stops early
 * or waits out a timeout), and so does one skipped at once (the network's,
 * offline). Scenarios not in `names` (removed since) are dropped. Keys
 * sorted, for a quiet diff.
 */
export function refreshed(durations, results, names) {
  const seconds = new Map();
  for (const r of results) {
    if (!r.ok || !Number.isFinite(r.seconds) || (r.skipped && r.seconds < 1)) continue;
    seconds.set(r.name, [...(seconds.get(r.name) ?? []), r.seconds]);
  }
  const next = { ...durations };
  for (const [name, values] of seconds) {
    const sorted = values.sort((a, b) => a - b);
    const mid = sorted.length / 2;
    next[name] = sorted.length % 2 ? sorted[Math.floor(mid)] : Math.round((sorted[mid - 1] + sorted[mid]) / 2);
  }
  return Object.fromEntries(
    Object.keys(next)
      .filter((name) => names.includes(name))
      .sort((a, b) => a.localeCompare(b))
      .map((name) => [name, next[name]]),
  );
}
