<script lang="ts">
  import type { SearchHit, Vod } from "./api";
  import { formatTime } from "./platform";
  import Icon from "./Icon.svelte";

  let {
    hits,
    truncated,
    vods,
    query,
    scope,
    onplay,
    onclearscope,
  }: {
    hits: SearchHit[];
    truncated: boolean;
    vods: Map<number, Vod>;
    query: string;
    /** The VOD the search is limited to, if any. */
    scope: Vod | undefined;
    onplay: (vodId: number, time: number) => void;
    onclearscope: () => void;
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
  {#if hits.length > 0}
    <p class="summary muted">
      <strong>{hits.length}{truncated ? "+" : ""}</strong> match{hits.length === 1 ? "" : "es"} for
      <strong>“{query}”</strong>
      in {groups.length} VOD{groups.length === 1 ? "" : "s"}{#if truncated}. Showing the first {hits.length}; add a word
        to narrow it down{/if}
    </p>
  {/if}
  {#each groups as group (group.vodId)}
    <section>
      {#if !scope}
        <h2>
          {#if group.vod}<span class="platform {group.vod.platform}"></span>{/if}
          <span class="title">{group.vod?.title ?? "Removed VOD"}</span>
          <span class="n muted">{group.hits.length}</span>
        </h2>
      {/if}
      {#each group.hits as hit (`${hit.source}-${hit.id}`)}
        <button class="row" onclick={() => onplay(hit.vod_id, hit.time)}>
          <span class="time">{formatTime(hit.time)}</span>
          <span class="text" class:chat={hit.source === "chat"}>
            {#if hit.source === "chat"}<span class="said-in" title="Written in chat"
                ><Icon name="message" size={12} /></span
              ><span class="author" style:--author={hit.color}>{hit.author}</span>{/if}
            {#each hit.parts as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
          </span>
        </button>
      {/each}
    </section>
  {:else}
    <div class="none">
      <span class="glyph muted"><Icon name="search" size={22} /></span>
      <p>
        No matches for <strong>“{query}”</strong>{#if scope}{" "}in <strong>{scope.title}</strong>{/if}.
      </p>
      <p class="muted">Search looks for these exact words, in this order. Try fewer words.</p>
      {#if scope}
        <button onclick={onclearscope}>Search all VODs</button>
      {/if}
    </div>
  {/each}
</div>

<style>
  .results {
    padding: 10px 20px 32px;
    max-width: 960px;
  }

  .none {
    margin: 10vh auto;
    max-width: 420px;
    text-align: center;
  }

  .none p {
    margin: 0 0 10px;
  }

  .glyph {
    width: 44px;
    height: 44px;
    margin-bottom: 10px;
  }

  .summary {
    font-size: 12px;
    margin: 4px 0 12px;
  }

  .summary strong {
    color: var(--text);
    font-weight: 600;
  }

  section {
    margin-bottom: 20px;
  }

  h2 {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    gap: 8px;
    align-items: baseline;
    margin: 0 -20px 4px;
    padding: 6px 20px;
    font-size: 13px;
    background: var(--bg);
  }

  h2 .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .n {
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }

  .text {
    min-width: 0;
  }

  .text.chat {
    font-size: 13px;
  }

  .said-in {
    color: var(--muted);
    margin-right: 6px;
  }

  /* As in chat beside the transcript (ChatMessage.svelte). */
  .author {
    font-weight: 600;
    margin-right: 6px;
    color: color-mix(in srgb, var(--author, var(--muted)) 65%, var(--text));
  }
</style>
