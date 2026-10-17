<!--
  The player's screen: a box the picture shows in. The app's player draws
  outside the page, on a native surface over the window (crates/player
  present) that covers the box, so nothing of the page shows over it. The
  box tells the surface where it is whenever that changes. Where there's no surface (macOS, for now), the box stays
  black.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./api";

  let box: HTMLDivElement;

  onMount(() => {
    let stopped = false;
    let last = "";
    const report = () => {
      if (stopped) return;
      const r = box.getBoundingClientRect();
      const rect = { x: r.x, y: r.y, width: r.width, height: r.height };
      const now = JSON.stringify(rect);
      if (now === last) return;
      last = now;
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

<div class="screen" bind:this={box}></div>

<style>
  /* Fills its box (which is positioned), however the box is sized: a
     percentage height would be worked out from the box's aspect ratio
     before its max-height caps it (WebKit). */
  .screen {
    position: absolute;
    inset: 0;
    background: #000;
  }
  :global(html.screen) .screen {
    background: transparent;
  }
</style>
