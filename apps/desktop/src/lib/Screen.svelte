<!--
  The player's screen: a box the picture shows in. The app's player draws
  outside the page, on a native surface over the window (crates/player
  present) that covers the box, so nothing of the page shows over it. The
  box tells the surface where it is whenever that changes. Where there's no surface, the box stays black.
  While a panel or dialog (or anything marked data-over-screen) overlaps the
  box, the surface is hidden, so what's open shows; the frame showing then
  stays in the box, drawn by the page, rather than black.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./api";

  let box: HTMLDivElement;
  let still: HTMLCanvasElement;
  // The frame drawn by the page while the surface is hidden.
  let stillShown = $state(false);

  /** Draws the frame showing now into the box, for when the surface hides;
   *  false if there's none. */
  async function showStill(): Promise<boolean> {
    const bytes = new DataView(await api.playerStill().catch(() => new ArrayBuffer(0)));
    if (bytes.byteLength < 8) return false;
    const width = bytes.getUint32(0, true);
    const height = bytes.getUint32(4, true);
    if (bytes.byteLength < 8 + width * height * 3) return false;
    const rgb = new Uint8Array(bytes.buffer, 8, width * height * 3);
    const rgba = new Uint8ClampedArray(width * height * 4);
    for (let i = 0, j = 0; i < rgb.length; i += 3, j += 4) {
      rgba[j] = rgb[i];
      rgba[j + 1] = rgb[i + 1];
      rgba[j + 2] = rgb[i + 2];
      rgba[j + 3] = 255;
    }
    still.width = width;
    still.height = height;
    still.getContext("2d")?.putImageData(new ImageData(rgba, width, height), 0, 0);
    stillShown = true;
    return true;
  }

  onMount(() => {
    let stopped = false;
    let last = "";
    // Counted up at each change, so a still that comes after the panel's
    // gone again doesn't hide the surface.
    let seq = 0;
    const overlaps = (a: DOMRect, b: DOMRect) =>
      a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
    const report = () => {
      if (stopped) return;
      const r = box.getBoundingClientRect();
      const covered = [...document.querySelectorAll("[role=dialog], [data-over-screen]")].some((el) =>
        overlaps(el.getBoundingClientRect(), r),
      );
      const rect = covered ? null : { x: r.x, y: r.y, width: r.width, height: r.height };
      const now = JSON.stringify(rect);
      if (now === last) return;
      const mine = ++seq;
      const wasShown = last !== "" && last !== "null";
      last = now;
      if (covered && wasShown) {
        // Covered now: the frame drawn in the page first, then the surface
        // hidden, so the box doesn't show black between the two.
        void showStill().then(() => {
          if (mine === seq && !stopped) api.setScreen(null).catch(() => {});
        });
        return;
      }
      if (!covered) stillShown = false;
      api.setScreen(rect).catch(() => {});
    };
    const sizes = new ResizeObserver(report);
    sizes.observe(box);
    sizes.observe(document.documentElement);
    // Moved by anything else (a panel opening, a scroll): checked each frame.
    let frame = requestAnimationFrame(function tick() {
      report();
      frame = requestAnimationFrame(tick);
    });
    return () => {
      stopped = true;
      cancelAnimationFrame(frame);
      sizes.disconnect();
      api.setScreen(null).catch(() => {});
    };
  });
</script>

<div class="screen" bind:this={box}>
  <canvas class="still" class:shown={stillShown} bind:this={still}></canvas>
</div>

<style>
  /* Fills its box (which is positioned), however the box is sized: a
     percentage height would be worked out from the box's aspect ratio
     before its max-height caps it (WebKit). */
  .screen {
    position: absolute;
    inset: 0;
    background: #000;
  }

  /* The frame, fitted as the surface fits it, while the surface is hidden. */
  .still {
    width: 100%;
    height: 100%;
    object-fit: contain;
    visibility: hidden;
  }

  .still.shown {
    visibility: visible;
  }
</style>
