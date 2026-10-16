<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { api, events, type LibraryItem, type SearchHit, type SetupStatus, type UnlistenFn, type Vod } from "./api";
  import Library, { type LibraryRow } from "./Library.svelte";
  import Player from "./Player.svelte";
  import Results from "./Results.svelte";
  import SearchFilter, { type Sources } from "./SearchFilter.svelte";
  import Settings from "./Settings.svelte";
  import Setup from "./Setup.svelte";
  import Transcript from "./Transcript.svelte";
  import EditView from "./EditView.svelte";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { load, save } from "./stored";
  import Popover from "./Popover.svelte";
  import { AUTHOR, SUPPORT } from "./links";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { DEMO, demoOff, keysForApp } from "./demo";
  import { os } from "./platform";
  import { updates } from "./updates.svelte";
  import { savePlayable } from "./playable";
  import { settings } from "./appSettings.svelte";
  import { theme } from "./theme.svelte";

  let setup = $state<SetupStatus | null>(null);
  let ready = $derived(setup !== null && setup.model && setup.yt_dlp && setup.ffmpeg && !setup.running);

  let vods = $state<Vod[]>([]);
  let vodsById = $derived(new Map(vods.map((v) => [v.id, v])));
  let selectedId = $state<number | null>(null);
  let selected = $derived(selectedId === null ? undefined : vodsById.get(selectedId));

  let query = $state("");
  // With a VOD selected, searches cover just it unless the filter is set to all VODs.
  let scopeAll = $state(false);
  let scopeId = $derived(scopeAll ? null : selectedId);
  // What a search looks through, remembered across launches. Both are on at
  // first, so chat search is there to be found. More filters join these as
  // the search grows; the scope isn't kept, since it follows the selection.
  const SOURCES = "vodscribe.searchSources";
  let sources = $state<Sources>(savedSources());
  function savedSources(): Sources {
    try {
      const saved = JSON.parse(load(SOURCES) ?? "null");
      const audio = saved?.audio !== false;
      const chat = saved?.chat !== false;
      // Never neither, whatever was saved.
      return audio || chat ? { audio, chat } : { audio: true, chat: true };
    } catch {
      return { audio: true, chat: true };
    }
  }
  $effect(() => {
    save(SOURCES, JSON.stringify({ audio: sources.audio, chat: sources.chat }));
  });
  let results = $state<{ hits: SearchHit[]; truncated: boolean } | null>(null);
  let searchError = $state<string | null>(null);

  let showSettings = $state(false);
  let settingsEl = $state<HTMLElement>();

  let searchInput = $state<HTMLInputElement>();
  // The search shortcut, as this computer writes it.
  const searchKey = os === "mac" ? "⌘F" : "Ctrl+F";
  function onWindowKey(e: KeyboardEvent) {
    if (!keysForApp()) return;
    // Ctrl/Cmd+F jumps to search from anywhere, even another text field.
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      searchInput?.focus();
      searchInput?.select();
    }
  }
  // A link dropped where nothing handles it (during setup, say) would otherwise
  // navigate the whole window to it. The library's own drop handling still runs.
  // The website's demo leaves its page's drops alone.
  function keepDropsInApp(e: DragEvent) {
    if (!DEMO) e.preventDefault();
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

  // The library's list: VODs and edits in one order (`library_order`), by
  // key ("vod:3", "edit:2"). One added since it was read goes on top as it
  // appears, as the library puts it.
  let order = $state<string[]>([]);
  const keyOf = (item: LibraryItem) => `${item.kind}:${item.id}`;
  async function loadOrder() {
    order = (await api.libraryOrder().catch(() => [])).map(keyOf);
  }
  $effect(() => {
    const keys = [...vods.map((v) => `vod:${v.id}`), ...edits.list.map((e) => `edit:${e.id}`)];
    untrack(() => {
      const known = new Set(order);
      const added = keys.filter((key) => !known.has(key));
      if (added.length) order = [...added, ...order];
    });
  });
  const rows = $derived.by(() => {
    const byKey = new Map<string, LibraryRow>([
      ...vods.map((vod) => [`vod:${vod.id}`, { kind: "vod", vod }] as const),
      ...edits.list.map((edit) => [`edit:${edit.id}`, { kind: "edit", edit }] as const),
    ]);
    return order.map((key) => byKey.get(key)).filter((row) => row !== undefined);
  });

  let reordering: Promise<void> = Promise.resolve();
  // Rows missing from `items` (added since the reorder began) stay on top.
  function reorder(items: LibraryItem[]) {
    const listed = new Set(items.map(keyOf));
    order = [
      ...rows.map((r) => (r.kind === "vod" ? `vod:${r.vod.id}` : `edit:${r.edit.id}`)).filter((k) => !listed.has(k)),
      ...items.map(keyOf),
    ];
    const position = new Map(order.map((key, i) => [key, i]));
    vods = [...vods].sort((a, b) => (position.get(`vod:${a.id}`) ?? -1) - (position.get(`vod:${b.id}`) ?? -1));
    // One save at a time, in order: holding Alt+↓ sends many.
    const all = order.map((key) => {
      const [kind, id] = key.split(":");
      return { kind, id: Number(id) } as LibraryItem;
    });
    reordering = reordering.then(() =>
      api.reorderLibrary(all).catch(async (e) => {
        console.error("reorder failed", e);
        // If reloading fails too, the chain must still go on for the next reorder.
        vods = await api.listVods().catch(() => vods);
        await loadOrder();
      }),
    );
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

  // The theme chosen in Settings, and again whenever it changes.
  $effect(() => {
    const chosen = settings.value?.theme;
    if (chosen) theme.apply(chosen);
  });

  onMount(async () => {
    // The window starts hidden (tauri.conf.json) and is shown now that the page
    // is in place, in the theme chosen, rather than as a blank white window or
    // in the system's colors first. (Not after an animation frame: a hidden
    // window may never get one.)
    await settings.load();
    theme.apply(settings.value?.theme ?? "system");
    void getCurrentWindow()
      .show()
      .catch(() => {});
    unlisten.push(updates.start());
    // What this computer plays, for videos kept from other sites; in the background.
    void savePlayable();
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
    try {
      await refreshSetup();
      vods = await api.listVods();
      await loadOrder();
    } finally {
      // The busy cursor (app.css `html.starting`) goes once the library is in.
      document.documentElement.classList.remove("starting");
    }
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
      const r = await api.search(q, { vodId: scopeId, speech: sources.audio, chat: sources.chat });
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
    void sources.audio;
    void sources.chat;
    clearTimeout(timer);
    timer = setTimeout(runSearch, 120);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    updates.busy = vods.some((v) => v.status === "downloading" || v.status === "transcribing");
  });

  function play(vodId: number, time: number) {
    edits.openEdit(null);
    keepPressedInPlace();
    if (playing?.vodId === vodId) playing.at = { time, play: true };
    else playing = { vodId, at: { time, play: true } };
  }

  // Starting the player can move what was clicked: the player opens or
  // reappears above the list, swaps its poster for the video, and its frame
  // taking focus can scroll the page. For a moment after a click, each frame
  // (before it's painted) scrolls the list back so the clicked line stays put,
  // unless the user scrolls meanwhile.
  let body = $state<HTMLElement>();
  let pressed: { el: HTMLElement; at: number } | null = null;
  $effect(() => {
    const list = body;
    if (!list) return;
    const remember = (e: PointerEvent) => {
      const el = (e.target as HTMLElement).closest<HTMLElement>("button");
      pressed = el ? { el, at: performance.now() } : null;
    };
    list.addEventListener("pointerdown", remember);
    return () => list.removeEventListener("pointerdown", remember);
  });
  function keepPressedInPlace() {
    const list = body;
    const p = pressed;
    pressed = null;
    if (!list || !p || performance.now() - p.at > 1000 || !list.contains(p.el)) return;
    const el = p.el;
    const top = el.getBoundingClientRect().top;
    const until = performance.now() + 1000;
    let stop = false;
    const userScrolled = () => (stop = true);
    list.addEventListener("wheel", userScrolled, { passive: true });
    list.addEventListener("keydown", userScrolled);
    const step = () => {
      if (!stop && el.isConnected) {
        // The app's page never scrolls (the website's demo is part of one that does).
        if (!DEMO && document.scrollingElement) document.scrollingElement.scrollTop = 0;
        const moved = el.getBoundingClientRect().top - top;
        if (Math.abs(moved) >= 1) list.scrollTop += moved;
      }
      if (!stop && performance.now() < until) requestAnimationFrame(step);
      else {
        list.removeEventListener("wheel", userScrolled);
        list.removeEventListener("keydown", userScrolled);
      }
    };
    requestAnimationFrame(step);
  }

  // Selecting a VOD opens its player paused at the start, ready to jump around.
  /** Back out of the open edit, to the VOD that was open before it. */
  function closeEdit() {
    const back = playing?.vodId ?? null;
    edits.openEdit(null);
    if (back !== null) selectedId = back;
  }

  function select(id: number | null) {
    edits.openEdit(null);
    selectedId = id;
    if (id !== null && playing?.vodId !== id) playing = { vodId: id, at: { time: 0, play: false } };
  }

  // Edits show in the clip bar under the player (ClipStrip.svelte); one opened
  // there takes the main area, and a VOD picked (or played) takes it back.
  edits.start();
  $effect(() => {
    if (edits.openId !== null) selectedId = null;
  });
</script>

<svelte:window onkeydown={onWindowKey} ondragover={keepDropsInApp} ondrop={keepDropsInApp} />

{#if setup === null}
  <!-- Loading -->
{:else if !ready}
  <Setup status={setup} onchange={refreshSetup} />
{:else}
  <div class="app">
    <Library {rows} {selectedId} onselect={select} onreorder={reorder} onopenedit={(id) => edits.openEdit(id)} />
    <!-- In the website's demo, the page has its own <main>. -->
    <svelte:element this={DEMO ? "div" : "main"} class="main">
      <div class="toolbar">
        {#if edits.openId !== null}
          <!-- Out of the edit, to the VOD that was open: the window's way back. -->
          <button class="icon back" onclick={closeEdit} title="Back to {playingVod?.title ?? 'the VOD'}">
            <Icon name="arrow-left" size={18} />
          </button>
        {/if}
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
        </div>
        <SearchFilter bind:scopeAll bind:sources {selected} />
        <div class="settings-anchor" bind:this={settingsEl}>
          <button
            class="icon"
            class:on={showSettings}
            onclick={() => (showSettings = !showSettings)}
            {...demoOff(
              "The web demo has no settings. In the app: GPU speed-up, video downloader updates and storage.",
            )}
            aria-label="Settings"
            aria-expanded={showSettings}
            title="Settings"><Icon name="settings" size={18} /></button
          >
          <Popover bind:open={showSettings} anchor={settingsEl} label="Settings" width={340} padding={0}>
            <Settings
              oncleared={() => {
                showSettings = false;
                refreshSetup();
              }}
            />
          </Popover>
        </div>
      </div>

      {#if edits.openId !== null && !results && !searchError}
        <EditView vods={vodsById} />
      {:else}
        {#if playing && playingVod}
          {#key playing.vodId}
            <Player vod={playingVod} at={playing.at} />
          {/key}
        {/if}

        <div class="body" bind:this={body}>
          {#if searchError}
            <p class="error-text pad">{searchError}</p>
          {:else if results}
            <Results
              hits={results.hits}
              truncated={results.truncated}
              vods={vodsById}
              query={query.trim()}
              scope={scopeAll ? undefined : selected}
              columns={sources.audio && sources.chat}
              onplay={play}
              onclearscope={() => (scopeAll = true)}
            />
          {:else if selected}
            <!-- Made afresh for each VOD, so nothing of the last one (its lines, row
               heights, a seek in progress) shows under the next while it loads. -->
            {#key selected.id}
              <Transcript vod={selected} onplay={play} />
            {/key}
          {:else}
            <div class="home">
              <div class="empty">
                <span class="glyph muted"><Icon name={vods.length === 0 ? "film" : "search"} size={28} /></span>
                {#if vods.length === 0}
                  <h2>Your library is empty</h2>
                  <p class="muted">
                    Paste a link from Kick, Twitch, YouTube or another video site on the left, or drop a video file
                    here. VODscribe turns what's said into text on your computer, and then you can search every word.
                  </p>
                {:else}
                  <h2>Search every VOD</h2>
                  <p class="muted">
                    Press <kbd>{searchKey}</kbd> to search all {vods.length} VOD{vods.length === 1 ? "" : "s"}, or pick
                    one on the left to read its transcript.
                  </p>
                  <p class="keys muted">
                    <Icon name="keyboard" size={14} /> While a VOD is open: <kbd>Space</kbd> play/pause,
                    <kbd>←</kbd><kbd>→</kbd>
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
      {/if}
    </svelte:element>
  </div>
{/if}

<style>
  .app {
    display: grid;
    grid-template-columns: minmax(280px, 340px) 1fr;
    /* The website's demo sets its own height. */
    height: var(--app-height, 100vh);
  }

  .main {
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

  .settings-anchor {
    position: relative;
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
