<script lang="ts">
  import type { SearchHit, Vod } from "./api";
  import { DEMO_NAMES } from "./demo";
  import { formatTime } from "./platform";
  import Icon from "./Icon.svelte";

  let {
    hits,
    truncated,
    vods,
    query,
    scope,
    columns,
    onplay,
    onclearscope,
  }: {
    hits: SearchHit[];
    truncated: boolean;
    vods: Map<number, Vod>;
    query: string;
    /** The VOD the search is limited to, if any. */
    scope: Vod | undefined;
    /** Audio and chat matches side by side, as beside the transcript. */
    columns: boolean;
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

  // In columns, each audio match with the chat matches from its time until the
  // next audio match's (hits are in time order within a VOD). Chat matches
  // before the first audio match get a row of their own.
  interface Row {
    audio: SearchHit | null;
    chat: SearchHit[];
  }
  function rowsOf(hits: SearchHit[]): Row[] {
    const rows: Row[] = [];
    for (const hit of hits) {
      if (hit.source === "speech") rows.push({ audio: hit, chat: [] });
      else {
        if (rows.length === 0) rows.push({ audio: null, chat: [] });
        rows[rows.length - 1].chat.push(hit);
      }
    }
    return rows;
  }
  const key = (hit: SearchHit) => `${hit.source}-${hit.id}`;
</script>

{#snippet match(hit: SearchHit)}
  <button class="row" class:from-chat={hit.source === "chat"} onclick={() => onplay(hit.vod_id, hit.time)}>
    <span class="time">{formatTime(hit.time)}</span>
    <span class="text">
      {#if hit.source === "chat"}{#if !columns}<span class="chat-icon" title="Written in chat"
            ><Icon name="message" size={12} /></span
          >{/if}<span class="chat-author" style:--author={hit.color} title={DEMO_NAMES}>{hit.author}</span>{/if}
      {#each hit.parts as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
    </span>
  </button>
{/snippet}

<div class="results" class:columns>
  {#if hits.length > 0}
    <p class="summary muted">
      <strong>{hits.length}{truncated ? "+" : ""}</strong> match{hits.length === 1 ? "" : "es"} for
      <strong>“{query}”</strong>
      in {groups.length} VOD{groups.length === 1 ? "" : "s"}{#if truncated}. Showing the first {hits.length}; add a word
        to narrow it down{/if}
    </p>
    {#if columns}
      <div class="line heads">
        <span class="eyebrow">Audio</span>
        <span class="eyebrow chat-head">Chat</span>
      </div>
    {/if}
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
      {#if columns}
        {#each rowsOf(group.hits) as row (row.audio ? key(row.audio) : `lead-${group.vodId}`)}
          <div class="line">
            {#if row.audio}{@render match(row.audio)}{:else}<span></span>{/if}
            <div class="chat">
              {#each row.chat as hit (key(hit))}{@render match(hit)}{/each}
            </div>
          </div>
        {/each}
      {:else}
        {#each group.hits as hit (key(hit))}{@render match(hit)}{/each}
      {/if}
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

  .row.from-chat .text {
    font-size: 13px;
  }

  .results.columns {
    max-width: 1320px;
  }

  /* As beside the transcript (Transcript.svelte). */
  .line {
    display: grid;
    grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
    gap: 16px;
  }

  .chat {
    border-left: 1px solid var(--border);
    padding-left: 18px;
    min-width: 0;
  }

  .heads {
    margin-bottom: 4px;
  }

  .heads .chat-head {
    padding-left: 17px;
  }

  .chat-icon {
    color: var(--muted);
    margin-right: 6px;
  }
</style>
