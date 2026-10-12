<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount, untrack } from "svelte";
  import type { Vod } from "./api";
  import Icon from "./Icon.svelte";
  import { canEmbed, createPlayer, type EmbeddedPlayer } from "./embed";
  import { formatTime, platformName, watchUrl } from "./platform";

  // One instance per VOD (the parent keys it by id). `at` is a fresh object per
  // jump, so jumping to the same timestamp twice still seeks.
  let { vod, at }: { vod: Vod; at: { time: number; play: boolean } } = $props();

  // Collapsing hides the video but keeps the player loaded; a jump reopens it.
  let collapsed = $state(false);
  let showKeys = $state(false);

  // Clicking the timestamp copies a link to that moment on the VOD's site.
  let copied = $state(false);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  async function copyLink() {
    await writeText(watchUrl(vod, at.time));
    copied = true;
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copied = false), 1500);
  }

  let host = $state<HTMLDivElement>();
  let player: EmbeddedPlayer | null = null;
  // A jump requested while the player is still loading, applied once it's ready.
  let pending: { time: number; play: boolean } | null = null;


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
    createPlayer(target, vod, start.time, start.play)?.then(
      (p) => {
        // Closed while loading: don't leave an invisible player running.
        if (destroyed) return p.destroy();
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
    collapsed = false;
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
      <span class="meta muted">
        <span class="platform {vod.platform}">{site}</span>
        {#if vod.channel}<span class="sep">·</span>{vod.channel}{/if}
        {#if vod.duration}<span class="sep">·</span>{formatTime(vod.duration)}{/if}
      </span>
      {#if at.time > 0}
        <button class="stamp" class:copied onclick={copyLink} title="Copy a link to this moment">
          <span class="time">{formatTime(at.time)}</span>
          <Icon name={copied ? "check" : "copy"} size={12} />
          {#if copied}<span class="copied-text">Copied</span>{/if}
        </button>
      {/if}
    </div>
    <button
      class="icon keys"
      class:on={showKeys}
      onclick={() => (showKeys = !showKeys)}
      aria-expanded={showKeys}
      aria-label="Keyboard shortcuts"
      title="Keyboard shortcuts"><Icon name="keyboard" size={16} /></button
    >
    <button class="link open" onclick={() => openUrl(watchUrl(vod, at.time))}>
      Open on {site} <Icon name="external" size={12} />
    </button>
    {#if canEmbed(vod)}
      <button
        class="icon"
        onclick={() => (collapsed = !collapsed)}
        aria-expanded={!collapsed}
        aria-label={collapsed ? "Show player" : "Hide player"}
        title={collapsed ? "Show player" : "Hide player"}
      >
        <Icon name={collapsed ? "chevron-down" : "chevron-up"} size={16} />
      </button>
    {/if}
  </div>
  {#if showKeys}
    <p class="legend muted">
      <kbd>Space</kbd> or <kbd>K</kbd> play/pause <span class="sep">·</span>
      <kbd>←</kbd><kbd>→</kbd> 5 s <span class="sep">·</span>
      <kbd>Shift</kbd>+<kbd>←</kbd><kbd>→</kbd> 1 s <span class="sep">·</span>
      <kbd>J</kbd><kbd>L</kbd> 10 s
    </p>
  {/if}
  {#if canEmbed(vod)}
    <div class="stage" class:collapsed>
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
    gap: 10px;
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
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .meta {
    flex: none;
    font-size: 12px;
    white-space: nowrap;
  }

  .stamp {
    flex: none;
    gap: 4px;
    height: 22px;
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 12px;
    border-radius: var(--radius-sm);
  }

  .stamp:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }

  .stamp.copied {
    color: var(--ok);
  }

  .sep {
    margin: 0 4px;
  }

  .legend {
    margin: 0;
    padding: 0 16px 8px;
    font-size: 12px;
  }

  .legend kbd + kbd {
    margin-left: 2px;
  }

  .keys,
  .head .icon {
    width: 28px;
    height: 28px;
  }

  .open {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .stage {
    background: var(--stage);
  }

  /* Hidden, not unmounted: the player keeps its state and keeps playing. */
  .stage.collapsed {
    display: none;
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
