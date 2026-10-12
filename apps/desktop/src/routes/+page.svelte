<script lang="ts">
  import "../app.css";
  import { onDestroy, onMount } from "svelte";
  import { api, events, type SearchHit, type SetupStatus, type UnlistenFn, type Vod } from "$lib/api";
  import Library from "$lib/Library.svelte";
  import Player from "$lib/Player.svelte";
  import Results from "$lib/Results.svelte";
  import Settings from "$lib/Settings.svelte";
  import Setup from "$lib/Setup.svelte";
  import Transcript from "$lib/Transcript.svelte";

  let setup = $state<SetupStatus | null>(null);
  let ready = $derived(setup !== null && setup.model && setup.yt_dlp && setup.ffmpeg && !setup.running);

  let vods = $state<Vod[]>([]);
  let vodsById = $derived(new Map(vods.map((v) => [v.id, v])));
  let selectedId = $state<number | null>(null);
  let selected = $derived(selectedId === null ? undefined : vodsById.get(selectedId));

  let query = $state("");
  let results = $state<{ hits: SearchHit[]; truncated: boolean } | null>(null);
  let searchError = $state<string | null>(null);

  // `at` is replaced on every jump; the player seeks rather than reloading.
  let showSettings = $state(false);
  let settingsEl = $state<HTMLElement>();

  // Close the settings panel on Escape or a click outside it.
  function onWindowKey(e: KeyboardEvent) {
    if (e.key === "Escape") showSettings = false;
  }
  function onWindowClick(e: MouseEvent) {
    if (showSettings && settingsEl && !settingsEl.contains(e.target as Node)) showSettings = false;
  }

  let playing = $state<{ vodId: number; at: { time: number; play: boolean } } | null>(null);
  let playingVod = $derived(playing ? vodsById.get(playing.vodId) : undefined);

  const unlisten: UnlistenFn[] = [];

  async function refreshSetup() {
    setup = await api.setupStatus();
  }

  function upsert(vod: Vod) {
    const i = vods.findIndex((v) => v.id === vod.id);
    if (i >= 0) vods[i] = vod;
    else vods.unshift(vod);
  }

  onMount(async () => {
    unlisten.push(
      await events.vod((vod) => {
        upsert(vod);
        if (vod.status === "done") runSearch();
      }),
      await events.vodProgress((id, status, progress) => {
        const vod = vods.find((v) => v.id === id);
        if (vod) {
          vod.status = status;
          vod.progress = progress;
        }
      }),
      await events.vodRemoved((id) => {
        vods = vods.filter((v) => v.id !== id);
        if (playing?.vodId === id) playing = null;
        runSearch();
      }),
    );
    await refreshSetup();
    vods = await api.listVods();
  });
  onDestroy(() => unlisten.forEach((u) => u()));

  // Searches run as you type; a sequence number drops responses that arrive out of order.
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  async function runSearch() {
    const mine = ++seq;
    const q = query.trim();
    if (!q) {
      results = null;
      return;
    }
    try {
      const r = await api.search(q, selectedId);
      if (mine === seq) {
        results = r;
        searchError = null;
      }
    } catch (e) {
      if (mine === seq) searchError = String(e);
    }
  }

  $effect(() => {
    void query;
    void selectedId;
    clearTimeout(timer);
    timer = setTimeout(runSearch, 120);
  });

  function play(vodId: number, time: number) {
    if (playing?.vodId === vodId) playing.at = { time, play: true };
    else playing = { vodId, at: { time, play: true } };
  }

  // Selecting a VOD opens its player paused at the start, ready to jump around.
  function select(id: number | null) {
    selectedId = id;
    if (id !== null && playing?.vodId !== id) playing = { vodId: id, at: { time: 0, play: false } };
  }
</script>

<svelte:window onkeydown={onWindowKey} onclick={onWindowClick} />

<svelte:head>
  <title>VODscribe</title>
</svelte:head>

{#if setup === null}
  <!-- Loading -->
{:else if !ready}
  <Setup status={setup} onchange={refreshSetup} />
{:else}
  <div class="app">
    <Library {vods} {selectedId} onselect={select} />
    <main>
      <div class="search">
        <input type="search" placeholder="Search for a phrase…" bind:value={query} />
        {#if selected}
          <span class="scope">
            in <strong>{selected.title}</strong>
            <button class="link" onclick={() => (selectedId = null)} aria-label="Search all VODs">✕</button>
          </span>
        {/if}
        <div class="settings-anchor" bind:this={settingsEl}>
          <button
            class="gear"
            class:on={showSettings}
            onclick={() => (showSettings = !showSettings)}
            aria-label="Settings"
            aria-expanded={showSettings}
            title="Settings">⚙</button
          >
          {#if showSettings}
            <div class="popover" role="dialog" aria-label="Settings">
              <Settings
                oncleared={() => {
                  showSettings = false;
                  refreshSetup();
                }}
              />
            </div>
          {/if}
        </div>
      </div>

      {#if playing && playingVod}
        {#key playing.vodId}
          <Player vod={playingVod} at={playing.at} onclose={() => (playing = null)} />
        {/key}
      {/if}

      <div class="body">
        {#if searchError}
          <p class="error-text pad">{searchError}</p>
        {:else if results}
          <Results
            hits={results.hits}
            truncated={results.truncated}
            vods={vodsById}
            query={query.trim()}
            scope={selected}
            onplay={play}
            onclearscope={() => (selectedId = null)}
          />
        {:else if selected}
          <Transcript vod={selected} onplay={play} />
        {:else}
          <div class="empty muted">
            {#if vods.length === 0}
              <p>Add a VOD on the left. Once it's transcribed, every word in it is searchable here.</p>
            {:else}
              <p>Type a phrase to search all {vods.length} VOD{vods.length === 1 ? "" : "s"}, or pick one on the left to read its transcript.</p>
            {/if}
          </div>
        {/if}
      </div>
    </main>
  </div>
{/if}

<style>
  .app {
    display: grid;
    grid-template-columns: minmax(260px, 320px) 1fr;
    height: 100vh;
  }

  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .search {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  .search input {
    flex: 1;
    font-size: 15px;
  }

  .settings-anchor {
    position: relative;
  }

  .gear {
    padding: 6px 10px;
  }

  .gear.on {
    background: var(--surface-2);
  }

  .popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 10;
    width: 320px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
  }

  .scope {
    display: flex;
    gap: 6px;
    align-items: center;
    max-width: 40%;
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
  }

  .scope strong {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text);
  }

  .body {
    flex: 1;
    overflow-y: auto;
  }

  .empty {
    max-width: 420px;
    margin: 15vh auto;
    text-align: center;
    padding: 0 16px;
  }

  .pad {
    padding: 0 16px;
  }
</style>
