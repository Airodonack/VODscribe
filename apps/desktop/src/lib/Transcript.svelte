<script lang="ts">
  import { flushSync } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { api, events, type ChatLine, type Line, type Vod } from "./api";
  import ChatMessage from "./ChatMessage.svelte";
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

  function read(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }

  let root: HTMLDivElement;

  // Showing or hiding chat changes every row's height; keep the line at the
  // top of the view where it was, rather than jumping to another time.
  function toggleChat() {
    const scroller = root.closest<HTMLElement>(".body");
    const top = scroller?.getBoundingClientRect().top ?? 0;
    // The first line that starts in view: one cut off at the top may shrink out of sight.
    const anchor = [...root.querySelectorAll<HTMLElement>(".line[data-start]")].find(
      (el) => el.getBoundingClientRect().top >= top,
    );
    const offset = anchor ? anchor.getBoundingClientRect().top - top : 0;
    // Read now: rows are keyed by position, so after the change this element may hold another line.
    const start = anchor?.dataset.start;
    // Applied at once, so the rows are measured as they now are.
    flushSync(() => (chatOpen = !chatOpen));
    try {
      localStorage.setItem(CHAT_OPEN, String(chatOpen));
    } catch {
      // Not remembered, which is fine.
    }
    if (!scroller || start === undefined) return;
    const same = root.querySelector<HTMLElement>(`.line[data-start="${start}"]`);
    if (same) scroller.scrollTop += same.getBoundingClientRect().top - top - offset;
  }

  let chatSeq = 0;
  $effect(() => {
    const id = vodId;
    const status = vod.chat;
    const mine = ++chatSeq;
    chat = [];
    chatError = null;
    if (status !== "done") return;
    api.chat(id).then(
      (c) => mine === chatSeq && (chat = c),
      (e) => mine === chatSeq && (chatError = String(e)),
    );
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

  let showChat = $derived(hasChat && chatOpen);
  let play = (time: number) => onplay(vod.id, time);
</script>

<div class="transcript" class:with-chat={showChat} bind:this={root}>
  <div class="head">
    <p class="status muted">
      <span class="eyebrow">Transcript</span>
      {#if vod.status === "transcribing"}
        <span class="sep">·</span>{Math.round(vod.progress * 100)}% done; new lines appear as they're produced
      {:else if lines.length > 0}
        <span class="sep">·</span>{lines.length} line{lines.length === 1 ? "" : "s"}
      {/if}
    </p>
    {#if hasChat}
      <div class="chat-head">
        <span class="muted chat-status">
          {#if vod.chat === "pending"}
            Fetching chat{chatProgress !== null ? ` · ${Math.round(chatProgress * 100)}%` : "…"}
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
          class:on={chatOpen}
          onclick={toggleChat}
          aria-expanded={chatOpen}
          title={chatOpen ? "Hide chat" : "Show chat beside the transcript"}
        >
          <Icon name="message" size={14} /> Chat <Icon name={chatOpen ? "chevron-up" : "chevron-down"} size={14} />
        </button>
      </div>
    {/if}
  </div>
  {#if error}
    <p class="error-text">{error}</p>
  {:else if lines.length === 0}
    <p class="muted">
      {vod.status === "done" ? "No speech was found in this VOD." : "The transcript appears here as it is produced."}
    </p>
  {/if}
  {#each rows as row, r (r)}
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
            <ChatMessage {message} onplay={play} />
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
        {#each chat as message, m (m)}
          <ChatMessage {message} onplay={play} />
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .transcript {
    padding: 10px 20px 32px;
    max-width: 960px;
    /* toggleChat keeps the view in place itself; the browser's own scroll
       anchoring (Firefox, Chromium) would move it a second time. */
    overflow-anchor: none;
  }

  .transcript.with-chat {
    max-width: 1320px;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin: 0 0 8px;
  }

  .status {
    margin: 0;
    font-size: 12px;
  }

  .chat-head {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
  }

  .chat-toggle {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    padding: 3px 8px;
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
</style>
