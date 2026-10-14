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
  let hasChat = $derived(vod.platform !== "local");
  // A VOD without a chat replay (a video that wasn't a livestream, say): its
  // toggle is grayed out and the column stays shut.
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

  // Makes `change`, keeping the row at the top of the view where it was:
  // showing or hiding chat changes every row's height. (Heights that change
  // as rows are measured or their chat arrives are kept in place by `remeasure`.)
  function keepPlace(change: () => void) {
    // A pixel's leeway: scroll positions are rounded, so the row at the top
    // can otherwise read as the one ending just above it.
    const y = viewY();
    const index = rowAtY(y + 1);
    const within = y - offsets[index];
    const key = rows[index] ? keyOf(rows[index]) : null;
    flushSync(change);
    if (!scroller || key === null) return;
    const now = rows.findIndex((r) => keyOf(r) === key);
    if (now >= 0) scroller.scrollTop = listTop + offsets[now] + within - headHeight;
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
    /** Unique among the rows, and kept as lines are added: a line's place in
     *  the transcript (lines can share a start), and where its row ends, so a
     *  row whose next line arrives is fetched again. */
    key: string;
    line: Line | null;
    from: number;
    to: number;
  }
  let rows = $derived.by((): Row[] => {
    const out: Row[] = lines.map((line, i) => {
      const to = lines[i + 1]?.start ?? FAR;
      return { key: `line ${i} to ${to}`, line, from: line.start, to };
    });
    if (!showChat) return out;
    if (lines.length === 0) {
      const minutes = Math.ceil((vod.duration ?? 0) / 60);
      return Array.from({ length: minutes }, (_, k) => {
        const to = k === minutes - 1 ? FAR : (k + 1) * 60;
        return { key: `minute ${k} to ${to}`, line: null, from: k * 60, to };
      });
    }
    return [{ key: `before to ${lines[0].start}`, line: null, from: -FAR, to: lines[0].start }, ...out];
  });
  const keyOf = (row: Row) => row.key;

  // Only the rows in and near the view are laid out; empty space of the right
  // height stands in for the rest, so a long VOD with a busy chat opens, toggles
  // chat and jumps around at once, however far it goes. Each row's height is
  // measured the first time it's laid out and kept for the session (per row,
  // and per whether chat is shown, since that lays rows out differently); until
  // then it's estimated from its text and how many messages it shows.
  const measured = new Map<string, number>();
  let measuredGen = $state(0);
  const heightKey = (row: Row) => `${showChat ? "chat" : "text"}|${keyOf(row)}`;
  let width = $state(0);
  function estimate(row: Row): number {
    const textWidth = Math.max(160, (showChat ? width * 0.6 : width) - 110);
    const text = row.line ? 10 + 22.5 * Math.max(1, Math.ceil((row.line.text.length * 7.8) / textWidth)) : 0;
    if (!showChat) return text;
    const win = windows.get(keyOf(row));
    if (!row.line && !win?.total) return 0;
    // Before its chat is fetched, a row is guessed to hold a couple of messages.
    const messages = win ? win.messages.length : 2;
    const chat = messages * 21 + (win && win.total > win.messages.length ? 22 : 0) + (messages ? 4 : 0);
    return Math.max(text, chat);
  }
  /** Where each row starts in the list, and (last) where it ends. */
  let offsets = $derived.by(() => {
    void measuredGen;
    void windows;
    const out = new Float64Array(rows.length + 1);
    rows.forEach((row, i) => (out[i + 1] = out[i] + (measured.get(heightKey(row)) ?? estimate(row))));
    return out;
  });
  /** The row at `y` in the list (clamped). */
  function rowAtY(y: number): number {
    let lo = 0;
    let hi = rows.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (offsets[mid] <= y) lo = mid;
      else hi = mid - 1;
    }
    return Math.max(0, lo);
  }

  // The scroller's position, and where the list starts in it (below the header).
  let scrollTop = $state(0);
  let listTop = $state(0);
  let topPad = $state<HTMLElement>();
  /** The top of the view, in the list: just below the header, which stays at the top. */
  const viewY = () => (scroller?.scrollTop ?? 0) + headHeight - listTop;
  // Laid out: what's in view and this much either side, so scrolling finds rows ready.
  const AROUND = 1000;
  let range = $derived.by((): [number, number] => {
    const top = scrollTop + headHeight - listTop;
    const from = rowAtY(top - AROUND);
    const to = Math.min(rows.length, rowAtY(top + railHeight + AROUND) + 1);
    return [from, Math.max(from, to)];
  });

  // Measures rows as they're laid out or change size. A row above the view
  // that turns out taller or shorter than thought moves the view by as much, so
  // what's being read stays put (WebKit has no scroll anchoring of its own).
  let resizes: ResizeObserver | undefined;
  function remeasure(entries: ResizeObserverEntry[]) {
    const y = viewY();
    let shift = 0;
    for (const entry of entries) {
      const el = entry.target as HTMLElement;
      const index = Number(el.dataset.index);
      const row = rows[index];
      if (!row || !el.isConnected) continue;
      const key = heightKey(row);
      const before = measured.get(key) ?? estimate(row);
      const height = el.offsetHeight;
      if (height === before) continue;
      measured.set(key, height);
      if (offsets[index] + before <= y) shift += height - before;
    }
    flushSync(() => measuredGen++);
    if (shift && scroller) scroller.scrollTop += shift;
  }
  function measure(node: HTMLElement) {
    resizes ??= new ResizeObserver(remeasure);
    resizes.observe(node);
    return { destroy: () => resizes?.unobserve(node) };
  }
  $effect(() => () => resizes?.disconnect());

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
        // Row keys repeat from VOD to VOD; their heights don't.
        measured.clear();
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

  /** Fetches the windows of the rows laid out (and a few more) that aren't loaded, or are stale. */
  function loadWindows() {
    if (!canHaveChat || !showChat) return;
    const id = vodId;
    const gen = chatGen;
    // The rows laid out, and a few more either side to have ready.
    const [from, to] = range;
    const wanted: Row[] = [];
    rows.slice(Math.max(0, from - 10), to + 10).forEach((row) => {
      const key = keyOf(row);
      if (loading.has(key)) return;
      if (!windows.has(key) || stale.has(key)) wanted.push(row);
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
        const next = new Map(windows);
        groups.forEach(([, batch], g) =>
          batch.forEach((row, i) => {
            const key = keyOf(row);
            next.set(key, results[g][i]);
            stale.delete(key);
            loading.delete(key);
          }),
        );
        // Rows fetched above the view grow (their estimates now count their
        // messages), which would push the view down.
        keepPlace(() => (windows = next));
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
    void range;
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
      loadWindows();
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

  /** When row `i` starts; the row of chat before the first line starts at 0. */
  const rowStart = (i: number) => rows[i]?.line?.start ?? Math.max(0, rows[i]?.from ?? 0);

  // The part of the VOD in view, for the minimap's box.
  let view = $derived.by(() => {
    if (!showMap || rows.length === 0) return null;
    const top = scrollTop + headHeight - listTop;
    const first = rowAtY(top);
    const after = rowAtY(top + railHeight - headHeight) + 1;
    const end = after < rows.length ? rowStart(after) : mapDuration;
    return { from: rowStart(first) / mapDuration, to: Math.min(1, end / mapDuration) };
  });
  $effect(() => {
    const s = scroller;
    // Where the list starts moves with the header, and with a message above the rows.
    void headHeight;
    void lines.length;
    if (!s) return;
    let frame = 0;
    const read = () => {
      scrollTop = s.scrollTop;
      if (topPad) listTop = topPad.getBoundingClientRect().top - s.getBoundingClientRect().top + s.scrollTop;
    };
    const later = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(read);
    };
    read();
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
    scroller.scrollTop = listTop + offsets[lo] - headHeight;
  }
</script>

<div class="layout">
  <div class="head" bind:clientHeight={headHeight}>
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
          title={noChat ? "This VOD has no chat replay" : showChat ? "Hide chat" : "Show chat beside the transcript"}
        >
          <Icon name="message" size={14} /> Chat <Icon name={showChat ? "chevron-up" : "chevron-down"} size={14} />
        </button>
        <Timeline {vod} {emotes} {counts} {eventCounts} />
      </div>
    {/if}
  </div>
  <div class="cols">
    <div class="transcript" id="transcript" class:with-chat={showChat} bind:this={root} bind:clientWidth={width}>
      {#if error}
        <p class="error-text">{error}</p>
      {:else if lines.length === 0}
        <p class="muted">
          {vod.status === "done"
            ? "No speech was found in this VOD."
            : "The transcript appears here as it is produced."}
        </p>
      {/if}
      <div bind:this={topPad} style:height="{offsets[range[0]]}px"></div>
      {#each rows.slice(range[0], range[1]) as row, k (keyOf(row))}
        {@const key = keyOf(row)}
        {@const win = windows.get(key)}
        <div class="line" data-index={range[0] + k} use:measure class:empty={!row.line && !win?.total}>
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
      <div style:height="{offsets[rows.length] - offsets[range[1]]}px"></div>
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
</style>
