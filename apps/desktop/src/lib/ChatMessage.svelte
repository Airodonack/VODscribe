<script lang="ts">
  import type { ChatLine } from "./api";
  import Emote from "./Emote.svelte";
  import { formatTime } from "./platform";

  let { message, onplay }: { message: ChatLine; onplay: (time: number) => void } = $props();
</script>

<button class="message" onclick={() => onplay(message.time)} title="Play from {formatTime(message.time)}">
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

  .author {
    font-weight: 600;
    margin-right: 6px;
    /* Names keep a hint of their chat color, toned toward the text color so
       every one is readable on this theme's background. */
    color: color-mix(in srgb, var(--author, var(--muted)) 65%, var(--text));
  }
</style>
