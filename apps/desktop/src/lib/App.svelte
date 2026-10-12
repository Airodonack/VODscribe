<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, events, type SearchHit, type SetupStatus, type UnlistenFn, type Vod } from "./api";
  import Library from "./Library.svelte";
  import Player from "./Player.svelte";
  import Results from "./Results.svelte";
  import Settings from "./Settings.svelte";
  import Setup from "./Setup.svelte";
  import Transcript from "./Transcript.svelte";
  import Icon from "./Icon.svelte";
  import { AUTHOR, SUPPORT } from "./links";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { demoOff, keysForApp } from "./demo";

  let setup = $state<SetupStatus | null>(null);
  let ready = $derived(setup !== null && setup.model && setup.yt_dlp && setup.ffmpeg && !setup.running);

  let vods = $state<Vod[]>([]);
  let vodsById = $derived(new Map(vods.map((v) => [v.id, v])));
  let selectedId = $state<number | null>(null);
  let selected = $derived(selectedId === null ? undefined : vodsById.get(selectedId));

  let query = $state("");
  // With a VOD selected, searches cover just it unless the scope chip is toggled to all.
  let scopeAll = $state(false);
  let scopeId = $derived(scopeAll ? null : selectedId);
  let results = $state<{ hits: SearchHit[]; truncated: boolean } | null>(null);
  let searchError = $state<string | null>(null);

  let showSettings = $state(false);
  let settingsEl = $state<HTMLElement>();

  // Close the settings panel on Escape or a click outside it.
  let searchInput = $state<HTMLInputElement>();
  // The search shortcut, as this computer writes it.
  const searchKey = /Mac/.test(navigator.platform) ? "⌘F" : "Ctrl+F";
  function onWindowKey(e: KeyboardEvent) {
    if (!keysForApp()) return;
    if (e.key === "Escape") showSettings = false;
    // Ctrl/Cmd+F jumps to search from anywhere, even another text field.
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      searchInput?.focus();
      searchInput?.select();
    }
  }
  function onWindowClick(e: MouseEvent) {
    if (showSettings && settingsEl && !settingsEl.contains(e.target as Node)) showSettings = false;
  }

  // `at` is replaced on every jump; the player seeks rather than reloading.
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

  // VODs missing from `ids` (added since the reorder began) stay on top.
  function reorder(ids: number[]) {
    const listed = new Set(ids);
    vods = [
      ...vods.filter((v) => !listed.has(v.id)),
      ...ids.map((id) => vodsById.get(id)).filter((v) => v !== undefined),
    ];
    api.reorderVods(vods.map((v) => v.id)).catch(async (e) => {
      console.error("reorder failed", e);
      vods = await api.listVods();
    });
    runSearch();
  }

  // Events that arrive before the first list has loaded are held and applied
  // after it, so the list (possibly read before them) doesn't overwrite them.
  let loaded = false;
  const early: (() => void)[] = [];
  const afterLoad =
    <A extends unknown[]>(f: (...args: A) => void) =>
    (...args: A) =>
      loaded ? f(...args) : early.push(() => f(...args));

  onMount(async () => {
    unlisten.push(
      await events.vod(
        afterLoad((vod) => {
          upsert(vod);
          if (vod.status === "done") runSearch();
        }),
      ),
      await events.vodProgress(
        afterLoad((id, status, progress) => {
          const vod = vods.find((v) => v.id === id);
          if (vod) {
            vod.status = status;
            vod.progress = progress;
          }
        }),
      ),
      await events.vodRemoved(
        afterLoad((id) => {
          vods = vods.filter((v) => v.id !== id);
          if (playing?.vodId === id) playing = null;
          runSearch();
        }),
      ),
    );
    await refreshSetup();
    vods = await api.listVods();
    loaded = true;
    early.splice(0).forEach((apply) => apply());
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
      searchError = null;
      return;
    }
    try {
      const r = await api.search(q, scopeId);
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
    void scopeId;
    clearTimeout(timer);
    timer = setTimeout(runSearch, 120);
    return () => clearTimeout(timer);
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

{#if setup === null}
  <!-- Loading -->
{:else if !ready}
  <Setup status={setup} onchange={refreshSetup} />
{:else}
  <div class="app">
    <Library {vods} {selectedId} onselect={select} onreorder={reorder} />
    <main>
      <div class="toolbar">
        <div class="search">
          <span class="search-icon muted"><Icon name="search" /></span>
          <input
            type="search"
            placeholder="Search for a phrase…"
            bind:value={query}
            bind:this={searchInput}
            aria-label="Search"
            title="Search ({searchKey})"
          />
          {#if selected}
            <button
              class="scope"
              class:all={scopeAll}
              onclick={() => (scopeAll = !scopeAll)}
              aria-pressed={!scopeAll}
              title={scopeAll ? `Click to search only “${selected.title}”` : "Click to search all VODs"}
            >
              {scopeAll ? "All VODs" : "This VOD"}
            </button>
          {/if}
        </div>
        <div class="settings-anchor" bind:this={settingsEl}>
          <button
            class="icon"
            class:on={showSettings}
            onclick={() => (showSettings = !showSettings)}
            {...demoOff("In the app: GPU speed-up, video downloader updates and storage.")}
            aria-label="Settings"
            aria-expanded={showSettings}
            title="Settings"><Icon name="settings" size={18} /></button
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
          <Player vod={playingVod} at={playing.at} />
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
            scope={scopeAll ? undefined : selected}
            onplay={play}
            onclearscope={() => (scopeAll = true)}
          />
        {:else if selected}
          <Transcript vod={selected} onplay={play} />
        {:else}
          <div class="home">
            <div class="empty">
              <span class="glyph muted"><Icon name={vods.length === 0 ? "film" : "search"} size={28} /></span>
              {#if vods.length === 0}
                <h2>Your library is empty</h2>
                <p class="muted">
                  Paste a Kick, Twitch or YouTube link on the left, or drop a video file here. VODscribe turns what's
                  said into text on your computer, and then you can search every word.
                </p>
              {:else}
                <h2>Search every VOD</h2>
                <p class="muted">
                  Press <kbd>{searchKey}</kbd> to search all {vods.length} VOD{vods.length === 1 ? "" : "s"}, or pick
                  one on the left to read its transcript.
                </p>
                <p class="keys muted">
                  <Icon name="keyboard" size={14} /> While a VOD is open: <kbd>Space</kbd> play/pause, <kbd>←</kbd><kbd
                    >→</kbd
                  >
                  5 s,
                  <kbd>J</kbd><kbd>L</kbd> 10 s, <kbd>Shift</kbd>+arrows 1 s.
                </p>
              {/if}
            </div>
            <button class="credit" onclick={() => openUrl(SUPPORT)}>
              Made with <span class="heart">❤︎</span> by {AUTHOR}<span class="sep">·</span><span class="cta"
                >Help cover hosting on Ko-fi</span
              >
            </button>
          </div>
        {/if}
      </div>
    </main>
  </div>
{/if}

<style>
  .app {
    display: grid;
    grid-template-columns: minmax(280px, 340px) 1fr;
    /* The website's demo sets its own height. */
    height: var(--app-height, 100vh);
  }

  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .toolbar {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 10px 14px 10px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .search {
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .search-icon {
    position: absolute;
    left: 11px;
    display: flex;
    pointer-events: none;
  }

  .search input {
    padding-left: 34px;
    font-size: 15px;
  }

  /* Toggles whether a search covers the selected VOD or all of them. */
  .scope {
    position: absolute;
    right: 34px;
    height: 24px;
    padding: 0 10px;
    border: 1px solid transparent;
    border-radius: 12px;
    background: var(--accent-soft);
    color: var(--text);
    font-size: 12px;
    font-weight: 600;
  }

  .scope:hover:not(:disabled) {
    background: var(--accent-soft);
    border-color: var(--accent);
  }

  .scope.all {
    background: var(--surface-2);
    color: var(--muted);
    font-weight: 500;
  }

  .search:has(.scope) input {
    padding-right: 130px;
  }

  .settings-anchor {
    position: relative;
  }

  .popover {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 10;
    width: 340px;
    max-height: calc(var(--app-height, 100vh) - 80px);
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
  }

  .body {
    flex: 1;
    overflow-y: auto;
  }

  .home {
    display: flex;
    flex-direction: column;
    align-items: center;
    min-height: 100%;
  }

  .empty {
    max-width: 440px;
    margin: 14vh auto 32px;
    text-align: center;
    padding: 0 16px;
  }

  .empty h2 {
    margin: 0 0 6px;
    font-size: 17px;
  }

  .empty p {
    margin: 0 0 12px;
  }

  .keys {
    font-size: 12px;
    margin-top: 20px;
    line-height: 2;
  }

  .credit {
    gap: 0;
    margin: auto 0 18px;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 12px;
    font-weight: 400;
  }

  .credit:hover:not(:disabled) {
    color: var(--text);
  }

  .credit:hover .cta {
    text-decoration: underline;
  }

  .heart {
    color: #e5484d;
    margin: 0 3px;
  }

  .credit .sep {
    margin: 0 8px;
  }

  .cta {
    color: var(--accent);
  }

  .pad {
    padding: 0 16px;
  }
</style>
