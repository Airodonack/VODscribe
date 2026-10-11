<script lang="ts">
  import { api, type Line, type Vod } from "./api";
  import { formatTime } from "./platform";

  let { vod, onplay }: { vod: Vod; onplay: (vodId: number, time: number) => void } = $props();

  let lines = $state<Line[]>([]);
  let error = $state<string | null>(null);

  // Reload when the VOD changes, and periodically while it is still transcribing
  // (segments are indexed as they are produced).
  let vodId = $derived(vod.id);
  let step = $derived(vod.status === "transcribing" ? Math.floor(vod.progress * 20) : vod.status);
  $effect(() => {
    const id = vodId;
    void step;
    api.transcript(id).then(
      (l) => {
        if (id === vod.id) lines = l;
      },
      (e) => (error = String(e)),
    );
  });
</script>

<div class="transcript">
  <h2>{vod.title}</h2>
  {#if error}
    <p class="error-text">{error}</p>
  {:else if lines.length === 0}
    <p class="muted">
      {vod.status === "done" ? "No speech was found in this VOD." : "The transcript appears here as it is produced."}
    </p>
  {/if}
  {#each lines as line, i (i)}
    <button class="line" onclick={() => onplay(vod.id, line.start)}>
      <span class="time">{formatTime(line.start)}</span>
      <span>{line.text}</span>
    </button>
  {/each}
</div>

<style>
  .transcript {
    padding: 8px 16px 32px;
  }

  h2 {
    font-size: 15px;
    margin: 8px 0 12px;
  }

  .line {
    display: flex;
    gap: 12px;
    text-align: left;
    border: none;
    background: none;
    border-radius: 6px;
    padding: 4px 8px;
    margin: 0 -8px;
    width: calc(100% + 16px);
  }

  .time {
    flex: none;
    min-width: 56px;
    padding-top: 1px;
  }
</style>
