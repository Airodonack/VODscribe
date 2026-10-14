<script lang="ts">
  import { theme } from "./theme.svelte";
  // A strip beside the transcript with the whole VOD top to bottom: how busy
  // it was at each moment, drawn as a mirrored waveform, and a box for the part
  // that's in view. Clicking or dragging on it scrolls there.

  let {
    times = [],
    histogram,
    duration,
    view,
    label,
    overlays = [],
    events = [],
    eventColor = "#888888",
    onseek,
  }: {
    /** When each line started (with no chat to draw), in seconds. */
    times?: number[];
    /** Or, instead, how many happened in each of equal stretches of `duration`
     *  (chat, which can be far too many messages to send one by one). */
    histogram?: number[];
    duration: number;
    /** The part of the VOD in view, as 0..1 fractions of it. */
    view: { from: number; to: number } | null;
    /** What the waveform shows, for its tooltip. */
    label: string;
    /** Timeline layers drawn over it, each scaled to its own peak so its spikes stand out. */
    overlays?: { color: string; times: number[] }[];
    /** Chat events, with about what each was worth: bars across the whole
     *  strip, each moment as opaque as the money it saw, next to the VOD's biggest. */
    events?: { time: number; value: number }[];
    eventColor?: string;
    onseek: (time: number) => void;
  } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let width = $state(0);
  let height = $state(0);

  // One bar per two pixels of height, each summing a moment's events and its
  // neighbors' (a little smoothing). Scaled nearly linearly, so bursts stand
  // out; the slight curve keeps a quieter stretch visible next to one.
  function levelsOf(times: number[], bins: number): Float64Array {
    const counts = new Float64Array(bins);
    if (duration <= 0) return counts;
    for (const t of times) counts[Math.min(bins - 1, Math.max(0, Math.floor((t / duration) * bins)))]++;
    return shape(counts);
  }

  /** `histogram`'s counts, gathered into `bins`. */
  function rebin(histogram: number[], bins: number): Float64Array {
    const counts = new Float64Array(bins);
    histogram.forEach((n, i) => (counts[Math.min(bins - 1, Math.floor((i / histogram.length) * bins))] += n));
    return shape(counts);
  }

  function shape(counts: Float64Array): Float64Array {
    const smooth = counts.map((c, i) => (counts[i - 1] ?? c) * 0.25 + c * 0.5 + (counts[i + 1] ?? c) * 0.25);
    const top = Math.max(...smooth, 1);
    return smooth.map((c) => Math.pow(c / top, 0.8));
  }
  let bins = $derived(Math.max(1, Math.floor(height / 2)));
  let levels = $derived(histogram ? rebin(histogram, bins) : levelsOf(times, bins));
  let layerLevels = $derived(overlays.map((o) => ({ color: o.color, levels: levelsOf(o.times, bins) })));

  // Each moment's events: what they were worth together, by the 2 px slot
  // they're in (for how strong a bar is), and each one's exact place, where
  // its bar is drawn, so a bar is where the box is when its moment is in view.
  let eventMoments = $derived.by(() => {
    const values = new Float64Array(bins);
    if (duration <= 0 || events.length === 0) return null;
    const at = events.map((e) => {
      const f = Math.min(1, Math.max(0, e.time / duration));
      const i = Math.min(bins - 1, Math.floor(f * bins));
      values[i] += e.value;
      return { f, i };
    });
    return { at, values, topValue: Math.max(...values, 0.01) };
  });
  /** How opaque a moment worth `value` is: the biggest is solid, the least still shows. */
  const strength = (value: number, top: number) => 0.22 + 0.78 * Math.min(1, value / top);

  $effect(() => {
    const ctx = canvas?.getContext("2d");
    // The colors are read from the page, so a change of theme redraws.
    void theme.dark;
    if (!canvas || !ctx || width === 0 || height === 0) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const mid = width / 2;
    const step = height / levels.length;
    const draw = (levels: Float64Array, color: string) => {
      ctx.fillStyle = color;
      levels.forEach((level, i) => {
        const half = Math.max(level > 0 ? 0.75 : 0, level * (mid - 3));
        if (half > 0) ctx.fillRect(mid - half, i * step, half * 2, Math.max(1, step - 0.5));
      });
    };
    // With layers or events on, all of chat steps back to gray beneath them.
    const style = getComputedStyle(canvas);
    draw(levels, layerLevels.length || eventMoments ? style.borderTopColor : style.color);
    for (const layer of layerLevels) draw(layer.levels, layer.color);
    if (eventMoments) {
      const { at, values, topValue } = eventMoments;
      ctx.fillStyle = eventColor;
      // A bar across the whole strip, on top of the rest, centered on its moment.
      const thick = Math.max(1.5, step - 0.5);
      for (const { f, i } of at) {
        ctx.globalAlpha = strength(values[i], topValue);
        ctx.fillRect(0, f * height - thick / 2, width, thick);
      }
      ctx.globalAlpha = 1;
    }
  });

  // Clicking or dragging centers the view on the moment under the pointer.
  // While dragging, the box is drawn there too, like a scrollbar's thumb: the
  // view's box can lean off the pointer where the transcript has a long
  // stretch without speech, which is little to scroll past.
  let dragging = false;
  /** Where a drag holds the box's middle, as a fraction of the strip. */
  let held = $state<number | null>(null);
  function seekTo(e: PointerEvent) {
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const fraction = Math.min(1, Math.max(0, (e.clientY - box.top) / box.height));
    if (dragging) held = fraction;
    onseek(fraction * duration);
  }
  function release() {
    dragging = false;
    held = null;
  }
  /** The box: the view, or while dragging, the view's size at the pointer.
   *  At least a few pixels tall (a long VOD's view can be under one), grown
   *  evenly about its middle, so its middle stays on the view's. */
  const MIN_BOX = 6;
  let shown = $derived.by(() => {
    if (!view) return null;
    const size = Math.min(1, Math.max(view.to - view.from, height > 0 ? MIN_BOX / height : 0));
    const middle = held ?? (view.from + view.to) / 2;
    const from = Math.min(1 - size, Math.max(0, middle - size / 2));
    return { from, to: from + size };
  });
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
    // Not the start of a text selection, or of dragging a selection away.
    e.preventDefault();
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
  onpointerup={release}
  onpointercancel={release}
>
  <canvas bind:this={canvas}></canvas>
  {#if shown}
    <div class="view" style:top="{shown.from * 100}%" style:height="{(shown.to - shown.from) * 100}%"></div>
  {/if}
</div>

<style>
  .minimap {
    position: relative;
    /* A strip to drag on, never text to select. */
    user-select: none;
    -webkit-user-select: none;
    -webkit-user-drag: none;
    width: 100%;
    height: 100%;
    cursor: pointer;
    touch-action: none;
  }

  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    /* The waveform's colors, read (resolved) by the drawing code: alone, and
       under layers. The canvas has no border; the property only carries a color. */
    color: color-mix(in srgb, var(--accent) 55%, transparent);
    border-top-color: color-mix(in srgb, var(--muted) 35%, transparent);
  }

  .view {
    position: absolute;
    left: 0;
    right: 0;
    border-radius: 3px;
    background: color-mix(in srgb, var(--text) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--text) 25%, transparent);
    pointer-events: none;
  }
</style>
