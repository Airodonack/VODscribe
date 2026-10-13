<script lang="ts">
  // A download's current step, as setup and Settings show it: the exact
  // component ("Downloading cuBLAS 12.8", "… matches its SHA-256"), which of
  // the install's files it is ("(3 of 10)"), and the megabytes of that file.
  import type { DownloadProgress } from "./api";
  import Icon from "./Icon.svelte";
  import Progress from "./Progress.svelte";

  let {
    progress,
    waiting,
    bar = true,
    oncancel,
  }: {
    progress: DownloadProgress | null;
    /** Shown before the first step arrives. */
    waiting?: string;
    /** A bar across the whole install, sliding while its size is unknown. */
    bar?: boolean;
    /** Stops the download; shown as an x at the end of the bar. */
    oncancel?: () => void;
  } = $props();

  const mb = (bytes: number) => Math.round(bytes / 1e6).toLocaleString();
  let downloading = $derived(!!progress?.step.startsWith("Downloading") && progress.file_done > 0);
</script>

<div class="download">
  {#if progress || waiting}
    <span class="muted step">
      {progress?.step ?? waiting}{#if progress && progress.parts > 1}&nbsp;({progress.part} of {progress.parts}){/if}…
      {#if progress && downloading}
        <span class="mb"
          >{mb(progress.file_done)}{#if progress.file_size}&nbsp;/ {mb(progress.file_size)}{/if} MB</span
        >
      {/if}
    </span>
  {/if}
  {#if bar}
    <div class="row">
      <Progress
        value={progress?.total ? progress.done / progress.total : 0}
        indeterminate={progress !== null && progress.total === null}
      />
      {#if oncancel}
        <button class="icon cancel" onclick={oncancel} aria-label="Cancel the download" title="Cancel"
          ><Icon name="x" size={14} /></button
        >
      {/if}
    </div>
  {/if}
</div>

<style>
  .download {
    display: grid;
    gap: 5px;
  }

  .step {
    font-size: 12px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .cancel {
    flex: none;
    width: 22px;
    height: 22px;
  }

  .cancel:hover:not(:disabled) {
    color: var(--danger);
    background: var(--danger-soft);
  }

  .mb {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
