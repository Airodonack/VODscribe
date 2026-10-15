<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, tick, untrack } from "svelte";
  import { api, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import ClipBar from "./ClipBar.svelte";
  import ClipStrip from "./ClipStrip.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";
  import { keysForApp } from "./demo";
  import { canEmbed, createPlayer, playsHere, type EmbeddedPlayer } from "./embed";
  import { formatTime, keepsVideo, playbackHint, siteName, watchUrl } from "./platform";

  // One instance per VOD (the parent keys it by id). `at` is a fresh object per
  // jump, so jumping to the same timestamp twice still seeks.
  let { vod, at }: { vod: Vod; at: { time: number; play: boolean } } = $props();

  // Collapsing hides the video but keeps the player loaded; a jump reopens it.
  let collapsed = $state(false);
  let showKeys = $state(false);

  // Clicking the timestamp copies a link to that moment on the VOD's site.
  let copied = $state<"ok" | "failed" | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  async function copyLink() {
    copied = await writeText(watchUrl(vod, at.time)).then(
      () => "ok" as const,
      (e) => (console.error("copy failed", e), "failed" as const),
    );
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copied = null), 1500);
  }

  const site = $derived(siteName(vod));

  const isLocal = $derived(vod.source_path !== null);
  let error = $state<string | null>(null);
  // Audio alone (a podcast, a song): a bar with the controls, not a 16:9 frame.
  // Known from the file type; a file whose type didn't say is found out once loaded.
  let detected = $state(false);
  // Follows the VOD: one opened while still downloading learns once it's done.
  let audioOnly = $derived(vod.audio_only || detected);
  // The app's own player, for a file played here, steps a frame at a time.
  const framed = $derived(playsHere(vod) && !audioOnly);

  // The player loads on demand: until then the VOD's (local) thumbnail stands in,
  // so selecting a VOD contacts nobody. Clicking it, a timestamp, or play starts it.
  let active = $state(false);
  let host = $state<HTMLDivElement>();
  let player: EmbeddedPlayer | null = null;
  // A jump requested while the player is still loading, applied once it's ready.
  let pending: { time: number; play: boolean } | null = null;
  let destroyed = false;

  async function activate(time: number, play = true) {
    if (active) return;
    active = true;
    await tick();
    if (!host || destroyed) return;
    createPlayer(host, vod, time, play, (e) => (error = e))?.then(
      (p) => {
        // Closed while loading: don't leave an invisible player running.
        if (destroyed) return p.destroy();
        player = p;
        if (pending) p.seek(pending.time, pending.play);
        const video = host?.querySelector("video, audio");
        video?.addEventListener("loadedmetadata", () => {
          if (video instanceof HTMLVideoElement && video.videoWidth === 0) detected = true;
        });
      },
      (e) => (error = String(e)),
    );
  }

  // Opened by a jump (a search result or transcript line): start straight away.
  // Later jumps arrive as a new `at` object.
  let handled = untrack(() => at);
  if (handled.play && untrack(() => canEmbed(vod))) activate(handled.time);
  $effect(() => {
    if (at === handled) return;
    handled = at;
    const { time, play } = at;
    collapsed = false;
    if (!active) activate(time, play);
    else if (player) player.seek(time, play);
    else pending = { time, play };
  });

  clips.open(
    untrack(() => vod.id),
    untrack(() => !playsHere(vod)),
  );

  /** Plays from `time`, starting the player if it isn't yet. */
  function playFrom(time: number) {
    collapsed = false;
    if (!active) activate(time);
    else if (player) player.seek(time, true);
    else pending = { time, play: true };
  }

  // A VOD whose video has just been downloaded switches to the app's own
  // player, where it was and as it was (playing or not).
  let here = untrack(() => playsHere(vod));
  $effect(() => {
    const now = playsHere(vod);
    if (now === here) return;
    here = now;
    if (!active || !player) return;
    const time = player.time();
    const play = player.playing();
    player.destroy();
    player = null;
    active = false;
    error = null;
    activate(time, play);
  });

  onDestroy(() => {
    destroyed = true;
    clearTimeout(copiedTimer);
    player?.destroy();
  });

  // Keyboard control for whichever player is open. Keys pressed while the
  // player's own iframe has focus go to that player instead.
  const STEPS: Record<string, number> = { ArrowLeft: -5, ArrowRight: 5, j: -10, l: 10 };
  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (e.ctrlKey || e.metaKey || e.altKey || !canEmbed(vod) || !keysForApp()) return;
    if (target?.closest("input, textarea, select, [contenteditable], [role=dialog]")) return;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (!active && (key === " " || key === "k")) {
      activate(at.time);
      e.preventDefault();
      return;
    }
    if (key === "Escape" && clips.ghost) {
      clips.ghost = null;
    } else if (key === "Enter" && clips.ghost && !target?.closest("button, a")) {
      clips.keep();
    } else if (!player) {
      return;
    } else if (key === "i" || key === "o") {
      const mark = key === "i" ? clips.markIn : clips.markOut;
      mark.call(clips, player.time(), vod.duration);
    } else if ((key === "," || key === ".") && player.step) {
      player.step(key === "," ? -1 : 1);
    } else if (key in STEPS) {
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
      {#if at.time > 0 && isLocal}
        <span class="stamp"><span class="time">{formatTime(at.time)}</span></span>
      {:else if at.time > 0}
        <button
          class="stamp"
          class:copied={copied === "ok"}
          class:failed={copied === "failed"}
          onclick={copyLink}
          title="Copy a link to this moment"
        >
          <span class="time">{formatTime(at.time)}</span>
          <Icon name={copied === "ok" ? "check" : copied === "failed" ? "error" : "copy"} size={12} />
          {#if copied}<span class="copied-text">{copied === "ok" ? "Copied" : "Couldn't copy"}</span>{/if}
        </button>
      {/if}
    </div>
    <button
      class="icon"
      class:on={showKeys}
      onclick={() => (showKeys = !showKeys)}
      aria-expanded={showKeys}
      aria-label="Keyboard shortcuts"
      title="Keyboard shortcuts"><Icon name="keyboard" size={16} /></button
    >
    {#if isLocal}
      <button class="link open" onclick={() => api.showFile(vod.id).catch((e) => (error = String(e)))}>
        Show in folder
      </button>
    {:else}
      <button class="link open" onclick={() => openUrl(watchUrl(vod, at.time))}>
        Open on {site}
        <Icon name="external" size={12} />
      </button>
    {/if}
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
      <kbd>J</kbd><kbd>L</kbd> 10 s <span class="sep">·</span>
      <kbd>I</kbd><kbd>O</kbd> clip from, to <span class="sep">·</span>
      <kbd>Enter</kbd> keep the clip
      {#if framed}
        <span class="sep">·</span> <kbd>,</kbd><kbd>.</kbd> a frame back or on (paused)
        <span class="sep">·</span> <kbd>F</kbd> full screen
      {/if}
    </p>
  {/if}
  {#if canEmbed(vod)}
    <div class="stage" class:collapsed class:audio={audioOnly}>
      <div class="frame" class:audio={audioOnly} class:own={active && playsHere(vod)}>
        {#if active}
          <div class="host" bind:this={host}></div>
        {:else if audioOnly}
          <button class="poster bar" onclick={() => activate(at.time)} aria-label="Play {vod.title}">
            <Icon name="play" size={16} /> Play from {formatTime(at.time)}
          </button>
        {:else}
          <button class="poster" onclick={() => activate(at.time)} aria-label="Play {vod.title}">
            {#if vod.thumb}<Thumb name={vod.thumb} />{/if}
            <span class="play"><Icon name="play" size={26} /></span>
          </button>
        {/if}
      </div>
    </div>
    {#if !collapsed}
      {#if active && vod.duration}
        <ClipBar
          {vod}
          duration={vod.duration}
          time={() => player?.time() ?? at.time}
          onseek={(t) => (player ? player.seek(t, false) : (pending = { time: t, play: false }))}
        />
      {/if}
      <ClipStrip onplay={playFrom} />
    {/if}
    {#if error}
      <p class="error-text note">
        {error}
        {#if isLocal}
          <button class="link" onclick={() => api.openFile(vod.id).catch((e) => (error = String(e)))}
            >Open it in your video player</button
          >
        {:else if vod.media && /can't be played|couldn't be played/.test(error)}
          {playbackHint()} Meanwhile, use “Open on {site}”.
        {/if}
      </p>
    {/if}
  {:else}
    <p class="muted note">
      {#if keepsVideo(vod) && vod.status !== "done" && vod.status !== "error"}
        The video plays here once it's downloaded. Meanwhile, use “Open on {site}” to watch from {formatTime(at.time)}.
      {:else if keepsVideo(vod)}
        This video isn't kept on this computer. Use “Open on {site}” to watch from {formatTime(at.time)}.
      {:else}
        {site} doesn't offer an embeddable VOD player. Use “Open on {site}” to watch from {formatTime(at.time)}.
      {/if}
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

  .stamp.failed {
    color: var(--danger);
  }

  .legend {
    margin: 0;
    padding: 0 16px 8px;
    font-size: 12px;
  }

  .legend kbd + kbd {
    margin-left: 2px;
  }

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
    /* The website's demo, a window within a page, sets a smaller share of its own height. */
    max-height: var(--player-max-height, 44vh);
    margin: 0 auto;
  }

  /* The app's own player sizes its picture itself (LocalPlayer.svelte). */
  .frame.own {
    aspect-ratio: auto;
    max-height: none;
  }

  .host {
    width: 100%;
    height: 100%;
  }

  /* Audio alone: the controls in the panel's own colors, at their own height,
     rather than floating on a video's black stage. */
  .stage.audio {
    background: var(--surface);
    padding: 0 16px 8px;
  }

  .frame.audio {
    aspect-ratio: auto;
    height: auto;
    max-height: none;
  }

  .frame.audio .host {
    height: auto;
  }

  .poster.bar {
    height: 40px;
    border-radius: var(--radius);
    align-items: center;
    justify-content: center;
    gap: 8px;
    background: var(--surface-2);
    color: var(--text);
    font-size: 13px;
  }

  .poster.bar:hover {
    background: var(--accent);
    color: var(--accent-text);
  }

  .poster {
    position: relative;
    display: flex;
    width: 100%;
    height: 100%;
    padding: 0;
    border: none;
    border-radius: 0;
    background: #000;
    overflow: hidden;
  }

  .poster :global(img) {
    opacity: 0.85;
    transition: opacity 0.15s;
  }

  .poster:hover :global(img) {
    opacity: 1;
  }

  .play {
    position: relative;
    z-index: 1;
    margin: auto;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding-left: 4px;
    background: rgb(0 0 0 / 0.6);
    color: #fff;
    transition:
      transform 0.15s,
      background 0.15s;
  }

  .poster:hover .play {
    transform: scale(1.06);
    background: var(--accent);
  }

  .note {
    margin: 0;
    padding: 0 16px 12px;
  }
</style>
