<script lang="ts">
  // The app's own player for files played here (player.rs, crates/player):
  // the picture is drawn on the screen under this box (Screen.svelte), not
  // by the page, which keeps the controls. Its controls, timeline and full
  // screen are the player panel's (Player.svelte), as for every player;
  // mounted by embed.ts, which hands these functions over. Captions from
  // the transcript are drawn over the picture by the screen
  // (screenOverlay.ts): nothing of the page can draw over it.

  import { onDestroy, onMount, untrack } from "svelte";
  import { api, type Cue, type Vod } from "./api";
  import { holdToCarry } from "./dragout";
  import * as overlay from "./screenOverlay";
  import { cueAt } from "./media";
  import { NativeDriver } from "./nativeDriver";
  import Screen from "./Screen.svelte";
  import { prefs } from "./playerPrefs.svelte";

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

  // What the player says, and what's asked of it (nativeDriver.ts).
  const driver = new NativeDriver(untrack(() => start));
  const now = () => driver.time();

  let time = $state(untrack(() => start));
  let cues = $state.raw<Cue[]>([]);
  let destroyed = false;

  const cue = $derived(prefs.captions ? cueAt(cues, time) : null);

  // The caption, over the picture: drawn again when it, the box's size or
  // full screen changes.
  let boxSize = $state({ width: 0, height: 0, fullscreen: false });
  $effect(() => {
    const { width, height, fullscreen } = boxSize;
    const text = width && height ? cue?.text : null;
    overlay.show(text ? overlay.caption(text, width, height, fullscreen) : null);
  });
  $effect(() => {
    const el = picture;
    if (!el) return;
    const sizes = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      boxSize = { width: r.width, height: r.height, fullscreen: !!el.closest(".fullscreen") };
    });
    sizes.observe(el);
    return () => sizes.disconnect();
  });

  onMount(() => {
    const saved = prefs.volume();
    if (saved !== null) driver.volume.set(saved);
    api.playerOpen(vod.id, start, autoplay).then(
      (opened) => {
        if (destroyed) return;
        driver.opened(opened);
        onerror?.(null);
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
    time = now();
    frame = requestAnimationFrame(follow);
  }

  onDestroy(() => {
    destroyed = true;
    cancelAnimationFrame(frame);
    press.stop();
    api.setScreenOverlay(null).catch(() => {});
    driver.destroy();
  });

  export const seek = (seconds: number, play: boolean) => driver.seek(seconds, play);
  export const currentTime = () => now();
  export const isPlaying = () => driver.playing();
  /** Where the frame on screen is, as the player last said (null: none shows). */
  export const shownTime = () => driver.shown();
  /** When the last seek's reply came (nativeDriver.ts). */
  export const seekSettled = () => driver.settled();
  export const mediaDuration = () => driver.length;
  export const frameRate = () => driver.fps;
  export const togglePlay = () => driver.togglePlay();
  /** Steps `frames` frames on (back, if negative), paused. */
  export const step = (frames: number) => driver.step(frames);

  export const sound = {
    get: () => driver.volume.get(),
    muted: () => driver.volume.muted(),
    set(v: number) {
      driver.volume.set(v);
      prefs.setVolume(v);
    },
    setMuted: (m: boolean) => driver.volume.setMuted(m),
  };

  export const speed = driver.speed;

  export const captions = {
    available: () => cues.length > 0,
    on: () => prefs.captions,
    toggle: () => prefs.toggleCaptions(),
  };

  // A press held still on the picture carries the stretch the playhead is
  // in out (dragout.ts); a click plays or pauses.
  let picture = $state<HTMLDivElement>();
  const press = holdToCarry(
    () => vod,
    () => now(),
    // The VOD's length, as the timeline's: the file's may differ by a little.
    () => vod.duration || mediaDuration(),
    () => picture,
    () => togglePlay(),
  );
</script>

<div class="player own-player">
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
    <Screen />
  </div>
</div>

<style>
  .player {
    background: #000;
  }
</style>
