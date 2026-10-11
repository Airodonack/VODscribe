<script lang="ts">
  import type { SearchHit, Vod } from "./api";
  import { formatTime } from "./platform";

  let {
    hits,
    truncated,
    vods,
    onplay,
  }: {
    hits: SearchHit[];
    truncated: boolean;
    vods: Map<number, Vod>;
    onplay: (vodId: number, time: number) => void;
  } = $props();

  // Hits arrive sorted by VOD, so grouping consecutive runs is enough.
  let groups = $derived.by(() => {
    const out: { vod: Vod | undefined; vodId: number; hits: SearchHit[] }[] = [];
    for (const hit of hits) {
      const last = out.at(-1);
      if (last?.vodId === hit.vod_id) last.hits.push(hit);
      else out.push({ vodId: hit.vod_id, vod: vods.get(hit.vod_id), hits: [hit] });
    }
    return out;
  });
</script>

<div class="results">
  <p class="summary muted">
    {hits.length}{truncated ? "+" : ""} match{hits.length === 1 ? "" : "es"} in {groups.length} VOD{groups.length === 1
      ? ""
      : "s"}{#if truncated}. Showing the first {hits.length}; refine the phrase to narrow it down{/if}
  </p>
  {#each groups as group (group.vodId)}
    <section>
      <h2>
        {group.vod?.title ?? "Removed VOD"}
        <span class="muted">{group.hits.length}</span>
      </h2>
      {#each group.hits as hit (hit.segment_id)}
        <button class="hit" onclick={() => onplay(hit.vod_id, hit.time)}>
          <span class="time">{formatTime(hit.time)}</span>
          <span class="text">
            {#each hit.parts as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
          </span>
        </button>
      {/each}
    </section>
  {:else}
    <p class="muted">No matches. Search finds exact phrases, so try fewer words.</p>
  {/each}
</div>

<style>
  .results {
    padding: 8px 16px 32px;
  }

  .summary {
    font-size: 12px;
    margin: 4px 0 12px;
  }

  section {
    margin-bottom: 20px;
  }

  h2 {
    font-size: 13px;
    margin: 0 0 4px;
    display: flex;
    gap: 8px;
    align-items: baseline;
  }

  .hit {
    display: flex;
    gap: 12px;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    border-radius: 6px;
    padding: 6px 8px;
    margin: 0 -8px;
    width: calc(100% + 16px);
  }

  .time {
    flex: none;
    min-width: 56px;
    padding-top: 1px;
  }
</style>
