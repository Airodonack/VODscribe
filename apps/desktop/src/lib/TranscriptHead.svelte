<script lang="ts">
  import { api, type CastMember, type Stretch, type Vod } from "./api";
  import { castDetail, speakerColor } from "./annotated";
  import { channelLabel, channelUrl } from "./accounts";
  import { displayName } from "./castNames";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Popover from "./Popover.svelte";
  import { formatSpan } from "./platform";
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
    cast = [],
    stretch = null,
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
    /** Who's in the VOD, from an annotations import: the Cast button lists them. */
    cast?: CastMember[];
    /** The stretch being transcribed, if any. */
    stretch?: Stretch | null;
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

  // Why the chat fetch stopped, short enough for the header: the site's own
  // words, after what the app was doing ("Twitch wouldn't give the chat:
  // service error"); the whole of it in the tooltip.
  const chatWhy = $derived.by(() => {
    const full = vod?.chat_error?.trim() ?? "";
    const why = full.split(": ").at(-1) ?? "";
    return why.length > 48 ? `${why.slice(0, 47)}…` : why;
  });

  // Which stretch is being transcribed; the whole VOD needs no saying.
  const span = $derived(
    stretch && !(stretch.start <= 0 && stretch.end === null)
      ? `${formatSpan(stretch.start, stretch.end ?? vod?.duration ?? 0)} · `
      : "",
  );

  let castOpen = $state(false);
  let castAnchor = $state<HTMLElement>();
</script>

<div class="head" bind:offsetHeight={height}>
  <p class="status muted" title={vod?.status === "transcribing" ? "New lines appear as they're produced" : undefined}>
    <span class="eyebrow">Transcript</span>
    {#if vod?.status === "transcribing"}
      <span class="sep">·</span><Spinner label="Transcribing" />
      Transcribing {span}{Math.round(vod.progress * 100)}%
    {:else if lineCount > 0}
      <span class="sep">·</span>{lineCount} line{lineCount === 1 ? "" : "s"}
    {/if}
  </p>
  {#if cast.length}
    <div class="cast-anchor" bind:this={castAnchor}>
      <button
        class="cast-toggle"
        class:on={castOpen}
        onclick={() => (castOpen = !castOpen)}
        aria-expanded={castOpen}
        title="Who's in this VOD"
      >
        <Icon name="users" size={14} /> Cast <span class="muted">{cast.length}</span>
      </button>
      <Popover bind:open={castOpen} anchor={castAnchor} label="Cast" width={280}>
        <ul class="cast">
          {#each cast as member (member.name)}
            <li>
              <span class="swatch" style:background={speakerColor(cast, member.name)}></span>
              <span class="who">
                <!-- The streamer who plays them is in the name ("GEEGA (Gigi)"), so only their channel follows. -->
                <strong>{displayName(member)}</strong>
                <span class="muted">{castDetail(member)}</span>
                {#if member.account}
                  {@const url = channelUrl(member.account)}
                  <span class="muted"
                    ><a class="channel" href={url} onclick={(e) => (e.preventDefault(), openUrl(url))}
                      >{channelLabel(member.account)}</a
                    ></span
                  >
                {/if}
              </span>
            </li>
          {/each}
        </ul>
      </Popover>
    </div>
  {/if}
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
          <!-- What was fetched stays, and Try again carries on from where it stopped. -->
          <span
            class="error-text"
            title={`${vod.chat_error ?? "The chat fetch failed"}${chatCount > 0 ? `. ${chatCount.toLocaleString()} message${chatCount === 1 ? "" : "s"} fetched before it stopped; Try again fetches the rest.` : ""}`}
            >{chatCount > 0 ? "Chat stopped partway" : "Couldn't fetch chat"}{chatWhy ? `: ${chatWhy}` : ""}</span
          >
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

  .cast-anchor {
    position: relative;
    flex: none;
    /* Beside the transcript's status, with the chat's at the far side. */
    margin-right: auto;
  }

  .cast-toggle {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    padding: 3px 8px;
  }

  .cast-toggle.on {
    border-color: var(--accent);
  }

  .cast {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }

  .cast li {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  /* The colour their lines' names have in the transcript. */
  .swatch {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
    font-size: 13px;
  }

  .who .muted {
    font-size: 12px;
  }

  .channel {
    color: var(--accent);
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
