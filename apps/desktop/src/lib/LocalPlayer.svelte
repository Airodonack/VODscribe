<script lang="ts">
  // The player for files played here where the app's own player can't
  // (sound only, or no screen to draw on): the webview's <video>, with
  // captions from the transcript over it.
  // Its controls, timeline and full screen are the player panel's
  // (Player.svelte), as for every player; mounted by embed.ts, which hands
  // these functions over.

  import { landingSeeks } from "./mediaSeek";
  import { onDestroy, onMount } from "svelte";
  import { api, type Cue, type Vod } from "./api";
  import { cuts } from "./cuts.svelte";
  import { holdToCarry } from "./dragout";
  import { DEFAULT_FPS, cueAt, elementSound, stepTime, unplayable } from "./media";
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
    // Through seek: before its metadata is in, a video may drop where it's put.
    seek(start, false);
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
    if (!video.paused) {
      endIfStuck(video);
      frame = requestAnimationFrame(follow);
    }
  }

  // WebKitGTK can stall a file started in its last second (seen with an mp3
  // on 2.50): "playing" for good, its time stuck, never ending. Stuck that
  // near the end for a while, it's taken as the end it was about to reach.
  let stuck = { at: -1, since: 0 };
  function endIfStuck(v: HTMLVideoElement) {
    const left = v.duration - v.currentTime;
    if (!(left < 1.5)) {
      stuck.at = -1;
      return;
    }
    if (v.currentTime !== stuck.at) stuck = { at: v.currentTime, since: performance.now() };
    else if (performance.now() - stuck.since > 1500) {
      v.pause();
      v.currentTime = v.duration;
    }
  }

  onDestroy(() => {
    destroyed = true;
    cancelAnimationFrame(frame);
    press.stop();
    // Stop the download too: a <video> removed while loading keeps fetching.
    video?.removeAttribute("src");
    video?.load();
  });

  // Seeks kept until they land (mediaSeek.ts).
  let seeks: ReturnType<typeof landingSeeks> | null = null;
  export function seek(seconds: number, play: boolean) {
    if (!video) return;
    seeks ??= landingSeeks(video);
    seeks.seek(seconds);
    time = seconds;
    if (play) video.play().catch(() => {});
  }
  function onMetadata() {
    noPicture = video!.videoWidth === 0;
  }

  export const currentTime = () => seeks?.wanted() ?? video?.currentTime ?? 0;
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

  export const sound = elementSound(
    () => video,
    (volume) => save(VOLUME, String(volume)),
  );

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

  // A press held still on the picture carries the stretch the playhead is
  // in out (dragout.ts); a click plays or pauses.
  let picture = $state<HTMLDivElement>();
  const press = holdToCarry(
    () => vod,
    () => video?.currentTime ?? 0,
    // The VOD's length, as the timeline's: the file's may differ by a little.
    () => vod.duration || mediaDuration(),
    () => picture,
    () => togglePlay(),
  );

  function onError() {
    onerror?.(unplayable(video?.error));
  }
</script>

<div class="player own-player" class:pictureless>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="picture"
    bind:this={picture}
    onclick={press.click}
    onpointerdown={press.down}
    onpointermove={press.move}
    onpointerup={press.up}
    onpointercancel={press.up}
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
    color: var(--stage-text);
    text-align: left;
    pointer-events: auto;
  }

  /* A sound-only file's stage is the panel's own surface (Player.svelte,
     .stage.audio), light in the light theme: the panel's text, not the
     dark stage's. */
  :global(.stage.audio) .caption.under {
    color: var(--text);
  }
</style>
