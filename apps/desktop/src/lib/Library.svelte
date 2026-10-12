<script lang="ts">
  import { api, type Vod } from "./api";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import { formatTime, platformName } from "./platform";

  let {
    vods,
    selectedId,
    onselect,
  }: { vods: Vod[]; selectedId: number | null; onselect: (id: number | null) => void } = $props();

  let url = $state("");
  let adding = $state(false);
  let addError = $state<string | null>(null);

  async function add(event: SubmitEvent) {
    event.preventDefault();
    adding = true;
    addError = null;
    try {
      await api.addVod(url);
      url = "";
    } catch (e) {
      addError = String(e);
    } finally {
      adding = false;
    }
  }

  const statusLabel: Record<Vod["status"], string> = {
    queued: "Waiting to download",
    downloading: "Downloading",
    ready: "Waiting to transcribe",
    transcribing: "Transcribing",
    paused: "Paused",
    done: "",
    error: "Failed",
  };

  function retranscribe(vod: Vod) {
    if (confirm(`Re-transcribe “${vod.title}”? Its current transcript is replaced; the audio may need to download again.`)) {
      api.retranscribeVod(vod.id);
    }
  }

  function cancel(vod: Vod) {
    if (confirm(`Cancel “${vod.title}”? Its partial download and transcript will be discarded.`)) {
      api.cancelVod(vod.id);
    }
  }

  function remove(vod: Vod) {
    if (confirm(`Remove “${vod.title}” and its transcript?`)) {
      if (selectedId === vod.id) onselect(null);
      api.deleteVod(vod.id);
    }
  }

  // Thumbnails that fail to load fall back to the platform tile.
  let broken = $state(new Set<number>());

  const toggle = (vod: Vod) => onselect(vod.id === selectedId ? null : vod.id);

  // The whole row selects, except its action buttons (and the main button,
  // which handles itself so it also works from the keyboard).
  function onRowClick(e: MouseEvent, vod: Vod) {
    if ((e.target as HTMLElement).closest("button")) return;
    toggle(vod);
  }
</script>

<aside>
  <div class="brand">
    <Logo size={22} tile />
    <span class="name">VODscribe</span>
  </div>

  <form onsubmit={add}>
    <input type="url" placeholder="Paste a VOD link…" bind:value={url} disabled={adding} aria-label="VOD link" />
    <button class="primary" disabled={adding || !url.trim()}>{adding ? "Adding…" : "Add"}</button>
  </form>
  {#if addError}
    <p class="error-text add-error">{addError}</p>
  {/if}

  <div class="list">
    {#each vods as vod (vod.id)}
      {@const active = vod.status === "downloading" || vod.status === "transcribing"}
      {@const showBar = active || (vod.status === "paused" && vod.progress > 0)}
      {@const platform = platformName[vod.platform] ?? vod.platform}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div class="vod" class:selected={vod.id === selectedId} onclick={(e) => onRowClick(e, vod)}>
        <button class="main" onclick={() => toggle(vod)}>
          <span class="thumb {vod.platform}">
            {#if vod.thumbnail && !broken.has(vod.id)}
              <img
                src={vod.thumbnail}
                alt=""
                loading="lazy"
                onerror={() => {
                  broken.add(vod.id);
                  broken = new Set(broken);
                }}
              />
            {:else}
              <Icon name="play" size={14} />
            {/if}
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

        {#if vod.status !== "done" || showBar || vod.error || vod.outdated || vod.note}
          <div class="status-area">
            {#if vod.status !== "done"}
              <span class="status" class:active class:error-text={vod.status === "error"}>
                {#if active}<span class="pulse" aria-hidden="true"></span>{/if}
                {#if vod.status === "error"}<Icon name="error" size={13} />{/if}
                {statusLabel[vod.status]}{#if showBar}<span class="pct">{Math.round(vod.progress * 100)}%</span>{/if}
              </span>
            {/if}
            {#if showBar}
              <span class="bar" class:paused={vod.status === "paused"}><span style:width="{vod.progress * 100}%"></span></span>
            {/if}
            {#if vod.status === "error" && vod.error}
              <span class="note error-text" title={vod.error}>{vod.error}</span>
            {:else if vod.outdated}
              <span class="note warn-text" title="This transcript was made with a previous speech model.">
                <Icon name="warning" size={13} />
                {vod.status === "done" ? "Older model; may be missing speech" : "Older model; resuming starts over"}
              </span>
            {:else if vod.note}
              <span class="note muted" title={vod.note}>{vod.note}</span>
            {/if}
          </div>
        {/if}

        <div class="actions">
          {#if vod.status === "error"}
            <button class="act" onclick={() => api.retryVod(vod.id)} title="Retry" aria-label="Retry">
              <Icon name="redo" size={15} />
            </button>
          {:else if vod.status === "paused"}
            <!-- Resuming an outdated transcript starts over, like Re-transcribe. -->
            <button
              class="act"
              class:warn={vod.outdated}
              onclick={() => api.resumeVod(vod.id)}
              title={vod.outdated ? "Resume (starts over with the current model)" : "Resume"}
              aria-label="Resume"
            >
              <Icon name="play" size={15} />
            </button>
            <button class="act" onclick={() => cancel(vod)} title="Cancel" aria-label="Cancel">
              <Icon name="x" size={15} />
            </button>
          {:else if vod.status !== "done"}
            <button class="act" onclick={() => api.pauseVod(vod.id)} title="Pause" aria-label="Pause">
              <Icon name="pause" size={15} />
            </button>
            <button class="act" onclick={() => cancel(vod)} title="Cancel" aria-label="Cancel">
              <Icon name="x" size={15} />
            </button>
          {:else}
            <button
              class="act"
              class:warn={vod.outdated}
              onclick={() => retranscribe(vod)}
              title={vod.outdated ? "Re-transcribe with the current model" : "Re-transcribe"}
              aria-label="Re-transcribe"
            >
              <Icon name="redo" size={15} />
            </button>
          {/if}
          <button class="act" onclick={() => remove(vod)} title="Remove" aria-label="Remove">
            <Icon name="trash" size={15} />
          </button>
        </div>
      </div>
    {:else}
      <p class="muted empty">No VODs yet. Paste a link above to transcribe one.</p>
    {/each}
  </div>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--border);
    background: var(--surface);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 53px;
    padding: 0 14px;
    border-bottom: 1px solid var(--border);
  }


  .name {
    font-weight: 700;
    font-size: 15px;
    letter-spacing: -0.01em;
  }

  form {
    display: flex;
    gap: 6px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }

  .add-error {
    margin: 0;
    padding: 8px 12px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
    white-space: pre-wrap;
  }

  .list {
    overflow-y: auto;
    flex: 1;
  }

  .vod {
    cursor: pointer;
    padding: 10px 12px 9px;
    border-bottom: 1px solid var(--border);
    transition: background 0.1s;
  }

  .vod:hover {
    background: var(--bg);
  }

  .vod.selected {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 var(--accent);
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

  .main:hover:not(:disabled) {
    background: none;
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

  .thumb img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
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

  .sep {
    margin: 0 4px;
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

  .note {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
  }

  .note :global(svg) {
    flex: none;
  }

  .note {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .bar {
    display: block;
    height: 3px;
    border-radius: 2px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .bar.paused > span {
    background: var(--muted);
  }

  .bar > span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.25s;
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

  .vod:hover .act,
  .vod.selected .act {
    opacity: 1;
  }

  .act:hover:not(:disabled) {
    background: var(--surface-3);
    color: var(--text);
  }

  .act.warn {
    color: var(--warn);
  }

  .empty {
    padding: 16px 12px;
  }
</style>
