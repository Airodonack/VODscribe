// Performance budgets (tests/e2e/budgets.json): for each platform, the most
// each timed step of the perf-* scenarios may take, in ms. A scenario takes
// each measure several times and is judged on the median (Measures, below);
// perf-compare.mjs --budgets judges a whole run's perf.json the same way.
//
// budgets.json: { "<platform>": { machine, build, measured, scenarios: {
//   "<scenario>": { "<step>": { budget, measured, from } } } } }, the
// platform as node names it (linux, darwin, win32). A step with no budget
// for the platform is logged, never failed.

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
export const BUDGETS = path.join(here, "..", "budgets.json");

/** The middle value (the mean of the two middle ones for an even count), rounded to the ms. */
export function median(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length / 2;
  return sorted.length % 2 ? sorted[Math.floor(mid)] : Math.round((sorted[mid - 1] + sorted[mid]) / 2);
}

/** The `p`th percentile (0 to 100) by nearest rank: a value that was measured, at or above p% of them. */
export function percentile(values, p) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length, Math.max(1, Math.ceil((p / 100) * sorted.length))) - 1];
}

/** The budgets file's entry for `platform`, or null when it has none. */
export function budgetsFor(doc, platform) {
  return doc?.[platform]?.scenarios ? doc[platform] : null;
}

/**
 * Each step of `scenario` measured (`medians`: step → ms) against its
 * budget in `budgets` (budgetsFor's), times `scale`: { step, ms, budget,
 * over } in the order measured; budget null where there's none.
 */
export function judge(budgets, scenario, medians, scale = 1) {
  const mine = budgets?.scenarios?.[scenario] ?? {};
  return Object.entries(medians).map(([step, ms]) => {
    const limit = mine[step]?.budget;
    const budget = typeof limit === "number" ? Math.round(limit * scale) : null;
    return { step, ms, budget, over: budget !== null && ms > budget };
  });
}

/**
 * A run's perf.json against the budgets file: every scenario's step with a
 * budget on the run's platform (perf.json's `on.platform`), its median
 * across the run's repeats against the budget. Returns { platform, rows:
 * [{ scenario, step, ms, budget, over }], unbudgeted: ["<scenario>: <step>"]
 * of the perf-* scenarios' own steps without one }.
 */
export function againstBudgets(perf, doc, scale = 1) {
  const platform = String(perf.on?.platform ?? "").split(" ")[0];
  const budgets = budgetsFor(doc, platform);
  const rows = [];
  const unbudgeted = [];
  for (const [scenario, s] of Object.entries(perf.scenarios ?? {})) {
    const medians = Object.fromEntries(Object.entries(s.timings ?? {}).map(([step, t]) => [step, t.median]));
    for (const row of judge(budgets, scenario, medians, scale)) {
      if (row.budget !== null) rows.push({ scenario, ...row });
      else if (scenario.startsWith("perf-") && !HARNESS_STEPS.test(row.step))
        unbudgeted.push(`${scenario}: ${row.step}`);
    }
  }
  return { platform, rows, unbudgeted };
}

/** The steps every scenario's start or transcription times (run.mjs, App): not a perf scenario's own. */
const HARNESS_STEPS = /^((re)?start to first screen|transcribed \()/;

/** budgets.json as committed (or `file`). */
export function loadBudgets(file = BUDGETS) {
  return JSON.parse(readFileSync(file, "utf8"));
}

/**
 * What a perf scenario measures: each sample kept with the scenario's
 * timings (ctx.timing: results.json and perf.json get each step's median),
 * then judged at the end against the budgets, the misses failing the
 * scenario with their numbers. E2E_BUDGET_SCALE multiplies every budget
 * (a slower computer than the one they were set on). Under --parallel the
 * numbers are taken beside other apps, so they're logged, not judged.
 */
export class Measures {
  constructor(ctx, scenario) {
    this.ctx = ctx;
    this.scenario = scenario;
    this.samples = new Map();
  }

  /** One sample of `step` (ms), taken between `from` and `to` on ctx.clock (the profile's window for it). */
  add(step, ms, from, to) {
    this.ctx.timing(step, ms, from, to);
    this.samples.set(step, [...(this.samples.get(step) ?? []), ms]);
  }

  /** Each step's median against its budget, logged; throws naming the misses. */
  check() {
    const medians = Object.fromEntries([...this.samples].map(([step, ms]) => [step, median(ms)]));
    const scale = Number(process.env.E2E_BUDGET_SCALE || 1);
    const rows = judge(budgetsFor(loadBudgets(), process.platform), this.scenario, medians, scale);
    const said = (r) =>
      `${r.step}: ${r.ms} ms (median of ${this.samples.get(r.step).join(", ")})` +
      (r.budget === null ? ", no budget here" : `, budget ${r.budget} ms${r.over ? ": OVER" : ""}`);
    for (const r of rows) this.ctx.log(said(r));
    if (process.env.E2E_WORKER) {
      this.ctx.log("taken beside other apps (--parallel): the budgets aren't judged");
      return;
    }
    const over = rows.filter((r) => r.over);
    if (over.length) throw new Error(`over budget: ${over.map(said).join("; ")}`);
  }
}
