// Parses every GitHub workflow file and checks its shape: each job has a list
// of steps, each step a mapping with a `run` or a `uses`. A step line that
// lost its indentation (one did, 2026-10-31: the Checks workflow ran no job
// at all) parses as YAML but not as a workflow; this says so before a push.
//
//     node scripts/check-workflows.mjs [files...]   (default: .github/workflows/*.yml)
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { parse } from "yaml";

const dir = ".github/workflows";
const files =
  process.argv.length > 2
    ? process.argv.slice(2)
    : readdirSync(dir)
        .filter((f) => /\.ya?ml$/.test(f))
        .map((f) => path.join(dir, f));
let bad = 0;
const fail = (file, why) => {
  console.error(`${file}: ${why}`);
  bad++;
};
for (const file of files) {
  let doc;
  try {
    doc = parse(readFileSync(file, "utf8"));
  } catch (e) {
    fail(file, `doesn't parse: ${e.message.split("\n")[0]}`);
    continue;
  }
  if (!doc || typeof doc !== "object") {
    fail(file, "isn't a mapping");
    continue;
  }
  if (!doc.on) fail(file, "has no `on`");
  if (!doc.jobs || typeof doc.jobs !== "object") {
    fail(file, "has no jobs");
    continue;
  }
  for (const [name, job] of Object.entries(doc.jobs)) {
    if (!job || typeof job !== "object") {
      fail(file, `job ${name} isn't a mapping`);
      continue;
    }
    if (job.uses) continue; // a reusable workflow call has no steps
    if (!Array.isArray(job.steps) || job.steps.length === 0) {
      fail(file, `job ${name} has no steps list`);
      continue;
    }
    job.steps.forEach((step, i) => {
      if (!step || typeof step !== "object" || Array.isArray(step))
        fail(file, `job ${name}, step ${i + 1} isn't a mapping`);
      else if (!("run" in step) && !("uses" in step))
        fail(file, `job ${name}, step ${i + 1} has neither run nor uses`);
    });
  }
  for (const key of Object.keys(doc))
    if (key.startsWith("-") || key.includes("run:"))
      fail(file, `stray key at the top: ${JSON.stringify(key)}`);
}
if (bad) process.exit(1);
console.log(`${files.length} workflow files parse as workflows`);
