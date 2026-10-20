<script lang="ts">
  import { api, type Vod } from "./api";
  import type { EventKind } from "./events";
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";
  import Timeline from "./Timeline.svelte";

  // The transcript view's header (Transcript.svelte), across the transcript and
  // the minimap: how far transcription is, the chat's status and its Chat
  // button, and the Timeline button. An edit's (no `vod`) has its line count,
  // its Chat button, and the Timeline button of the VOD with most time in it.
  let {
    vod,
    timelineVod,
    lineCount,
    chatCount,
    chatProgress,
    chatError = $bindable(),
    hasChat,
    showChat,
    noChat,
    ontoggle,
    emotes,
    counts,
    eventCounts,
    height = $bindable(),
  }: {
    vod: Vod | null;
    /** Whose Timeline panel it shows: the VOD, or an edit's with most time in it. */
    timelineVod: Vod | null;
    lineCount: number;
    /** Messages in the chat (so far, while it's fetched). */
    chatCount: number;
    /** How far the chat fetch is, 0 to 1, when known. */
    chatProgress: number | null;
    chatError: string | null;
    /** Whether the VOD's platform has chat replays. */
    hasChat: boolean;
    showChat: boolean;
    /** The VOD has no chat replay (it wasn't a livestream, say). */
    noChat: boolean;
    ontoggle: () => void;
    emotes: Map<string, string>;
    counts: Map<string, number>;
    eventCounts: Map<EventKind, number>;
    /** The header's height, for what's laid out below it. */
    height: number;
  } = $props();
</script>

<div class="head" bind:offsetHeight={height}>
  <p class="status muted" title={vod?.status === "transcribing" ? "New lines appear as they're produced" : undefined}>
    <span class="eyebrow">Transcript</span>
    {#if vod?.status === "transcribing"}
      <span class="sep">·</span><Spinner label="Transcribing" />
      Transcribing {Math.round(vod.progress * 100)}%
    {:else if lineCount > 0}
      <span class="sep">·</span>{lineCount} line{lineCount === 1 ? "" : "s"}
    {/if}
  </p>
  {#if hasChat}
    <div class="chat-head">
      <span
        class="muted chat-status"
        title={vod?.chat === "pending"
          ? `Fetching chat: ${chatCount.toLocaleString()} message${chatCount === 1 ? "" : "s"} so far`
          : undefined}
      >
        {#if vod?.chat === "pending"}
          <!-- Beside the Chat button, so the spinner and a percentage say enough. -->
          <Spinner label="Fetching chat" />
          {chatProgress !== null ? `${Math.round(chatProgress * 100)}%` : ""}
        {:else if vod?.chat === "none"}
          No chat replay
        {:else if vod?.chat === "failed"}
          <span class="error-text">Couldn't fetch chat</span>
          <button class="link accent" onclick={() => api.retryChat(vod.id).catch((e) => (chatError = String(e)))}
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
        onclick={ontoggle}
        disabled={noChat}
        aria-expanded={showChat}
        title={noChat ? "This VOD has no chat replay" : showChat ? "Hide chat" : "Show chat beside the transcript"}
      >
        <Icon name="message" size={14} /> Chat <Icon name={showChat ? "chevron-up" : "chevron-down"} size={14} />
      </button>
      {#if timelineVod}<Timeline vod={timelineVod} {emotes} {counts} {eventCounts} />{/if}
    </div>
  {/if}
</div>

<style>
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
</style>
