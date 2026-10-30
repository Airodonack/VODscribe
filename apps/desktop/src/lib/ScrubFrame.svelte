<script lang="ts">
  // The timeline's thumbnail: a small picture of the moment under the
  // pointer (or the selection's edge being dragged), from the storyboard
  // the app keeps of each file here (storyboards.rs): the keyframe at or
  // before it, however far back. Drawn into a canvas, which says the
  // moment it shows (`data-time`) and whether that's its exact frame
  // (`data-exact`). One
  // request at a time, the newest moment winning. Nothing shows until a
  // frame has (`shown`, kept from one hover to the next); after that, a
  // moment with none (footage not here, the storyboard still being made
  // there) leaves the box empty in its place, so the tip over its lower edge
  // stays put rather than jumping back up to the bar's top. See-through, so the
  // waveform under it shows: the timeline's other picture of the video.
  //
  // `exact`, when given, is shown instead of the storyboard's: the player's
  // own frame of the moment, once it has it.

  import { onDestroy } from "svelte";
  import { api, type Doc } from "./api";
  import { latest } from "./latest";
  import { FRAME_HEIGHT } from "./scrubFrame";

  let {
    doc,
    time,
    within,
    left,
    exact = null,
    shown = $bindable(0),
  }: {
    doc: Doc;
    /** The moment to show, in `doc`'s time. */
    time: number;
    /** How far off (in seconds) a stored frame may be and still show. */
    within: number;
    /** Where it goes, in pixels from the timeline's left. */
    left: number;
    /** The exact frame of the moment, preferred to the storyboard's while given. */
    exact?: { time: number; image: ImageBitmap } | null;
    /** Its width in CSS pixels as drawn, kept while it's empty; 0 until a frame has shown. */
    shown?: number;
  } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let at = $state<number | null>(null);
  let isExact = $state(false);
  const ratio = typeof devicePixelRatio === "number" ? devicePixelRatio : 1;

  function draw(image: ImageBitmap, time: number, exactly: boolean) {
    if (!canvas || !image.height) return;
    const width = (FRAME_HEIGHT * image.width) / image.height;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(FRAME_HEIGHT * ratio);
    canvas.getContext("2d")?.drawImage(image, 0, 0, canvas.width, canvas.height);
    at = time;
    isExact = exactly;
    shown = width;
  }

  /** None for this moment: the box stays, empty. */
  function clear() {
    if (canvas) canvas.getContext("2d")?.clearRect(0, 0, canvas.width, canvas.height);
    at = null;
    isExact = false;
  }

  // The storyboard's frame of each moment, one asked at a time (latest.ts);
  // one come for a moment since moved on from still shows while the newest
  // is asked: a frame near the pointer beats none.
  type Found = { time: number; image: ImageBitmap } | null;
  const asking = latest<{ doc: Doc; time: number; within: number }, Found>({
    async fetch(want) {
      const found = await api.storyboardFrame(want.doc, want.time, want.within);
      if (!found) return null;
      return {
        time: found.time,
        image: await createImageBitmap(new Blob([found.jpeg as BlobPart], { type: "image/jpeg" })),
      };
    },
    got(found) {
      // The exact frame is shown instead.
      if (exact) return;
      if (found) draw(found.image, found.time, false);
      else clear();
    },
    free: (found) => found?.image.close(),
    between: true,
    // Not in the app (the site's demo), or no frame to read: none shows.
    failed: () => clear(),
  });

  $effect(() => asking.want({ doc, time, within }));

  $effect(() => {
    if (exact) draw(exact.image, exact.time, true);
  });

  onDestroy(() => asking.clear());
</script>

<canvas
  bind:this={canvas}
  class="scrub-frame"
  class:empty={at === null}
  class:none={!shown}
  data-time={at}
  data-exact={isExact}
  style:left="{left}px"
  style:height="{FRAME_HEIGHT}px"
  style:width="{shown}px"
></canvas>

<style>
  /* Inside the timeline's bar, centred on the hover line, the tip across its
     lower edge: above it would be the picture's box, where the app's own
     player draws over the page. Its see-through lets the waveform and the
     hover line under it show. */
  .scrub-frame {
    position: absolute;
    top: 2px;
    border-radius: var(--radius-sm);
    outline: 1px solid color-mix(in srgb, var(--text) 35%, transparent);
    opacity: 0.7;
    pointer-events: none;
    z-index: 2;
  }

  /* No frame for this moment: the box, faintly, where the frame was. */
  .scrub-frame.empty {
    background: color-mix(in srgb, var(--text) 10%, transparent);
  }

  .scrub-frame.none {
    display: none;
  }
</style>
