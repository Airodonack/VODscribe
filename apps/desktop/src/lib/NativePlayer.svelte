<script lang="ts">
  // The app's own player for files played here (player.rs, crates/player):
  // the picture is drawn on the screen under this box (Screen.svelte), not
  // by the page, which keeps the controls. Its controls, timeline and full
  // screen are the player panel's (Player.svelte), as for every player;
  // mounted by embed.ts, which hands these functions over. Captions from
  // the transcript are drawn over the picture by the screen
  // (screenOverlay.ts): nothing of the page can draw over it.

  import { onDestroy, onMount, untrack } from "svelte";
  import { api, events, type Cue, type Vod } from "./api";
  import { holdToCarry, stretchCarry } from "./dragout";
  import * as overlay from "./screenOverlay";
  import { cueAt } from "./media";
  import { afterClose, NativeDriver } from "./nativeDriver";
  import Screen from "./Screen.svelte";
  import Thumb from "./Thumb.svelte";
  import { prefs } from "./playerPrefs.svelte";
  import { os } from "./platform";

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
  /** Whether it's open and has shown a frame: till then the VOD's library
   *  picture stands in the box, rather than the screen's black. */
  let isOpen = false;
  let pictured = $state(false);
  /** How long a file's open may take before it's taken as waiting on macOS's
   *  question (milliseconds): one takes some 30 ms otherwise. */
  const ASKING_AFTER = 1000;

  const cue = $derived(prefs.captions ? cueAt(cues, time) : null);

  /** The folder of the file, if it's one macOS asks the person's leave to
   *  read (Downloads, Desktop, Documents) the first time the app opens a file
   *  there: the open waits on their answer, the picture black meanwhile. */
  const guarded = $derived(
    os === "mac" ? /^\/Users\/[^/]+\/(Downloads|Desktop|Documents)\//.exec(vod.source_path ?? "")?.[1] : undefined,
  );
  /** That folder, while the open has taken long enough to be waiting on
   *  macOS's question. */
  let asking = $state<string | null>(null);
  let askingSoon: ReturnType<typeof setTimeout> | undefined;

  // The caption, over the picture: drawn again when it, the box's size or
  // full screen changes. While the open waits on macOS's question, what it waits for.
  let boxSize = $state({ width: 0, height: 0, fullscreen: false });
  $effect(() => {
    const { width, height, fullscreen } = boxSize;
    if (!width || !height) overlay.show(null);
    else if (asking) overlay.show(overlay.hint(`Waiting for permission to read your ${asking} folder…`, width, height));
    else overlay.show(cue ? overlay.caption(cue.text, width, height, fullscreen) : null);
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
    // As last set, in any player (or on the bar before this one was made).
    const saved = prefs.volume();
    if (saved !== null) driver.volume.set(saved);
    const rate = prefs.speed();
    if (rate !== null) driver.speed.set(rate);
    const folder = guarded;
    askingSoon = folder ? setTimeout(() => (asking = folder), ASKING_AFTER) : undefined;
    const answered = () => {
      clearTimeout(askingSoon);
      asking = null;
    };
    // Gone before the last view's player closed: not opened at all.
    afterClose(() => (destroyed ? Promise.reject(new Error("closed")) : api.playerOpen(vod.id, start, autoplay))).then(
      (opened) => {
        answered();
        if (destroyed) return;
        isOpen = true;
        driver.opened(opened);
        onerror?.(null);
      },
      // Closed before it opened (the page moved on): nothing to say.
      (e) => {
        answered();
        if (!destroyed) onerror?.(String(e));
      },
    );
    fetchCues();
    follow();
  });

  // Asked again as the VOD changes: a stretch transcribed brings its captions.
  function fetchCues() {
    api.captions(vod.id).then(
      (c) => !destroyed && (cues = c),
      (e) => console.error("captions", e),
    );
  }
  const changed = events.vod((v) => v.id === vod.id && fetchCues());

  // The position, for the captions: smoothly while playing.
  let frame = 0;
  function follow() {
    if (destroyed) return;
    time = now();
    // Its first frame is on the screen (the player shows it before it says so).
    if (!pictured && isOpen && driver.shown() !== null) pictured = true;
    frame = requestAnimationFrame(follow);
  }

  onDestroy(() => {
    destroyed = true;
    clearTimeout(askingSoon);
    void changed.then((u) => u());
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

  export const speed = {
    get: () => driver.speed.get(),
    set(rate: number) {
      driver.speed.set(rate);
      prefs.setSpeed(rate);
    },
  };

  // A press held still on the picture carries the stretch the playhead is
  // in out (dragout.ts); a click plays or pauses.
  let picture = $state<HTMLDivElement>();
  const press = holdToCarry(
    // The VOD's length, as the timeline's: the file's may differ by a little.
    () => stretchCarry(vod, now(), vod.duration || mediaDuration(), picture),
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
    <!-- While macOS asks, the screen shows what it waits for (`asking`). -->
    <Screen hidden={!pictured && !asking} />
    {#if !pictured && vod.thumb}<Thumb name={vod.thumb} />{/if}
  </div>
</div>

<style>
  .player {
    background: #000;
  }
</style>
