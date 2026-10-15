<script lang="ts">
  // The timeline under every player: the whole VOD to start, zoomable to a
  // few seconds, with its waveform where there is one, its marks (cuts.svelte.ts),
  // its clips and the range picked. A left click or drag only ever moves the
  // playhead; a mark's flag selects it and drags it (a frame at a time); the
  // edges of the range picked, and of a clip, can be dragged. The wheel zooms around the
  // pointer and Shift+wheel (or sideways) pans; zoomed in, it follows the
  // playhead until panned.

  import { onMount, untrack } from "svelte";
  import { api, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import { cuts } from "./cuts.svelte";
  import { columns, frameStart, PEAKS_RATE, tickStep } from "./media";
  import { formatTime } from "./platform";
  import { theme } from "./theme.svelte";

  let {
    vod,
    duration,
    time,
    fps,
    onseek,
  }: {
    vod: Vod;
    duration: number;
    /** Where the player is now. */
    time: number;
    /** The video's frame rate, for marks (assumed, for players that don't say). */
    fps: number;
    onseek: (time: number) => void;
  } = $props();

  const SHORTEST = 10;
  const HEIGHT = 48;
  // The whole VOD to start.
  let span = $state(untrack(() => duration));
  let viewStart = $state(0);
  let follow = true;
  let width = $state(0);
  let levels = $state.raw<Uint8Array>(new Uint8Array());

  onMount(() => {
    api.peaks(vod.id).then(
      (l) => (levels = l),
      (e) => console.error("waveform", e),
    );
  });

  // A VOD whose length was unknown (or wrong) until the player said: keep the view inside it.
  $effect(() => {
    void duration;
    untrack(() => setView(viewStart, span >= duration ? duration : span));
  });

  // Zoomed in, the view keeps the playhead in sight while it plays.
  $effect(() => {
    const now = time;
    untrack(() => {
      if (follow && span < duration && (now < viewStart || now > viewStart + span * 0.92)) setView(now - span * 0.08);
    });
  });

  function setView(start: number, length = span) {
    span = Math.min(duration, Math.max(Math.min(SHORTEST, duration), length));
    viewStart = Math.min(Math.max(0, start), Math.max(0, duration - span));
  }

  const x = (t: number) => ((t - viewStart) / span) * width;
  const pct = (t: number) => `${((t - viewStart) / span) * 100}%`;
  const widthPct = (a: number, b: number) => `${((x(b) - x(a)) / (width || 1)) * 100}%`;

  let bar = $state<HTMLDivElement>();
  function timeAt(clientX: number): number {
    const box = bar!.getBoundingClientRect();
    return Math.min(duration, Math.max(0, viewStart + ((clientX - box.left) / box.width) * span));
  }

  // The waveform of the stretch shown: played (left of the playhead) in the
  // accent, the rest muted; a plain track where there's no waveform.
  let canvas = $state<HTMLCanvasElement>();
  const ratio = typeof devicePixelRatio === "number" ? devicePixelRatio : 1;
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
    const played = style.getPropertyValue("--played").trim();
    const rest = style.getPropertyValue("--unplayed").trim();
    const at = x(time) * ratio;
    const mid = h / 2 + 6 * ratio;
    if (!levels.length) {
      ctx.fillStyle = rest;
      ctx.fillRect(0, mid - 2 * ratio, w, 4 * ratio);
      ctx.fillStyle = played;
      ctx.fillRect(0, mid - 2 * ratio, Math.max(0, at), 4 * ratio);
      return;
    }
    // Only the stretch shown: the levels from its start, as many as it holds.
    const first = Math.floor(viewStart * PEAKS_RATE);
    const shown = levels.subarray(first, Math.ceil((viewStart + span) * PEAKS_RATE) + 1);
    const bars = columns(shown, span, Math.floor(width / 2));
    for (let i = 0; i < bars.length; i++) {
      const px = i * 2 * ratio;
      const b = Math.max(ratio, bars[i] * (h - 16 * ratio));
      ctx.fillStyle = px < at ? played : rest;
      ctx.fillRect(px, mid - b / 2, ratio, b);
    }
  });

  const ticks = $derived.by(() => {
    if (!width || !span) return [];
    const step = tickStep(span, width);
    const out = [];
    for (let t = Math.ceil(viewStart / step) * step; t <= viewStart + span; t += step) out.push(t);
    return out;
  });

  // Pressing: scrubbing (the playhead follows), or an edge of the range picked
  // or of a clip. Followed on the window: the bar's contents changing under
  // the pointer mid-drag ended drags in WebKitGTK.
  type Drag =
    | { kind: "scrub" }
    | { kind: "mark"; id: number; at: number; x: number; moved: boolean }
    | { kind: "ghost"; edge: "start" | "end" }
    | { kind: "clip"; id: number; edge: "start" | "end"; start: number; end: number };
  let drag = $state<Drag | null>(null);
  let hover = $state<number | null>(null);

  function onDown(e: PointerEvent) {
    if (e.button !== 0) return;
    const handle = (e.target as HTMLElement).closest<HTMLElement>("[data-edge]");
    const edge = handle?.dataset.edge as "start" | "end" | undefined;
    const flag = (e.target as HTMLElement).closest<HTMLElement>("[data-mark]");
    const mark = flag && cuts.list.find((m) => m.id === Number(flag.dataset.mark));
    if (mark) {
      drag = { kind: "mark", id: mark.id, at: mark.start, x: e.clientX, moved: false };
      cuts.selected = mark.id;
    } else if (handle?.dataset.clip && edge) {
      const clip = clips.list.find((c) => c.id === Number(handle.dataset.clip));
      if (clip) drag = { kind: "clip", id: clip.id, edge, start: clip.start, end: clip.end };
    } else if (edge) {
      drag = { kind: "ghost", edge };
    } else {
      drag = { kind: "scrub" };
      cuts.selected = null;
      follow = true;
      onseek(timeAt(e.clientX));
    }
    e.preventDefault();
  }

  function onMove(e: PointerEvent) {
    if (bar) {
      const box = bar.getBoundingClientRect();
      const inside = e.clientX >= box.left && e.clientX <= box.right && e.clientY >= box.top && e.clientY <= box.bottom;
      hover = inside || drag ? Math.min(box.width, Math.max(0, e.clientX - box.left)) : null;
    }
    if (!drag) return;
    const t = timeAt(e.clientX);
    if (drag.kind === "scrub") {
      onseek(t);
    } else if (drag.kind === "mark") {
      if (!drag.moved && Math.abs(e.clientX - drag.x) < 3) return;
      drag.moved = true;
      drag.at = frameStart(t, fps);
      onseek(drag.at);
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

  function onUp() {
    const done = drag;
    drag = null;
    if (done?.kind === "mark") {
      // A click on a flag selects the mark and shows its frame; a drag moves it.
      if (done.moved) cuts.move(done.id, done.at);
      else onseek(done.at);
    }
    if (done?.kind === "clip") {
      const clip = clips.list.find((c) => c.id === done.id);
      if (clip && (clip.start !== done.start || clip.end !== done.end)) clips.move(done.id, done.start, done.end);
    }
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const sideways = e.shiftKey ? e.deltaY : e.deltaX;
    if (Math.abs(sideways) > Math.abs(e.shiftKey ? 0 : e.deltaY)) {
      follow = false;
      setView(viewStart + (sideways / width) * span);
      return;
    }
    // Zoom about the time under the pointer, keeping it where it is.
    const at = timeAt(e.clientX);
    const share = (at - viewStart) / span;
    const length = Math.min(duration, Math.max(SHORTEST, span * Math.exp(e.deltaY / 300)));
    follow = length < span ? follow : true;
    setView(at - share * length, length);
  }

  /** A clip's range as shown: while its edge is dragged, where the drag has it. */
  function rangeOf(id: number, start: number, end: number) {
    return drag?.kind === "clip" && drag.id === id ? { start: drag.start, end: drag.end } : { start, end };
  }

  const hoverTime = $derived(hover === null || !width ? null : viewStart + (hover / width) * span);
</script>

<svelte:window onpointermove={onMove} onpointerup={onUp} onpointercancel={() => (drag = null)} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="scrubber"
  class:zoomed={span < duration}
  bind:this={bar}
  bind:clientWidth={width}
  onpointerdown={onDown}
  onpointerleave={() => !drag && (hover = null)}
  onwheel={onWheel}
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
        data-band={clip.id}
        style:left={pct(r.start)}
        style:width={widthPct(r.start, r.end)}
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
    <div class="band ghost" style:left={pct(g.start)} style:width={widthPct(g.start, g.end)}>
      <span class="edge start" data-edge="start"></span>
      <span class="edge end" data-edge="end"></span>
    </div>
  {/if}
  {#if time >= viewStart && time <= viewStart + span}
    <span class="playhead" style:left={pct(time)}></span>
  {/if}
  {#each cuts.list as mark (mark.id)}
    {@const at = drag?.kind === "mark" && drag.id === mark.id ? drag.at : mark.start}
    {#if at >= viewStart && at <= viewStart + span}
      <span
        class="cut"
        class:selected={cuts.selected === mark.id}
        data-mark={mark.id}
        style:left={pct(at)}
        title="Mark at {formatTime(at)} · drag to move it"
      ></span>
    {/if}
  {/each}
  {#if hoverTime !== null && hover !== null}
    <span class="hover" style:left="{hover}px"></span>
    <span class="tip" style:left="{Math.min(Math.max(hover, 28), width - 28)}px">{formatTime(hoverTime)}</span>
  {/if}
</div>

<style>
  .scrubber {
    --played: var(--accent);
    --unplayed: color-mix(in srgb, var(--muted) 50%, transparent);
    position: relative;
    height: 48px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    cursor: pointer;
    touch-action: none;
    user-select: none;
  }

  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    border-radius: inherit;
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
    white-space: nowrap;
  }

  .band {
    position: absolute;
    top: 13px;
    bottom: 3px;
    min-width: 2px;
    border-radius: 3px;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border: 1px solid var(--accent);
    overflow: hidden;
  }

  .band.ghost {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
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
    top: 0;
    bottom: 0;
    width: 8px;
    cursor: ew-resize;
  }

  .edge.start {
    left: 0;
  }

  .edge.end {
    right: 0;
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

  /* A mark: a line between two frames, with a flag at the top to grab. */
  .cut {
    --cut: #f59e0b;
    position: absolute;
    top: 0;
    bottom: 0;
    width: 11px;
    margin-left: -5px;
    cursor: ew-resize;
    z-index: 1;
  }

  .cut::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 5px;
    width: 1px;
    background: var(--cut);
  }

  .cut::after {
    content: "";
    position: absolute;
    top: 0;
    left: 5px;
    width: 7px;
    height: 9px;
    border-radius: 0 2px 2px 0;
    background: var(--cut);
  }

  .cut.selected::before {
    width: 2px;
    left: 4.5px;
  }

  .cut.selected::after {
    box-shadow: 0 0 0 2px var(--surface);
    height: 11px;
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

  .hover {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: color-mix(in srgb, var(--text) 35%, transparent);
    pointer-events: none;
  }

  .tip {
    position: absolute;
    bottom: calc(100% + 4px);
    transform: translateX(-50%);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    background: var(--text);
    color: var(--surface);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    white-space: nowrap;
    z-index: 2;
  }
</style>
