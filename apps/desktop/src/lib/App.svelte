<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import {
    api,
    docKey,
    docOfKey,
    events,
    OVERTAKEN,
    type LibraryItem,
    type SearchHit,
    type SearchResponse,
    type SetupStatus,
    type UnlistenFn,
    type Vod,
  } from "./api";
  import Library, { type LibraryRow } from "./Library.svelte";
  import Player from "./Player.svelte";
  import Results from "./Results.svelte";
  import SearchFilter, { DEFAULT_WITHIN, type Sources } from "./SearchFilter.svelte";
  import SearchGrouping from "./SearchGrouping.svelte";
  import { removeFilter } from "./queryChips";
  import Settings from "./Settings.svelte";
  import Setup from "./Setup.svelte";
  import Transcript from "./Transcript.svelte";
  import { ownPlayer } from "./ownPlayer.svelte";
  import EditView from "./EditView.svelte";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { load, save } from "./stored";
  import Modal from "./Modal.svelte";
  import { AUTHOR, SUPPORT } from "./links";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { DEMO, demoOff, keysForApp } from "./demo";
  import { is, keyHint, labelOf } from "./keys.svelte";
  import { typesText, type ActionId } from "./keymap";
  import { updates } from "./updates.svelte";
  import { savePlayable } from "./playable";
  import { settings } from "./appSettings.svelte";
  import { theme } from "./theme.svelte";
  import { cuts } from "./cuts.svelte";
  import { clips } from "./clips.svelte";
  import { toast } from "./toasts.svelte";
  import Toast from "./Toast.svelte";
  import type { Doc } from "./api";
  import { nav, searching, type Search, type Step } from "./nav.svelte";
  import { showsProgress } from "./stretches";
  import { removing } from "./removal.svelte";
  import { selectionKey } from "./bulk";

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
  // The filter's window and who, for this run of the app: defaults the typed
  // within: and from: override.
  let within = $state(DEFAULT_WITHIN);
  let from = $state("");
  let results = $state<SearchResponse | null>(null);
  // The sources the results are from: typed in: over the filter's.
  const lookingThrough = $derived(results?.query.sources ?? sources);
  let searchError = $state<string | null>(null);
  // The results set aside for the VOD under them (a match double-clicked),
  // the words kept in the box: back again when the box is focused or
  // edited, a VOD is picked from the list, or Back takes them there.
  let aside = $state(false);
  const showResults = $derived(!aside && (results !== null || searchError !== null));

  let showSettings = $state(false);

  let searchInput = $state<HTMLInputElement>();
  // The search shortcut, as this computer writes it.
  const searchKey = $derived(labelOf("search.focus"));
  // The empty library's tip: a VOD's main keys, as the map has them now; one
  // whose actions have no key is left out.
  const KEY_TIPS: { ids: ActionId[]; does: string }[] = [
    { ids: ["play.toggle"], does: "play/pause" },
    { ids: ["seek.back5", "seek.forward5"], does: "5 s" },
    { ids: ["seek.back10", "seek.forward10"], does: "10 s" },
    { ids: ["seek.back1", "seek.forward1"], does: "1 s" },
  ];
  const keyTips = $derived(
    KEY_TIPS.map((t) => ({ does: t.does, keys: t.ids.map(labelOf).filter(Boolean) })).filter((t) => t.keys.length),
  );
  // The document in view, whose history undo and redo go through: the edit
  // open, or else the VOD shown (its marks and clip bar). The backend's told,
  // as changes made now are made from it.
  const view = $derived<Doc | null>(
    edits.openId !== null
      ? { kind: "edit", id: edits.openId }
      : selectedId !== null
        ? { kind: "vod", id: selectedId }
        : null,
  );
  $effect(() => {
    if (!DEMO) api.setView(view).catch(() => {});
  });

  // Back and forward (nav.svelte.ts): a step each time what's open changes,
  // or a search's results come up in its place or go.
  const search = $derived<Search>({
    query,
    all: scopeAll,
    audio: sources.audio,
    chat: sources.chat,
    within,
    from,
    aside,
  });
  $effect(() => {
    const doc = view;
    const now = search;
    untrack(() => {
      nav.arrived(doc, now);
      // Where the body is scrolled to now (swapping results and transcript
      // may move it without a scroll).
      noteScroll();
    });
  });
  nav.attach(goTo);
  /** How far the body (the results, or else the transcript) is scrolled, for back and forward. */
  function noteScroll() {
    const top = body?.scrollTop ?? 0;
    nav.note(view, searching(search) ? { results: top } : { scroll: top });
  }
  /** Takes step `step`: its view open again, paused where it was, with its
   *  search, and the search's results if it had them, scrolled as they were
   *  (the search runs again, so a VOD removed since has no rows there). */
  function goTo(step: Step) {
    revealing = null;
    query = step.search.query;
    aside = step.search.aside;
    scopeAll = step.search.all;
    sources = { audio: step.search.audio, chat: step.search.chat };
    within = step.search.within;
    from = step.search.from;
    const top = searching(step.search) ? step.left.results : step.left.scroll;
    const doc = step.doc;
    if (doc?.kind === "edit") {
      edits.openEdit(doc.id);
      if (top) bringScroll(top);
      return;
    }
    edits.openEdit(null);
    selectedId = doc?.id ?? null;
    // Brought back as it was left (Player.svelte reads nav.bringBack).
    if (doc) playing = { vodId: doc.id, at: null };
    if (top) bringScroll(top);
  }
  /** Scrolls the transcript (or results) back to `top`, as it fills in. */
  function bringScroll(top: number) {
    const until = performance.now() + 2000;
    const step = () => {
      const list = body;
      if (list) list.scrollTop = top;
      if (performance.now() < until && (!list || Math.abs(list.scrollTop - top) > 1)) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }
  /** What a step goes to, for the arrows' titles. */
  function named(step: Step | undefined): string {
    const doc = step?.doc;
    if (!doc) return "the library";
    if (doc.kind === "vod") return `“${vodsById.get(doc.id)?.title ?? "the VOD"}”`;
    const edit = edits.list.find((e) => e.id === doc.id) ?? (edits.open?.id === doc.id ? edits.open : undefined);
    return `“${edit?.title ?? "the edit"}”`;
  }
  // The mouse's back and forward buttons.
  function onMouseNav(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    if (e.type === "mouseup") e.button === 3 ? nav.goBack() : nav.goForward();
  }

  /** Undoes (or redoes) the newest change in the view's history, and says what it was. */
  function undoRedo(redo: boolean, on?: Doc) {
    // Playing a search result shows its VOD (marks, clip bar) without
    // selecting it: what's changed there is undone there too.
    const doc = on ?? view ?? (playing && edits.openId === null ? { kind: "vod" as const, id: playing.vodId } : null);
    if (!doc || DEMO) return;
    (redo ? api.redo(doc) : api.undo(doc)).then(
      (done) =>
        toast.show(
          done ? `${redo ? "Redone" : "Undone"}: ${done.label}` : redo ? "Nothing to redo" : "Nothing to undo",
          // The toast's button: on what was undone, whatever's open by then.
          done ? { label: redo ? "Undo" : "Redo", run: () => undoRedo(!redo, doc) } : undefined,
        ),
      (e) => toast.show(`Couldn't ${redo ? "redo" : "undo"}: ${e}`),
    );
  }

  function onWindowKey(e: KeyboardEvent) {
    // Taken already by what has the focus (Alt+← moving an emote provider).
    if (!keysForApp() || e.defaultPrevented) return;
    // A modal window (Settings) has the keys while it's open: undo, back and
    // search mustn't reach the view behind it. Escape closes it (Modal.svelte).
    if (document.querySelector('[aria-modal="true"]')) return;
    // Ctrl/Cmd+Z undoes, Ctrl/Cmd+Shift+Z or Ctrl+Y redoes; text fields keep their own.
    const target = e.target as HTMLElement | null;
    const typing = !!target?.closest("input, textarea, select, [contenteditable]");
    const undo = is(e, "undo");
    if (!typing && (undo || is(e, "redo"))) {
      e.preventDefault();
      undoRedo(!undo);
      return;
    }
    // Back and forward: Alt+Left and Alt+Right, or Cmd+[ and Cmd+] on a Mac.
    const back = is(e, "nav.back");
    if (!typing && (back || is(e, "nav.forward"))) {
      e.preventDefault();
      if (back) nav.goBack();
      else nav.goForward();
      return;
    }
    // A set selected: Escape lets it go, Delete removes the library's;
    // Ctrl+A (⌘A) selects all of the clip bar or the list (bulk.ts).
    if (!typing && selectionKey(e, rows, (id) => selectedId === id && pick(null))) {
      e.preventDefault();
      return;
    }
    // Ctrl/Cmd+F jumps to search from anywhere, even another text field;
    // a key of Search's that types a character there (Search on "/") is the field's.
    if (is(e, "search.focus") && !(typing && typesText(e))) {
      e.preventDefault();
      aside = false;
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
  // The VOD in the player's place, and the jump asked of it (none: it opens
  // where it was left, Player.svelte).
  let playing = $state<{ vodId: number; at: { time: number; play: boolean } | null } | null>(null);
  let playingVod = $derived(playing ? vodsById.get(playing.vodId) : undefined);

  const unlisten: UnlistenFn[] = [];

  async function refreshSetup() {
    setup = await api.setupStatus();
  }

  // News of a VOD taken out of the list (kept for edits) doesn't put it back.
  function upsert(vod: Vod) {
    const i = vods.findIndex((v) => v.id === vod.id);
    if (!vod.listed) {
      if (i >= 0) vods.splice(i, 1);
    } else if (i >= 0) vods[i] = vod;
    else vods.unshift(vod);
  }

  // The library's list: VODs and edits in one order (`library_order`), by
  // key ("vod:3", "edit:2"). One added since it was read goes on top as it
  // appears, as the library puts it.
  let order = $state<string[]>([]);
  async function loadOrder() {
    order = (await api.libraryOrder().catch(() => [])).map(docKey);
  }
  $effect(() => {
    const keys = [
      ...vods.map((v) => docKey({ kind: "vod", id: v.id })),
      ...edits.list.map((e) => docKey({ kind: "edit", id: e.id })),
    ];
    untrack(() => {
      const known = new Set(order);
      const added = keys.filter((key) => !known.has(key));
      if (added.length) order = [...added, ...order];
    });
  });
  const rows = $derived.by(() => {
    const byKey = new Map<string, LibraryRow>([
      ...vods.map((vod) => [docKey({ kind: "vod", id: vod.id }), { kind: "vod", vod }] as const),
      ...edits.list.map((edit) => [docKey({ kind: "edit", id: edit.id }), { kind: "edit", edit }] as const),
    ]);
    return order.map((key) => byKey.get(key)).filter((row) => row !== undefined);
  });

  let reordering: Promise<void> = Promise.resolve();
  // Rows missing from `items` (added since the reorder began) stay on top.
  function reorder(items: LibraryItem[]) {
    const listed = new Set(items.map(docKey));
    const rowDoc = (r: LibraryRow) =>
      docKey(r.kind === "vod" ? { kind: "vod", id: r.vod.id } : { kind: "edit", id: r.edit.id });
    order = [...rows.map(rowDoc).filter((k) => !listed.has(k)), ...items.map(docKey)];
    const position = new Map(order.map((key, i) => [key, i]));
    const at = (id: number) => position.get(docKey({ kind: "vod", id })) ?? -1;
    vods = [...vods].sort((a, b) => at(a.id) - at(b.id));
    // One save at a time, in order: holding Alt+↓ sends many.
    const all: LibraryItem[] = order.map(docOfKey);
    reordering = reordering.then(() =>
      api.reorderLibrary(all).catch(async (e) => {
        console.error("reorder failed", e);
        // If reloading fails too, the chain must still go on for the next reorder.
        await reload().catch(() => {});
      }),
    );
    runSearch();
  }

  // Events that arrive while the list loads (at the start, or again after a
  // reorder failed) are held and applied after it, so the list (possibly read
  // before them) doesn't overwrite them.
  let loaded = false;
  const early: (() => void)[] = [];
  const afterLoad =
    <A extends unknown[]>(f: (...args: A) => void) =>
    (...args: A) =>
      loaded ? f(...args) : early.push(() => f(...args));
  async function reload() {
    loaded = false;
    try {
      vods = await api.listVods();
      await loadOrder();
    } finally {
      loaded = true;
      early.splice(0).forEach((apply) => apply());
    }
  }

  // The theme chosen in Settings, and again whenever it changes.
  $effect(() => {
    const chosen = settings.value?.theme;
    if (chosen) theme.apply(chosen);
  });

  onMount(async () => {
    // Whether VODs from sites play in the app's own player: known before one opens.
    void ownPlayer.ask();
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
    // A document put back by undo or redo: what shows it loads it again.
    unlisten.push(
      await events.restored((doc) => {
        if (doc.kind === "edit") {
          edits.reload(doc.id);
          cuts.reload(doc);
          clips.reload(doc);
        } else {
          cuts.reload(doc);
          clips.reload(doc);
        }
      }),
    );
    // What this computer plays, for videos kept from other sites; in the background.
    void savePlayable();
    unlisten.push(
      await events.vod(
        afterLoad((vod) => {
          const was = vodsById.get(vod.id);
          upsert(vod);
          // Search covers what's transcribed and the chat fetched: it's run
          // again when more of a VOD is transcribed, a request ends (its lines
          // in) or starts over lines, or its chat comes in.
          const changed =
            !was ||
            vod.transcribed !== was.transcribed ||
            vod.status !== was.status ||
            vod.stretch !== was.stretch ||
            vod.chat !== was.chat;
          if (changed) runSearch();
        }),
      ),
      await events.vodProgress(
        afterLoad((id, stretch, status, progress) => {
          const vod = vods.find((v) => v.id === id);
          // The row shows a running request before any other (stretches.ts).
          if (vod && showsProgress(vod.stretch, vod.status, stretch, status)) {
            vod.stretch = stretch;
            vod.status = status;
            vod.progress = progress;
          }
        }),
      ),
      await events.vodRemoved(
        afterLoad((id) => {
          nav.forget({ kind: "vod", id });
          removing.delete(id);
          vods = vods.filter((v) => v.id !== id);
          if (selectedId === id) selectedId = null;
          if (playing?.vodId === id) playing = null;
          runSearch();
        }),
      ),
    );
    try {
      await refreshSetup();
      await reload();
    } finally {
      // The busy cursor (app.css `html.starting`) goes once the library is in.
      document.documentElement.classList.remove("starting");
    }
  });
  onDestroy(() => unlisten.forEach((u) => u()));

  // Searches run as you type; a sequence number drops responses that arrive
  // out of order. Each is one typed: one overtaken by a newer one stops, and
  // if the page's newest was overtaken (two sent close together arriving the
  // other way round), it's asked again.
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
      const r = await api.search(
        q,
        {
          vodId: scopeId,
          speech: sources.audio,
          chat: sources.chat,
          within,
          from: from.trim() || null,
        },
        true,
      );
      if (mine === seq) {
        results = r;
        searchError = null;
      }
    } catch (e) {
      if (mine !== seq) return;
      if (String(e) === OVERTAKEN) void runSearch();
      else searchError = String(e);
    }
  }

  $effect(() => {
    void query;
    void scopeId;
    void sources.audio;
    void sources.chat;
    void within;
    void from;
    clearTimeout(timer);
    timer = setTimeout(runSearch, 120);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    updates.busy = vods.some((v) => v.status === "downloading" || v.status === "transcribing");
  });

  // An edit open shows in the player's place, its transcript in the body's,
  // unless a search's results are up.
  const editOpen = $derived(edits.openId !== null && !showResults);

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

  /** The search match last double-clicked, for its VOD's transcript to show (Transcript.svelte). */
  let revealing = $state<SearchHit | null>(null);
  /** Match `hit` double-clicked: its two clicks have played it, and now the
   *  results give way to its VOD's transcript, scrolled to it in the
   *  transcript or the chat (a step: back brings the results back). The
   *  words stay in the box, and the search's scope with them: results over
   *  every VOD stay so with this one open, to come back as they were. */
  function reveal(hit: SearchHit) {
    if (scopeId !== hit.vod_id) scopeAll = true;
    select(hit.vod_id);
    aside = true;
    revealing = hit;
  }

  // Selecting a VOD opens its player paused, ready to jump around.
  /** Out of the open edit to VOD `vodId`, paused at `time` (a source link was
   *  followed); what the edit plays of it stays lit (`edits.lit`). */
  function openVodAt(vodId: number, time: number) {
    const lit = edits.lit;
    edits.openEdit(null);
    edits.lit = lit;
    selectedId = vodId;
    if (playing?.vodId === vodId) playing.at = { time, play: false };
    else playing = { vodId, at: { time, play: false } };
  }

  /** Opens VOD `id` (or none), paused where its playhead was left this session (its start if it's new),
   *  unless it's the one on screen. */
  function select(id: number | null) {
    if (id !== edits.lit?.vodId) edits.lit = null;
    // On screen already: as it is. Else it opens where it was left (Player.svelte).
    const shown = edits.openId === null && playing?.vodId === id;
    edits.openEdit(null);
    selectedId = id;
    if (id !== null && !shown) playing = { vodId: id, at: null };
  }

  /** VOD `id` picked from the list: opened, with a search's results over it
   *  (limited to it) if the box has words, even ones set aside. */
  function pick(id: number | null) {
    aside = false;
    select(id);
  }

  // Edits show in the clip bar under the player (ClipStrip.svelte); one opened
  // there takes the main area, and a VOD picked (or played) takes it back.
  edits.start();
  $effect(() => {
    if (edits.openId !== null) selectedId = null;
  });
</script>

<svelte:window
  onkeydown={onWindowKey}
  ondragover={keepDropsInApp}
  ondrop={keepDropsInApp}
  onmousedown={onMouseNav}
  onmouseup={onMouseNav}
/>

{#if setup === null}
  <!-- Loading -->
{:else if !ready}
  <Setup status={setup} onchange={refreshSetup} />
{:else}
  <div class="app">
    <Library {rows} {selectedId} onselect={pick} onreorder={reorder} onopenedit={(id) => edits.openEdit(id)} />
    <!-- In the website's demo, the page has its own <main>. -->
    <svelte:element this={DEMO ? "div" : "main"} class="main">
      <div class="toolbar">
        <!-- The window's way back and forward between what was open (nav.svelte.ts). -->
        <div class="nav">
          <button
            class="icon back"
            onclick={() => nav.goBack()}
            disabled={!nav.back.length}
            title={nav.back.length ? `Back to ${named(nav.back.at(-1))}${keyHint("nav.back")}` : "Back"}
            aria-label="Back"
          >
            <Icon name="arrow-left" size={18} />
          </button>
          <button
            class="icon forward"
            onclick={() => nav.goForward()}
            disabled={!nav.forward.length}
            title={nav.forward.length ? `Forward to ${named(nav.forward.at(-1))}${keyHint("nav.forward")}` : "Forward"}
            aria-label="Forward"
          >
            <Icon name="arrow-right" size={18} />
          </button>
        </div>
        <div class="search">
          <span class="search-icon muted"><Icon name="search" /></span>
          <input
            type="search"
            placeholder="Search for a phrase…"
            bind:value={query}
            bind:this={searchInput}
            onfocus={() => (aside = false)}
            onclick={() => (aside = false)}
            oninput={() => (aside = false)}
            aria-label="Search"
            autocorrect="off"
            autocapitalize="off"
            spellcheck="false"
            title="Search{keyHint('search.focus')}"
          />
        </div>
        <SearchFilter
          bind:scopeAll
          bind:sources
          bind:within
          bind:from
          {selected}
          vods={vodsById}
          typed={results?.query ?? null}
        />
        <button
          class="icon"
          class:on={showSettings}
          onclick={() => (showSettings = !showSettings)}
          {...demoOff("The web demo has no settings. In the app: GPU speed-up, video downloader updates and storage.")}
          aria-label="Settings"
          aria-haspopup="dialog"
          aria-expanded={showSettings}
          title="Settings"><Icon name="settings" size={18} /></button
        >
        <Modal bind:open={showSettings} label="Settings">
          <Settings
            oncleared={() => {
              showSettings = false;
              refreshSetup();
            }}
          />
        </Modal>
      </div>

      {#if showResults && results}
        <SearchGrouping query={results.query} onremove={(text) => (query = removeFilter(query, text))} />
      {/if}

      {#if editOpen}
        <EditView vods={vodsById} onopenvod={openVodAt} />
      {:else if playing && playingVod}
        {#key playing.vodId}
          <Player vod={playingVod} at={playing.at} />
        {/key}
      {/if}

      <div class="body" bind:this={body} onscroll={noteScroll}>
        {#if showResults && searchError}
          <p class="error-text pad">{searchError}</p>
        {:else if showResults && results?.query.error}
          <!-- What's wrong with the text shows under the box, in the grouping's place. -->
        {:else if showResults && results}
          <Results
            hits={results.hits}
            truncated={results.truncated}
            vods={vodsById}
            query={query.trim()}
            scope={scopeAll ? undefined : selected}
            columns={lookingThrough.audio && lookingThrough.chat}
            onplay={play}
            onreveal={reveal}
            onclearscope={() => (scopeAll = true)}
          />
        {:else if editOpen}
          {#key edits.openId}
            <Transcript edit={edits.openId} vods={vodsById} onplay={(time) => edits.playAt(time)} />
          {/key}
        {:else if selected}
          <!-- Made afresh for each VOD, so nothing of the last one (its lines, row
               heights, a seek in progress) shows under the next while it loads. -->
          {#key selected.id}
            <Transcript
              vod={selected}
              reveal={revealing?.vod_id === selected.id ? revealing : null}
              onrevealed={() => (revealing = null)}
              onplay={(time) => play(selected.id, time)}
            />
          {/key}
        {:else}
          <div class="home">
            <div class="empty">
              <span class="glyph muted"><Icon name={vods.length === 0 ? "film" : "search"} size={28} /></span>
              {#if vods.length === 0}
                <h2>Your library is empty</h2>
                <p class="muted">
                  Paste a link from Kick, Twitch, YouTube or another video site on the left, or drop a video file here.
                  VODscribe turns what's said into text on your computer, and then you can search every word.
                </p>
              {:else}
                <h2>Search every VOD</h2>
                <p class="muted">
                  <!-- Search's key can be taken away (Settings › Controls): then the box is to click. -->
                  {#if searchKey}
                    Press <kbd>{searchKey}</kbd> to search
                  {:else}
                    Search
                  {/if}
                  all {vods.length} VOD{vods.length === 1 ? "" : "s"}, or pick one on the left to read its transcript.
                </p>
                {#if keyTips.length}
                  <p class="keys muted">
                    <Icon name="keyboard" size={14} /> While a VOD is open:
                    {#each keyTips as tip, i (tip.does)}{i ? ", " : ""}{#each tip.keys as k, j (j)}<kbd>{k}</kbd>{/each}
                      {tip.does}{/each}.
                  </p>
                {/if}
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
    </svelte:element>
  </div>
  <Toast />
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

  .nav {
    display: flex;
    gap: 2px;
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
