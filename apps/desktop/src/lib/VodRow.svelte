<script lang="ts">
  // The contents of one library row: thumbnail, title, status and actions. The
  // row element itself (selection, dragging) belongs to Library, since
  // animate:flip has to sit directly inside its keyed list.
  import { api, type Vod } from "./api";
  import { demoOff } from "./demo";
  import { ask } from "./ask";
  import Icon from "./Icon.svelte";
  import { formatTime, platformName } from "./platform";
  import Progress from "./Progress.svelte";
  import Spinner from "./Spinner.svelte";
  import Thumb from "./Thumb.svelte";

  let {
    vod,
    reorderable,
    ontoggle,
    onkey,
    onremove,
    onerror,
  }: {
    vod: Vod;
    /** Whether the row can be dragged (there's more than one). */
    reorderable: boolean;
    ontoggle: () => void;
    onkey: (e: KeyboardEvent) => void;
    /** Called before the VOD is removed. */
    onremove: () => void;
    /** An action's error, or null when a new action starts. */
    onerror: (message: string | null) => void;
  } = $props();

  const statusLabel: Record<Vod["status"], string> = {
    queued: "Waiting to download",
    downloading: "Downloading",
    ready: "Waiting to transcribe",
    transcribing: "Transcribing",
    paused: "Paused",
    done: "",
    error: "Failed",
  };

  // Stopped by the user rather than failed.
  const stopped = $derived(vod.status === "error" && vod.error === "Stopped");
  const active = $derived(vod.status === "downloading" || vod.status === "transcribing");
  const showBar = $derived(active || (vod.status === "paused" && vod.progress > 0));
  const platform = $derived(platformName[vod.platform] ?? vod.platform);

  /** Runs an action, reporting its error if it fails. */
  function act(action: Promise<unknown>) {
    onerror(null);
    action.catch((e) => onerror(String(e)));
  }

  // Transcribing again first erases the old transcript, a moment for a long one.
  let clearing = $state(false);
  async function retranscribe() {
    if (
      await ask(
        `Transcribe “${vod.title}” again? Its current transcript will be replaced, and the audio may need to download again.`,
        "Transcribe again",
      )
    ) {
      clearing = true;
      onerror(null);
      api.retranscribeVod(vod.id).then(
        () => (clearing = false),
        (e) => {
          clearing = false;
          onerror(String(e));
        },
      );
    }
  }

  async function cancel() {
    if (
      await ask(
        `Cancel “${vod.title}”? What's been downloaded and transcribed so far will be thrown away.`,
        "Stop and discard",
        "Keep going",
      )
    ) {
      act(api.cancelVod(vod.id));
    }
  }

  // Removing a VOD with a long transcript and busy chat takes a few seconds
  // (its text is erased, not just dropped); the row says so meanwhile, and
  // goes when it's done.
  let removing = $state(false);
  async function remove() {
    if (
      await ask(
        `Remove “${vod.title}”? Its transcript and any downloaded audio are erased from this computer.`,
        "Remove",
        "Keep",
      )
    ) {
      onremove();
      removing = true;
      onerror(null);
      api.deleteVod(vod.id).catch((e) => {
        removing = false;
        onerror(String(e));
      });
    }
  }
</script>

<button
  class="main"
  class:removing
  onclick={ontoggle}
  onkeydown={onkey}
  title={reorderable ? "Drag to reorder (Alt+↑/↓)" : undefined}
>
  <span class="thumb {vod.platform}">
    <!-- The platform tile shows until (and unless) the thumbnail covers it. -->
    <Icon name="play" size={14} />
    {#if vod.thumb}<Thumb name={vod.thumb} />{/if}
    {#if vod.duration}<span class="duration">{formatTime(vod.duration)}</span>{/if}
  </span>
  <span class="text">
    <span class="title">{vod.title}</span>
    <span class="meta muted">
      <span class="platform {vod.platform}">{platform}</span>
      {#if vod.channel}<span class="sep">·</span>{vod.channel}{/if}
    </span>
  </span>
</button>

{#if removing || clearing}
  <div class="status-area">
    <span class="status">
      <Spinner label={removing ? "Removing" : "Clearing the old transcript"} />
      {removing ? "Removing…" : "Clearing the old transcript…"}
    </span>
  </div>
{:else if vod.status !== "done" || showBar || vod.error || vod.outdated || vod.note}
  <div class="status-area">
    {#if vod.status !== "done"}
      <span class="status" class:active class:error-text={vod.status === "error" && !stopped}>
        {#if active}<span class="pulse" aria-hidden="true"></span>{/if}
        {#if vod.status === "error" && !stopped}<Icon name="error" size={13} />{/if}
        {stopped ? "Stopped" : statusLabel[vod.status]}{#if showBar}<span class="pct"
            >{Math.round(vod.progress * 100)}%</span
          >{/if}
      </span>
    {/if}
    {#if showBar}
      <Progress value={vod.progress} paused={vod.status === "paused"} thin />
    {/if}
    {#if vod.status === "error" && vod.error && !stopped}
      <span class="note error-text" title={vod.error}>{vod.error}</span>
    {:else if vod.outdated}
      <span
        class="note warn-text"
        title="VODscribe's speech recognition has improved since this was made. Transcribe it again for a better transcript."
      >
        <Icon name="warning" size={13} />
        {vod.status === "done" ? "Made with an older version" : "Will restart with the newer version"}
      </span>
    {:else if vod.note}
      <span class="note muted" title={vod.note}>{vod.note}</span>
    {/if}
  </div>
{/if}

<div class="actions" class:removing inert={removing || clearing}>
  {#if vod.status === "error"}
    <button class="act" onclick={() => act(api.retryVod(vod.id))} title="Retry" aria-label="Retry">
      <Icon name="redo" size={15} />
    </button>
  {:else if vod.status === "paused"}
    <!-- Resuming an outdated transcript starts over, like transcribing again. -->
    <button
      class="act"
      class:warn={vod.outdated}
      onclick={() => act(api.resumeVod(vod.id))}
      title={vod.outdated ? "Resume (starts over with the newer version)" : "Resume"}
      aria-label="Resume"
    >
      <Icon name="play" size={15} />
    </button>
    <button class="act" onclick={cancel} title="Cancel" aria-label="Cancel">
      <Icon name="x" size={15} />
    </button>
  {:else if vod.status !== "done"}
    <button class="act" onclick={() => act(api.pauseVod(vod.id))} title="Pause" aria-label="Pause">
      <Icon name="pause" size={15} />
    </button>
    <button class="act" onclick={cancel} title="Cancel" aria-label="Cancel">
      <Icon name="x" size={15} />
    </button>
  {:else}
    <button
      class="act"
      class:warn={vod.outdated}
      onclick={retranscribe}
      title={vod.outdated ? "Transcribe again with the newer version" : "Transcribe again"}
      aria-label="Transcribe again"
      {...demoOff("The web demo can't transcribe. In the app, this transcribes the VOD again.")}
    >
      <Icon name="redo" size={15} />
    </button>
  {/if}
  <button
    class="act"
    onclick={remove}
    title="Remove"
    aria-label="Remove"
    {...demoOff("The web demo keeps its two VODs. In the app, this removes a VOD and its transcript.")}
  >
    <Icon name="trash" size={15} />
  </button>
</div>

<style>
  /* Being removed: faded, and its actions out of reach. */
  .removing {
    opacity: 0.5;
  }

  .main {
    display: flex;
    gap: 10px;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    padding: 0;
    white-space: normal;
    align-items: flex-start;
  }

  .thumb {
    position: relative;
    flex: none;
    width: 80px;
    aspect-ratio: 16 / 9;
    border-radius: var(--radius-sm);
    background: var(--surface-3);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .thumb.youtube {
    background: color-mix(in srgb, var(--youtube) 18%, var(--surface-3));
  }

  .thumb.twitch {
    background: color-mix(in srgb, var(--twitch) 22%, var(--surface-3));
  }

  .thumb.kick {
    background: color-mix(in srgb, var(--kick) 20%, var(--surface-3));
  }

  .duration {
    position: absolute;
    right: 3px;
    bottom: 3px;
    padding: 0 4px;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.75);
    color: #fff;
    font-family: var(--mono);
    font-size: 10px;
    line-height: 16px;
    font-variant-numeric: tabular-nums;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .title {
    font-weight: 600;
    font-size: 13px;
    line-height: 1.35;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .meta {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .status-area {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 7px;
    font-size: 12px;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .pct {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .pulse {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1.6s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  /* Notes wrap, so an error reads in full. */
  .note {
    display: flex;
    align-items: flex-start;
    gap: 5px;
    min-width: 0;
    line-height: 1.4;
  }

  .note :global(svg) {
    flex: none;
  }

  .actions {
    display: flex;
    gap: 2px;
    justify-content: flex-end;
    margin-top: 4px;
    margin-right: -6px;
  }

  .act {
    width: 26px;
    height: 24px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    border-radius: var(--radius-sm);
    opacity: 0.75;
  }

  :global(.vod:hover) .act,
  :global(.vod.selected) .act {
    opacity: 1;
  }

  .act:hover:not(:disabled) {
    background: var(--surface-3);
    color: var(--text);
  }

  .act.warn {
    color: var(--warn);
  }
</style>
