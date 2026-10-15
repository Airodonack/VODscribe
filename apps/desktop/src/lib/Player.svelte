<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, tick, untrack } from "svelte";
  import { api, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import ClipStrip from "./ClipStrip.svelte";
  import { cuts } from "./cuts.svelte";
  import { exportStretch, stretchAt } from "./dragout";
  import { OWN_VODS_NOTE, PERMISSION_NOTE } from "./permission";
  import Popover from "./Popover.svelte";
  import Scrubber from "./Scrubber.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";
  import { keysForApp } from "./demo";
  import { canEmbed, createPlayer, playsHere, type EmbeddedPlayer } from "./embed";
  import { formatTime, keepsVideo, os, playbackHint, siteName, watchUrl } from "./platform";

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
  let player = $state.raw<EmbeddedPlayer | null>(null);
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

  cuts.open(untrack(() => vod.id));
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

  // What the controls show, read from the player every frame (it may be in
  // an iframe that only reports by message, so there's nothing to bind to).
  let now = $state(untrack(() => at.time));
  let playing = $state(false);
  let volume = $state(1);
  let muted = $state(false);
  let rate = $state(1);
  let captionsAvailable = $state(false);
  let captionsOn = $state(false);
  let playerDuration = $state(0);
  // Exact for the app's own player; others don't say, so marks there are as near as 30 fps gets.
  let fps = $state(30);
  let poll = 0;
  function read() {
    poll = requestAnimationFrame(read);
    const p = player;
    if (!p) return;
    now = p.time();
    playing = p.playing();
    playerDuration = p.duration?.() ?? 0;
    fps = p.fps?.() ?? 30;
    cuts.fps = fps;
    if (p.volume) [volume, muted] = [p.volume.get(), p.volume.muted()];
    if (p.rate) rate = p.rate.get();
    captionsAvailable = p.captions?.available() ?? false;
    captionsOn = p.captions?.on() ?? false;
  }
  poll = requestAnimationFrame(read);
  // The VOD's length as the site said, or as the player found when the site didn't say.
  const duration = $derived(vod.duration || playerDuration);

  /** Moves the playhead, starting the player there (paused) if it isn't yet. */
  function seekTo(time: number) {
    now = time;
    if (player) player.seek(time, false);
    else if (active) pending = { time, play: false };
    else activate(time, false);
  }

  function togglePlay() {
    if (player) player.togglePlay();
    else if (!active) activate(now);
  }

  // Export: the stretch the playhead is in becomes a clip, a pill in the clip
  // bar. Hovering the button lights the stretch up on the timeline.
  let exportButton = $state<HTMLElement>();
  let exportHover = $state(false);
  const stretch = $derived(duration ? stretchAt(now, duration) : undefined);
  const dragKey = os === "mac" ? "Option" : os === "linux" ? "Alt (or Ctrl)" : "Alt";
  async function exportHere() {
    if (stretch) await exportStretch(stretch.start, stretch.end);
  }
  let hideNote = $state(false);

  /** A time to the hundredth of a second, for marks: 1:02:03.25. */
  const precise = (t: number) => `${formatTime(t)}.${String(Math.floor((t % 1) * 100)).padStart(2, "0")}`;

  // The line beside the controls: facts only (the buttons and their tips say
  // how). ≈ marks times a site's player can only show roughly.
  const context = $derived.by(() => {
    const approx = playsHere(vod) ? "" : "≈ ";
    const picked = cuts.selection;
    if (picked && cuts.selectedEdge)
      return `Selection ${cuts.selectedEdge} at ${approx}${precise(picked[cuts.selectedEdge])} · , . move it a frame`;
    if (picked)
      return `${approx}${formatTime(picked.start)}–${formatTime(picked.end)} selected · ${formatTime(picked.end - picked.start)}`;
    const selected = cuts.list.find((m) => m.id === cuts.selected);
    if (selected) return `Cut at ${approx}${precise(selected.start)} · , . move it a frame · Delete removes it`;
    if (stretch && between)
      return `${approx}${formatTime(stretch.start)}–${formatTime(stretch.end)} · ${formatTime(stretch.end - stretch.start)}`;
    return "";
  });
  // Whether the playhead's stretch lies between two cuts (not the VOD's start or end).
  const between = $derived(
    !!stretch &&
      cuts.list.some((m) => Math.abs(m.start - stretch.start) < 1e-6) &&
      cuts.list.some((m) => Math.abs(m.start - stretch.end) < 1e-6),
  );

  /** Split: cuts both ends of the selection, else cuts here (or takes away the cut here). */
  function clip() {
    if (cuts.selection) cuts.cutSelection();
    else cuts.toggle(now, fps);
  }

  /** Export: the selection, else the stretch the playhead is in, as a clip in the clip bar. */
  async function exportClip() {
    const range = cuts.selection ?? stretch;
    if (!range || (!cuts.selection && !cuts.list.length)) return;
    if (cuts.selection) await cuts.cutSelection();
    await exportStretch(range.start, range.end);
  }
  const canExport = $derived(!!cuts.selection || cuts.list.length > 0);

  const SPEEDS = [1, 1.25, 1.5, 2, 0.5, 0.75];
  function nextSpeed() {
    player?.rate?.set(SPEEDS[(SPEEDS.indexOf(rate) + 1) % SPEEDS.length]);
  }

  function toggleMute() {
    const sound = player?.volume;
    if (!sound) return;
    // Unmuting at no volume would stay silent: back to full.
    if (sound.muted() && sound.get() === 0) sound.set(1);
    else sound.setMuted(!sound.muted());
  }

  // Full screen is the window's, with the player panel laid over the page:
  // WebKitGTK ends the app on the page's own full screen (requestFullscreen).
  let fullscreen = $state(false);
  function setFullscreen(on: boolean) {
    fullscreen = on;
    if (on) collapsed = false;
    getCurrentWindow()
      .setFullscreen(on)
      .catch((e) => console.error("full screen", e));
  }

  onDestroy(() => {
    destroyed = true;
    cancelAnimationFrame(poll);
    clearTimeout(copiedTimer);
    if (fullscreen) setFullscreen(false);
    player?.destroy();
  });

  // Keyboard control for whichever player is open. Keys pressed while the
  // player's own iframe has focus go to that player instead.
  const STEPS: Record<string, number> = { ArrowLeft: -5, ArrowRight: 5, j: -10, l: 10 };
  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (e.altKey || !canEmbed(vod) || !keysForApp()) return;
    if (target?.closest("input, textarea, select, [contenteditable], [role=dialog]")) return;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    // B cuts and E exports, as in video editors (also with Ctrl, or ⌘ on a Mac).
    if (key === "b" || key === "e") {
      if (key === "b") clip();
      else exportClip();
      e.preventDefault();
      return;
    }
    if (e.ctrlKey || e.metaKey) return;
    if (key === "m" || key === "c") {
      if (key === "m") toggleMute();
      else player?.captions?.toggle();
      e.preventDefault();
      return;
    }
    if (key === "Escape" && cuts.selection) {
      cuts.clearSelection();
      e.preventDefault();
      return;
    }
    const moving = cuts.selected !== null || (cuts.selection && cuts.selectedEdge);
    if (moving && (key === "," || key === "." || key === "<" || key === ">")) {
      // A selected mark moves a frame (ten with Shift), and the picture shows its frame.
      const frames = (key === "," || key === "<" ? -1 : 1) * (e.shiftKey ? 10 : 1);
      const at = cuts.nudgeSelected(frames, fps, duration);
      // A quarter-frame in: the frame the cut starts, whichever way the player rounds.
      if (at !== null) seekTo(Math.min(duration, at + 0.25 / fps));
      e.preventDefault();
      return;
    }
    if (cuts.selected !== null && (key === "Delete" || key === "Backspace" || key === "Escape")) {
      if (key === "Escape") cuts.selected = null;
      else cuts.remove(cuts.selected);
      e.preventDefault();
      return;
    }
    if (key === "f" || (key === "Escape" && fullscreen)) {
      setFullscreen(key === "f" ? !fullscreen : false);
      e.preventDefault();
      return;
    }
    if (!active && (key === " " || key === "k")) {
      activate(at.time);
      e.preventDefault();
      return;
    }
    if (!player) {
      return;
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

<section class:fullscreen>
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
      <kbd>B</kbd> cut here (<kbd>,</kbd><kbd>.</kbd> move the selected cut, <kbd>Delete</kbd> removes it)
      <span class="sep">·</span>
      <kbd>E</kbd> export the stretch as a clip <span class="sep">·</span>
      <kbd>M</kbd> mute <span class="sep">·</span>
      <kbd>C</kbd> captions <span class="sep">·</span>
      <kbd>{dragKey}</kbd>+drag on the timeline: the stretch out as a clip
      <span class="sep">·</span> <kbd>F</kbd> full screen
      {#if framed}
        <span class="sep">·</span> <kbd>,</kbd><kbd>.</kbd> a frame back or on (paused)
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
      <Popover
        bind:open={() => !!clips.asking, (open) => !open && clips.answer(false)}
        anchor={exportButton}
        label="Before downloading a clip"
        width={340}
      >
        <div class="note">
          <p>{PERMISSION_NOTE}</p>
          <p class="muted">{OWN_VODS_NOTE}</p>
          <label><input type="checkbox" bind:checked={hideNote} /> Don't show this again</label>
          <div class="buttons">
            <button onclick={() => clips.answer(false)}>Cancel</button>
            <button class="primary" onclick={() => clips.answer(true, hideNote)}>Download clip</button>
          </div>
        </div>
      </Popover>
      <div class="controls">
        {#if duration}
          <Scrubber
            {vod}
            {duration}
            time={now}
            {fps}
            highlight={exportHover && canExport ? (cuts.selection ?? stretch) : null}
            current={between ? stretch : null}
            ownPlayer={playsHere(vod)}
            onseek={seekTo}
          />
        {/if}
        <div class="bar">
          <button
            class="control"
            onclick={togglePlay}
            aria-label={playing ? "Pause" : "Play"}
            title={playing ? "Pause" : "Play"}
          >
            <Icon name={playing ? "pause" : "play"} size={16} />
          </button>
          <!-- The time takes the room of the longest it can be (the length, under it
               unseen), so nothing beside it moves. -->
          <span class="time"
            ><span class="now"
              ><span class="widest" aria-hidden="true">{formatTime(duration)}</span><span>{formatTime(now)}</span></span
            >
            / {formatTime(duration)}</span
          >
          <button
            class="action clip"
            class:picking={!!cuts.selection}
            onclick={clip}
            title={cuts.selection
              ? "Cut both ends of the selection (B)"
              : "Cut here, between this frame and the one before; the stretches between cuts can be exported as clips (B)"}
          >
            <Icon name="scissors" size={14} /> Split <kbd>B</kbd>
          </button>
          <button
            class="action export"
            bind:this={exportButton}
            onclick={exportClip}
            onmouseenter={() => (exportHover = true)}
            onmouseleave={() => (exportHover = false)}
            disabled={!canExport}
            title="Export the lit-up stretch as a clip, to the clip bar (E). Or {dragKey}-drag a stretch of the timeline straight into your video editor."
          >
            <Icon name="download" size={14} /> Export <kbd>E</kbd>
          </button>
          <span class="context muted" title={context}>{context}</span>
          {#if player?.volume && !player.ownControls}
            <button
              class="control"
              onclick={toggleMute}
              aria-label={muted ? "Unmute" : "Mute"}
              title={muted ? "Unmute (M)" : "Mute (M)"}
            >
              <Icon name={muted ? "volume-off" : "volume"} size={16} />
            </button>
            <input
              class="volume"
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={muted ? 0 : volume}
              oninput={(e) => player?.volume?.set(Number(e.currentTarget.value))}
              aria-label="Volume"
            />
          {/if}
          {#if player?.rate && !player.ownControls}
            <button class="control speed" onclick={nextSpeed} title="Playback speed">{rate}×</button>
          {/if}
          {#if captionsAvailable}
            <button
              class="control"
              class:on={captionsOn}
              onclick={() => player?.captions?.toggle()}
              aria-pressed={captionsOn}
              aria-label="Captions"
              title={captionsOn ? "Hide captions (C)" : "Show captions (C)"}
            >
              <Icon name="captions" size={16} />
            </button>
          {/if}
          {#if !audioOnly}
            <button
              class="control"
              onclick={() => setFullscreen(!fullscreen)}
              aria-label={fullscreen ? "Exit full screen" : "Full screen"}
              title={fullscreen ? "Exit full screen (Esc)" : "Full screen (F)"}
            >
              <Icon name={fullscreen ? "minimize" : "maximize"} size={16} />
            </button>
          {/if}
        </div>
      </div>
      {#if clips.shown.length || clips.carrying || clips.error}
        <ClipStrip {vod} ownPlayer={playsHere(vod)} onplay={playFrom} />
      {/if}
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

  /* Full screen: the panel over the whole window, the picture as large as fits. */
  section.fullscreen {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    border: none;
  }

  .fullscreen .head,
  .fullscreen .legend {
    display: none;
  }

  .fullscreen .stage {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .fullscreen .frame {
    flex: 1;
    width: 100%;
    max-height: none;
    aspect-ratio: auto;
  }

  /* The timeline over the controls: the same for every player. */
  .controls {
    padding: 8px 16px 2px;
  }

  /* Not "row": that's the transcript's lines, styled app-wide. */
  .bar {
    display: flex;
    align-items: center;
    gap: 4px;
    height: var(--controls-height);
    margin: 0 -6px;
  }

  .control {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 30px;
    height: 30px;
    padding: 0 6px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
  }

  .control:hover {
    background: var(--surface-2);
  }

  .control.on {
    color: var(--accent);
  }

  /* Split and Export: labelled, with their keys, but otherwise like the other controls. */
  .action {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 30px;
    padding: 0 6px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
    font-size: 12px;
  }

  .action:hover:not(:disabled) {
    background: var(--surface-2);
  }

  .action:disabled {
    opacity: 0.45;
  }

  .action kbd {
    min-width: 0;
    padding: 0 4px;
    font-size: 10px;
    line-height: 15px;
    color: var(--muted);
  }

  .clip :global(svg) {
    color: var(--cut);
  }

  /* A selection waits to be cut: Split takes its color. */
  .clip.picking {
    color: var(--pick);
    background: color-mix(in srgb, var(--pick) 14%, transparent);
  }

  .clip.picking :global(svg) {
    color: var(--pick);
  }

  .export :global(svg) {
    color: var(--accent);
  }

  .note {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 13px;
  }

  .note p {
    margin: 0;
  }

  .note label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .note .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .speed {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    min-width: 42px;
  }

  .bar .time {
    flex: none;
    padding: 0 6px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    white-space: nowrap;
  }

  .now {
    display: inline-grid;
    justify-items: end;
  }

  .now > span {
    grid-area: 1 / 1;
  }

  .widest {
    visibility: hidden;
  }

  .context {
    flex: 1;
    min-width: 0;
    padding: 0 8px;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .volume {
    flex: none;
    width: 64px;
    accent-color: var(--accent);
  }

  @media (max-width: 560px) {
    .volume,
    .speed {
      display: none;
    }
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
