// The budget comparison (lib/budgets.mjs): medians, percentiles, a
// scenario's steps judged, and a run's perf.json against a budgets file.

import assert from "node:assert/strict";
import test from "node:test";

import { againstBudgets, budgetsFor, judge, loadBudgets, median, percentile } from "./budgets.mjs";

const doc = {
  linux: {
    machine: "a test computer",
    scenarios: {
      "perf-player": { "open to first frame": { budget: 200 }, "seek to frame shown": { budget: 60 } },
      "perf-library": { "transcript scroll, frame gap p95": { budget: 30 } },
    },
  },
};

test("median: the middle one, or the two middle ones' mean", () => {
  assert.equal(median([5, 1, 3]), 3);
  assert.equal(median([4, 1, 3, 2]), 3);
  assert.equal(median([10, 11]), 11);
  assert.equal(median([]), null);
});

test("percentile: by nearest rank, a value that was measured", () => {
  const values = Array.from({ length: 100 }, (_, i) => i + 1);
  assert.equal(percentile(values, 95), 95);
  assert.equal(percentile(values, 100), 100);
  assert.equal(percentile(values, 0), 1);
  assert.equal(percentile([16, 17, 16, 120], 95), 120);
  assert.equal(percentile([16.6, 16.7, 50], 50), 16.7);
  assert.equal(percentile([], 95), null);
});

test("budgetsFor: the platform's entry, or none", () => {
  assert.equal(budgetsFor(doc, "linux").machine, "a test computer");
  assert.equal(budgetsFor(doc, "darwin"), null);
  assert.equal(budgetsFor(null, "linux"), null);
});

test("judge: over only beyond the budget, a step without one never over", () => {
  const rows = judge(budgetsFor(doc, "linux"), "perf-player", {
    "open to first frame": 200,
    "seek to frame shown": 61,
    "seek: time shown": 900,
  });
  assert.deepEqual(rows, [
    { step: "open to first frame", ms: 200, budget: 200, over: false },
    { step: "seek to frame shown", ms: 61, budget: 60, over: true },
    { step: "seek: time shown", ms: 900, budget: null, over: false },
  ]);
});

test("judge: a scale for a slower computer, and no budgets at all", () => {
  const [row] = judge(budgetsFor(doc, "linux"), "perf-player", { "seek to frame shown": 61 }, 1.5);
  assert.deepEqual(row, { step: "seek to frame shown", ms: 61, budget: 90, over: false });
  assert.deepEqual(judge(null, "perf-player", { "seek to frame shown": 61 }), [
    { step: "seek to frame shown", ms: 61, budget: null, over: false },
  ]);
});

test("againstBudgets: a run's medians on its own platform", () => {
  const perf = {
    on: { platform: "linux 6.12.113+deb13-amd64" },
    scenarios: {
      "perf-player": {
        timings: {
          "open to first frame": { median: 150, least: 120, most: 400 },
          "seek to frame shown": { median: 75, least: 40, most: 80 },
          "seek: time shown": { median: 30 },
          "start to first screen": { median: 1100 },
        },
      },
      "perf-library": { timings: { "transcript scroll, frame gap p95": { median: 17 } } },
      clip: { timings: { "clip made (2 s)": { median: 103 } } },
    },
  };
  const { platform, rows, unbudgeted } = againstBudgets(perf, doc);
  assert.equal(platform, "linux");
  assert.deepEqual(rows, [
    { scenario: "perf-player", step: "open to first frame", ms: 150, budget: 200, over: false },
    { scenario: "perf-player", step: "seek to frame shown", ms: 75, budget: 60, over: true },
    { scenario: "perf-library", step: "transcript scroll, frame gap p95", ms: 17, budget: 30, over: false },
  ]);
  // Only the perf scenarios' own steps are expected to have budgets (not the app's start).
  assert.deepEqual(unbudgeted, ["perf-player: seek: time shown"]);
  assert.deepEqual(againstBudgets({ ...perf, on: { platform: "win32 10.0" } }, doc).rows, []);
});

test("the committed budgets.json: each budget a positive number of ms", () => {
  const committed = loadBudgets();
  for (const [platform, on] of Object.entries(committed)) {
    if (!on?.scenarios) continue;
    assert.ok(on.machine, `${platform}: the machine they were set on`);
    for (const [scenario, steps] of Object.entries(on.scenarios)) {
      assert.match(scenario, /^perf-/);
      for (const [step, b] of Object.entries(steps))
        assert.ok(Number.isInteger(b.budget) && b.budget > 0, `${platform} ${scenario} ${step}: ${b.budget}`);
    }
  }
});
