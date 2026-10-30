<script lang="ts">
  // The jobs under way, at the bottom of the library (as a file manager
  // shows its copies): the clips and edits being made, one at a time, and
  // the removed VODs being erased beside them (desktop job_list.rs). A line
  // says what's going on, with the progress of what's being made; clicked,
  // it opens the list above it: each job with its name, what it's doing and
  // how far, and Cancel. The clips and edits waiting can be dragged into
  // another order. Shown only while there's something to show.
  import { onMount } from "svelte";
  import { flip } from "svelte/animate";
  import { api, events, type Job } from "./api";
  import { DEMO } from "./demo";
  import Icon from "./Icon.svelte";
  import { inOrder, jobLine, summary, waitingIds } from "./jobs";
  import Popover from "./Popover.svelte";
  import Progress from "./Progress.svelte";
  import { Reorder } from "./reorder.svelte";
  import Spinner from "./Spinner.svelte";

  let jobs = $state<Job[]>([]);
  let open = $state(false);
  let button = $state<HTMLElement>();
  let width = $state(260);

  onMount(() => {
    // The web demo makes nothing.
    if (DEMO) return;
    let stop: (() => void) | undefined;
    let gone = false;
    // News that comes before the first list is newer than it.
    let heard = false;
    events
      .jobs((list) => {
        heard = true;
        jobs = list;
      })
      .then(
        (u) => (gone ? u() : (stop = u)),
        () => {},
      );
    api.listJobs().then(
      (list) => !heard && (jobs = list),
      () => {},
    );
    return () => {
      gone = true;
      stop?.();
    };
  });

  // With nothing left, the list closes with its button.
  $effect(() => {
    if (!jobs.length) open = false;
  });

  const rendering = $derived(jobs.find((j) => j.kind !== "erase" && j.running));
  const running = $derived(jobs.filter((j) => j.kind !== "erase" && j.running));
  const erasing = $derived(jobs.filter((j) => j.kind === "erase"));
  const byId = $derived(new Map(jobs.filter((j) => j.kind !== "erase" && !j.running).map((j) => [j.id, j])));

  // The clips and edits waiting, dragged into another order: shown so at
  // once, and the queue told.
  const reorder = new Reorder(
    () => waitingIds(jobs),
    (ids) => {
      jobs = inOrder(jobs, ids);
      void api.reorderJobs(ids).catch((e) => console.error("reorder jobs", e));
    },
  );
  const waiting = $derived(reorder.order.map((id) => byId.get(id)).filter((j) => j !== undefined));

  // It leaves the list with the news that it's out of the queue (one being
  // made, once its ffmpeg has ended).
  const cancel = (job: Job) => void api.cancelJob(job.kind, job.id).catch((e) => console.error("cancel job", e));
</script>

{#snippet inner(job: Job)}
  <span class="text">
    <span class="label" title={job.label}>{job.label}</span>
    <span class="muted line">{jobLine(job)}</span>
    {#if job.running}
      <Progress thin value={job.progress ?? 0} indeterminate={job.progress === null} />
    {/if}
  </span>
  <button
    class="row-act cancel"
    onclick={() => cancel(job)}
    title={job.kind === "erase" ? "Stop: the rest is erased when VODscribe next starts" : "Cancel"}
    aria-label="Cancel {job.label}"
  >
    <Icon name="x" size={15} />
  </button>
{/snippet}

{#if jobs.length}
  <div class="jobs-bar" bind:clientWidth={width}>
    <button
      class="open"
      bind:this={button}
      onclick={() => (open = !open)}
      aria-expanded={open}
      aria-label="Jobs: {summary(jobs)}"
      title={open ? "Hide the jobs" : "Show the jobs"}
    >
      <Spinner size={12} />
      <span class="summary">{summary(jobs)}</span>
      <Icon name={open ? "chevron-down" : "chevron-up"} size={14} />
    </button>
    {#if rendering}
      <Progress thin value={rendering.progress ?? 0} indeterminate={rendering.progress === null} />
    {/if}
  </div>
{/if}

<Popover bind:open anchor={button} label="Jobs" width={Math.max(220, width - 16)} padding={4} above>
  <div class="jobs" role="list" aria-label="Jobs">
    {#each running as job (job.id)}
      <div class="job" role="listitem" data-job="{job.kind}-{job.id}">{@render inner(job)}</div>
    {/each}
    <div class="waiting" bind:this={reorder.list}>
      {#each waiting as job (job.id)}
        <!-- Dragged anywhere but its Cancel to move it in the queue. -->
        <div
          class="job movable"
          class:lifted={reorder.lifted(job.id)}
          role="listitem"
          data-job="{job.kind}-{job.id}"
          title="Drag to make it sooner or later"
          style:transform={reorder.transform(job.id)}
          bind:this={reorder.rows[job.id]}
          animate:flip={{ duration: reorder.flipDuration(job.id) }}
          onclickcapture={reorder.clickCapture}
          onpointerdown={(e) => {
            if (!(e.target as HTMLElement).closest("button")) reorder.start(e, job.id);
          }}
        >
          {@render inner(job)}
        </div>
      {/each}
    </div>
    {#each erasing as job (job.id)}
      <div class="job" role="listitem" data-job="{job.kind}-{job.id}">{@render inner(job)}</div>
    {/each}
  </div>
</Popover>

<style>
  /* Under the library's list, its rows' look: a line of small muted text. */
  .jobs-bar {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 8px 8px;
    border-top: 1px solid var(--border);
    background: var(--surface);
  }

  .open {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 4px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--muted);
    font-size: 12px;
    font-weight: 400;
    text-align: left;
  }

  .open:hover:not(:disabled) {
    background: var(--bg);
    color: var(--text);
  }

  .summary {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* As a menu's choices: rows lit under the pointer. */
  .jobs,
  .waiting {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* Rows' offsetTop is measured from here; see Reorder. */
  .waiting {
    position: relative;
  }

  .job {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 6px 8px 8px;
    border-radius: var(--radius-sm);
    background: var(--surface);
  }

  .job:hover {
    background: var(--bg);
  }

  .job:hover .row-act {
    opacity: 1;
  }

  .movable {
    cursor: grab;
    user-select: none;
    -webkit-user-select: none;
  }

  .job.lifted {
    z-index: 1;
    cursor: grabbing;
    box-shadow: var(--shadow);
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .label {
    font-weight: 600;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line {
    font-size: 12px;
  }

  .cancel {
    flex: none;
  }
</style>
