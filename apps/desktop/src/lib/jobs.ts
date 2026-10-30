// What the library's list of jobs says (Jobs.svelte): the line under its
// button, and each job's line in the list. The jobs are desktop job_list.rs's.

import type { Job } from "./api";

const counted = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** The button's line: what's being made, then what's being erased ("Making
 *  2 clips and 1 edit · Erasing 1 removed VOD"); empty with no jobs. */
export function summary(jobs: Job[]): string {
  const of = (kind: Job["kind"]) => jobs.filter((j) => j.kind === kind).length;
  const [clips, edits, erasing] = [of("clip"), of("edit"), of("erase")];
  const made = [clips && counted(clips, "clip", "clips"), edits && counted(edits, "edit", "edits")].filter(Boolean);
  const parts = [];
  if (made.length) parts.push(`Making ${made.join(" and ")}`);
  if (erasing) parts.push(`Erasing ${counted(erasing, "removed VOD", "removed VODs")}`);
  return parts.join(" · ");
}

/** A job's line under its name: what it's doing and how far, or that it waits. */
export function jobLine(job: Job): string {
  if (!job.running) return "Waiting";
  const doing = job.kind === "erase" ? "Erasing" : job.kind === "edit" ? "Rendering" : "Making the clip";
  return job.progress === null ? `${doing}…` : `${doing} · ${Math.round(job.progress * 100)}%`;
}

/** The ids of the clips and edits waiting, in their order: the ones that can be dragged. */
export const waitingIds = (jobs: Job[]) => jobs.filter((j) => j.kind !== "erase" && !j.running).map((j) => j.id);

/** `jobs` with the clips and edits waiting put in the order of `ids` (as dragged), till the news says so. */
export function inOrder(jobs: Job[], ids: number[]): Job[] {
  const waiting = jobs.filter((j) => j.kind !== "erase" && !j.running);
  const moved = ids.map((id) => waiting.find((j) => j.id === id)).filter((j) => j !== undefined);
  let next = 0;
  return jobs.map((j) => (waiting.includes(j) && moved.length === waiting.length ? moved[next++] : j));
}
