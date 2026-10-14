<script lang="ts">
  import { flushSync } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { api, events, type ChatLine, type Line, type Vod } from "./api";
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

  // Chat: loaded once it has been fetched, beside the transcript, and
  // collapsible (remembered across VODs and launches).
  const CHAT_OPEN = "vodscribe.chatOpen";
  let chat = $state<ChatLine[]>([]);
  let chatError = $state<string | null>(null);
  let chatProgress = $state<number | null>(null);
  let chatOpen = $state(read(CHAT_OPEN) !== "false");
  let hasChat = $derived(vod.platform !== "local" && vod.chat !== undefined);
  // A video that wasn't a livestream has no chat: its toggle is grayed out and the column stays shut.
  let chatStatus = $derived(vod.chat);
  let noChat = $derived(chatStatus === "none");

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

  // Depends only on which VOD and its chat status (both derived, so they change
  // only when their values do): the VOD itself is replaced on every update, and
  // reloading then would empty the column and throw the scroll position.
  let chatSeq = 0;
  let chatOf: number | null = null;
  $effect(() => {
    const id = vodId;
    const status = chatStatus;
    const mine = ++chatSeq;
    if (chatOf !== id) {
      chatOf = id;
      chat = [];
    }
    chatError = null;
    // While it's fetched, what's arrived so far (it's saved a batch at a time).
    if (status !== "done" && status !== "pending") return;
    api.chat(id).then(
      (c) => mine === chatSeq && keepPlace(() => (chat = c)),
      (e) => mine === chatSeq && (chatError = String(e)),
    );
  });

  // Each saved batch, at most this often: a busy chat saves a batch a second,
  // and every reload lays the rows out again.
  const RELOAD_EVERY = 1500;
  $effect(() => {
    const id = vodId;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let last = 0;
    const reload = () => {
      timer = undefined;
      last = Date.now();
      const mine = ++chatSeq;
      api.chat(id).then(
        (c) => mine === chatSeq && keepPlace(() => (chat = c)),
        () => {},
      );
    };
    const unlisten = events.chatSaved((vid) => {
      if (vid !== id || timer) return;
      timer = setTimeout(reload, Math.max(0, RELOAD_EVERY - (Date.now() - last)));
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

  // Each transcript line with the chat messages sent while it was being said:
  // from its start to the next line's. Messages before the first line get a
  // row of their own.
  interface Row {
    line: Line | null;
    chat: ChatLine[];
  }
  let rows = $derived.by((): Row[] => {
    const out: Row[] = lines.map((line) => ({ line, chat: [] }));
    if (!chatOpen || chat.length === 0) return out;
    let lead: Row | null = null;
    let i = 0;
    for (const message of chat) {
      // Both lists are in time order, so one pass places every message.
      while (i < lines.length && lines[i].start <= message.time) i++;
      if (i === 0) (lead ??= { line: null, chat: [] }).chat.push(message);
      else out[i - 1].chat.push(message);
    }
    return lead ? [lead, ...out] : out;
  });

  // A row shows this many messages until it's expanded, so a burst of chat
  // doesn't push the transcript apart.
  const SHOWN = 8;
  let expanded = new SvelteSet<number>();
  $effect(() => {
    void vodId;
    expanded.clear();
  });

  let showChat = $derived(hasChat && chatOpen && !noChat);

  // Rows are added as the end comes into view, so a long VOD with a busy chat
  // opens at once rather than laying out every row first.
  const PAGE = 200;
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
  let mapTimes = $derived(chat.length > 0 ? chat.map((m) => m.time) : lines.map((l) => l.start));
  let mapLabel = $derived(chat.length > 0 ? "How busy chat was over the VOD" : "Speech over the VOD");
  let mapDuration = $derived(Math.max(vod.duration ?? 0, lines.at(-1)?.start ?? 0, chat.at(-1)?.time ?? 0));
  let showMap = $derived(lines.length > 0 && mapDuration > 0);

  // The header spans the transcript and the minimap, and stays at the top.
  let head = $state<HTMLElement>();
  let headHeight = $state(0);

  // Timeline layers (layers.svelte.ts): saved chat searches drawn over the
  // minimap, their messages highlighted in chat.
  $effect(() => void layers.load());
  let shownLayers = $derived(hasChat && !noChat ? layers.shownFor(vod) : []);
  /** Emote images in this VOD's chat, by lowercase name. */
  let emotes = $derived.by(() => {
    const out = new Map<string, string>();
    for (const m of chat) for (const p of m.parts) if (p.type === "emote") out.set(p.name.toLowerCase(), p.url);
    return out;
  });
  let timeOf = $derived(new Map(chat.map((m) => [m.id, m.time])));
  let matches = $state<{ id: string; color: string; ids: Set<number> }[]>([]);
  let matchSeq = 0;
  $effect(() => {
    const id = vodId;
    const wanted = shownLayers.map((l) => ({ id: l.id, color: l.color, terms: l.terms.map((t) => t.text) }));
    void chat.length;
    const mine = ++matchSeq;
    if (wanted.length === 0 || chat.length === 0) {
      matches = [];
      return;
    }
    api
      .chatMatches(
        id,
        wanted.map((w) => w.terms),
      )
      .then(
        (found) =>
          mine === matchSeq && (matches = wanted.map((w, i) => ({ id: w.id, color: w.color, ids: new Set(found[i]) }))),
        () => mine === matchSeq && (matches = []),
      );
  });
  // Chat events (events.ts): known from each message's kind, no search needed.
  let eventCounts = $derived.by(() => {
    const out = new Map<EventKind, number>();
    for (const m of chat) if (m.kind !== "message") out.set(m.kind, (out.get(m.kind) ?? 0) + 1);
    return out;
  });
  let eventMatches = $derived(
    hasChat && !noChat
      ? EVENTS.filter((e) => layers.events.includes(e.kind)).map((e) => ({
          id: e.kind,
          color: eventColor(vod.platform),
          ids: new Set(chat.filter((m) => m.kind === e.kind).map((m) => m.id)),
        }))
      : [],
  );
  let allMatches = $derived([...eventMatches, ...matches]);
  const timesOf = (ids: Set<number>) => [...ids].map((id) => timeOf.get(id) ?? 0);
  let overlays = $derived(matches.map((m) => ({ color: m.color, times: timesOf(m.ids) })));
  let eventMarks = $derived.by(() => {
    const shown = new Set(layers.events);
    if (eventMatches.length === 0) return [];
    return chat
      .filter((m) => m.kind !== "message" && shown.has(m.kind))
      .map((m) => ({ time: m.time, value: eventUsd(m, vod.platform) }));
  });
  let counts = $derived(new Map(matches.map((m) => [m.id, m.ids.size])));
  /** The color of the first shown event or layer that matched message `id`, if any. */
  const highlightOf = (id: number) => allMatches.find((m) => m.ids.has(id))?.color ?? null;

  /** Where the view starts: below the header, which stays at the top. */
  const viewTop = () => head?.getBoundingClientRect().bottom ?? 0;
  /** When row `i` starts; the row of chat before the first line starts at 0. */
  const rowStart = (i: number) => rows[i]?.line?.start ?? 0;

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
      frame = requestAnimationFrame(updateView);
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
            ? `Fetching chat: ${chat.length.toLocaleString()} message${chat.length === 1 ? "" : "s"} so far`
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
            {chat.length.toLocaleString()} message{chat.length === 1 ? "" : "s"}
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
        <div class="line" data-start={row.line?.start}>
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
              {#each expanded.has(r) ? row.chat : row.chat.slice(0, SHOWN) as message, m (m)}
                <ChatMessage {message} platform={vod.platform} highlight={highlightOf(message.id)} onplay={play} />
              {/each}
              {#if row.chat.length > SHOWN && !expanded.has(r)}
                <button class="link more" onclick={() => expanded.add(r)}>{row.chat.length - SHOWN} more</button>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      {#if showChat && lines.length === 0 && chat.length > 0}
        <div class="line">
          <span></span>
          <div class="chat">
            {#each chat.slice(0, shown) as message, m (m)}
              <ChatMessage {message} platform={vod.platform} highlight={highlightOf(message.id)} onplay={play} />
            {/each}
          </div>
        </div>
      {/if}
      {#if shown < Math.max(rows.length, showChat && lines.length === 0 ? chat.length : 0)}
        <div class="more-rows" bind:this={sentinel}></div>
      {/if}
    </div>
    {#if showMap}
      <div class="rail" style:top="{headHeight}px" style:height="{Math.max(0, railHeight - headHeight)}px">
        <Minimap
          times={mapTimes}
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

  .chat-head :global(button) {
    flex: none;
  }

  .chat-toggle {
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

  .more-rows {
    height: 1px;
  }
</style>
