<script lang="ts">
  // The player for files played here (a file on this computer, or a video kept
  // from a site without an embed): the webview's <video> with controls of the
  // app's own, a waveform to seek on, captions from the transcript, and frame
  // stepping. Mounted by embed.ts, which hands its functions to Player.svelte.

  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onDestroy, onMount } from "svelte";
  import { api, type Cue } from "./api";
  import { clips } from "./clips.svelte";
  import { HOLD_MS, flyAndDrag } from "./dragout";
  import Icon from "./Icon.svelte";
  import { columns, cueAt, stepTime } from "./media";
  import { formatTime } from "./platform";
  import { theme } from "./theme.svelte";

  let {
    vodId,
    src,
    start,
    autoplay,
    audio,
    onerror,
  }: {
    vodId: number;
    src: string;
    start: number;
    autoplay: boolean;
    /** Known to be sound alone: a bar of controls, no picture. */
    audio: boolean;
    onerror?: (message: string | null) => void;
  } = $props();

  const SPEEDS = [1, 1.25, 1.5, 2, 0.5, 0.75];
  const VOLUME = "vodscribe.volume";
  const CAPTIONS = "vodscribe.captions";

  function read(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }
  function write(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      // Not remembered this time; nothing else depends on it.
    }
  }

  let video = $state<HTMLVideoElement>();
  let root = $state<HTMLDivElement>();
  let paused = $state(true);
  let time = $state(0);
  let duration = $state(0);
  const savedVolume = Number(read(VOLUME));
  let volume = $state(read(VOLUME) !== null && savedVolume >= 0 && savedVolume <= 1 ? savedVolume : 1);
  let muted = $state(false);
  let speed = $state(1);
  // A file whose type didn't say it's sound alone finds out once loaded.
  let noPicture = $state(false);
  const pictureless = $derived(audio || noPicture);
  let fullscreen = $state(false);
  let captionsOn = $state(read(CAPTIONS) !== "false");
  let cues = $state.raw<Cue[]>([]);
  let levels = $state.raw<Uint8Array>(new Uint8Array());
  // Until the file's own is known.
  let fps = 30;
  let destroyed = false;

  const cue = $derived(captionsOn ? cueAt(cues, time) : null);

  onMount(() => {
    const v = video!;
    v.volume = volume;
    v.src = src;
    v.currentTime = start;
    if (autoplay) v.play().catch(() => {});
    api.captions(vodId).then(
      (c) => (cues = c),
      (e) => console.error("captions", e),
    );
    api.peaks(vodId).then(
      (l) => (levels = l),
      (e) => console.error("waveform", e),
    );
    api.frameRate(vodId).then(
      (f) => f && (fps = f),
      (e) => console.error("frame rate", e),
    );
  });

  // The position, smoothly while playing (timeupdate only comes a few times a second).
  let frame = 0;
  function follow() {
    cancelAnimationFrame(frame);
    if (!video || destroyed) return;
    time = video.currentTime;
    if (!video.paused) frame = requestAnimationFrame(follow);
  }

  onDestroy(() => {
    destroyed = true;
    cancelAnimationFrame(frame);
    if (fullscreen) setFullscreen(false);
    clearTimeout(hold);
    // Stop the download too: an <video> removed while loading keeps fetching.
    video?.removeAttribute("src");
    video?.load();
  });

  export function seek(seconds: number, play: boolean) {
    if (!video) return;
    video.currentTime = seconds;
    time = seconds;
    if (play) video.play().catch(() => {});
  }

  export const currentTime = () => video?.currentTime ?? 0;
  export const isPlaying = () => !!video && !video.paused;

  export function togglePlay() {
    if (!video) return;
    if (video.paused) video.play().catch(() => {});
    else video.pause();
  }

  /** Steps `frames` frames on (back, if negative), paused. */
  export function step(frames: number) {
    if (!video || pictureless) return;
    video.pause();
    seek(stepTime(video.currentTime, frames, fps), false);
  }

  // A press held still on the picture picks up the made clip it's in, flying
  // its chip from the clip bar (or the strip) to the pointer, to drag it out.
  let hold: ReturnType<typeof setTimeout> | undefined;
  let held = false;
  let down = false;
  let pointer = { x: 0, y: 0 };
  function onPictureDown(e: PointerEvent) {
    if (e.button !== 0) return;
    const now = video?.currentTime ?? 0;
    const clip = clips.list.find((c) => c.status === "ready" && c.start <= now && now < c.end);
    down = true;
    held = false;
    pointer = { x: e.clientX, y: e.clientY };
    if (!clip) return;
    hold = setTimeout(() => {
      held = true;
      const shown = [`.clipbar [data-band="${clip.id}"]`, `.chip[data-clip="${clip.id}"]`]
        .map((s) => document.querySelector(s)?.getBoundingClientRect())
        .find((r) => r && r.width > 0 && r.right > 0 && r.left < innerWidth);
      flyAndDrag(clip, shown ?? root!.getBoundingClientRect(), pointer.x, pointer.y, () => down);
    }, HOLD_MS);
  }
  function onPictureMove(e: PointerEvent) {
    if (Math.hypot(e.clientX - pointer.x, e.clientY - pointer.y) >= 4) clearTimeout(hold);
    pointer = { x: e.clientX, y: e.clientY };
  }
  function onPictureUp() {
    down = false;
    clearTimeout(hold);
  }
  function onPictureClick() {
    // A press that picked a clip up isn't a click.
    if (held) held = false;
    else togglePlay();
  }

  function onError() {
    const unsupported = video?.error?.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED;
    onerror?.(unsupported ? "This video's format can't be played here." : "This video couldn't be played.");
  }

  function setVolume(value: number) {
    volume = value;
    muted = value === 0;
    if (video) {
      video.volume = value;
      video.muted = muted;
    }
    write(VOLUME, String(value));
  }

  function toggleMute() {
    if (!video) return;
    // Unmuting at no volume would stay silent: back to full.
    if (muted && volume === 0) setVolume(1);
    else video.muted = muted = !muted;
  }

  function nextSpeed() {
    speed = SPEEDS[(SPEEDS.indexOf(speed) + 1) % SPEEDS.length];
    if (video) video.playbackRate = speed;
  }

  function toggleCaptions() {
    captionsOn = !captionsOn;
    write(CAPTIONS, String(captionsOn));
  }

  // Full screen is the window's, with the player laid over the page: WebKitGTK
  // closes the app on the page's own full screen (requestFullscreen).
  function setFullscreen(on: boolean) {
    fullscreen = on;
    getCurrentWindow()
      .setFullscreen(on)
      .catch((e) => console.error("full screen", e));
  }
  const toggleFullscreen = () => setFullscreen(!fullscreen);

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && fullscreen) {
      setFullscreen(false);
      e.preventDefault();
    } else if (e.key.toLowerCase() === "f" && !e.ctrlKey && !e.metaKey && !e.altKey && !pictureless) {
      const target = e.target as HTMLElement | null;
      if (target?.closest("input, textarea, select, [contenteditable], [role=dialog]")) return;
      toggleFullscreen();
      e.preventDefault();
    }
  }

  // The waveform, drawn on a canvas sized to its box in device pixels.
  let canvas = $state<HTMLCanvasElement>();
  let width = $state(0);
  let height = $state(0);
  const ratio = typeof devicePixelRatio === "number" ? devicePixelRatio : 1;
  // A one-pixel bar every two pixels.
  const bars = $derived(columns(levels, duration, Math.floor(width / 2)));

  $effect(() => {
    if (!canvas || !root) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const w = Math.round(width * ratio);
    const h = Math.round(height * ratio);
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
    // Colors from the theme, read again when it changes.
    void theme.dark;
    const style = getComputedStyle(root);
    const played = style.getPropertyValue("--played").trim();
    const rest = style.getPropertyValue("--unplayed").trim();
    const at = duration > 0 ? (time / duration) * w : 0;
    ctx.clearRect(0, 0, w, h);
    if (!levels.length) {
      // No waveform (not read yet, or never): a plain track.
      const y = Math.round(h / 2 - ratio * 2);
      ctx.fillStyle = rest;
      ctx.fillRect(0, y, w, ratio * 4);
      ctx.fillStyle = played;
      ctx.fillRect(0, y, at, ratio * 4);
    } else {
      // Bars grow from the middle; even silence shows a line.
      for (let i = 0; i < bars.length; i++) {
        const x = i * 2 * ratio;
        const bar = Math.max(ratio, bars[i] * h);
        ctx.fillStyle = x < at ? played : rest;
        ctx.fillRect(x, (h - bar) / 2, ratio, bar);
      }
    }
    ctx.fillStyle = played;
    ctx.fillRect(Math.min(at, w - ratio * 2), 0, ratio * 2, h);
  });

  // Seeking on the waveform: click, or drag.
  let hover = $state<number | null>(null);
  let dragging = false;
  function timeAt(e: PointerEvent): number {
    const box = canvas!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)) * duration;
  }
  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0 || !duration) return;
    dragging = true;
    canvas!.setPointerCapture(e.pointerId);
    seek(timeAt(e), false);
  }
  function onPointerMove(e: PointerEvent) {
    const box = canvas!.getBoundingClientRect();
    hover = e.clientX - box.left;
    if (dragging) seek(timeAt(e), false);
  }
  function onPointerUp() {
    dragging = false;
  }
  const hoverTime = $derived(hover === null || !width ? null : Math.min(1, Math.max(0, hover / width)) * duration);
</script>

<svelte:window onkeydown={onKey} />

<div class="player" class:pictureless class:fullscreen bind:this={root}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="picture"
    onclick={onPictureClick}
    ondblclick={toggleFullscreen}
    onpointerdown={onPictureDown}
    onpointermove={onPictureMove}
    onpointerup={onPictureUp}
    onpointercancel={onPictureUp}
  >
    <video
      bind:this={video}
      playsinline
      onplay={() => ((paused = false), follow())}
      onpause={() => ((paused = true), follow())}
      onseeked={follow}
      ontimeupdate={() => paused && follow()}
      ondurationchange={() => (duration = Number.isFinite(video!.duration) ? video!.duration : 0)}
      onloadedmetadata={() => (noPicture = video!.videoWidth === 0)}
      onplaying={() => onerror?.(null)}
      onerror={onError}
    ></video>
    {#if cue && !pictureless}
      <p class="caption"><span>{cue.text}</span></p>
    {/if}
  </div>
  {#if cue && pictureless}
    <p class="caption under">{cue.text}</p>
  {/if}
  <div class="bar">
    <button
      class="control"
      onclick={togglePlay}
      aria-label={paused ? "Play" : "Pause"}
      title={paused ? "Play" : "Pause"}
    >
      <Icon name={paused ? "play" : "pause"} size={16} />
    </button>
    <!-- The time takes the room of the longest it can be (the duration, under it
         unseen), so the waveform beside it never moves. -->
    <span class="time"
      ><span class="now"
        ><span class="widest" aria-hidden="true">{formatTime(duration)}</span><span>{formatTime(time)}</span></span
      >
      / {formatTime(duration)}</span
    >
    <div class="seek" bind:clientWidth={width} bind:clientHeight={height}>
      <canvas
        bind:this={canvas}
        onpointerdown={onPointerDown}
        onpointermove={onPointerMove}
        onpointerup={onPointerUp}
        onpointercancel={onPointerUp}
        onpointerleave={() => (hover = null)}
        aria-label="Seek"
      ></canvas>
      {#if hoverTime !== null && hover !== null}
        <span class="tip" style:left="{Math.min(Math.max(hover, 24), width - 24)}px">{formatTime(hoverTime)}</span>
      {/if}
    </div>
    <button
      class="control"
      onclick={toggleMute}
      aria-label={muted ? "Unmute" : "Mute"}
      title={muted ? "Unmute" : "Mute"}
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
      oninput={(e) => setVolume(Number(e.currentTarget.value))}
      aria-label="Volume"
    />
    <button class="control speed" onclick={nextSpeed} title="Playback speed">{speed}×</button>
    {#if cues.length}
      <button
        class="control"
        class:on={captionsOn}
        onclick={toggleCaptions}
        aria-pressed={captionsOn}
        aria-label="Captions"
        title={captionsOn ? "Hide captions" : "Show captions"}
      >
        <Icon name="captions" size={16} />
      </button>
    {/if}
    {#if !pictureless}
      <button
        class="control"
        onclick={toggleFullscreen}
        aria-label={fullscreen ? "Exit full screen" : "Full screen"}
        title={fullscreen ? "Exit full screen" : "Full screen"}
      >
        <Icon name={fullscreen ? "minimize" : "maximize"} size={16} />
      </button>
    {/if}
  </div>
</div>

<style>
  .player {
    --played: var(--accent);
    --unplayed: color-mix(in srgb, var(--muted) 45%, transparent);
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: var(--stage);
  }

  .player.pictureless {
    height: auto;
    background: var(--surface-2);
    border-radius: var(--radius);
  }

  .player.fullscreen {
    position: fixed;
    inset: 0;
    z-index: 1000;
    width: auto;
    height: auto;
  }

  /* As large as a site's player, with the controls the panel's full width beneath. */
  .picture {
    position: relative;
    display: flex;
    width: 100%;
    aspect-ratio: 16 / 9;
    /* The website's demo, a window within a page, sets a smaller share of its own height. */
    max-height: var(--player-max-height, 44vh);
  }

  .fullscreen .picture {
    flex: 1;
    min-height: 0;
    aspect-ratio: auto;
    max-height: none;
  }

  .pictureless .picture {
    display: none;
  }

  video {
    display: block;
    width: 100%;
    height: 100%;
    background: #000;
  }

  .caption {
    position: absolute;
    left: 8%;
    right: 8%;
    bottom: 10px;
    margin: 0;
    text-align: center;
    pointer-events: none;
    font-size: clamp(13px, 2.4vh, 22px);
    line-height: 1.35;
  }

  .fullscreen .caption {
    bottom: 4vh;
    font-size: 3.4vh;
  }

  .caption span {
    padding: 2px 8px;
    border-radius: 4px;
    background: rgb(0 0 0 / 0.72);
    color: #fff;
    box-decoration-break: clone;
    -webkit-box-decoration-break: clone;
  }

  .caption.under {
    position: static;
    padding: 8px 12px 0;
    font-size: 13px;
    color: var(--text);
    text-align: left;
    pointer-events: auto;
  }

  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    height: var(--controls-height);
    padding: 0 8px;
    background: var(--surface);
    color: var(--text);
  }

  .pictureless .bar {
    background: none;
  }

  .fullscreen .bar {
    background: rgb(20 20 19 / 0.9);
    color: #ecece9;
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

  .pictureless .control:hover {
    background: var(--surface-3);
  }

  .control.on {
    color: var(--accent);
  }

  .speed {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    min-width: 42px;
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

  .time {
    flex: none;
    padding: 0 6px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    white-space: nowrap;
  }

  .seek {
    position: relative;
    flex: 1;
    min-width: 60px;
    height: 28px;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
    cursor: pointer;
    touch-action: none;
  }

  .tip {
    position: absolute;
    bottom: calc(100% + 4px);
    transform: translateX(-50%);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    background: var(--text);
    color: var(--surface);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    white-space: nowrap;
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
</style>
