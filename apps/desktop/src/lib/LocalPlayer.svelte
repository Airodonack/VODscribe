<script lang="ts">
  // The player for files played here (a file on this computer, or a video
  // kept): the webview's <video>, with captions from the transcript over it.
  // Its controls, timeline and full screen are the player panel's
  // (Player.svelte), as for every player; mounted by embed.ts, which hands
  // these functions over.

  import { onDestroy, onMount } from "svelte";
  import { api, type Cue, type Vod } from "./api";
  import { cuts } from "./cuts.svelte";
  import { DRAG_SLOP, HOLD_MS, carry, rectOf, stretchAt } from "./dragout";
  import { DEFAULT_FPS, cueAt, stepTime } from "./media";
  import { load, save } from "./stored";

  let {
    vod,
    src,
    start,
    autoplay,
    audio,
    onerror,
  }: {
    vod: Vod;
    src: string;
    start: number;
    autoplay: boolean;
    /** Known to be sound alone: no picture. */
    audio: boolean;
    onerror?: (message: string | null) => void;
  } = $props();

  const VOLUME = "vodscribe.volume";
  const CAPTIONS = "vodscribe.captions";

  let video = $state<HTMLVideoElement>();
  let time = $state(0);
  // A file whose type didn't say it's sound alone finds out once loaded.
  let noPicture = $state(false);
  const pictureless = $derived(audio || noPicture);
  let captionsOn = $state(load(CAPTIONS) !== "false");
  let cues = $state.raw<Cue[]>([]);
  // Until the file's own is known.
  let fps: number | null = null;
  let destroyed = false;

  const cue = $derived(captionsOn ? cueAt(cues, time) : null);

  onMount(() => {
    const v = video!;
    const saved = Number(load(VOLUME));
    if (load(VOLUME) !== null && saved >= 0 && saved <= 1) v.volume = saved;
    v.src = src;
    v.currentTime = start;
    if (autoplay) v.play().catch(() => {});
    api.captions(vod.id).then(
      (c) => (cues = c),
      (e) => console.error("captions", e),
    );
    api.frameRate(vod.id).then(
      (f) => (fps = f),
      (e) => console.error("frame rate", e),
    );
  });

  // The position, for the captions: smoothly while playing.
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
    clearTimeout(hold);
    // Stop the download too: a <video> removed while loading keeps fetching.
    video?.removeAttribute("src");
    video?.load();
  });

  export function seek(seconds: number, play: boolean) {
    if (!video) return;
    video.currentTime = seconds;
    time = seconds;
    // Before its metadata is in, a video may say 0 and drop the seek
    // (WebKitGTK 2.54): kept, said, and made again once it's in.
    wanted = video.readyState < HTMLMediaElement.HAVE_METADATA ? seconds : null;
    if (play) video.play().catch(() => {});
  }
  /** A seek made before the video's metadata was in (`seek`). */
  let wanted: number | null = null;
  function onMetadata() {
    noPicture = video!.videoWidth === 0;
    if (wanted !== null && Math.abs(video!.currentTime - wanted) > 0.01) video!.currentTime = wanted;
    wanted = null;
  }

  export const currentTime = () => wanted ?? video?.currentTime ?? 0;
  export const isPlaying = () => !!video && !video.paused;
  export const mediaDuration = () => (video && Number.isFinite(video.duration) ? video.duration : 0);
  export const frameRate = () => (pictureless ? null : fps);

  export function togglePlay() {
    if (!video) return;
    if (video.paused) video.play().catch(() => {});
    else video.pause();
  }

  /** Steps `frames` frames on (back, if negative), paused. */
  export function step(frames: number) {
    if (!video || pictureless) return;
    video.pause();
    seek(stepTime(video.currentTime, frames, fps ?? DEFAULT_FPS), false);
  }

  export const sound = {
    get: () => video?.volume ?? 1,
    muted: () => video?.muted ?? false,
    set(volume: number) {
      if (!video) return;
      video.volume = volume;
      video.muted = volume === 0;
      save(VOLUME, String(volume));
    },
    setMuted(muted: boolean) {
      if (video) video.muted = muted;
    },
  };

  export const speed = {
    get: () => video?.playbackRate ?? 1,
    set: (rate: number) => {
      if (video) video.playbackRate = rate;
    },
  };

  export const captions = {
    available: () => cues.length > 0,
    on: () => captionsOn,
    toggle() {
      captionsOn = !captionsOn;
      save(CAPTIONS, String(captionsOn));
    },
  };

  // A press held still on the picture carries the stretch between cuts that
  // the playhead is in (dragout.ts), flying from the timeline to the pointer.
  let hold: ReturnType<typeof setTimeout> | undefined;
  let held = false;
  let pointer = { x: 0, y: 0 };
  let picture = $state<HTMLDivElement>();
  function onPictureDown(e: PointerEvent) {
    if (e.button !== 0 || !video) return;
    // The VOD's length, as the timeline's: the file's may differ by a little.
    const range = stretchAt(video.currentTime, vod.duration || mediaDuration());
    held = false;
    pointer = { x: e.clientX, y: e.clientY };
    if (!range || !cuts.list.length) return;
    hold = setTimeout(() => {
      held = true;
      const from = rectOf(range.start, range.end) ?? picture!.getBoundingClientRect();
      carry({ vod, start: range.start, end: range.end, ownPlayer: true, from, x: pointer.x, y: pointer.y });
    }, HOLD_MS);
  }
  function onPictureMove(e: PointerEvent) {
    if (Math.hypot(e.clientX - pointer.x, e.clientY - pointer.y) >= DRAG_SLOP) clearTimeout(hold);
    pointer = { x: e.clientX, y: e.clientY };
  }
  function onPictureUp() {
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
</script>

<div class="player" class:pictureless>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="picture"
    bind:this={picture}
    onclick={onPictureClick}
    onpointerdown={onPictureDown}
    onpointermove={onPictureMove}
    onpointerup={onPictureUp}
    onpointercancel={onPictureUp}
  >
    <video
      bind:this={video}
      playsinline
      onplay={follow}
      onpause={follow}
      onseeked={follow}
      ontimeupdate={() => video?.paused && follow()}
      onloadedmetadata={onMetadata}
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
</div>

<style>
  .player {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
  }

  /* As large as a site's player. */
  .picture {
    position: relative;
    display: flex;
    width: 100%;
    aspect-ratio: 16 / 9;
    /* The website's demo, a window within a page, sets a smaller share of its own height. */
    max-height: var(--player-max-height, 44vh);
    background: #000;
  }

  /* Full screen (the player panel's): as tall as there's room for. */
  :global(.fullscreen) .picture {
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

  :global(.fullscreen) .caption {
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
    padding: 8px 16px 0;
    font-size: 13px;
    color: var(--text);
    text-align: left;
    pointer-events: auto;
  }
</style>
