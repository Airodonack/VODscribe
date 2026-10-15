<script lang="ts">
  import { flushSync, untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { api, events, type ChatOverview, type ChatWindow, type Line, type Vod } from "./api";
  import ChatMessage from "./ChatMessage.svelte";
  import Minimap from "./Minimap.svelte";
  import TranscriptHead from "./TranscriptHead.svelte";
  import { layers } from "./layers.svelte";
  import { eventColor } from "./events";
  import { Marks } from "./marks.svelte";
  import { formatTime } from "./platform";
  import { buildRows, linesSpan, type Row } from "./rows";
  import { clips } from "./clips.svelte";

  let { vod, onplay }: { vod: Vod; onplay: (vodId: number, time: number) => void } = $props();

  let lines = $state.raw<Line[]>([]);
  let error = $state<string | null>(null);

  // Reload when the VOD changes, and periodically while it is still transcribing
  // (segments are indexed as they are produced).
  let vodId = $derived(vod.id);
  let step = $derived(vod.status === "transcribing" ? Math.floor(vod.progress * 20) : vod.status);
  // Only the latest request's answer counts, so a slow older reload can't win.
  // While transcribing, lines are only added at the end: just those are asked for.
  let seq = 0;
  $effect(() => {
    const id = vodId;
    void step;
    const mine = ++seq;
    const have = untrack(() => lines);
    const after = vod.status === "transcribing" && have.length ? have[have.length - 1].start : undefined;
    api.transcript(id, after).then(
      (l) => {
        if (mine !== seq) return;
        lines = after === undefined ? l : [...have, ...l];
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
    const key = rows[index] ? rows[index].key : null;
    flushSync(change);
    if (!scroller || key === null) return;
    const now = rows.findIndex((r) => r.key === key);
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

  // The rows: each line with its chat, and minutes of chat where there's no line (rows.ts).
  let rows = $derived(buildRows(lines, vod.duration ?? 0, showChat));

  // Only the rows in and near the view are laid out; empty space of the right
  // height stands in for the rest, so a long VOD with a busy chat opens, toggles
  // chat and jumps around at once, however far it goes. Each row's height is
  // measured the first time it's laid out and kept for the session (per row,
  // and per whether chat is shown, since that lays rows out differently); until
  // then it's estimated from its text and how many messages it shows.
  const measured = new Map<string, number>();
  let measuredGen = $state(0);
  /** Each row's key for its measured height, made once per set of rows. */
  let heightKeys = $derived(rows.map((row) => `${showChat ? "chat" : "text"}|${row.key}`));
  let width = $state(0);
  function estimate(row: Row): number {
    const textWidth = Math.max(160, (showChat ? width * 0.6 : width) - 110);
    const text = row.line ? 10 + 22.5 * Math.max(1, Math.ceil((row.line.text.length * 7.8) / textWidth)) : 0;
    if (!showChat) return text;
    const win = windows.get(row.key);
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
    const keys = heightKeys;
    rows.forEach((row, i) => (out[i + 1] = out[i] + (measured.get(keys[i]) ?? estimate(row))));
    return out;
  });
  /** The last row whose `at(i)` is at most `x`, or the first row (rows are in order). */
  function lastRowAtOrBefore(at: (i: number) => number, x: number): number {
    let lo = 0;
    let hi = rows.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (at(mid) <= x) lo = mid;
      else hi = mid - 1;
    }
    return Math.max(0, lo);
  }
  /** The row at `y` in the list (clamped). */
  const rowAtY = (y: number) => lastRowAtOrBefore((i) => offsets[i], y);

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
    let changed = false;
    for (const entry of entries) {
      const el = entry.target as HTMLElement;
      const index = Number(el.dataset.index);
      const row = rows[index];
      if (!row || !el.isConnected) continue;
      const key = heightKeys[index];
      const before = measured.get(key) ?? estimate(row);
      const height = el.offsetHeight;
      if (height === before) continue;
      measured.set(key, height);
      changed = true;
      if (offsets[index] + before <= y) shift += height - before;
    }
    // Rows laid out at the height they were thought to be change nothing, but
    // just after a seek, the one sought may be among them.
    if (!changed) {
      if (aimed()) place();
      return;
    }
    flushSync(() => measuredGen++);
    // Just after a seek, the moment sought stays centered; otherwise what's read stays put.
    if (aimed()) place();
    else if (shift && scroller) scroller.scrollTop += shift;
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
  const limitOf = (row: Row) => limits.get(row.key) ?? SHOWN;
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
      const key = row.key;
      if (loading.has(key)) return;
      if (!windows.has(key) || stale.has(key)) wanted.push(row);
    });
    if (wanted.length === 0) return;
    for (const row of wanted) loading.add(row.key);
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
            const key = row.key;
            next.set(key, results[g][i]);
            stale.delete(key);
            loading.delete(key);
          }),
        );
        // Rows fetched above the view grow (their estimates now count their
        // messages), which would push the view down.
        if (aimed()) {
          flushSync(() => (windows = next));
          place();
        } else keepPlace(() => (windows = next));
      },
      (e) => {
        for (const row of wanted) loading.delete(row.key);
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
    const key = row.key;
    limits.set(key, limitOf(row) + MORE);
    stale.add(key);
    loadWindows();
  }

  // The chat as a whole: how many messages, how busy each stretch was, and the events.
  const BINS = 1000;
  let overview = $state.raw<ChatOverview | null>(null);
  let mapDuration = $derived(Math.max(vod.duration ?? 0, lines.at(-1)?.start ?? 0));
  /** The counts, and with `events` the subs, gifts and tips too, which
   *  otherwise keep what was had: while a fetch is saving, they're a megabyte
   *  and more for a busy chat, so they're read now and then, not each time. */
  const EVENTS_EVERY = 15000;
  let eventsAt = 0;
  // Only the latest of each kind counts, so a quick refresh of the counts
  // can't cancel a slower one that brings the events (a fetch just finished).
  let eventsSeq = 0;
  let countsSeq = 0;
  function refreshOverview(withEvents = true) {
    if (!canHaveChat) return void (overview = null);
    const id = vodId;
    // A refresh with the events has the counts too, newer than any asked for before it.
    const mine = withEvents ? ++eventsSeq : ++countsSeq;
    if (withEvents) {
      countsSeq++;
      eventsAt = Date.now();
    }
    const latest = () => id === vodId && mine === (withEvents ? eventsSeq : countsSeq);
    api.chatOverview(id, mapDuration, BINS, withEvents).then(
      (o) => latest() && (overview = { ...o, events: o.events ?? (overview?.events || []) }),
      (e) => latest() && (chatError = String(e)),
    );
  }
  let chatCount = $derived(overview?.count ?? 0);

  // While a fetch saves batches: at most this often, the counts and the rows in
  // view are fetched again; other rows when they come into view.
  const REFRESH_EVERY = 1500;
  $effect(() => {
    const id = vodId;
    // A change of status (the fetch done) reads everything afresh; a refresh
    // still waiting here would only come after it.
    void chatStatus;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let last = 0;
    const refresh = () => {
      timer = undefined;
      last = Date.now();
      for (const key of windows.keys()) stale.add(key);
      refreshOverview(Date.now() - eventsAt >= EVENTS_EVERY);
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

  const play = (time: number) => onplay(vod.id, time);

  // Pressing on a line and dragging across others picks them as a clip
  // (clips.svelte.ts); a click without a drag still plays the line.
  let drag = $state<{ from: number; to: number } | null>(null);
  let pointer = { x: 0, y: 0 };
  let moved = false;
  let swallowClick = false;
  let scrolling = 0;

  /** The index in `rows` of the line row under the point, if any. */
  function lineRowAt(x: number, y: number): number | null {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>(".line");
    const i = el && root.contains(el) ? Number(el.dataset.index) : NaN;
    return Number.isInteger(i) && rows[i]?.line ? i : null;
  }

  function onRowDown(e: PointerEvent, index: number) {
    if (e.button !== 0 || e.pointerType === "touch") return;
    drag = { from: index, to: index };
    moved = false;
    pointer = { x: e.clientX, y: e.clientY };
  }

  function onPointerMove(e: PointerEvent) {
    if (!drag) return;
    pointer = { x: e.clientX, y: e.clientY };
    follow();
    // Near the top or bottom of the view, it scrolls on, for picks longer than a screen.
    if (!scrolling) scrolling = requestAnimationFrame(autoScroll);
  }

  function follow() {
    if (!drag) return;
    const i = lineRowAt(pointer.x, pointer.y);
    if (i !== null && i !== drag.to) {
      drag.to = i;
      moved = true;
    }
  }

  function autoScroll() {
    scrolling = 0;
    if (!drag || !scroller) return;
    const box = scroller.getBoundingClientRect();
    const edge = 48;
    const speed =
      pointer.y < box.top + edge
        ? -(box.top + edge - pointer.y)
        : pointer.y > box.bottom - edge
          ? pointer.y - (box.bottom - edge)
          : 0;
    if (!speed) return;
    scroller.scrollBy(0, Math.max(-24, Math.min(24, speed / 2)));
    follow();
    scrolling = requestAnimationFrame(autoScroll);
  }

  function onPointerUp() {
    if (!drag) return;
    const { from, to } = drag;
    drag = null;
    cancelAnimationFrame(scrolling);
    scrolling = 0;
    if (!moved) return;
    swallowClick = true;
    const first = lines.indexOf(rows[from].line!);
    const last = lines.indexOf(rows[to].line!);
    if (first < 0 || last < 0) return;
    const { start, end } = linesSpan(lines, first, last);
    // Picked on a VOD the player isn't showing: show it there, from the pick's start.
    if (clips.vodId !== vod.id) {
      clips.open(vod.id);
      clips.pick(start, end, vod.duration);
      play(start);
    } else {
      clips.pick(start, end, vod.duration);
    }
  }

  function onRowClick(time: number) {
    if (swallowClick) {
      swallowClick = false;
      return;
    }
    play(time);
  }

  /** Whether row `index` is in the lines being dragged across, or in the range picked. */
  function picked(index: number, row: Row): boolean {
    if (drag) return index >= Math.min(drag.from, drag.to) && index <= Math.max(drag.from, drag.to);
    const ghost = clips.vodId === vod.id ? clips.ghost : null;
    return !!ghost && !!row.line && row.line.start >= ghost.start - 0.01 && row.line.start < ghost.end - 0.01;
  }

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
  // Chat alone is enough to draw: it can be fetched before the transcript has a line.
  let showMap = $derived((lines.length > 0 || chatCount > 0) && mapDuration > 0);

  // The header spans the transcript and the minimap, and stays at the top.
  let headHeight = $state(0);

  // The Timeline layers' matches and the chat events, on the minimap and in chat (marks.svelte.ts).
  $effect(() => void layers.load());
  const marks = new Marks({
    get vod() {
      return vod;
    },
    get canHaveChat() {
      return canHaveChat;
    },
    get chatCount() {
      return chatCount;
    },
    get overview() {
      return overview;
    },
  });

  /** When row `i` starts; the row of chat before the first line starts at 0. */
  const rowStart = (i: number) => rows[i]?.line?.start ?? Math.max(0, rows[i]?.from ?? 0);

  /** When row `i` ends: when the next one starts, or (the last) when the VOD ends. */
  const rowEnd = (i: number) => (i + 1 < rows.length ? rowStart(i + 1) : Math.max(mapDuration, rowStart(i)));

  // Times and places in the list map to each other evenly within each row:
  // a row's time is spread over its height. So the minimap's box moves as
  // smoothly as the list scrolls, even across a long row (a stretch without
  // speech), and seeking to a moment and reading the box back agree.

  /** The moment at `y` in the list. */
  function timeAtY(y: number): number {
    if (rows.length === 0) return 0;
    const i = rowAtY(y);
    const height = offsets[i + 1] - offsets[i];
    const f = height > 0 ? Math.min(1, Math.max(0, (y - offsets[i]) / height)) : 0;
    return rowStart(i) + f * (rowEnd(i) - rowStart(i));
  }

  /** The row moment `time` is in (clamped). */
  const rowAtTime = (time: number) => lastRowAtOrBefore(rowStart, time);

  /** Where moment `time` is in the list: the inverse of `timeAtY`. */
  function yAtTime(time: number): number {
    if (rows.length === 0) return 0;
    const lo = rowAtTime(time);
    const span = rowEnd(lo) - rowStart(lo);
    const f = span > 0 ? Math.min(1, Math.max(0, (time - rowStart(lo)) / span)) : 0;
    return offsets[lo] + f * (offsets[lo + 1] - offsets[lo]);
  }

  // The part of the VOD in view, for the minimap's box.
  let view = $derived.by(() => {
    if (!showMap || rows.length === 0) return null;
    const top = scrollTop + headHeight - listTop;
    const from = timeAtY(top);
    const to = timeAtY(top + railHeight - headHeight);
    return { from: from / mapDuration, to: Math.min(1, to / mapDuration) };
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

  // Seeking scrolls a moment to the middle of the view, as the minimap's box
  // centers on where it's clicked or dragged. Rows far off have estimated
  // heights until they're laid out and measured, and grow when their chat
  // arrives, so for a moment after a seek, each of those re-centers it
  // (`remeasure`, `loadWindows`), until the reader scrolls by themselves.
  const SETTLE_MS = 2000;
  let aim: { time: number; until: number } | null = null;
  const aimed = () => aim !== null && performance.now() < aim.until;

  /** Scrolls the moment aimed at to the middle of the view: the chat message
   *  nearest it once its row is laid out (a pixel of the minimap can be many
   *  seconds, so a click a hair past an event still finds it), until then
   *  where the row's time puts it. */
  function place() {
    if (!aim || !scroller) return;
    const middle = headHeight + (railHeight - headHeight) / 2;
    const message = nearestMessage(aim.time);
    if (message) {
      const top = message.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
      scroller.scrollTop += top - middle;
    } else {
      scroller.scrollTop = listTop + yAtTime(aim.time) - middle;
    }
  }

  /** The laid-out chat message nearest moment `time` (within 30 s), found in
   *  the chat fetched for its row and the rows beside it, then on the page:
   *  only that one element, so dragging the minimap doesn't search them all. */
  function nearestMessage(time: number): HTMLElement | null {
    if (!showChat || rows.length === 0) return null;
    const at = rowAtTime(time);
    let best: { index: number; time: number } | null = null;
    let gap = 30;
    for (let index = at - 1; index <= at + 1; index++) {
      const row = rows[index];
      for (const m of (row && windows.get(row.key)?.messages) || []) {
        const d = Math.abs(m.time - time);
        if (d < gap) [best, gap] = [{ index, time: m.time }, d];
      }
    }
    if (!best) return null;
    return root.querySelector<HTMLElement>(`.line[data-index="${best.index}"] [data-time="${best.time}"]`);
  }

  function seek(time: number) {
    aim = { time, until: performance.now() + SETTLE_MS };
    place();
  }

  // Scrolling by wheel, touch or keys is the reader's own: no more re-centering.
  $effect(() => {
    const s = scroller;
    if (!s) return;
    const stop = () => (aim = null);
    const kinds = ["wheel", "touchstart", "keydown"] as const;
    for (const kind of kinds) s.addEventListener(kind, stop, { passive: true });
    return () => kinds.forEach((kind) => s.removeEventListener(kind, stop));
  });
</script>

<svelte:window onpointermove={onPointerMove} onpointerup={onPointerUp} onpointercancel={onPointerUp} />

<div class="layout">
  <TranscriptHead
    {vod}
    lineCount={lines.length}
    {chatCount}
    {chatProgress}
    bind:chatError
    {hasChat}
    {showChat}
    {noChat}
    ontoggle={toggleChat}
    emotes={marks.emotes}
    counts={marks.counts}
    eventCounts={marks.eventCounts}
    bind:height={headHeight}
  />
  <div class="cols">
    <div
      class="transcript"
      id="transcript"
      class:with-chat={showChat}
      class:dragging={drag}
      bind:this={root}
      bind:clientWidth={width}
    >
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
      {#each rows.slice(range[0], range[1]) as row, k (row.key)}
        {@const key = row.key}
        {@const win = windows.get(key)}
        <div
          class="line"
          data-index={range[0] + k}
          use:measure
          class:empty={!row.line && !win?.total}
          class:picked={row.line && picked(range[0] + k, row)}
        >
          {#if row.line}
            {@const line = row.line}
            <button
              class="row"
              onpointerdown={(e) => onRowDown(e, range[0] + k)}
              onclick={() => onRowClick(line.start)}
            >
              <span class="time">{formatTime(line.start)}</span>
              <span>{line.text}</span>
            </button>
          {:else}
            <span></span>
          {/if}
          {#if showChat}
            <div class="chat">
              {#each win?.messages ?? [] as message (message.id)}
                <ChatMessage
                  {message}
                  platform={vod.platform}
                  highlight={marks.highlightOf(message.id)}
                  words={marks.words}
                  onplay={play}
                />
              {/each}
              {#if win && win.total > win.messages.length}
                <button class="link accent more" onclick={() => expand(row)} disabled={loading.has(key)}
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
          overlays={marks.overlays}
          events={marks.eventMarks}
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
    user-select: none;
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

  .dragging {
    user-select: none;
    cursor: ns-resize;
  }

  .line.picked > :global(.row) {
    background: var(--accent-soft);
  }
</style>
