// Refreshes tests/e2e/durations.json, the seconds each scenario took when it
// last passed, from runs' results: what run.mjs --parallel and --shard deal
// scenarios by (lib/durations.mjs).
//
//   node tests/e2e/durations.mjs <out dir>...
//
// Each <out dir> is one with a results.json (E2E_OUT, a cell's or a
// runner's). A scenario that ran in several gets the median of its times;
// one that failed (or was skipped at once) everywhere keeps the time it had.

import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { readDurations, refreshed } from "./lib/durations.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const dirs = process.argv.slice(2);
if (!dirs.length) {
  console.error("usage: node tests/e2e/durations.mjs <out dir>...");
  process.exit(2);
}
const file = path.join(here, "durations.json");
const names = readdirSync(path.join(here, "scenarios"))
  .filter((f) => f.endsWith(".mjs"))
  .map((f) => f.slice(0, -4));
const results = dirs.flatMap((dir) => {
  const json = path.join(dir, "results.json");
  if (!existsSync(json)) throw new Error(`no results.json in ${dir}`);
  return JSON.parse(readFileSync(json, "utf8"));
});
const before = readDurations(file);
const after = refreshed(before, results, names);
writeFileSync(file, `${JSON.stringify(after, null, 2)}\n`);
const changed = Object.keys(after).filter((name) => before[name] !== after[name]);
for (const name of changed) console.log(`  ${name}: ${before[name] ?? "none"} → ${after[name]} s`);
const missing = names.filter((name) => !(name in after));
console.log(
  `durations: ${changed.length} changed, ${Object.keys(after).length} known` +
    (missing.length ? `; none yet for ${missing.join(", ")}` : ""),
);
