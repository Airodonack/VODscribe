<script lang="ts">
  // The app's own player for files played here (player.rs, crates/player):
  // the picture is drawn on the screen under this box (Screen.svelte), not
  // by the page, which keeps the controls. Its controls, timeline and full
  // screen are the player panel's (Player.svelte), as for every player;
  // mounted by embed.ts, which hands these functions over. Captions from
  // the transcript go under the picture: nothing of the page can draw over
  // the screen.

  import { onDestroy, onMount, untrack } from "svelte";
  import { api, events, type Cue, type PlayerStatus, type UnlistenFn, type Vod } from "./api";
  import { cuts } from "./cuts.svelte";
  import { DRAG_SLOP, HOLD_MS, carry, rectOf, stretchAt } from "./dragout";
  import { cueAt } from "./media";
  import Screen from "./Screen.svelte";
  import { load, save } from "./stored";

  let {
    vod,
    start,
    autoplay,
    onerror,
  }: {
    vod: Vod;
    start: number;
    autoplay: boolean;
    onerror?: (message: string | null) => void;
  } = $props();

  const VOLUME = "vodscribe.volume";
  const CAPTIONS = "vodscribe.captions";

  // The time as last told, run on by the page's clock while playing.
  let told: PlayerStatus = { time: untrack(() => start), playing: false };
  let toldAt = performance.now();
  const now = () => {
    const t = told.time + (told.playing ? (performance.now() - toldAt) / 1000 : 0);
    return length ? Math.min(t, length) : t;
  };
  function tell(status: PlayerStatus) {
    told = status;
    toldAt = performance.now();
    time = status.time;
  }

  let time = $state(untrack(() => start));
  let fps: number | null = null;
  let length = 0;
  let captionsOn = $state(load(CAPTIONS) !== "false");
  let cues = $state.raw<Cue[]>([]);
  let volume = $state(1);
  let muted = $state(false);
  let destroyed = false;
  let unlisten: UnlistenFn | null = null;
  // Until the player is open, what's asked of it is kept for then.
  let ready = false;
  let wanted: { time: number; play: boolean } | null = null;

  const cue = $derived(captionsOn ? cueAt(cues, time) : null);

  onMount(() => {
    events.playerTime((s) => !destroyed && tell(s)).then((u) => (destroyed ? u() : (unlisten = u)));
    const saved = Number(load(VOLUME));
    if (load(VOLUME) !== null && saved >= 0 && saved <= 1) volume = saved;
    api.playerOpen(vod.id, start, autoplay).then(
      (opened) => {
        if (destroyed) return;
        ready = true;
        fps = opened.fps;
        length = opened.duration ?? 0;
        api.playerSetVolume(muted ? 0 : volume).catch(() => {});
        onerror?.(null);
        if (wanted) seek(wanted.time, wanted.play);
      },
      // Closed before it opened (the page moved on): nothing to say.
      (e) => !destroyed && onerror?.(String(e)),
    );
    api.captions(vod.id).then(
      (c) => (cues = c),
      (e) => console.error("captions", e),
    );
    follow();
  });

  // The position, for the captions: smoothly while playing.
  let frame = 0;
  function follow() {
    if (destroyed) return;
    if (told.playing) time = now();
    frame = requestAnimationFrame(follow);
  }

  onDestroy(() => {
    destroyed = true;
    cancelAnimationFrame(frame);
    clearTimeout(hold);
    unlisten?.();
    api.playerClose().catch(() => {});
  });

  export function seek(seconds: number, play: boolean) {
    tell({ time: seconds, playing: play });
    if (!ready) {
      wanted = { time: seconds, play };
      return;
    }
    api.playerSeek(seconds, play).then(tell, () => {});
  }

  export const currentTime = () => now();
  export const isPlaying = () => told.playing;
  export const mediaDuration = () => length;
  export const frameRate = () => fps;

  export function togglePlay() {
    const playing = !told.playing;
    tell({ time: now(), playing });
    if (!ready) {
      wanted = { time: told.time, play: playing };
      return;
    }
    (playing ? api.playerPlay() : api.playerPause()).then(tell, () => {});
  }

  /** Steps `frames` frames on (back, if negative), paused. */
  export function step(frames: number) {
    if (ready) api.playerStep(frames).then(tell, () => {});
  }

  export const sound = {
    get: () => volume,
    muted: () => muted,
    set(v: number) {
      volume = v;
      muted = v === 0;
      save(VOLUME, String(v));
      api.playerSetVolume(v).catch(() => {});
    },
    setMuted(m: boolean) {
      muted = m;
      api.playerSetVolume(m ? 0 : volume).catch(() => {});
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
    if (e.button !== 0) return;
    const range = stretchAt(now(), vod.duration || length);
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
</script>

<div class="player">
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
    <Screen />
  </div>
  {#if cue}
    <p class="caption">{cue.text}</p>
  {/if}
</div>

<style>
  .player {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
  }

  /* As large as a site's player (LocalPlayer.svelte). */
  .picture {
    position: relative;
    display: flex;
    width: 100%;
    aspect-ratio: 16 / 9;
    max-height: var(--player-max-height, 44vh);
    background: #000;
  }

  :global(.fullscreen) .picture {
    flex: 1;
    min-height: 0;
    aspect-ratio: auto;
    max-height: none;
  }

  .caption {
    margin: 0;
    padding: 8px 16px 0;
    font-size: 13px;
    color: var(--text);
  }
</style>
