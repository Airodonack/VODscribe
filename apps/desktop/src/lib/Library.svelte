<script lang="ts">
  import { api, type Vod } from "./api";
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
</script>

<aside>
  <form onsubmit={add}>
    <input type="url" placeholder="Paste a Kick, Twitch or YouTube VOD link" bind:value={url} disabled={adding} />
    <button class="primary" disabled={adding || !url.trim()}>{adding ? "Adding…" : "Add"}</button>
  </form>
  {#if addError}
    <p class="error-text add-error">{addError}</p>
  {/if}

  <div class="list">
    {#each vods as vod (vod.id)}
      {@const active = vod.status === "downloading" || vod.status === "transcribing"}
      {@const showBar = active || (vod.status === "paused" && vod.progress > 0)}
      <div class="vod" class:selected={vod.id === selectedId}>
        <button class="main" onclick={() => onselect(vod.id === selectedId ? null : vod.id)}>
          <span class="title">{vod.title}</span>
          <span class="meta muted">
            {platformName[vod.platform] ?? vod.platform}
            {#if vod.channel}· {vod.channel}{/if}
            {#if vod.duration}· {formatTime(vod.duration)}{/if}
          </span>
          {#if vod.status !== "done"}
            <span class="status" class:error-text={vod.status === "error"}>
              {statusLabel[vod.status]}{#if showBar}&nbsp;{Math.round(vod.progress * 100)}%{/if}
            </span>
          {/if}
          {#if showBar}
            <span class="bar" class:paused={vod.status === "paused"}><span style:width="{vod.progress * 100}%"></span></span>
          {/if}
          {#if vod.status === "error" && vod.error}
            <span class="err muted" title={vod.error}>{vod.error}</span>
          {:else if vod.outdated}
            <span class="note warn">
              {vod.status === "done" ? "Older model; may be missing speech." : "Older model; resuming starts over."}
            </span>
          {:else if vod.note}
            <span class="note muted" title={vod.note}>{vod.note}</span>
          {/if}
        </button>
        <div class="actions">
          {#if vod.status === "error"}
            <button class="link" onclick={() => api.retryVod(vod.id)}>Retry</button>
          {:else if vod.status === "paused"}
            <!-- Resuming an outdated transcript starts over, like Re-transcribe. -->
            <button class="link" class:warn={vod.outdated} onclick={() => api.resumeVod(vod.id)}>Resume</button>
            <button class="link" onclick={() => cancel(vod)}>Cancel</button>
          {:else if vod.status !== "done"}
            <button class="link" onclick={() => api.pauseVod(vod.id)}>Pause</button>
            <button class="link" onclick={() => cancel(vod)}>Cancel</button>
          {:else}
            <button class="link" class:warn={vod.outdated} onclick={() => retranscribe(vod)}>Re-transcribe</button>
          {/if}
          <button class="link" onclick={() => remove(vod)}>Remove</button>
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

  form {
    display: flex;
    gap: 6px;
    padding: 12px;
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
    border-bottom: 1px solid var(--border);
    padding: 10px 12px;
  }

  .vod.selected {
    background: var(--surface-2);
    box-shadow: inset 3px 0 var(--accent);
  }

  .main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    padding: 0;
  }

  .main:hover:not(:disabled) {
    background: none;
  }

  .title {
    font-weight: 600;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .meta,
  .status,
  .err,
  .note {
    font-size: 12px;
  }

  .warn {
    color: var(--danger);
  }

  .err,
  .note {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .bar {
    display: block;
    height: 3px;
    border-radius: 2px;
    background: var(--border);
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
    gap: 12px;
    margin-top: 4px;
  }

  .empty {
    padding: 16px 12px;
  }
</style>
