// The library's list of jobs, its words (jobs.ts): node --test apps/desktop/src/lib/jobs.test.mjs
// Each test is a spec, named as its sentence, with the words it's said in:
// CHANGELOG.md, 0.7.1: "A list shows what's being made." ("the clips and
// edits being made, how far the current one is, and the VODs being erased
// after you removed them") and "Waiting clips and edits can be reordered.",
// 0.7.4: "The GPU add-on carries on after a restart. It shows in the jobs
// list with Cancel.", and jobs.ts's doc comments. The list's own contents
// are desktop job_list/tests.rs's.
import assert from "node:assert/strict";
import { test } from "node:test";
import { inOrder, jobLine, summary, waitingIds, waitingJobs } from "./jobs.ts";

/** @type {(kind: import("./api").Job["kind"], id: number, running?: boolean, progress?: number | null) => import("./api").Job} */
const job = (kind, id, running = false, progress = null) => ({ kind, id, label: `${kind} ${id}`, running, progress });

/** The ids of `jobs`, in their order. @type {(jobs: { id: number }[]) => number[]} */
const ids = (jobs) => jobs.map((j) => j.id);

/** jobs.ts `summary`: "empty with no jobs". */
test("the line under the button is empty with no jobs", () => {
  assert.equal(summary([]), "");
});

/** jobs.ts `summary`: "Making 2 clips". */
test("the line counts the clips being made", () => {
  assert.equal(summary([job("clip", 1, true)]), "Making 1 clip");
  assert.equal(summary([job("clip", 1, true), job("clip", 2)]), "Making 2 clips");
});

/** jobs.ts `summary`: "Rendering 1 edit". */
test("the line counts the edits being rendered", () => {
  assert.equal(summary([job("edit", 3, true)]), "Rendering 1 edit");
  assert.equal(summary([job("edit", 3, true), job("edit", 4)]), "Rendering 2 edits");
});

/** jobs.ts `summary`: "Erasing 1 removed VOD". */
test("the line counts the removed VODs being erased", () => {
  assert.equal(summary([job("erase", 7, true)]), "Erasing 1 removed VOD");
  assert.equal(summary([job("erase", 7, true), job("erase", 8)]), "Erasing 2 removed VODs");
});

/** jobs.ts `summary`: "Receiving 1 file". */
test("the line counts the files being received", () => {
  assert.equal(summary([job("receive", 1, true)]), "Receiving 1 file");
  assert.equal(summary([job("receive", 1, true), job("receive", 2, true)]), "Receiving 2 files");
});

/** jobs.ts `summary`: "the clips being made, the edits being rendered, then what's being erased, then what's being
 *  received ("Making 2 clips · Rendering 1 edit · Erasing 1 removed VOD · Receiving 1 file")". */
test("the line names clips, then edits, then erasures, then files received, apart by a dot", () => {
  const jobs = [
    job("receive", 1, true),
    job("erase", 7, true),
    job("edit", 3),
    job("clip", 1, true),
    job("clip", 2),
    job("clip", 5),
  ];
  assert.equal(summary(jobs), "Making 3 clips · Rendering 1 edit · Erasing 1 removed VOD · Receiving 1 file");
  assert.equal(
    summary(jobs.filter((j) => j.kind !== "edit" && j.kind !== "clip")),
    "Erasing 1 removed VOD · Receiving 1 file",
  );
});

/** jobs.ts `summary`: "then the GPU add-on ("... · Downloading the GPU add-on")". */
test("the line names the GPU add-on being downloaded, last", () => {
  assert.equal(summary([job("addon", 0, true)]), "Downloading the GPU add-on");
  assert.equal(
    summary([job("addon", 0, true), job("receive", 1, true), job("clip", 1, true)]),
    "Making 1 clip · Receiving 1 file · Downloading the GPU add-on",
  );
});

/** jobs.ts `jobLine`: "A job's line under its name: what it's doing and how far, or that it waits". */
test("a job waiting says it waits, whatever it is", () => {
  for (const kind of /** @type {const} */ (["clip", "edit", "erase"])) assert.equal(jobLine(job(kind, 1)), "Waiting");
  assert.equal(jobLine(job("clip", 1, false, 0.5)), "Waiting", "progress left over from a job before it");
});

/** jobs.ts `jobLine`: "what it's doing". */
test("a job begun, not yet said how far, says what it's doing", () => {
  assert.equal(jobLine(job("clip", 1, true)), "Making the clip…");
  assert.equal(jobLine(job("edit", 2, true)), "Rendering…");
  assert.equal(jobLine(job("erase", 3, true)), "Erasing…");
  assert.equal(jobLine(job("receive", 4, true)), "Receiving…");
});

/** jobs.ts `jobLine`: "what it's doing and how far"; 0.7.1: "how far the current one is". */
test("a job being made says how far it is in whole percent", () => {
  assert.equal(jobLine(job("clip", 1, true, 0.424)), "Making the clip · 42%");
  assert.equal(jobLine(job("clip", 1, true, 0.426)), "Making the clip · 43%");
  assert.equal(jobLine(job("edit", 2, true, 1)), "Rendering · 100%");
  assert.equal(jobLine(job("erase", 5, true, 0.3)), "Erasing · 30%");
  assert.equal(jobLine(job("receive", 1, true, 0.5)), "Receiving · 50%");
  assert.equal(jobLine(job("clip", 1, true, 0)), "Making the clip · 0%", "begun and said it's at the start");
});

/** jobs.ts `jobLine`: "what it's doing and how far"; 0.7.4: "It shows in the jobs list". */
test("the GPU add-on says it's downloading and how far", () => {
  assert.equal(jobLine(job("addon", 0, true)), "Downloading…");
  assert.equal(jobLine(job("addon", 0, true, 0.424)), "Downloading · 42%");
});

/** jobs.ts `waitingJobs`: "The clips and edits waiting, in their order: the ones that can be dragged". */
test("the clips and edits waiting are the ones that can be dragged, in their order", () => {
  const jobs = [
    job("clip", 1, true),
    job("clip", 2),
    job("edit", 3),
    job("clip", 4),
    job("erase", 9, true),
    job("erase", 10),
  ];
  assert.deepEqual(waitingJobs(jobs), [jobs[1], jobs[2], jobs[3]], "not the one being made, not the erasures");
  assert.deepEqual(waitingJobs([]), []);
});

/** jobs.ts `waitingIds`: "The ids of the clips and edits waiting, in their order". */
test("the ids sent to reorder are those of the clips and edits waiting, in their order", () => {
  const jobs = [job("edit", 8, true), job("clip", 2), job("edit", 3), job("erase", 9), job("clip", 4)];
  assert.deepEqual(waitingIds(jobs), [2, 3, 4]);
});

/** jobs.ts `inOrder`: "`jobs` with the clips and edits waiting put in the order of `ids` (as dragged)"; 0.7.1: "Drag a clip or edit that's waiting up or down the list". */
test("a drag shows at once, the clips and edits waiting in the order dragged", () => {
  const jobs = [job("clip", 1, true), job("clip", 2), job("edit", 3), job("clip", 4), job("erase", 9, true)];
  assert.deepEqual(ids(inOrder(jobs, [4, 2, 3])), [1, 4, 2, 3, 9]);
  assert.deepEqual(ids(inOrder(jobs, [3, 4, 2])), [1, 3, 4, 2, 9]);
});

/** jobs.ts `inOrder`: "(as dragged), till the news says so". */
test("a drag leaves the job being made and the erasures where they are", () => {
  const jobs = [job("clip", 1, true), job("clip", 2), job("edit", 3), job("erase", 9, true), job("erase", 10)];
  assert.deepEqual(ids(inOrder(jobs, [3, 2])), [1, 3, 2, 9, 10]);
});

/** jobs.ts `inOrder`: "till the news says so": the list changed meanwhile, the news is the truth. */
test("a drag naming other jobs than are waiting now shows the list as the news has it", () => {
  const jobs = [job("clip", 1, true), job("clip", 2), job("edit", 3), job("clip", 4), job("erase", 9, true)];
  // One ended since the order was read.
  assert.deepEqual(ids(inOrder(jobs, [4, 2])), [1, 2, 3, 4, 9]);
  // One of those it names ended since, and the three left are all that waits.
  assert.deepEqual(ids(inOrder(jobs, [4, 2, 3, 6])), [1, 4, 2, 3, 9], "an id no longer waiting doesn't count");
  assert.deepEqual(ids(inOrder(jobs, [])), [1, 2, 3, 4, 9]);
});
