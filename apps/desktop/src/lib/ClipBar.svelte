<script lang="ts">
  // The clip bar under the player: a stretch of the VOD around where it's
  // playing, with its waveform where there is one, its clips and the range
  // picked. Clicking seeks; dragging picks a range (clips.svelte.ts); the edges
  // of the range picked, and of a clip, can be dragged. The wheel zooms around
  // the pointer and Shift+wheel (or sideways) pans; it follows the playhead
  // until panned.

  import { onDestroy, onMount, untrack } from "svelte";
  import { api, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import { columns, PEAKS_RATE, tickStep } from "./media";
  import { formatTime } from "./platform";
  import { theme } from "./theme.svelte";

  let {
    vod,
    duration,
    time,
    onseek,
  }: {
    vod: Vod;
    duration: number;
    /** Where the player is now. */
    time: () => number;
    onseek: (time: number) => void;
  } = $props();

  const SHORTEST = 10;
  let span = $state(
    Math.min(
      120,
      untrack(() => duration),
    ),
  );
  let viewStart = $state(0);
  let follow = true;
  let now = $state(0);
  let width = $state(0);
  let levels = $state.raw<Uint8Array>(new Uint8Array());
  let frame = 0;

  onMount(() => {
    api.peaks(vod.id).then(
      (l) => (levels = l),
      (e) => console.error("waveform", e),
    );
    const tick = () => {
      now = time();
      if (follow && (now < viewStart || now > viewStart + span * 0.92)) setView(now - span * 0.08);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
  });
  onDestroy(() => cancelAnimationFrame(frame));

  function setView(start: number, length = span) {
    span = Math.min(duration, Math.max(SHORTEST, length));
    viewStart = Math.min(Math.max(0, start), Math.max(0, duration - span));
  }

  const x = (t: number) => ((t - viewStart) / span) * width;
  const pct = (t: number) => `${((t - viewStart) / span) * 100}%`;

  let bar = $state<HTMLDivElement>();
  function timeAt(clientX: number): number {
    const box = bar!.getBoundingClientRect();
    return Math.min(duration, Math.max(0, viewStart + ((clientX - box.left) / box.width) * span));
  }

  // The waveform of the stretch shown, and the ticks.
  let canvas = $state<HTMLCanvasElement>();
  const ratio = typeof devicePixelRatio === "number" ? devicePixelRatio : 1;
  const HEIGHT = 44;
  $effect(() => {
    if (!canvas || !bar) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    void theme.dark;
    const w = Math.round(width * ratio);
    const h = Math.round(HEIGHT * ratio);
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
    ctx.clearRect(0, 0, w, h);
    const style = getComputedStyle(bar);
    // Only the stretch shown: the levels from its start, as many as it holds.
    const first = Math.floor(viewStart * PEAKS_RATE);
    const shown = levels.subarray(first, Math.ceil((viewStart + span) * PEAKS_RATE) + 1);
    const bars = columns(shown, span, Math.floor(width / 2));
    ctx.fillStyle = style.getPropertyValue("--wave").trim();
    const mid = h / 2 + 4 * ratio;
    for (let i = 0; i < bars.length; i++) {
      const b = Math.max(ratio, bars[i] * (h - 14 * ratio));
      ctx.fillRect(i * 2 * ratio, mid - b / 2, ratio, b);
    }
  });

  const ticks = $derived.by(() => {
    if (!width) return [];
    const step = tickStep(span, width);
    const out = [];
    for (let t = Math.ceil(viewStart / step) * step; t <= viewStart + span; t += step) out.push(t);
    return out;
  });

  // Dragging: a new range, an edge of the range picked, or an edge of a clip.
  type Drag =
    | { kind: "new"; from: number; x: number; moved: boolean }
    | { kind: "ghost"; edge: "start" | "end" }
    | { kind: "clip"; id: number; edge: "start" | "end"; start: number; end: number };
  let drag = $state<Drag | null>(null);

  function onDown(e: PointerEvent) {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    const handle = target.closest<HTMLElement>("[data-edge]");
    if (handle?.dataset.clip) {
      const clip = clips.list.find((c) => c.id === Number(handle.dataset.clip));
      if (clip)
        drag = {
          kind: "clip",
          id: clip.id,
          edge: handle.dataset.edge as "start" | "end",
          start: clip.start,
          end: clip.end,
        };
    } else if (handle) {
      drag = { kind: "ghost", edge: handle.dataset.edge as "start" | "end" };
    } else {
      drag = { kind: "new", from: timeAt(e.clientX), x: e.clientX, moved: false };
    }
    e.preventDefault();
  }

  function onMove(e: PointerEvent) {
    if (!drag) return;
    const t = timeAt(e.clientX);
    if (drag.kind === "new") {
      if (!drag.moved && Math.abs(e.clientX - drag.x) < 4) return;
      drag.moved = true;
      clips.pick(drag.from, t, duration);
    } else if (drag.kind === "ghost" && clips.ghost) {
      const { start, end } = clips.ghost;
      if (drag.edge === "start") clips.pick(t, end, duration);
      else clips.pick(start, t, duration);
      // Dragged past the other edge: it's now that one.
      if (clips.ghost && (drag.edge === "start" ? t > end : t < start))
        drag.edge = drag.edge === "start" ? "end" : "start";
    } else if (drag.kind === "clip") {
      if (drag.edge === "start") drag.start = Math.min(t, drag.end - 0.1);
      else drag.end = Math.max(t, drag.start + 0.1);
    }
  }

  function onUp(e: PointerEvent) {
    if (!drag) return;
    const done = drag;
    drag = null;
    if (!done) return;
    if (done.kind === "new" && !done.moved) {
      follow = true;
      onseek(timeAt(e.clientX));
    } else if (done.kind === "clip") {
      const clip = clips.list.find((c) => c.id === done.id);
      if (clip && (clip.start !== done.start || clip.end !== done.end)) clips.move(done.id, done.start, done.end);
    }
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    follow = false;
    const sideways = e.shiftKey ? e.deltaY : e.deltaX;
    if (Math.abs(sideways) > Math.abs(e.shiftKey ? 0 : e.deltaY)) {
      setView(viewStart + (sideways / width) * span);
      return;
    }
    // Zoom about the time under the pointer, keeping it where it is.
    const at = timeAt(e.clientX);
    const share = (at - viewStart) / span;
    const length = Math.min(duration, Math.max(SHORTEST, span * Math.exp(e.deltaY / 300)));
    setView(at - share * length, length);
  }

  /** A clip's range as shown: while its edge is dragged, where the drag has it. */
  function rangeOf(id: number, start: number, end: number) {
    return drag?.kind === "clip" && drag.id === id ? { start: drag.start, end: drag.end } : { start, end };
  }
</script>

<!-- Followed on the window, not the bar: the range appearing under the pointer
     mid-drag ended the drag in WebKitGTK. -->
<svelte:window onpointermove={onMove} onpointerup={onUp} onpointercancel={() => (drag = null)} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="clipbar"
  bind:this={bar}
  bind:clientWidth={width}
  onpointerdown={onDown}
  onwheel={onWheel}
  title="Drag to pick a clip · wheel to zoom · Shift+wheel to scroll"
>
  <canvas bind:this={canvas} style:height="{HEIGHT}px"></canvas>
  {#each ticks as t (t)}
    <span class="tick" style:left={pct(t)}>{formatTime(t)}</span>
  {/each}
  {#each clips.list as clip (clip.id)}
    {@const r = rangeOf(clip.id, clip.start, clip.end)}
    {#if r.end > viewStart && r.start < viewStart + span}
      <div
        class="band"
        style:left={pct(r.start)}
        style:width="{((x(r.end) - x(r.start)) / (width || 1)) * 100}%"
        title={clip.title}
      >
        <!-- Kept in view when the clip starts before the stretch shown. -->
        <span class="label" style:margin-left="{Math.max(0, -x(r.start))}px">{clip.title}</span>
        <span class="edge start" data-edge="start" data-clip={clip.id}></span>
        <span class="edge end" data-edge="end" data-clip={clip.id}></span>
      </div>
    {/if}
  {/each}
  {#if clips.ghost && clips.ghost.end > viewStart && clips.ghost.start < viewStart + span}
    {@const g = clips.ghost}
    <div class="band ghost" style:left={pct(g.start)} style:width="{((x(g.end) - x(g.start)) / (width || 1)) * 100}%">
      <span class="edge start" data-edge="start"></span>
      <span class="edge end" data-edge="end"></span>
    </div>
  {/if}
  {#if now >= viewStart && now <= viewStart + span}
    <span class="playhead" style:left={pct(now)}></span>
  {/if}
</div>

<style>
  .clipbar {
    --wave: color-mix(in srgb, var(--muted) 55%, transparent);
    position: relative;
    height: 44px;
    margin: 6px 16px 0;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    overflow: hidden;
    cursor: crosshair;
    touch-action: none;
    user-select: none;
  }

  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
  }

  .tick {
    position: absolute;
    top: 0;
    padding: 1px 0 0 3px;
    border-left: 1px solid var(--border);
    height: 10px;
    font-size: 9px;
    line-height: 1;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }

  .band {
    position: absolute;
    top: 12px;
    bottom: 3px;
    min-width: 2px;
    border-radius: 3px;
    background: color-mix(in srgb, var(--accent) 28%, transparent);
    border: 1px solid var(--accent);
    cursor: pointer;
  }

  .band.ghost {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    border-style: dashed;
  }

  .label {
    display: block;
    padding: 1px 6px;
    font-size: 10px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text);
    pointer-events: none;
  }

  .edge {
    position: absolute;
    top: -1px;
    bottom: -1px;
    width: 8px;
    cursor: ew-resize;
  }

  .edge.start {
    left: -4px;
  }

  .edge.end {
    right: -4px;
  }

  .edge::after {
    content: "";
    position: absolute;
    top: 25%;
    bottom: 25%;
    left: 3px;
    width: 2px;
    border-radius: 1px;
    background: var(--accent);
  }

  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
    pointer-events: none;
  }
</style>
