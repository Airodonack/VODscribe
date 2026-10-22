<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, tick, untrack } from "svelte";
  import { api, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import { edits } from "./edits.svelte";
  import ClipStrip from "./ClipStrip.svelte";
  import { cuts, markKey } from "./cuts.svelte";
  import { DRAG_SLOP, carry, exportStretch, stretchAt } from "./dragout";
  import { frameStart, isUnplayable } from "./media";
  import PermissionNote from "./PermissionNote.svelte";
  import Popover from "./Popover.svelte";
  import PlayerBar, { toggleMute } from "./PlayerBar.svelte";
  import PlayerPanel from "./PlayerPanel.svelte";
  import { PlayerReading } from "./playerReading.svelte";
  import Scrubber from "./Scrubber.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";
  import { keysForApp } from "./demo";
  import { inOut, playerKey, transportKey, vodShortcuts } from "./playerKeys";
  import { canEmbed, createPlayer, playsHere, type EmbeddedPlayer } from "./embed";
  import {
    formatFraction,
    formatTime,
    keepsVideo,
    os,
    playbackHint,
    playsFromFile,
    siteName,
    watchUrl,
  } from "./platform";
  import { nav } from "./nav.svelte";

  // One instance per VOD (the parent keys it by id). `at` is a jump (a search
  // result, a transcript line, a source link), a fresh object each time, so
  // jumping to the same timestamp twice still seeks; null opens it where its
  // playhead was left this session (nav.svelte.ts), or at its start.
  let { vod, at }: { vod: Vod; at: { time: number; play: boolean } | null } = $props();
  const start = untrack(() => {
    const doc = { kind: "vod" as const, id: vod.id };
    return at?.time ?? nav.bringBack(doc)?.time ?? nav.lastSeen(doc)?.time ?? 0;
  });
  // Where it opened, or was last jumped to: the moment the header links to.
  let from = $state(start);

  // Collapsing hides the video but keeps the player loaded; a jump reopens it.
  let collapsed = $state(false);

  // Clicking the timestamp copies a link to that moment on the VOD's site.
  let copied = $state<"ok" | "failed" | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  async function copyLink() {
    copied = await writeText(watchUrl(vod, from)).then(
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
  if (handled?.play && untrack(() => canEmbed(vod))) activate(handled.time);
  $effect(() => {
    if (at === handled) return;
    handled = at;
    if (!at) return;
    const { time, play } = at;
    from = time;
    collapsed = false;
    if (!active) activate(time, play);
    else if (player) player.seek(time, play);
    else pending = { time, play };
  });

  cuts.open(untrack(() => vod.id));
  clips.open(
    untrack(() => vod.id),
    untrack(() => !playsFromFile(vod)),
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
    // Still loading: looked at again once it's in (this reads `player`).
    if (active && !player) return;
    if (now === here) return;
    here = now;
    // Its clips are cut from the file now (or downloaded again, once it's deleted).
    clips.open(vod.id, !playsFromFile(vod));
    if (!active || !player) return;
    const time = player.time();
    const play = player.playing();
    player.destroy();
    player = null;
    active = false;
    error = null;
    activate(time, play);
  });

  // What the controls show, read from the player every frame (playerReading.svelte.ts).
  const reading = new PlayerReading(start);
  let captionsAvailable = $state(false);
  let captionsOn = $state(false);
  let playerDuration = $state(0);
  let poll = 0;
  function read() {
    poll = requestAnimationFrame(read);
    const p = player;
    if (!p) return;
    reading.read(p);
    nav.note({ kind: "vod", id: vod.id }, { time: reading.now });
    playerDuration = p.duration?.() ?? 0;
    cuts.fps = reading.fps;
    captionsAvailable = p.captions?.available() ?? false;
    captionsOn = p.captions?.on() ?? false;
  }
  const now = $derived(reading.now);
  const playing = $derived(reading.playing);
  const fps = $derived(reading.fps);
  poll = requestAnimationFrame(read);
  // The VOD's length as the site said, or as the player found when the site didn't say.
  const duration = $derived(vod.duration || playerDuration);

  /** Moves the playhead, starting the player there (paused) if it isn't yet. */
  function seekTo(time: number) {
    reading.at(time);
    if (player) player.seek(time, false);
    else if (active) pending = { time, play: false };
    else activate(time, false);
  }

  function togglePlay() {
    if (player) player.togglePlay();
    else if (!active) activate(now);
  }

  // What a pill hovered in the clip bar (ClipStrip.svelte) is of this VOD: a
  // clip's stretch, or what an edit plays of it; or, with neither, what the
  // edit whose source link led here plays of it.
  const editRanges = $derived.by(() => {
    if (clips.hovered) return [clips.hovered];
    const lit = edits.lit?.vodId === vod.id ? edits.lit.editId : null;
    const id = edits.hovered ?? lit;
    return id === null ? [] : (edits.list.find((e) => e.id === id)?.ranges.filter((r) => r.vod_id === vod.id) ?? []);
  });

  // Clip (C): the stretch the playhead is in becomes a clip, a pill in the clip
  // bar. Hovering the button lights the stretch up on the timeline.
  let clipButton = $state<HTMLElement>();
  let clipHover = $state(false);
  const stretch = $derived(duration ? stretchAt(now, duration) : undefined);

  /** A time to the hundredth of a second, for marks: 1:02:03.25. */
  const precise = (t: number) => formatFraction(t, 2);

  // The line beside the controls: a fact or two (the shortcuts list says how,
  // and the line is kept for what's to come). ≈ marks times a site's player
  // can only show roughly.
  const context = $derived.by(() => {
    const approx = playsHere(vod) ? "" : "≈ ";
    const span = (a: number, b: number) => `${approx}${formatTime(a)}–${formatTime(b)} (${formatTime(b - a)})`;
    const picked = cuts.selection;
    if (picked && cuts.selectedEdge) return `${approx}${precise(picked[cuts.selectedEdge])}`;
    if (picked) return span(picked.start, picked.end);
    const selected = cuts.all.find((m) => m.id === cuts.selected);
    if (selected) return `${approx}${precise(selected.start)}${selected.text ? ` · ${selected.text}` : ""}`;
    if (stretch && between) return span(stretch.start, stretch.end);
    return "";
  });
  // Whether the playhead's stretch lies between two cuts (not the VOD's start or end).
  const between = $derived(
    !!stretch &&
      cuts.list.some((m) => Math.abs(m.start - stretch.start) < 1e-6) &&
      cuts.list.some((m) => Math.abs(m.start - stretch.end) < 1e-6),
  );

  /** Split: cuts both ends of the selection, else cuts here (or takes away the cut here). */
  function split() {
    if (cuts.selection) cuts.cutSelection();
    // Selected only when paused: cuts placed while watching leave the keys to the playhead.
    else cuts.toggle(now, fps, !playing);
  }

  /** Clip: the selection, else the stretch the playhead is in (with no cuts, the whole video), as a clip in the clip bar. */
  async function makeClip() {
    const range = cuts.selection ? await cuts.cutSelection() : stretch;
    if (range) await exportStretch(range.start, range.end);
  }
  const canClip = $derived(!!cuts.selection || !!stretch);

  // Clip dragged rather than clicked: the lit-up stretch carried, as a clip's
  // pill is (dragout.ts carry), with no key held: onto an edit or the clip
  // bar, or past the window's edge into another app. Pressed and let go in
  // place, it's a click.
  let clipPress: { x: number; y: number } | null = null;
  let clipDragged = false;
  function onClipDown(e: PointerEvent) {
    if (e.button === 0 && canClip) clipPress = { x: e.clientX, y: e.clientY };
  }
  function onClipMove(e: PointerEvent) {
    if (!clipPress || Math.hypot(e.clientX - clipPress.x, e.clientY - clipPress.y) < DRAG_SLOP) return;
    // Its release missed (it happened elsewhere): a press no more.
    if (!(e.buttons & 1)) return void (clipPress = null);
    clipPress = null;
    const picked = cuts.selection;
    const range = picked ? { start: frameStart(picked.start, fps), end: frameStart(picked.end, fps) } : stretch;
    if (!range || range.end <= range.start) return;
    // The let-go may land on the button again: not a click too (the click
    // comes just after the carry ends).
    clipDragged = true;
    clipHover = false;
    const from = clipButton?.getBoundingClientRect() ?? new DOMRect(e.clientX, e.clientY);
    carry({
      vod,
      start: range.start,
      end: range.end,
      ownPlayer: playsFromFile(vod),
      from,
      x: e.clientX,
      y: e.clientY,
    }).then(() => setTimeout(() => (clipDragged = false)));
  }
  function onClipClick() {
    if (clipDragged) clipDragged = false;
    else makeClip();
  }

  // Full screen (the panel's, PlayerPanel.svelte): the player shown.
  let fullscreen = $state(false);
  function setFullscreen(on: boolean) {
    fullscreen = on;
    if (on) collapsed = false;
  }

  onDestroy(() => {
    destroyed = true;
    cancelAnimationFrame(poll);
    clearTimeout(copiedTimer);
    player?.destroy();
    // The note asks about this VOD's clip.
    clips.answer(false);
  });

  // The note is shown with the controls.
  $effect(() => {
    if (collapsed) untrack(() => clips.answer(false));
  });

  // Keyboard control for whichever player is open. Keys pressed while the
  // player's own iframe has focus go to that player instead.
  function onKey(e: KeyboardEvent) {
    const key = playerKey(e);
    if (key === null || !canEmbed(vod) || !keysForApp()) return;
    // B cuts, as in video editors (also with Ctrl, or ⌘ on a Mac), and C
    // makes a clip (bare: Ctrl+C is copy).
    if (key === "b" || (key === "c" && !e.shiftKey && !e.ctrlKey && !e.metaKey)) {
      // Held down, it's one press.
      if (e.repeat) return e.preventDefault();
      collapsed = false;
      if (key === "b") split();
      else makeClip();
      e.preventDefault();
      return;
    }
    if (e.ctrlKey || e.metaKey) return;
    // I and O put the selection's in and out points at the playhead, as in
    // video editors; with none, I or O begins one.
    if ((key === "i" || key === "o") && !e.shiftKey) {
      const stops = cuts.list.map((m) => m.start);
      const picked = duration ? inOut(key, frameStart(now, fps), cuts.selection, stops, duration) : null;
      // The arrows go on moving the playhead, not an edge or a mark.
      if (picked) {
        cuts.selection = picked;
        cuts.selectedEdge = null;
        cuts.selected = null;
      }
      e.preventDefault();
      return;
    }
    // Shift with a letter is the player's own: captions (C) and mute (M).
    if (e.shiftKey && (key === "c" || key === "m")) {
      if (key === "c") player?.captions?.toggle();
      else toggleMute(player);
      e.preventDefault();
      return;
    }
    // M marks a highlight here (a marker, as in video editors), N starts a
    // chapter, each with its title to write.
    if (key === "m" || key === "n") {
      if (!e.repeat) cuts.add(key === "m" ? "highlight" : "chapter", now, fps);
      collapsed = false;
      e.preventDefault();
      return;
    }
    // The selected mark (or the selection's edge): renamed, moved, taken away.
    const pause = () => player?.playing() && player.togglePlay();
    if (markKey(key, e, { fps, length: duration, pause, seekTo })) {
      e.preventDefault();
      return;
    }
    if (!active && (key === " " || key === "k")) {
      activate(from);
      e.preventDefault();
      return;
    }
    const p = player;
    // No player yet (loading, or failed): only full screen is for us; Space
    // and the arrows keep their usual meaning (a focused button, scrolling).
    if (!p && key !== "f" && !(key === "Escape" && fullscreen)) return;
    const done = transportKey(key, e.shiftKey, {
      togglePlay: () => p?.togglePlay(),
      seekBy: (s) => p?.seek(Math.max(0, p.time() + s), false),
      step: p?.step && ((n) => p.step!(n)),
      fullscreen,
      setFullscreen,
    });
    // preventDefault also stops Space from clicking a focused button, such
    // as the search result that was just clicked.
    if (done) e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} onpointermove={onClipMove} onpointerup={() => (clipPress = null)} />

<PlayerPanel bind:fullscreen shortcuts={vodShortcuts(framed, os === "mac")} controls={vodControls} {stage} {below}>
  {#snippet head()}
    <div class="info">
      <strong title={vod.title}>{vod.title}</strong>
      <span class="meta muted">
        <span class="platform {vod.platform}">{site}</span>
        {#if vod.channel}<span class="sep">·</span>{vod.channel}{/if}
        {#if vod.duration}<span class="sep">·</span>{formatTime(vod.duration)}{/if}
      </span>
      {#if from > 0 && isLocal}
        <span class="stamp"><span class="time">{formatTime(from)}</span></span>
      {:else if from > 0}
        <button
          class="stamp"
          class:copied={copied === "ok"}
          class:failed={copied === "failed"}
          onclick={copyLink}
          title="Copy a link to this moment"
        >
          <span class="time">{formatTime(from)}</span>
          <Icon name={copied === "ok" ? "check" : copied === "failed" ? "error" : "copy"} size={12} />
          {#if copied}<span class="copied-text">{copied === "ok" ? "Copied" : "Couldn't copy"}</span>{/if}
        </button>
      {/if}
    </div>
  {/snippet}
  {#snippet headEnd()}
    {#if isLocal}
      <button class="link open" onclick={() => api.showFile(vod.id).catch((e) => (error = String(e)))}>
        Show in folder
      </button>
    {:else}
      <button class="link open" onclick={() => openUrl(watchUrl(vod, from))}>
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
  {/snippet}
</PlayerPanel>

{#snippet stage()}
  {#if canEmbed(vod)}
    <div class="stage" class:collapsed class:audio={audioOnly}>
      <div class="frame" class:audio={audioOnly} class:own={active && playsHere(vod)}>
        {#if active}
          <div class="host" bind:this={host}></div>
        {:else if audioOnly}
          <button class="poster bar" onclick={() => activate(from)} aria-label="Play {vod.title}">
            <Icon name="play" size={16} /> Play from {formatTime(from)}
          </button>
        {:else}
          <button class="poster" onclick={() => activate(from)} aria-label="Play {vod.title}">
            {#if vod.thumb}<Thumb name={vod.thumb} />{/if}
            <span class="play"><Icon name="play" size={26} /></span>
          </button>
        {/if}
      </div>
    </div>
    {#if !collapsed}
      <Popover
        bind:open={() => !!clips.asking, (open) => !open && clips.answer(false)}
        anchor={clipButton}
        label="Before downloading a clip"
        width={340}
      >
        <PermissionNote action="Download clip" onanswer={(download, hide) => clips.answer(download, hide)} />
      </Popover>
    {/if}
  {/if}
{/snippet}

{#snippet vodControls()}
  {#if canEmbed(vod) && !collapsed}
    {#if duration}
      <Scrubber
        {vod}
        memo={{ kind: "vod", id: vod.id }}
        {duration}
        time={now}
        shown={reading.shown}
        {fps}
        highlight={clipHover && canClip ? (cuts.selection ?? stretch) : null}
        ranges={editRanges}
        current={between ? stretch : null}
        ownPlayer={playsFromFile(vod)}
        onpause={() => player?.playing() && player.togglePlay()}
        onseek={seekTo}
      />
    {/if}
    <PlayerBar
      {player}
      {playing}
      now={reading.actual}
      {duration}
      volume={reading.volume}
      muted={reading.muted}
      rate={reading.rate}
      {fps}
      {captionsAvailable}
      {captionsOn}
      {fullscreen}
      canFullscreen
      ontoggleplay={togglePlay}
      onfullscreen={setFullscreen}
    >
      {#snippet actions()}
        <button
          class="action split"
          class:picking={!!cuts.selection}
          onclick={split}
          title={cuts.selection
            ? "Cut both ends of the selection (B)"
            : "Cut here, between this frame and the one before; each stretch between cuts can become a clip (B)"}
        >
          <Icon name="scissors" size={14} /> Split <kbd>B</kbd>
        </button>
        <button
          class="action clip"
          class:picking={!!cuts.selection}
          bind:this={clipButton}
          onpointerdown={onClipDown}
          onclick={onClipClick}
          onmouseenter={() => (clipHover = true)}
          onmouseleave={() => (clipHover = false)}
          disabled={!canClip}
          title="Clip the lit-up stretch, to the clip bar (C): with no cuts, the whole video. Or drag it: onto an edit, or straight into your video editor."
        >
          <Icon name="film" size={14} /> Clip <kbd>C</kbd>
        </button>
        {#if cuts.error}
          <span class="context error-text" title={cuts.error}>{cuts.error}</span>
        {:else}
          <span class="context muted" title={context}>{context}</span>
        {/if}
      {/snippet}
    </PlayerBar>
  {/if}
{/snippet}

{#snippet below()}
  {#if canEmbed(vod)}
    {#if !collapsed && (clips.shown.length || clips.carrying || clips.error || edits.forVod(vod.id).length)}
      <ClipStrip {vod} ownPlayer={playsFromFile(vod)} onplay={playFrom} />
    {/if}
    {#if error}
      <p class="error-text note">
        {error}
        {#if isLocal}
          <button class="link" onclick={() => api.openFile(vod.id).catch((e) => (error = String(e)))}
            >Open it in your video player</button
          >
        {:else if vod.media && isUnplayable(error)}
          {playbackHint()} Meanwhile, use “Open on {site}”.
        {/if}
      </p>
    {/if}
  {:else}
    <p class="muted note">
      {#if keepsVideo(vod) && vod.status !== "done" && vod.status !== "error"}
        The video plays here once it's downloaded. Meanwhile, use “Open on {site}” to watch from {formatTime(from)}.
      {:else if keepsVideo(vod)}
        This video isn't kept on this computer. Use “Open on {site}” to watch from {formatTime(from)}.
      {:else}
        {site} doesn't offer an embeddable VOD player. Use “Open on {site}” to watch from {formatTime(from)}.
      {/if}
    </p>
  {/if}
{/snippet}

<style>
  /* Full screen: the picture as large as fits (PlayerPanel.svelte). */
  :global(.fullscreen) .stage {
    display: flex;
  }

  :global(.fullscreen) .frame {
    flex: 1;
    width: 100%;
    max-height: none;
    aspect-ratio: auto;
  }

  .split :global(svg) {
    color: var(--cut);
  }

  /* A selection (made on the timeline or in the transcript) waits to be cut
     or clipped: Split and Clip take its color. (:not(:disabled) to win
     over the bar's own style for its buttons.) */
  .split.picking:not(:disabled),
  .clip.picking:not(:disabled) {
    color: var(--pick);
    background: color-mix(in srgb, var(--pick) 14%, transparent);
  }

  .split.picking :global(svg),
  .clip.picking :global(svg) {
    color: var(--pick);
  }

  .clip :global(svg) {
    color: var(--accent);
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

  /* Audio alone, full screen: black, its captions large in the middle (LocalPlayer.svelte). */
  :global(.fullscreen) .stage.audio {
    background: #000;
    padding: 0;
  }

  :global(.fullscreen) .frame.audio .host {
    height: 100%;
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
