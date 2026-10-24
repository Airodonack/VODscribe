// The summary of a sharded run: merge-results.mjs reads the results.json of
// each shard (CI's e2e job leaves one per shard, in an artifact of its own)
// and prints one summary in run.mjs's words, so a run split over many
// machines reads like one run.
//
//   node tests/e2e/merge-results.mjs [--expect N] <results.json | folder>...
//
// A folder is a shard's own (it holds results.json) or a folder of such
// folders, one level down (what download-artifact makes of several artifacts).
// The exit code is 1 when any scenario failed, when no results were found, or
// when fewer shards reported than --expect: a shard that crashed before it
// wrote results.json must not read as a pass. When GITHUB_STEP_SUMMARY is set
// the summary is added to the run's page too.

import { appendFileSync, existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

/** The results.json files under the given files and folders, as { label, file } in order. */
export function findResults(paths) {
  const found = [];
  for (const p of paths) {
    if (statSync(p).isFile()) {
      found.push({ label: path.basename(path.dirname(path.resolve(p))), file: p });
    } else if (existsSync(path.join(p, "results.json"))) {
      found.push({ label: path.basename(path.resolve(p)), file: path.join(p, "results.json") });
    } else {
      for (const entry of readdirSync(p).sort()) {
        const file = path.join(p, entry, "results.json");
        if (existsSync(file)) found.push({ label: entry, file });
      }
    }
  }
  return found;
}

/** The summary's lines, and whether the run passed. `shards` is [{ label, results }]. */
export function summarize(shards, expect = 0) {
  const all = shards.flatMap((s) => s.results.map((r) => ({ ...r, shard: s.label })));
  const failed = all.filter((r) => !r.ok);
  const flaky = all.filter((r) => r.ok && r.flaky);
  const passed = all.length - failed.length - flaky.length;
  const lines = [
    `e2e: ${passed} of ${all.length} passed${flaky.length ? `, ${flaky.length} flaky` : ""}, in ${shards.length} shard${shards.length === 1 ? "" : "s"}`,
  ];
  for (const s of shards) {
    const bad = s.results.filter((r) => !r.ok).length;
    const seconds = s.results.reduce((sum, r) => sum + (Number.isFinite(r.seconds) ? r.seconds : 0), 0);
    lines.push(
      `  ${bad ? "FAIL" : "ok  "} ${s.label}: ${s.results.length} scenarios, ${Math.round(seconds / 60)} min${bad ? `, ${bad} failed` : ""}`,
    );
  }
  for (const r of all) {
    if (r.flaky) lines.push(`  flaky ${r.name} [${r.shard}] (${r.seconds} s): failed first: ${r.firstError}`);
    else if (!r.ok) lines.push(`  FAIL ${r.name} [${r.shard}] (${r.seconds} s): ${r.error}`);
  }
  let ok = failed.length === 0;
  if (!shards.length) {
    lines.push("  FAIL no results.json found: no shard reported");
    ok = false;
  } else if (shards.length < expect) {
    lines.push(
      `  FAIL ${shards.length} of ${expect} shards reported (one crashed or was cancelled before it wrote results.json)`,
    );
    ok = false;
  }
  return { lines, ok };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { values, positionals } = parseArgs({
    options: { expect: { type: "string", default: "0" } },
    allowPositionals: true,
  });
  if (!positionals.length) {
    console.error("usage: node tests/e2e/merge-results.mjs [--expect N] <results.json | folder>...");
    process.exit(2);
  }
  const shards = findResults(positionals).map(({ label, file }) => ({
    label,
    results: JSON.parse(readFileSync(file, "utf8")),
  }));
  const { lines, ok } = summarize(shards, Number(values.expect));
  console.log(lines.join("\n"));
  if (process.env.GITHUB_STEP_SUMMARY)
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\`\`\`\n${lines.join("\n")}\n\`\`\`\n`);
  process.exit(ok ? 0 : 1);
}
