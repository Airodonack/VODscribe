// Compares two runs' timings and memory (run.mjs writes perf.json): each
// step both timed, its median before and after, and whether it moved beyond the
// spread the runs had (each run's least to most) by more than a threshold.
// A step that did is a regression (or a gain) worth a look; one within the
// spread is noise, whatever its median says.
//
//   node tests/e2e/perf-compare.mjs <before perf.json> <after perf.json> [--threshold 10]
//
// Exits 1 when a step got slower (or memory grew) beyond its spread and the
// threshold (%).

import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";

const { values: opts, positionals } = parseArgs({
  allowPositionals: true,
  options: { threshold: { type: "string", default: "10" } },
});
const [beforeFile, afterFile] = positionals;
if (!beforeFile || !afterFile) {
  console.error("usage: node tests/e2e/perf-compare.mjs <before perf.json> <after perf.json> [--threshold 10]");
  process.exit(2);
}
const threshold = Number(opts.threshold) / 100;
const [before, after] = [beforeFile, afterFile].map((f) => JSON.parse(readFileSync(f, "utf8")));

for (const [label, run] of [
  ["before", before],
  ["after", after],
])
  console.log(
    `${label}: ${run.commit ?? "?"} on ${run.date?.slice(0, 10)}, ${run.repeat} run(s)${run.profiled ? ", PROFILED" : ""}; ` +
      `${run.on?.cpu}, WebKit ${run.on?.webkit ?? "?"}, ${run.on?.display}`,
  );
if (before.profiled || after.profiled)
  console.log("(a profiled run's times are slowed by perf: compare unprofiled runs)");
console.log("");

const rows = [["scenario", "step", "before", "after", "change", ""]];
let worse = 0;
const compare = (scenario, step, a, b, unit) => {
  const change = (b.median - a.median) / Math.max(a.median, 1);
  // Beyond the spread: after's whole range clear of before's, and by more than the threshold.
  const beyond = Math.abs(change) > threshold && (b.least > a.most || b.most < a.least);
  const verdict = beyond ? (change > 0 ? "WORSE" : "better") : "";
  if (verdict === "WORSE") worse += 1;
  const show = (t) => `${t.median} ${unit} (${t.least}–${t.most})`;
  rows.push([scenario, step, show(a), show(b), `${change >= 0 ? "+" : ""}${(change * 100).toFixed(0)}%`, verdict]);
};
for (const [scenario, b] of Object.entries(after.scenarios ?? {})) {
  const a = before.scenarios?.[scenario];
  if (!a) continue;
  for (const [step, t] of Object.entries(b.timings ?? {}))
    if (a.timings?.[step]) compare(scenario, step, a.timings[step], t, "ms");
  if (a.memory && b.memory) compare(scenario, "memory at the end (Pss)", a.memory, b.memory, "MiB");
}
const widths = rows[0].map((_, i) => Math.max(...rows.map((r) => r[i].length)));
for (const r of rows)
  console.log(
    r
      .map((c, i) => c.padEnd(widths[i]))
      .join("  ")
      .trimEnd(),
  );
const only = (x, y) =>
  Object.entries(x.scenarios ?? {}).flatMap(([s, v]) =>
    Object.keys(v.timings ?? {})
      .filter((step) => !y.scenarios?.[s]?.timings?.[step])
      .map((step) => `${s}: ${step}`),
  );
if (only(before, after).length) console.log(`\nonly before: ${only(before, after).join("; ")}`);
if (only(after, before).length) console.log(`only after: ${only(after, before).join("; ")}`);
process.exit(worse ? 1 : 0);
