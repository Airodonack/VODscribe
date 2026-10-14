<script lang="ts">
  import { flushSync, untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { api, events, type ChatOverview, type ChatWindow, type Line, type Vod } from "./api";
  import ChatMessage from "./ChatMessage.svelte";
  import Minimap from "./Minimap.svelte";
  import Spinner from "./Spinner.svelte";
  import Timeline from "./Timeline.svelte";
  import { layers } from "./layers.svelte";
  import { EVENTS, eventColor, eventUsd, type EventKind } from "./events";
  import Icon from "./Icon.svelte";
  import { formatTime } from "./platform";

  let { vod, onplay }: { vod: Vod; onplay: (vodId: number, time: number) => void } = $props();

  let lines = $state<Line[]>([]);
  let error = $state<string | null>(null);

  // Reload when the VOD changes, and periodically while it is still transcribing
  // (segments are indexed as they are produced).
  let vodId = $derived(vod.id);
  let step = $derived(vod.status === "transcribing" ? Math.floor(vod.progress * 20) : vod.status);
  // Only the latest request's answer counts, so a slow older reload can't win.
  let seq = 0;
  $effect(() => {
    const id = vodId;
    void step;
    const mine = ++seq;
    api.transcript(id).then(
      (l) => {
        if (mine !== seq) return;
        lines = l;
        error = null;
      },
      (e) => {
        if (mine === seq) error = String(e);
      },
    );
  });

  // Chat, beside the transcript and collapsible (remembered across VODs and
  // launches). A busy VOD's is a hundred thousand messages and more, so the
  // view never holds it all: it asks for the counts and events it draws on the
  // minimap (`overview`), and for each row it shows, that row's first messages
  // (`windows`). Both catch up as a fetch saves more.
  const CHAT_OPEN = "vodscribe.chatOpen";
  let chatError = $state<string | null>(null);
  let chatProgress = $state<number | null>(null);
  let chatOpen = $state(read(CHAT_OPEN) !== "false");
  let hasChat = $derived(vod.platform !== "local" && vod.chat !== undefined);
  // A video that wasn't a livestream has no chat: its toggle is grayed out and the column stays shut.
  let chatStatus = $derived(vod.chat);
  let noChat = $derived(chatStatus === "none");
  let showChat = $derived(hasChat && chatOpen && !noChat);
  let canHaveChat = $derived(hasChat && (chatStatus === "done" || chatStatus === "pending"));

  function read(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }

  let root: HTMLDivElement;

  // Makes `change`, keeping the line at the top of the view where it was:
  // showing or hiding chat changes every row's height, and chat arriving while
  // it's fetched grows rows above the view (Kick's fills in from the end).
  // WebKit has no scroll anchoring of its own (`overflow-anchor`).
  function keepPlace(change: () => void) {
    const scroller = root?.closest<HTMLElement>(".body");
    // The view starts below the header, which stays at the top.
    const top = head?.getBoundingClientRect().bottom ?? 0;
    // The first line that starts in view: one cut off at the top may shrink out of sight.
    const anchor = root
      ? [...root.querySelectorAll<HTMLElement>(".line[data-start]")].find((el) => el.getBoundingClientRect().top >= top)
      : undefined;
    const offset = anchor ? anchor.getBoundingClientRect().top - top : 0;
    // Read now: rows are keyed by position, so after the change this element may hold another line.
    const start = anchor?.dataset.start;
    // Applied at once, so the rows are measured as they now are.
    flushSync(change);
    if (!scroller || start === undefined) return;
    const same = root.querySelector<HTMLElement>(`.line[data-start="${start}"]`);
    if (same) scroller.scrollTop += same.getBoundingClientRect().top - top - offset;
  }

  function toggleChat() {
    keepPlace(() => (chatOpen = !chatOpen));
    try {
      localStorage.setItem(CHAT_OPEN, String(chatOpen));
    } catch {
      // Not remembered, which is fine.
    }
  }

  // Each transcript line with the chat sent while it was being said: from its
  // start to the next line's. Chat before the first line gets a row of its own.
  // Without a transcript yet, chat goes in rows of a minute each.
  const FAR = 1e12;
  interface Row {
    line: Line | null;
    from: number;
    to: number;
  }
  let rows = $derived.by((): Row[] => {
    const out: Row[] = lines.map((line, i) => ({ line, from: line.start, to: lines[i + 1]?.start ?? FAR }));
    if (!showChat) return out;
    if (lines.length === 0) {
      const minutes = Math.ceil((vod.duration ?? 0) / 60);
      return Array.from({ length: minutes }, (_, k) => ({
        line: null,
        from: k * 60,
        to: k === minutes - 1 ? FAR : (k + 1) * 60,
      }));
    }
    return [{ line: null, from: -FAR, to: lines[0].start }, ...out];
  });
  const keyOf = (row: Row) => `${row.from}:${row.to}`;

  // Rows are added as the end comes into view, so a long VOD opens at once
  // rather than laying out every row first. With busy chat a row is a few
  // hundred pixels tall, so a page is several screens.
  const PAGE = 60;
  let shown = $state(PAGE);
  let sentinel = $state<HTMLElement>();
  $effect(() => {
    void vodId;
    shown = PAGE;
  });
  $effect(() => {
    void shown;
    if (!sentinel) return;
    // Made again after each page, since the end may still be in view.
    const io = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && (shown += PAGE), {
      root: root.closest(".body"),
      rootMargin: "0px 0px 1500px 0px",
    });
    io.observe(sentinel);
    return () => io.disconnect();
  });

  // A row shows this many messages at first, so a burst of chat doesn't push
  // the transcript apart; each "more" shows this many more.
  const SHOWN = 8;
  const MORE = 200;
  let windows = $state(new Map<string, ChatWindow>());
  /** Windows to fetch again: chat arrived since (a fetch is under way). */
  const stale = new Set<string>();
  /** How many messages expanded rows show, by window. */
  const limits = new SvelteMap<string, number>();
  const limitOf = (row: Row) => limits.get(keyOf(row)) ?? SHOWN;
  /** Windows being fetched: their "more" buttons wait. */
  const loading = new SvelteSet<string>();
  let chatGen = 0;

  // A different VOD, or a different chat status, starts afresh.
  let chatOf: number | null = null;
  $effect(() => {
    const id = vodId;
    void chatStatus;
    void canHaveChat;
    // Only those: what the loads read and change mustn't start this over.
    untrack(() => {
      chatGen++;
      if (chatOf !== id) {
        chatOf = id;
        windows = new Map();
        limits.clear();
        overview = null;
      } else {
        // Same VOD, now fetched in full (or failed): what's shown is refreshed in place.
        for (const key of windows.keys()) stale.add(key);
      }
      loading.clear();
      chatError = null;
      refreshOverview();
      loadWindows();
    });
  });

  /** Fetches the windows of the rows shown that aren't loaded (or are stale, if `visibleOnly`, in view). */
  function loadWindows(onlyStaleInView = false) {
    if (!canHaveChat || !showChat) return;
    const id = vodId;
    const gen = chatGen;
    const inView = onlyStaleInView ? visibleRows() : null;
    const wanted: Row[] = [];
    rows.slice(0, shown).forEach((row, i) => {
      const key = keyOf(row);
      if (loading.has(key)) return;
      const missing = !windows.has(key);
      const refresh = stale.has(key) && (!inView || (i >= inView[0] && i <= inView[1]));
      if (missing || refresh) wanted.push(row);
    });
    if (wanted.length === 0) return;
    for (const row of wanted) loading.add(keyOf(row));
    const fetch = (batch: Row[], cap: number) =>
      api.chatWindows(
        id,
        batch.map((r) => [r.from, r.to]),
        cap,
      );
    // One request per limit: almost every row has the first one.
    const byLimit = new Map<number, Row[]>();
    for (const row of wanted) byLimit.set(limitOf(row), [...(byLimit.get(limitOf(row)) ?? []), row]);
    const groups = [...byLimit];
    Promise.all(groups.map(([cap, batch]) => fetch(batch, cap))).then(
      (results) => {
        if (gen !== chatGen || id !== vodId) return;
        keepPlace(() => {
          const next = new Map(windows);
          groups.forEach(([, batch], g) =>
            batch.forEach((row, i) => {
              const key = keyOf(row);
              next.set(key, results[g][i]);
              stale.delete(key);
              loading.delete(key);
            }),
          );
          windows = next;
        });
      },
      (e) => {
        for (const row of wanted) loading.delete(keyOf(row));
        if (gen === chatGen) chatError = String(e);
      },
    );
  }
  // More rows shown (scrolling, a seek), chat shown, or new lines: their windows.
  $effect(() => {
    void rows;
    void shown;
    void showChat;
    untrack(() => loadWindows());
  });

  function expand(row: Row) {
    const key = keyOf(row);
    limits.set(key, limitOf(row) + MORE);
    stale.add(key);
    loadWindows();
  }

  // The chat as a whole: how many messages, how busy each stretch was, and the events.
  const BINS = 1000;
  let overview = $state<ChatOverview | null>(null);
  let mapDuration = $derived(Math.max(vod.duration ?? 0, lines.at(-1)?.start ?? 0));
  let overviewSeq = 0;
  function refreshOverview() {
    if (!canHaveChat) return void (overview = null);
    const id = vodId;
    const mine = ++overviewSeq;
    api.chatOverview(id, mapDuration, BINS).then(
      (o) => mine === overviewSeq && id === vodId && (overview = o),
      (e) => mine === overviewSeq && (chatError = String(e)),
    );
  }
  let chatCount = $derived(overview?.count ?? 0);

  // While a fetch saves batches: at most this often, the counts and the rows in
  // view are fetched again; other rows when they come into view.
  const REFRESH_EVERY = 1500;
  $effect(() => {
    const id = vodId;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let last = 0;
    const refresh = () => {
      timer = undefined;
      last = Date.now();
      for (const key of windows.keys()) stale.add(key);
      refreshOverview();
      loadWindows(true);
    };
    const unlisten = events.chatSaved((vid) => {
      if (vid !== id || timer) return;
      timer = setTimeout(refresh, Math.max(0, REFRESH_EVERY - (Date.now() - last)));
    });
    return () => {
      clearTimeout(timer);
      void unlisten.then((u) => u());
    };
  });

  $effect(() => {
    const id = vodId;
    chatProgress = null;
    const unlisten = events.chatProgress((vid, f) => vid === id && (chatProgress = f));
    return () => void unlisten.then((u) => u());
  });

  let play = (time: number) => onplay(vod.id, time);

  // The minimap at the right edge (Minimap.svelte): chat activity over the
  // whole VOD, or speech when there's no chat, as tall as the visible area.
  let scroller = $state<HTMLElement | null>(null);
  let railHeight = $state(0);
  $effect(() => {
    const s = root.closest<HTMLElement>(".body");
    scroller = s;
    if (!s) return;
    const ro = new ResizeObserver(() => (railHeight = s.clientHeight));
    ro.observe(s);
    return () => ro.disconnect();
  });
  let lineTimes = $derived(lines.map((l) => l.start));
  let mapLabel = $derived(chatCount > 0 ? "How busy chat was over the VOD" : "Speech over the VOD");
  let showMap = $derived(lines.length > 0 && mapDuration > 0);

  // The header spans the transcript and the minimap, and stays at the top.
  let head = $state<HTMLElement>();
  let headHeight = $state(0);

  // Timeline layers (layers.svelte.ts): saved chat searches drawn over the
  // minimap, their messages highlighted in chat.
  $effect(() => void layers.load());
  let shownLayers = $derived(hasChat && !noChat ? layers.shownFor(vod) : []);
  /** Emote images in this VOD's chat, by lowercase name, for the Timeline panel's pills. */
  let emotes = $state(new Map<string, string>());
  $effect(() => {
    const id = vodId;
    void chatStatus;
    emotes = new Map();
    if (!canHaveChat) return;
    api.chatEmotes(id).then(
      (list) => id === vodId && (emotes = new Map(list.map(([name, url]) => [name.toLowerCase(), url]))),
      () => {},
    );
  });
  let matches = $state<{ id: string; color: string; ids: Set<number>; times: number[] }[]>([]);
  let matchSeq = 0;
  // While chat arrives, the layers' searches run at most this often, not on every batch.
  const MATCH_EVERY = 5000;
  let matchedAt = 0;
  $effect(() => {
    const id = vodId;
    const wanted = shownLayers.map((l) => ({ id: l.id, color: l.color, terms: l.terms.map((t) => t.text) }));
    void chatCount;
    const arriving = chatStatus === "pending";
    const mine = ++matchSeq;
    if (wanted.length === 0 || chatCount === 0) {
      matches = [];
      return;
    }
    const run = () => {
      matchedAt = Date.now();
      api
        .chatMatches(
          id,
          wanted.map((w) => w.terms),
        )
        .then(
          (found) =>
            mine === matchSeq &&
            (matches = wanted.map((w, i) => ({
              id: w.id,
              color: w.color,
              ids: new Set(found[i].map(([id]) => id)),
              times: found[i].map(([, time]) => time),
            }))),
          () => mine === matchSeq && (matches = []),
        );
    };
    const wait = arriving ? Math.max(0, MATCH_EVERY - (Date.now() - matchedAt)) : 0;
    if (wait === 0) return void run();
    const timer = setTimeout(run, wait);
    return () => clearTimeout(timer);
  });
  // Chat events (events.ts): the overview has them all, few as they are.
  let chatEvents = $derived(overview?.events ?? []);
  let eventCounts = $derived.by(() => {
    const out = new Map<EventKind, number>();
    for (const m of chatEvents) if (m.kind !== "message") out.set(m.kind, (out.get(m.kind) ?? 0) + 1);
    return out;
  });
  let eventMatches = $derived(
    hasChat && !noChat
      ? EVENTS.filter((e) => layers.events.includes(e.kind)).map((e) => ({
          id: e.kind,
          color: eventColor(vod.platform),
          ids: new Set(chatEvents.filter((m) => m.kind === e.kind).map((m) => m.id)),
        }))
      : [],
  );
  let allMatches = $derived([...eventMatches, ...matches]);
  let overlays = $derived(matches.map((m) => ({ color: m.color, times: m.times })));
  let eventMarks = $derived.by(() => {
    const on = new Set(layers.events);
    if (eventMatches.length === 0) return [];
    return chatEvents
      .filter((m) => m.kind !== "message" && on.has(m.kind))
      .map((m) => ({ time: m.time, value: eventUsd(m, vod.platform) }));
  });
  let counts = $derived(new Map(matches.map((m) => [m.id, m.ids.size])));
  /** The color of the first shown event or layer that matched message `id`, if any. */
  const highlightOf = (id: number) => allMatches.find((m) => m.ids.has(id))?.color ?? null;

  /** Where the view starts: below the header, which stays at the top. */
  const viewTop = () => head?.getBoundingClientRect().bottom ?? 0;
  /** When row `i` starts; the row of chat before the first line starts at 0. */
  const rowStart = (i: number) => rows[i]?.line?.start ?? Math.max(0, rows[i]?.from ?? 0);

  /** The first of `els` (in page order) for which `past` holds, or els.length. */
  function firstWhere(els: NodeListOf<HTMLElement>, past: (el: HTMLElement) => boolean): number {
    let lo = 0;
    let hi = els.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (past(els[mid])) hi = mid;
      else lo = mid + 1;
    }
    return lo;
  }

  /** The first and last rows (indexes) in view, give or take a few. */
  function visibleRows(): [number, number] {
    if (!scroller) return [0, 0];
    const els = root.querySelectorAll<HTMLElement>(".line");
    const top = viewTop();
    const bottom = scroller.getBoundingClientRect().bottom;
    const first = firstWhere(els, (el) => el.getBoundingClientRect().bottom > top);
    const after = firstWhere(els, (el) => el.getBoundingClientRect().top >= bottom);
    return [Math.max(0, first - 5), after + 5];
  }

  // The part of the VOD in view, kept up to date as the list scrolls or grows.
  let view = $state<{ from: number; to: number } | null>(null);
  function updateView() {
    if (!scroller || !showMap) return (view = null);
    const els = root.querySelectorAll<HTMLElement>(".line");
    const top = viewTop();
    const bottom = scroller.getBoundingClientRect().bottom;
    const first = Math.min(
      firstWhere(els, (el) => el.getBoundingClientRect().bottom > top),
      els.length - 1,
    );
    const after = firstWhere(els, (el) => el.getBoundingClientRect().top >= bottom);
    const end = after < rows.length ? rowStart(after) : mapDuration;
    view = { from: rowStart(first) / mapDuration, to: Math.min(1, end / mapDuration) };
  }
  $effect(() => {
    const s = scroller;
    void rows;
    void shown;
    void railHeight;
    if (!s) return;
    let frame = 0;
    const later = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        updateView();
        // Rows whose chat arrived while they were out of view.
        if (stale.size) loadWindows(true);
      });
    };
    later();
    s.addEventListener("scroll", later, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      s.removeEventListener("scroll", later);
    };
  });

  /** Scrolls the row being said at `time` to the top of the view. */
  function seek(time: number) {
    if (!scroller) return;
    let lo = 0;
    let hi = rows.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (rowStart(mid) <= time) lo = mid;
      else hi = mid - 1;
    }
    if (lo >= shown) flushSync(() => (shown = lo + PAGE));
    const el = root.querySelectorAll<HTMLElement>(".line")[lo];
    if (el) scroller.scrollTop += el.getBoundingClientRect().top - viewTop();
  }
</script>

<div class="layout">
  <div class="head" bind:this={head} bind:clientHeight={headHeight}>
    <p class="status muted" title={vod.status === "transcribing" ? "New lines appear as they're produced" : undefined}>
      <span class="eyebrow">Transcript</span>
      {#if vod.status === "transcribing"}
        <span class="sep">·</span><Spinner label="Transcribing" />
        Transcribing {Math.round(vod.progress * 100)}%
      {:else if lines.length > 0}
        <span class="sep">·</span>{lines.length} line{lines.length === 1 ? "" : "s"}
      {/if}
    </p>
    {#if hasChat}
      <div class="chat-head">
        <span
          class="muted chat-status"
          title={vod.chat === "pending"
            ? `Fetching chat: ${chatCount.toLocaleString()} message${chatCount === 1 ? "" : "s"} so far`
            : undefined}
        >
          {#if vod.chat === "pending"}
            <!-- Beside the Chat button, so the spinner and a percentage say enough. -->
            <Spinner label="Fetching chat" />
            {chatProgress !== null ? `${Math.round(chatProgress * 100)}%` : ""}
          {:else if vod.chat === "none"}
            No chat replay
          {:else if vod.chat === "failed"}
            <span class="error-text">Couldn't fetch chat</span>
            <button class="link" onclick={() => api.retryChat(vod.id).catch((e) => (chatError = String(e)))}
              >Try again</button
            >
          {:else if chatError}
            <span class="error-text">{chatError}</span>
          {:else}
            {chatCount.toLocaleString()} message{chatCount === 1 ? "" : "s"}
          {/if}
        </span>
        <button
          class="chat-toggle"
          class:on={showChat}
          onclick={toggleChat}
          disabled={noChat}
          aria-expanded={showChat}
          title={noChat
            ? "This video has no chat replay (it wasn't a livestream)"
            : showChat
              ? "Hide chat"
              : "Show chat beside the transcript"}
        >
          <Icon name="message" size={14} /> Chat <Icon name={showChat ? "chevron-up" : "chevron-down"} size={14} />
        </button>
        <Timeline {vod} {emotes} {counts} {eventCounts} />
      </div>
    {/if}
  </div>
  <div class="cols">
    <div class="transcript" id="transcript" class:with-chat={showChat} bind:this={root}>
      {#if error}
        <p class="error-text">{error}</p>
      {:else if lines.length === 0}
        <p class="muted">
          {vod.status === "done"
            ? "No speech was found in this VOD."
            : "The transcript appears here as it is produced."}
        </p>
      {/if}
      {#each rows.slice(0, shown) as row, r (r)}
        {@const key = keyOf(row)}
        {@const win = windows.get(key)}
        <div class="line" data-start={row.line?.start} class:empty={!row.line && !win?.total}>
          {#if row.line}
            {@const line = row.line}
            <button class="row" onclick={() => play(line.start)}>
              <span class="time">{formatTime(line.start)}</span>
              <span>{line.text}</span>
            </button>
          {:else}
            <span></span>
          {/if}
          {#if showChat}
            <div class="chat">
              {#each win?.messages ?? [] as message (message.id)}
                <ChatMessage {message} platform={vod.platform} highlight={highlightOf(message.id)} onplay={play} />
              {/each}
              {#if win && win.total > win.messages.length}
                <button class="link more" onclick={() => expand(row)} disabled={loading.has(key)}
                  >{(win.total - win.messages.length).toLocaleString()} more</button
                >
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      {#if shown < rows.length}
        <div class="more-rows" bind:this={sentinel}></div>
      {/if}
    </div>
    {#if showMap}
      <div class="rail" style:top="{headHeight}px" style:height="{Math.max(0, railHeight - headHeight)}px">
        <Minimap
          times={lineTimes}
          histogram={chatCount > 0 ? overview?.histogram : undefined}
          duration={mapDuration}
          {view}
          label={mapLabel}
          {overlays}
          events={eventMarks}
          eventColor={eventColor(vod.platform)}
          onseek={seek}
        />
      </div>
    {/if}
  </div>
</div>

<style>
  .cols {
    display: flex;
    align-items: flex-start;
  }

  .rail {
    position: sticky;
    top: 0;
    flex: none;
    width: 48px;
    margin-left: auto;
    padding: 8px 6px;
    box-sizing: border-box;
    border-left: 1px solid var(--border);
  }

  .transcript {
    flex: 1 1 auto;
    min-width: 0;
    padding: 8px 20px 32px;
    max-width: 960px;
    /* toggleChat keeps the view in place itself; the browser's own scroll
       anchoring (Firefox, Chromium) would move it a second time. */
    overflow-anchor: none;
  }

  .transcript.with-chat {
    max-width: 1320px;
  }

  /* Across the transcript and the minimap, staying at the top while scrolling,
     so chat and the timeline can be changed from anywhere. */
  .head {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 14px 8px 20px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }

  /* One line each, whatever the width: cut short rather than wrapped. */
  .status,
  .chat-status {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status {
    margin: 0;
    font-size: 12px;
  }

  .chat-head {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    font-size: 12px;
  }

  .chat-toggle {
    /* Keeps its size; the status beside it is what gets cut short. */
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    padding: 3px 8px;
  }

  .chat-toggle:disabled {
    opacity: 0.5;
  }

  .chat-toggle.on {
    background: var(--accent-soft);
    border-color: transparent;
  }

  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 12px;
  }

  .link:hover {
    text-decoration: underline;
  }

  .with-chat .line {
    display: grid;
    grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
    gap: 16px;
  }

  .chat {
    border-left: 1px solid var(--border);
    padding: 2px 0 2px 8px;
    min-width: 0;
  }

  .chat:empty {
    padding: 0 0 0 8px;
  }

  .more {
    margin: 0 8px 4px;
  }

  /* The row for chat before the first line, when there's none. */
  .line.empty {
    display: none;
  }

  .more-rows {
    height: 1px;
  }
</style>
