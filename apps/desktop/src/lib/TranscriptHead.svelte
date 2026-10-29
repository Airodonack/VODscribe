<script lang="ts" module>
  /** The switch between the transcript and the chapters and highlights, on a
   *  window too narrow for their column (App.svelte). */
  export type PaneTabs = {
    /** Which shows: the transcript, or the pane in its place. */
    open: "transcript" | "annotations";
    /** The pane's segment: "Highlights", "Chapters" or "Chapters & highlights". */
    label: string;
    /** The pane has something to show; without, its segment is greyed and can't be picked. */
    enabled: boolean;
    onopen: (to: "transcript" | "annotations") => void;
  };
</script>

<script lang="ts">
  import { tick } from "svelte";
  import { api, type CastMember, type Stretch, type Vod } from "./api";
  import { castDetail, speakerColor } from "./annotated";
  import { castLink } from "./castLink.svelte";
  import { cuts } from "./cuts.svelte";
  import { displayName } from "./castNames";
  import { clock } from "./povs";
  import Popover from "./Popover.svelte";
  import { formatSpan } from "./platform";
  import type { EventKind } from "./events";
  import CastDisc from "./CastDisc.svelte";
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";
  import Timeline from "./Timeline.svelte";

  // The transcript view's header (Transcript.svelte), across the transcript and
  // the minimap: how far transcription is, the chat's status and its Chat
  // button, and the Timeline button. On a window too narrow for the chapters
  // and highlights' column, its label is the switch between the transcript
  // and them. An edit's (no `vod`) has its line count,
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
    tabs = null,
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
    tabs?: PaneTabs | null;
  } = $props();

  // The switch's two segments are tabs: ← and → between them, as tabs go,
  // the keyboard's focus going with the one picked.
  let switchEl = $state<HTMLElement>();
  function onSwitchKey(e: KeyboardEvent) {
    if (!tabs?.enabled || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
    e.preventDefault();
    tabs.onopen(tabs.open === "transcript" ? "annotations" : "transcript");
    void tick().then(() => switchEl?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus());
  }

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
  // The Cast list's links go to their streams at the playhead, when it's this VOD's.
  const playhead = $derived(vod && cuts.vodId === vod.id ? cuts.now : null);
</script>

<div class="head" bind:offsetHeight={height}>
  <div class="lead">
    {#if tabs}
      <!-- One control of two segments, greyed while the pane has nothing to show. -->
      <div
        class="switch"
        class:empty={!tabs.enabled}
        role="tablist"
        aria-label="Transcript, or {tabs.label.toLowerCase()}"
        bind:this={switchEl}
      >
        <button
          role="tab"
          id="tab-transcript"
          aria-selected={tabs.open === "transcript"}
          aria-controls="transcript-panel"
          tabindex={tabs.open === "transcript" ? 0 : -1}
          onclick={() => tabs.onopen("transcript")}
          onkeydown={onSwitchKey}>Transcript</button
        >
        <button
          role="tab"
          id="tab-annotations"
          aria-selected={tabs.open === "annotations"}
          aria-controls="annotations-panel"
          tabindex={tabs.open === "annotations" ? 0 : -1}
          disabled={!tabs.enabled}
          title={tabs.enabled ? undefined : "No chapters or highlights here"}
          onclick={() => tabs.onopen("annotations")}
          onkeydown={onSwitchKey}>{tabs.label}</button
        >
      </div>
    {/if}
    <p class="status muted" title={vod?.status === "transcribing" ? "New lines appear as they're produced" : undefined}>
      {#if !tabs}<span class="eyebrow">Transcript</span>{/if}
      {#if vod?.status === "transcribing"}
        <span class="sep">·</span><Spinner label="Transcribing" />
        Transcribing {span}{Math.round(vod.progress * 100)}%
      {:else if lineCount > 0}
        <span class="sep">·</span>{lineCount} line{lineCount === 1 ? "" : "s"}
      {/if}
    </p>
  </div>
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
            {@const to = castLink(member, vod, playhead)}
            <li>
              <CastDisc name={displayName(member)} color={speakerColor(cast, member.name)} {member} {to} size={20} />
              <span class="who">
                <!-- The streamer who plays them is in the name ("Gigi (GEEGA)"), so only their channel follows,
                     or their own stream at the playhead when it ran alongside. -->
                <strong>{displayName(member)}</strong>
                <span class="muted">{castDetail(member)}</span>
                {#if to}
                  <span class="muted"
                    ><a
                      class="channel"
                      href={to.href}
                      title={to.pov ? to.title : undefined}
                      onclick={(e) => (e.preventDefault(), to.follow())}
                      >{to.pov ? `Their stream at ${clock(to.pov.offset)}` : to.title}</a
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

  .lead {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  /* The switch: one bordered control, its segments in the eyebrow's type.
     The one showing is in the accent colour, the other in the text's; with
     nothing in the pane, all of it is muted, as the eyebrow is. */
  .switch {
    flex: none;
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .switch button {
    border: none;
    border-radius: 0;
    background: none;
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text);
  }

  .switch button:first-child {
    border-radius: calc(var(--radius-sm) - 1px) 0 0 calc(var(--radius-sm) - 1px);
  }

  .switch button:last-child {
    border-left: 1px solid var(--border);
    border-radius: 0 calc(var(--radius-sm) - 1px) calc(var(--radius-sm) - 1px) 0;
  }

  .switch button:hover:not(:disabled, [aria-selected="true"]) {
    background: var(--surface-2);
  }

  .switch button[aria-selected="true"] {
    color: var(--accent);
  }

  .switch.empty,
  .switch.empty button:last-child {
    border-color: var(--muted);
  }

  .switch.empty button,
  .switch.empty button[aria-selected="true"] {
    color: var(--muted);
  }

  .switch button:disabled {
    opacity: 1;
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

  /* Each with the disc their lines have in the transcript, beside their name. */
  .cast li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
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
