<script lang="ts">
  import type { ChatLine } from "./api";
  import Emote from "./Emote.svelte";
  import { eventBadge, eventColor } from "./events";
  import { formatTime } from "./platform";

  let {
    message,
    platform,
    highlight = null,
    onplay,
  }: {
    message: ChatLine;
    /** The VOD's, for naming events as the site does. */
    platform: string;
    /** The color of a timeline layer that matched it, if any. */
    highlight?: string | null;
    onplay: (time: number) => void;
  } = $props();
</script>

<button
  class="message"
  class:highlighted={highlight}
  style:--highlight={highlight}
  onclick={() => onplay(message.time)}
  title="Play from {formatTime(message.time)}"
>
  {#if message.kind !== "message"}<span class="badge" style:--event={eventColor(platform)}
      >{eventBadge(message, platform)}</span
    >{/if}
  <span class="author" style:--author={message.color}>{message.author}</span>
  {#each message.parts as part, i (i)}{#if part.type === "emote"}<Emote
        name={part.name}
        url={part.url}
      />{:else}{part.text}{/if}{/each}
</button>

<style>
  .message {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    border-radius: var(--radius-sm);
    padding: 2px 8px;
    font-size: 12.5px;
    line-height: 1.5;
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .message:hover {
    background: var(--surface-2);
  }

  /* A sub, gift or tip, in its event's color. */
  .badge {
    display: inline-block;
    margin-right: 6px;
    padding: 0 6px;
    border-radius: 8px;
    font-size: 11px;
    font-weight: 600;
    line-height: 1.6;
    color: color-mix(in srgb, var(--event) 70%, var(--text));
    background: color-mix(in srgb, var(--event) 18%, transparent);
  }

  /* Matched by a timeline layer: a bar and a tint in its color. */
  .message.highlighted {
    box-shadow: inset 2px 0 0 var(--highlight);
    background: color-mix(in srgb, var(--highlight) 12%, transparent);
  }

  .author {
    font-weight: 600;
    margin-right: 6px;
    /* Names keep a hint of their chat color, toned toward the text color so
       every one is readable on this theme's background. */
    color: color-mix(in srgb, var(--author, var(--muted)) 65%, var(--text));
  }
</style>
