<script lang="ts">
  // A strip beside the transcript with the whole VOD top to bottom: how busy
  // it was at each moment, drawn as a mirrored waveform, and a box for the part
  // that's in view. Clicking or dragging on it scrolls there.

  let {
    times,
    duration,
    view,
    label,
    onseek,
  }: {
    /** When each event happened (chat messages, or lines when there's no chat), in seconds. */
    times: number[];
    duration: number;
    /** The part of the VOD in view, as 0..1 fractions of it. */
    view: { from: number; to: number } | null;
    /** What the waveform shows, for its tooltip. */
    label: string;
    onseek: (time: number) => void;
  } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let width = $state(0);
  let height = $state(0);

  // One bar per two pixels of height, each summing a moment's events and its
  // neighbors' (a little smoothing). Scaled nearly linearly, so bursts stand
  // out; the slight curve keeps a quieter stretch visible next to one.
  let levels = $derived.by(() => {
    const bins = Math.max(1, Math.floor(height / 2));
    const counts = new Float64Array(bins);
    if (duration <= 0) return counts;
    for (const t of times) counts[Math.min(bins - 1, Math.max(0, Math.floor((t / duration) * bins)))]++;
    const smooth = counts.map((c, i) => (counts[i - 1] ?? c) * 0.25 + c * 0.5 + (counts[i + 1] ?? c) * 0.25);
    const top = Math.max(...smooth, 1);
    return smooth.map((c) => Math.pow(c / top, 0.8));
  });

  $effect(() => {
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || width === 0 || height === 0) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = getComputedStyle(canvas).color;
    const mid = width / 2;
    const step = height / levels.length;
    levels.forEach((level, i) => {
      const half = Math.max(level > 0 ? 0.75 : 0, level * (mid - 3));
      if (half > 0) ctx.fillRect(mid - half, i * step, half * 2, Math.max(1, step - 0.5));
    });
  });

  let dragging = false;
  function seekTo(e: PointerEvent) {
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const fraction = Math.min(1, Math.max(0, (e.clientY - box.top) / box.height));
    onseek(fraction * duration);
  }
</script>

<div
  class="minimap"
  bind:clientWidth={width}
  bind:clientHeight={height}
  role="scrollbar"
  aria-controls="transcript"
  aria-orientation="vertical"
  aria-valuenow={Math.round((view?.from ?? 0) * 100)}
  tabindex="-1"
  title="{label}. Click or drag to go there."
  onpointerdown={(e) => {
    dragging = true;
    try {
      // Keeps the drag going outside the strip.
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      // Not a pointer that can be captured; the click still seeks.
    }
    seekTo(e);
  }}
  onpointermove={(e) => dragging && seekTo(e)}
  onpointerup={() => (dragging = false)}
  onpointercancel={() => (dragging = false)}
>
  <canvas bind:this={canvas}></canvas>
  {#if view}
    <div class="view" style:top="{view.from * 100}%" style:height="{Math.max(0.5, (view.to - view.from) * 100)}%"></div>
  {/if}
</div>

<style>
  .minimap {
    position: relative;
    width: 100%;
    height: 100%;
    cursor: pointer;
    touch-action: none;
    user-select: none;
  }

  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    /* The waveform's color (read by the drawing code). */
    color: color-mix(in srgb, var(--accent) 55%, transparent);
  }

  .view {
    position: absolute;
    left: 0;
    right: 0;
    min-height: 4px;
    border-radius: 3px;
    background: color-mix(in srgb, var(--text) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--text) 25%, transparent);
    pointer-events: none;
  }
</style>
