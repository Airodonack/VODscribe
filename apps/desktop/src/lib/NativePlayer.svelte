<script lang="ts">
  // The app's own player for files played here, and VODs from sites from
  // their footage, fetched as they play (player.rs, crates/player): the
  // picture is drawn on the screen under this box (Screen.svelte), not by
  // the page, which keeps the controls. Waiting for footage, it says so
  // over the picture; if a site's VOD's footage can't be had at all, it
  // goes to the site's player (ownPlayer.svelte.ts). Its controls, timeline and full
  // screen are the player panel's (Player.svelte), as for every player;
  // mounted by embed.ts, which hands these functions over. Captions from
  // the transcript are drawn over the picture by the screen
  // (screenOverlay.ts): nothing of the page can draw over it.

  import { onDestroy, onMount, untrack } from "svelte";
  import { api, events, type Cue, type UnlistenFn, type Vod } from "./api";
  import { holdToCarry } from "./dragout";
  import * as overlay from "./screenOverlay";
  import { cueAt } from "./media";
  import { NativeDriver } from "./nativeDriver";
  import { ownPlayer } from "./ownPlayer.svelte";
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
  // Waiting for footage still to come (a VOD from a site); `unavailable`
  // once the site wouldn't give it (it's gone from there, say): then only
  // what's kept plays.
  let waiting = $state(false);
  let unavailable = $state(false);
  const sitePlayer = untrack(() => !vod.media && vod.platform !== "local");

  // The caption, over the picture (or that it's waiting for the video):
  // drawn again when it, the box's size or full screen changes.
  let boxSize = $state({ width: 0, height: 0, fullscreen: false });
  $effect(() => {
    const { width, height, fullscreen } = boxSize;
    const wait = unavailable ? "This part isn't kept here, and the site didn't give it" : "Loading the video…";
    const text = width && height ? (waiting ? wait : cue?.text) : null;
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
    if (sitePlayer)
      events
        .footage((fetched) => {
          if (destroyed || fetched.vod_id !== vod.id) return;
          unavailable = !!fetched.error;
          if (!fetched.error) return;
          // None of it to play: the site's player instead. Some played: what's
          // kept still plays, and it says why the rest doesn't.
          if (!driver.showed()) ownPlayer.fellBack.add(vod.id);
          else onerror?.(fetched.error);
        })
        .then((u) => (destroyed ? u() : (unlistenFootage = u)));
  });
  let unlistenFootage: UnlistenFn | null = null;

  // The position, for the captions: smoothly while playing.
  let frame = 0;
  function follow() {
    if (destroyed) return;
    time = now();
    waiting = driver.waiting();
    frame = requestAnimationFrame(follow);
  }

  onDestroy(() => {
    destroyed = true;
    unlistenFootage?.();
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
