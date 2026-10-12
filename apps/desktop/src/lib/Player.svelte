<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount, untrack } from "svelte";
  import type { Vod } from "./api";
  import Icon from "./Icon.svelte";
  import { canEmbed, createPlayer, type EmbeddedPlayer } from "./embed";
  import { formatTime, platformName, watchUrl } from "./platform";

  // One instance per VOD (the parent keys it by id). `at` is a fresh object per
  // jump, so jumping to the same timestamp twice still seeks.
  let { vod, at, onclose }: { vod: Vod; at: { time: number; play: boolean }; onclose: () => void } = $props();

  let host = $state<HTMLDivElement>();
  let player: EmbeddedPlayer | null = null;
  // A jump requested while the player is still loading, applied once it's ready.
  let pending: { time: number; play: boolean } | null = null;

  // Jumps land a little early: word timestamps and the players' seeking are
  // both slightly imprecise, and a cut-off first word is worse than a beat of lead-in.
  const LEAD_IN = 1;
  const leadIn = (t: number) => Math.max(0, t - LEAD_IN);

  // Keyboard control for whichever player is open. Keys pressed while the
  // player's own iframe has focus go to that player instead.
  const STEPS: Record<string, number> = { ArrowLeft: -5, ArrowRight: 5, j: -10, l: 10 };
  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (!player || e.ctrlKey || e.metaKey || e.altKey) return;
    if (target?.closest("input, textarea, select, [contenteditable]")) return;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (key in STEPS) {
      const step = e.shiftKey && key.startsWith("Arrow") ? Math.sign(STEPS[key]) : STEPS[key];
      player.seek(Math.max(0, player.time() + step), false);
    } else if (key === " " || key === "k") {
      // preventDefault below also stops Space from clicking a focused button,
      // such as the search result that was just clicked.
      player.togglePlay();
    } else {
      return;
    }
    e.preventDefault();
  }
  let error = $state<string | null>(null);
  const site = $derived(platformName[vod.platform] ?? "site");

  onMount(() => {
    if (!host) return;
    // The embed APIs replace the element they're given, so give them one Svelte doesn't own.
    const target = document.createElement("div");
    target.style.cssText = "width: 100%; height: 100%";
    host.appendChild(target);
    const start = untrack(() => at);
    createPlayer(target, vod, leadIn(start.time), start.play)?.then(
      (p) => {
        // Closed while loading: don't leave an invisible player running.
        if (destroyed) return p.destroy();
        player = p;
        if (pending) p.seek(leadIn(pending.time), pending.play);
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
    if (player) player.seek(leadIn(time), play);
    else pending = { time, play };
  });

  let destroyed = false;
  onDestroy(() => {
    destroyed = true;
    player?.destroy();
  });
</script>

<svelte:window onkeydown={onKey} />

<section>
  <div class="head">
    <div class="info">
      <strong title={vod.title}>{vod.title}</strong>
      {#if at.time > 0}<span class="time">{formatTime(at.time)}</span>{/if}
    </div>
    <span
      class="keys muted"
      title="Space or K: play/pause · ← →: 5 s (Shift: 1 s) · J L: 10 s"
      aria-label="Keyboard shortcuts: Space or K play/pause, arrows 5 seconds, Shift and arrows 1 second, J and L 10 seconds"
      ><Icon name="keyboard" size={15} /></span
    >
    <button class="link open" onclick={() => openUrl(watchUrl(vod, at.time))}>
      Open on {site} <Icon name="external" size={12} />
    </button>
    <button class="icon close" onclick={onclose} aria-label="Close player" title="Close player">
      <Icon name="x" size={16} />
    </button>
  </div>
  {#if canEmbed(vod)}
    <div class="stage">
      <div class="frame" bind:this={host}></div>
    </div>
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
    gap: 12px;
    padding: 6px 10px 6px 16px;
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

  .keys {
    display: flex;
    cursor: help;
  }

  .open {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .close {
    width: 28px;
    height: 28px;
  }

  .stage {
    background: var(--stage);
  }

  .frame {
    aspect-ratio: 16 / 9;
    max-height: 44vh;
    margin: 0 auto;
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
