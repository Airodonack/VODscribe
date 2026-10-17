<script lang="ts">
  import type { ChatLine, WordEmote } from "./api";
  import { pieces } from "./chatParts";
  import { DEMO_NAMES } from "./demo";
  import Emote from "./Emote.svelte";
  import { eventBadge, eventColor, eventRgb } from "./events";
  import { formatTime } from "./platform";

  let {
    message,
    platform,
    highlight = null,
    words = new Map(),
    onplay,
  }: {
    message: ChatLine;
    /** The VOD's, for naming events as the site does. */
    platform: string;
    /** The color of a timeline layer that matched it, if any. */
    highlight?: string | null;
    /** The words this chat's emote providers (7TV, BTTV, FFZ) show as emotes, by exact name. */
    words?: Map<string, WordEmote>;
    onplay: (time: number) => void;
  } = $props();

  let shown = $derived(pieces(message.parts, words));
</script>

<button
  class="message"
  class:highlighted={highlight}
  style:--highlight={highlight}
  data-time={message.time}
  onclick={() => onplay(message.time)}
  title="Play from {formatTime(message.time)}"
>
  {#if message.kind !== "message"}<span
      class="badge"
      style:--event={eventColor(platform)}
      style:--event-rgb={eventRgb(platform)}>{eventBadge(message, platform)}</span
    >{/if}
  <span class="chat-author" style:--author={message.color} title={DEMO_NAMES}>{message.author}</span>
  {#each shown as piece, i (i)}{#if piece.type === "text"}{piece.text}{:else if piece.overlays.length}<span
        class="stack"
        ><Emote name={piece.name} url={piece.url} />{#each piece.overlays as over, j (j)}<span class="over"
            ><Emote name={over.name} url={over.url} /></span
          >{/each}</span
      >{:else}<Emote name={piece.name} url={piece.url} />{/if}{/each}
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

  /* An emote with others drawn over it (a hat, a spin), centered on it. */
  .stack {
    position: relative;
    display: inline-block;
  }

  .over {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }

  /* Matched by a timeline layer: a bar and a tint in its color. */
  .message.highlighted {
    box-shadow: inset 2px 0 0 var(--highlight);
    background: color-mix(in srgb, var(--highlight) 12%, transparent);
  }
</style>
