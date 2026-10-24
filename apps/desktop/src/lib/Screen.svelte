<!--
  The player's screen: a box the picture shows in. The app's player draws
  outside the page, on a native surface over the window (crates/player
  present) that covers the box, so nothing of the page shows over it. The
  box tells the surface where it is whenever that changes. Where there's no surface, the box stays black.
  While a panel or dialog (or anything marked data-over-screen) overlaps the
  box, the surface is hidden, so what's open shows; the frame showing then
  stays in the box, drawn by the page, rather than black.
  Under a site's player (`under`, embed.ts) the picture goes under the page
  instead, so their player in the page stays on top: the box is clear, and
  every background behind it is painted around it, not through it. A panel
  over it then simply draws over the picture. Where the surface can't go
  under the page (X11), the page draws the picture itself, in the box.
-->
<script lang="ts" module>
  // The newest screen box is the one the surface follows: one going (the
  // page moved on to another VOD) neither moves nor hides it after.
  let newest = 0;
  // Backgrounds painted around a box (`holes`), as they were before any box
  // painted them, and which box painted each last: boxes can share
  // ancestors, so only the last one to paint an element puts it back.
  const before = new Map<HTMLElement, string>();
  const painter = new Map<HTMLElement, number>();
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./api";
  import * as overlay from "./screenOverlay";

  // `hidden`: no surface yet (the player has no frame to show), so the
  // page shows what's in the box rather than the surface's black.
  let { under = false, hidden = false }: { under?: boolean; hidden?: boolean } = $props();

  let box: HTMLDivElement;
  let still: HTMLCanvasElement;
  // What's over the picture (captions), where the page draws it.
  let over: HTMLDivElement;
  // Over the page (files), under it (a site's player over it), or drawn by
  // the page (under one, where the surface can't go under: X11). An
  // embed's isn't known till the app says.
  let mode = $state<"over" | "under" | "page" | null>(null);
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

  // This box, among all there have been (`newest`).
  const me = ++newest;

  // Under the page, every opaque background above the box is painted
  // around it, not through it: four bands of its own colour about the box
  // (no layout changes, no elements added). Put back as it was after.
  const holed = new Set<HTMLElement>();
  let holedAt: { x: number; y: number; width: number; height: number } | null = null;
  /** Puts `el`'s background back as it was before any box painted it. */
  const unpaint = (el: HTMLElement) => {
    el.style.background = before.get(el) ?? "";
    before.delete(el);
    painter.delete(el);
  };
  function holes(rect: { x: number; y: number; width: number; height: number } | null) {
    for (const el of holed) if (painter.get(el) === me) unpaint(el);
    holed.clear();
    holedAt = rect;
    if (!rect) return;
    for (let el = box.parentElement; el; el = el.parentElement) {
      // Another box's bands first go, to read its own colour.
      if (painter.has(el)) unpaint(el);
      const cs = getComputedStyle(el);
      if (cs.backgroundColor === "rgba(0, 0, 0, 0)" || cs.backgroundColor === "transparent") continue;
      const e = el.getBoundingClientRect();
      const px = (n: number) => `${Math.max(0, n)}px`;
      const [x, y, w, h] = [rect.x - e.left, rect.y - e.top, rect.width, rect.height];
      const c = `linear-gradient(${cs.backgroundColor}, ${cs.backgroundColor})`;
      const band = (pos: string, size: string) => `${c} ${pos} / ${size} no-repeat border-box`;
      before.set(el, el.style.background);
      painter.set(el, me);
      holed.add(el);
      el.style.background = [
        band(`0 0`, `100% ${px(y)}`),
        band(`0 ${px(y + h)}`, `100% calc(100% - ${px(y + h)})`),
        band(`0 ${px(y)}`, `${px(x)} ${px(h)}`),
        band(`${px(x + w)} ${px(y)}`, `calc(100% - ${px(x + w)}) ${px(h)}`),
      ].join(", ");
    }
  }

  /** Draws each new frame into the canvas, at most the box's device
   *  pixels, until `stopped()`. */
  function drawFrames(stopped: () => boolean) {
    let since: number | null = null;
    let size = "";
    let image: ImageData | null = null;
    const tick = async () => {
      if (stopped()) return;
      const r = box.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      const [width, height] = [Math.round(r.width * dpr), Math.round(r.height * dpr)];
      // A new size (full screen, say): the frame showing again, at it.
      if (`${width}x${height}` !== size) [size, since] = [`${width}x${height}`, null];
      const bytes = await api.playerFrame(since, width, height).catch(() => new ArrayBuffer(0));
      if (bytes.byteLength >= 16) {
        const view = new DataView(bytes);
        const [w, h] = [view.getUint32(0, true), view.getUint32(4, true)];
        since = Number(view.getBigUint64(8, true));
        if (bytes.byteLength >= 16 + w * h * 4) {
          if (image?.width !== w || image.height !== h) {
            image = new ImageData(w, h);
            still.width = w;
            still.height = h;
          }
          image.data.set(new Uint8ClampedArray(bytes, 16, w * h * 4));
          still.getContext("2d")?.putImageData(image, 0, 0);
          stillShown = true;
        }
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
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
      // Out of the page, or another box come since: the surface isn't this one's.
      if (stopped || !mode || newest !== me || !box.isConnected) return;
      const r = box.getBoundingClientRect();
      const here = { x: r.x, y: r.y, width: r.width, height: r.height };
      if (mode !== "over") {
        // Under the page, nothing needs hiding; drawn by the page, no surface.
        const rect = mode === "under" ? here : null;
        const now = JSON.stringify(rect);
        if (now === last) return;
        last = now;
        holes(rect);
        api.setScreen(rect, true).catch(() => {});
        return;
      }
      const covered = [...document.querySelectorAll("[role=dialog], [data-over-screen]")].some((el) =>
        overlaps(el.getBoundingClientRect(), r),
      );
      const rect = covered || hidden ? null : here;
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
    if (!under) mode = "over";
    else
      void api
        .screenUnder()
        .catch(() => false)
        .then((can) => {
          if (stopped) return;
          mode = can ? "under" : "page";
          if (mode === "page") {
            drawFrames(() => stopped);
            overlay.drawInPage((drawn) => {
              over.replaceChildren();
              if (!drawn) return;
              const { canvas, rect } = drawn;
              canvas.style.cssText = `position: absolute; left: ${rect.x}px; top: ${rect.y}px; width: ${rect.width}px; height: ${rect.height}px`;
              over.append(canvas);
            });
          }
          report();
        });
    // The theme changed: the bands again, in its colours (holes puts the
    // backgrounds back before reading them).
    const rehole = () => holedAt && holes(holedAt);
    const themes = new MutationObserver(rehole);
    themes.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "class"] });
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    scheme.addEventListener("change", rehole);
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
      if (mode === "page") overlay.drawInPage(null);
      themes.disconnect();
      scheme.removeEventListener("change", rehole);
      holes(null);
      cancelAnimationFrame(frame);
      sizes.disconnect();
      if (newest === me) api.setScreen(null).catch(() => {});
    };
  });
</script>

<div class="screen" class:clear={under} bind:this={box}>
  <canvas class="still" class:shown={stillShown} class:live={mode === "page"} bind:this={still}></canvas>
  <div class="over" bind:this={over}></div>
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

  /* Under a site's player: the picture shows through. */
  .screen.clear {
    background: transparent;
  }

  /* Drawn by the page: black about the picture, as the surface's. */
  .still.live {
    background: #000;
  }

  .over {
    position: absolute;
    inset: 0;
  }
</style>
