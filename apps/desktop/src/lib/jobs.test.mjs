// The library's list of jobs, its words (jobs.ts): node --test apps/desktop/src/lib/jobs.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { inOrder, jobLine, summary, waitingIds } from "./jobs.ts";

/** @type {(kind: import("./api").Job["kind"], id: number, running?: boolean, progress?: number | null) => import("./api").Job} */
const job = (kind, id, running = false, progress = null) => ({ kind, id, label: `${kind} ${id}`, running, progress });

test("the button says what's made and what's erased, counted", () => {
  assert.equal(summary([]), "");
  assert.equal(summary([job("clip", 1, true), job("clip", 2)]), "Making 2 clips");
  assert.equal(summary([job("edit", 3, true), job("clip", 1)]), "Making 1 clip and 1 edit");
  assert.equal(summary([job("erase", 7, true)]), "Erasing 1 removed VOD");
  assert.equal(
    summary([job("clip", 1, true), job("edit", 3), job("edit", 4), job("erase", 7, true), job("erase", 8)]),
    "Making 1 clip and 2 edits · Erasing 2 removed VODs",
  );
});

test("each job's line says what it's doing and how far, or that it waits", () => {
  assert.equal(jobLine(job("clip", 1)), "Waiting");
  assert.equal(jobLine(job("clip", 1, true)), "Making the clip…");
  assert.equal(jobLine(job("clip", 1, true, 0.424)), "Making the clip · 42%");
  assert.equal(jobLine(job("edit", 2, true, 1)), "Rendering · 100%");
  assert.equal(jobLine(job("erase", 5, true, 0.3)), "Erasing · 30%");
  assert.equal(jobLine(job("erase", 6)), "Waiting");
});

test("only the clips and edits waiting are dragged, and a drag shows at once", () => {
  const jobs = [job("clip", 1, true), job("clip", 2), job("edit", 3), job("clip", 4), job("erase", 9, true)];
  assert.deepEqual(waitingIds(jobs), [2, 3, 4]);
  assert.deepEqual(
    inOrder(jobs, [4, 2, 3]).map((j) => j.id),
    [1, 4, 2, 3, 9],
  );
  // The list changed meanwhile (one ended): as the news has it.
  assert.deepEqual(
    inOrder(jobs, [4, 2]).map((j) => j.id),
    [1, 2, 3, 4, 9],
  );
});
