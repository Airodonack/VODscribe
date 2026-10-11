<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount, untrack } from "svelte";
  import type { Vod } from "./api";
  import { canEmbed, createPlayer, type EmbeddedPlayer } from "./embed";
  import { formatTime, platformName, watchUrl } from "./platform";

  // One instance per VOD (the parent keys it by id). `at` is a fresh object per
  // jump, so jumping to the same timestamp twice still seeks.
  let { vod, at, onclose }: { vod: Vod; at: { time: number; play: boolean }; onclose: () => void } = $props();

  let host = $state<HTMLDivElement>();
  let player: EmbeddedPlayer | null = null;
  // A jump requested while the player is still loading, applied once it's ready.
  let pending: { time: number; play: boolean } | null = null;
  let error = $state<string | null>(null);
  const site = $derived(platformName[vod.platform] ?? "site");

  onMount(() => {
    if (!host) return;
    // The embed APIs replace the element they're given, so give them one Svelte doesn't own.
    const target = document.createElement("div");
    target.style.cssText = "width: 100%; height: 100%";
    host.appendChild(target);
    const start = untrack(() => at);
    createPlayer(target, vod, start.time, start.play)?.then(
      (p) => {
        player = p;
        if (pending) p.seek(pending.time, pending.play);
      },
      (e) => (error = String(e)),
    );
  });

  let first = true;
  $effect(() => {
    const { time, play } = at;
    if (first) {
      first = false;
      return;
    }
    if (player) player.seek(time, play);
    else pending = { time, play };
  });

  onDestroy(() => player?.destroy());
</script>

<section>
  <div class="head">
    <div class="info">
      <strong>{vod.title}</strong>
      {#if at.time > 0}<span class="time">{formatTime(at.time)}</span>{/if}
    </div>
    <button class="link" onclick={() => openUrl(watchUrl(vod, at.time))}>Open on {site}</button>
    <button class="link" onclick={onclose} aria-label="Close player">Close</button>
  </div>
  {#if canEmbed(vod)}
    <div class="frame" bind:this={host}></div>
    {#if error}<p class="error-text note">{error}</p>{/if}
  {:else}
    <p class="muted note">
      {site} doesn't offer an embeddable VOD player. Use “Open on {site}” to watch from {formatTime(at.time)}.
    </p>
  {/if}
</section>

<style>
  section {
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 8px 16px;
  }

  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 10px;
    align-items: baseline;
  }

  .info strong {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .frame {
    aspect-ratio: 16 / 9;
    max-height: 45vh;
    margin: 0 auto;
    background: #000;
  }

  .frame :global(iframe) {
    width: 100%;
    height: 100%;
    border: 0;
    display: block;
  }

  .note {
    margin: 0;
    padding: 0 16px 12px;
  }
</style>
