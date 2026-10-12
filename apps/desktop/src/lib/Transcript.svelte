<script lang="ts">
  import { api, type Line, type Vod } from "./api";
  import { formatTime, platformName } from "./platform";

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
  <header>
    <span class="eyebrow">Transcript</span>
    <h2>{vod.title}</h2>
    <p class="meta muted">
      <span class="platform {vod.platform}">{platformName[vod.platform] ?? vod.platform}</span>
      {#if vod.channel}<span class="sep">·</span>{vod.channel}{/if}
      {#if vod.duration}<span class="sep">·</span>{formatTime(vod.duration)}{/if}
      {#if vod.status === "transcribing"}
        <span class="sep">·</span>Transcribing, {Math.round(vod.progress * 100)}%; new lines appear as they're produced
      {:else if lines.length > 0}
        <span class="sep">·</span>{lines.length} line{lines.length === 1 ? "" : "s"}
      {/if}
    </p>
  </header>
  {#if error}
    <p class="error-text">{error}</p>
  {:else if lines.length === 0}
    <p class="muted">
      {vod.status === "done" ? "No speech was found in this VOD." : "The transcript appears here as it is produced."}
    </p>
  {/if}
  {#each lines as line, i (i)}
    <button class="row" onclick={() => onplay(vod.id, line.start)}>
      <span class="time">{formatTime(line.start)}</span>
      <span>{line.text}</span>
    </button>
  {/each}
</div>

<style>
  .transcript {
    padding: 14px 20px 32px;
    max-width: 960px;
  }

  header {
    margin-bottom: 14px;
  }

  h2 {
    font-size: 16px;
    margin: 2px 0 2px;
    line-height: 1.3;
  }

  .meta {
    margin: 0;
    font-size: 12px;
  }

  .sep {
    margin: 0 5px;
  }
</style>
