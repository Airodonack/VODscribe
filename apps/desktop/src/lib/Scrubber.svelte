<script lang="ts">
  // The timeline under every player: the whole VOD to start, zoomable to a
  // few seconds, with its waveform where there is one, its marks (cuts.svelte.ts)
  // and the clips made (an underline). A left click or drag only ever moves the
  // playhead; a mark's flag selects it and drags it (a frame at a time); an
  // Alt-drag (Option on a Mac, also Ctrl on Linux, where desktops often take
  // Alt-drag to move windows) makes the stretch under the pointer a clip and
  // drags it out of the window (dragout.ts). Holding the middle button and
  // dragging pans (sideways) and zooms (up and down) about where it was
  // pressed; a middle click fits the whole VOD. The wheel zooms around the
  // pointer and Shift+wheel (or sideways) pans; zoomed in, it follows the
  // playhead until panned.
  //
  // Chapters (cuts.svelte.ts) show in a lane above it once there are any,
  // each from its start to the next one's; highlights are stars on its top
  // edge. Hovering one shows its title or note, a click goes to it and selects
  // it (to move like a cut), and a double click opens its title to write.

  import { onMount, untrack } from "svelte";
  import { api, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import { cuts } from "./cuts.svelte";
  import { columns, frameStart, inFrame as frameAfter, PEAKS_RATE, tickStep } from "./media";
  import { DRAG_SLOP, carry, stretchAt } from "./dragout";
  import { formatTime, os } from "./platform";
  import Popover from "./Popover.svelte";
  import type { Mark } from "./api";
  import { theme } from "./theme.svelte";

  let {
    vod,
    duration,
    time,
    fps,
    highlight = null,
    current = null,
    ownPlayer = false,
    onseek,
    onpause,
  }: {
    vod: Vod;
    duration: number;
    /** Where the player is now. */
    time: number;
    /** The video's frame rate, for marks (assumed, for players that don't say). */
    fps: number;
    /** A stretch to light up: the one the Export button would make a clip of, while it's hovered. */
    highlight?: { start: number; end: number } | null;
    /** Whether the VOD plays in the app's own player (its frames can be read). */
    ownPlayer?: boolean;
    /** The stretch between two cuts that the playhead is in, shaded. */
    current?: { start: number; end: number } | null;
    onseek: (time: number) => void;
    /** Pauses the player: a cut or an edge picked up is placed with the picture still. */
    onpause: () => void;
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

  // Pressing: scrubbing (the playhead follows), a mark's flag, or an Alt-drag
  // out. Followed on the window: the bar's contents changing under the
  // pointer mid-drag ended drags in WebKitGTK.
  type Drag =
    | { kind: "scrub" }
    | { kind: "edge"; edge: "start" | "end"; at: number; x: number; moved: boolean }
    | { kind: "mark"; id: number; at: number; x: number; moved: boolean; away: boolean }
    | { kind: "view"; x: number; y: number; at: number; share: number; span: number; moved: boolean };
  let drag = $state<Drag | null>(null);
  let hover = $state<number | null>(null);

  // The stretch being taken out of the window, lit up while it's made.
  let taking = $state<{ start: number; end: number } | null>(null);

  /** The modifier that drags a stretch out: Alt (Option), or Ctrl too on Linux. */
  const dragsOut = (e: PointerEvent) => e.altKey || (os === "linux" && e.ctrlKey);

  function onDown(e: PointerEvent) {
    if (e.button === 1) {
      const at = timeAt(e.clientX);
      drag = { kind: "view", x: e.clientX, y: e.clientY, at, share: (at - viewStart) / span, span, moved: false };
      e.preventDefault();
      return;
    }
    if (e.button !== 0) return;
    if (dragsOut(e)) {
      // With no cuts, there's only the whole VOD: not a clip.
      const range = cuts.list.length ? stretchAt(timeAt(e.clientX), duration) : undefined;
      if (range) {
        e.preventDefault();
        takeOut(range, e.clientX, e.clientY);
      }
      return;
    }
    const flag = (e.target as HTMLElement).closest<HTMLElement>("[data-mark]");
    const mark = flag && cuts.all.find((m) => m.id === Number(flag.dataset.mark));
    const chapter = (e.target as HTMLElement).closest<HTMLElement>("[data-chapter]");
    const named = chapter && cuts.all.find((m) => m.id === Number(chapter.dataset.chapter));
    const edge = (e.target as HTMLElement).closest<HTMLElement>("[data-edge]")?.dataset.edge as
      "start" | "end" | undefined;
    if (edge && cuts.selection) {
      // An edge of the selection: selected, to drag or move a frame at a time.
      drag = { kind: "edge", edge, at: cuts.selection[edge], x: e.clientX, moved: false };
      onpause();
      cuts.selectedEdge = edge;
      cuts.selected = null;
    } else if (named && !mark) {
      // A chapter's band: to its start, selected.
      cuts.selected = named.id;
      cuts.clearSelection();
      onseek(inFrame(named.start));
    } else if (mark) {
      drag = { kind: "mark", id: mark.id, at: mark.start, x: e.clientX, moved: false, away: false };
      onpause();
      cuts.selected = mark.id;
      cuts.clearSelection();
    } else {
      // Anywhere else lets go of the cut and the selection.
      drag = { kind: "scrub" };
      cuts.selected = null;
      cuts.clearSelection();
      follow = true;
      onseek(timeAt(e.clientX));
    }
    e.preventDefault();
  }

  function onMove(e: PointerEvent) {
    const over = (e.target as HTMLElement | null)?.closest?.<HTMLElement>("[data-mark], [data-chapter]");
    const id = over && root?.contains(over) ? Number(over.dataset.mark ?? over.dataset.chapter) : null;
    hoverMark = id === null ? null : (cuts.all.find((m) => m.id === id && m.kind !== "cut") ?? null);
    if (bar) {
      const box = bar.getBoundingClientRect();
      const inside = e.clientX >= box.left && e.clientX <= box.right && e.clientY >= box.top && e.clientY <= box.bottom;
      const inLane = !!lane && e.clientY >= lane.getBoundingClientRect().top && e.clientY < box.top;
      const across = e.clientX >= box.left && e.clientX <= box.right;
      hover = inside || (inLane && across) || drag ? Math.min(box.width, Math.max(0, e.clientX - box.left)) : null;
    }
    if (!drag) return;
    if (drag.kind === "view") {
      const dx = e.clientX - drag.x;
      const dy = e.clientY - drag.y;
      if (!drag.moved && Math.hypot(dx, dy) < DRAG_SLOP) return;
      drag.moved = true;
      follow = false;
      // Up zooms in, down out; the time grabbed stays under the pointer as it pans.
      const length = Math.min(duration, Math.max(SHORTEST, drag.span * Math.exp(dy / 120)));
      const box = bar!.getBoundingClientRect();
      const share = (e.clientX - box.left) / box.width;
      setView(drag.at - share * length, length);
      return;
    }
    const t = timeAt(e.clientX);
    if (drag.kind === "scrub") {
      onseek(t);
    } else if (drag.kind === "edge") {
      if (!drag.moved && Math.abs(e.clientX - drag.x) < DRAG_SLOP) return;
      drag.moved = true;
      cuts.setEdge(drag.edge, frameStart(t, fps));
      drag.edge = cuts.selectedEdge ?? drag.edge;
      onseek(inFrame(frameStart(t, fps)));
    } else if (drag.kind === "mark") {
      // Pulled well off the timeline, up or down: let go there, it's removed.
      const box = bar!.getBoundingClientRect();
      drag.away = e.clientY < box.top - AWAY || e.clientY > box.bottom + AWAY;
      if (drag.away) {
        drag.moved = true;
        return;
      }
      if (!drag.moved && Math.abs(e.clientX - drag.x) < DRAG_SLOP) return;
      drag.moved = true;
      drag.at = frameStart(t, fps);
      onseek(inFrame(drag.at));
    }
  }

  /** Carries `range` (dragout.ts): to the clip bar, or out of the window as a clip's file. */
  async function takeOut(range: { start: number; end: number }, px: number, py: number) {
    taking = range;
    const box = bar!.getBoundingClientRect();
    const from = new DOMRect(
      box.left + Math.max(0, x(range.start)),
      box.top,
      Math.max(4, Math.min(width, x(range.end)) - Math.max(0, x(range.start))),
      HEIGHT,
    );
    try {
      await carry({ vod, start: range.start, end: range.end, ownPlayer, levels, from, x: px, y: py });
    } finally {
      taking = null;
    }
  }

  /** How far off the timeline a cut is dragged to remove it. */
  const AWAY = 60;

  const inFrame = (cut: number) => frameAfter(cut, fps, duration);

  function onUp() {
    const done = drag;
    drag = null;
    if (done?.kind === "view" && !done.moved) {
      follow = true;
      setView(0, duration);
    }
    if (done?.kind === "edge" && !done.moved) onseek(inFrame(done.at));
    if (done?.kind === "mark") {
      // A click on a flag selects the mark and shows its frame; a drag moves it.
      if (done.away) cuts.remove(done.id);
      else if (done.moved) cuts.move(done.id, done.at);
      else onseek(inFrame(done.at));
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

  // Embedded players (iframes) would take the pointer mid-drag otherwise.
  $effect(() => {
    if (!drag) return;
    document.body.classList.add("timeline-drag");
    return () => document.body.classList.remove("timeline-drag");
  });

  const hoverTime = $derived(hover === null || !width ? null : viewStart + (hover / width) * span);
  let hoverMark = $state<Mark | null>(null);

  /** Where mark `m` is drawn: where it's being dragged to, while it is. */
  const placed = (m: Mark) => (drag?.kind === "mark" && drag.id === m.id ? drag.at : m.start);

  // Each chapter from its start to the next one's (or the end), as drawn.
  const chapterSpans = $derived.by(() => {
    const starts = cuts.chapters.map((c) => ({ chapter: c, start: placed(c) })).sort((a, b) => a.start - b.start);
    return starts.map((c, i) => ({ ...c, end: starts[i + 1]?.start ?? duration }));
  });
  const nowChapter = $derived(chapterSpans.findIndex((c) => time >= c.start && time < c.end));
  const label = (m: Mark) => m.text ?? (m.kind === "chapter" ? "Untitled chapter" : "Highlight");

  // The title (a chapter's) or note (a highlight's) being written: in a
  // panel by it, saved when it closes, kept as it was on Escape.
  let root = $state<HTMLDivElement>();
  let lane = $state<HTMLDivElement>();
  const editing = $derived(cuts.all.find((m) => m.id === cuts.editing) ?? null);
  let draft = $state("");
  let keep = true;
  let anchor = $state<HTMLElement>();
  $effect(() => {
    const m = editing;
    if (!m) return;
    untrack(() => {
      draft = m.text ?? "";
      keep = true;
    });
    // Once it's drawn (a chapter just added makes the lane).
    queueMicrotask(() => {
      anchor = root?.querySelector<HTMLElement>(`[data-mark="${m.id}"]`) ?? bar;
    });
  });
  function closeEditor() {
    if (editing && keep) cuts.setText(editing.id, draft);
    cuts.editing = null;
  }
  const focus = (el: HTMLInputElement) => {
    requestAnimationFrame(() => el.focus());
  };
</script>

<svelte:window onpointermove={onMove} onpointerup={onUp} onpointercancel={() => (drag = null)} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="timeline" bind:this={root}>
  {#if chapterSpans.length}
    <div class="chapters" bind:this={lane} onpointerdown={onDown}>
      {#each chapterSpans as { chapter, start, end }, i (chapter.id)}
        {#if end > viewStart && start < viewStart + span}
          <div
            class="chapter"
            class:current={i === nowChapter}
            class:selected={cuts.selected === chapter.id}
            data-chapter={chapter.id}
            style:left={pct(Math.max(start, viewStart))}
            style:width={widthPct(Math.max(start, viewStart), Math.min(end, viewStart + span))}
            ondblclick={() => (cuts.editing = chapter.id)}
          >
            <span class="label" class:untitled={!chapter.text}>{label(chapter)}</span>
          </div>
          {#if start >= viewStart}
            <span
              class="chapter-start"
              class:selected={cuts.selected === chapter.id}
              class:away={drag?.kind === "mark" && drag.id === chapter.id && drag.away}
              data-mark={chapter.id}
              style:left={pct(start)}
            ></span>
          {/if}
        {/if}
      {/each}
    </div>
  {/if}
  <div
    class="scrubber"
    class:panning={drag?.kind === "view"}
    data-view-start={viewStart}
    data-span={span}
    bind:this={bar}
    bind:clientWidth={width}
    onpointerdown={onDown}
    onmousedown={(e) => e.button === 1 && e.preventDefault()}
    onauxclick={(e) => e.button === 1 && e.preventDefault()}
    onpointerleave={() => !drag && (hover = null)}
    onwheel={onWheel}
  >
    <canvas bind:this={canvas} style:height="{HEIGHT}px"></canvas>
    {#each ticks as t (t)}
      <span class="tick" style:left={pct(t)}>{formatTime(t)}</span>
    {/each}
    {#if current && current.end > viewStart && current.start < viewStart + span}
      <div
        class="between"
        style:left={pct(Math.max(current.start, viewStart))}
        style:width={widthPct(Math.max(current.start, viewStart), Math.min(current.end, viewStart + span))}
      ></div>
    {/if}
    {#if cuts.selection && cuts.selection.end > viewStart && cuts.selection.start < viewStart + span}
      {@const picked = cuts.selection}
      <div
        class="selection"
        style:left={pct(Math.max(picked.start, viewStart))}
        style:width={widthPct(Math.max(picked.start, viewStart), Math.min(picked.end, viewStart + span))}
        title="Selected: Split (B) cuts both ends"
      ></div>
      {#each ["start", "end"] as const as edge (edge)}
        {#if picked[edge] >= viewStart && picked[edge] <= viewStart + span}
          <span
            class="cut pick"
            class:selected={cuts.selectedEdge === edge}
            data-edge={edge}
            style:left={pct(picked[edge])}
            title="The selection's {edge} · drag to move it"
            >{#if cuts.selectedEdge === edge}<span class="foot"></span>{/if}</span
          >
        {/if}
      {/each}
    {/if}
    {#each [highlight, taking] as lit, i (i)}
      {#if lit && lit.end > viewStart && lit.start < viewStart + span}
        <div
          class="lit"
          style:left={pct(Math.max(lit.start, viewStart))}
          style:width={widthPct(Math.max(lit.start, viewStart), Math.min(lit.end, viewStart + span))}
        ></div>
      {/if}
    {/each}
    {#each clips.list as clip (clip.id)}
      {#if clip.end > viewStart && clip.start < viewStart + span}
        <!-- A clip made: an underline along its stretch. -->
        <span
          class="made {clip.status}"
          style:left={pct(clip.start)}
          style:width={widthPct(clip.start, clip.end)}
          title="Clip: {clip.title}"
        ></span>
      {/if}
    {/each}
    {#if time >= viewStart && time <= viewStart + span}
      <span class="playhead" style:left={pct(time)}></span>
    {/if}
    {#each cuts.list as mark (mark.id)}
      {@const at = drag?.kind === "mark" && drag.id === mark.id ? drag.at : mark.start}
      {#if at >= viewStart && at <= viewStart + span}
        <span
          class="cut"
          class:selected={cuts.selected === mark.id}
          class:plain={!!cuts.selection}
          class:away={drag?.kind === "mark" && drag.id === mark.id && drag.away}
          data-mark={mark.id}
          style:left={pct(at)}
          title="Cut at {formatTime(at)} · drag to move it, or off the timeline to remove it"
        >
          {#if cuts.selected === mark.id}<span class="foot"></span>{/if}
        </span>
      {/if}
    {/each}
    {#each cuts.highlights as h (h.id)}
      {@const at = placed(h)}
      {#if at >= viewStart && at <= viewStart + span}
        <span
          class="star"
          class:selected={cuts.selected === h.id}
          class:away={drag?.kind === "mark" && drag.id === h.id && drag.away}
          data-mark={h.id}
          style:left={pct(at)}
          ondblclick={() => (cuts.editing = h.id)}
        ></span>
      {/if}
    {/each}
    {#if hoverTime !== null && hover !== null}
      <span class="hover" style:left="{hover}px"></span>
    {/if}
  </div>
  {#if hoverTime !== null && hover !== null}
    <span class="tip" class:wide={!!hoverMark} style:left="{Math.min(Math.max(hover, 28), width - 28)}px"
      >{#if hoverMark}<strong>{label(hoverMark)}</strong> {formatTime(placed(hoverMark))}{:else}{formatTime(
          hoverTime,
        )}{/if}</span
    >
  {/if}
</div>

<Popover
  bind:open={() => !!editing, (open) => !open && closeEditor()}
  {anchor}
  label={editing?.kind === "chapter" ? "Chapter title" : "Highlight note"}
  width={280}
  padding={10}
>
  {#if editing}
    <form
      class="mark-editor"
      onsubmit={(e) => {
        e.preventDefault();
        closeEditor();
      }}
    >
      <input
        use:focus
        bind:value={draft}
        maxlength="200"
        placeholder={editing.kind === "chapter" ? "Chapter title" : "What happens here (optional)"}
        onkeydown={(e) => e.key === "Escape" && (keep = false)}
      />
      <p class="muted">
        {editing.kind === "chapter" ? "Chapter" : "Highlight"} at {formatTime(editing.start)} · Enter saves
      </p>
    </form>
  {/if}
</Popover>

<style>
  .timeline {
    position: relative;
  }

  /* Chapters: a band each, from its start to the next one's. */
  .chapters {
    position: relative;
    height: 20px;
    margin-bottom: 3px;
    cursor: pointer;
    user-select: none;
    touch-action: none;
  }

  .chapter {
    position: absolute;
    top: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    padding: 0 6px 0 8px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--surface-2);
    box-shadow: inset 2px 0 0 var(--chapter);
    font-size: 11px;
    color: var(--muted);
  }

  .chapter.current {
    background: color-mix(in srgb, var(--chapter) 16%, var(--surface-2));
    color: var(--text);
  }

  .chapter.selected {
    box-shadow:
      inset 2px 0 0 var(--picked),
      inset 0 0 0 1px var(--picked);
  }

  .label {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .label.untitled {
    font-style: italic;
  }

  /* A chapter's start, to grab and move. */
  .chapter-start {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 9px;
    margin-left: -4px;
    cursor: ew-resize;
    z-index: 1;
  }

  .chapter-start.away {
    opacity: 0.35;
  }

  /* A highlight: a star on the timeline's top edge. */
  .star {
    position: absolute;
    top: -6px;
    width: 13px;
    height: 13px;
    margin-left: -6.5px;
    background: var(--highlight);
    clip-path: polygon(50% 0, 61% 35%, 98% 35%, 68% 57%, 79% 91%, 50% 70%, 21% 91%, 32% 57%, 2% 35%, 39% 35%);
    cursor: ew-resize;
    z-index: 3;
  }

  .star.selected {
    background: var(--picked);
  }

  .star.away {
    opacity: 0.35;
  }

  .tip.wide {
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tip strong {
    font-weight: 600;
  }

  .mark-editor {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .mark-editor p {
    margin: 0;
    font-size: 12px;
  }

  .scrubber.panning {
    cursor: grabbing;
  }

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

  /* The stretch Export would take, or being taken out. */
  .lit {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    box-shadow: inset 0 0 0 1px var(--accent);
    border-radius: 3px;
    pointer-events: none;
  }

  .made {
    position: absolute;
    bottom: 1px;
    height: 3px;
    min-width: 2px;
    border-radius: 2px;
    background: var(--accent);
    pointer-events: none;
  }

  .made.waiting,
  .made.cutting {
    opacity: 0.5;
  }

  .made.error {
    background: var(--danger);
  }

  /* A cut: a line between two frames, with a notch at the top to grab. */
  .cut {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 13px;
    margin-left: -6px;
    cursor: ew-resize;
    z-index: 1;
    --color: var(--cut);
  }

  .cut::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 6px;
    width: 1px;
    background: var(--color);
  }

  .cut::after {
    content: "";
    position: absolute;
    top: 0;
    left: 1px;
    border-left: 5.5px solid transparent;
    border-right: 5.5px solid transparent;
    border-top: 8px solid var(--color);
  }

  /* A selection's edges: its color (a notch at the bottom too once selected). */
  .cut.pick {
    --color: var(--pick);
  }

  /* Selected (a cut, or a selection's edge): its own color, and a notch at
     the bottom, so it's told apart by shape as well as color. */
  .cut.selected {
    --color: var(--picked);
    z-index: 2;
  }

  /* While there's a selection, cuts are plain lines: its edges are the ones with notches. */
  .cut.plain:not(.selected)::after {
    display: none;
  }

  /* Dragged off the timeline: removed when let go. */
  .cut.away {
    opacity: 0.35;
  }

  .cut.away::before {
    background: repeating-linear-gradient(var(--color) 0 4px, transparent 4px 7px);
  }

  .foot {
    position: absolute;
    bottom: 0;
    left: 1px;
    border-left: 5.5px solid transparent;
    border-right: 5.5px solid transparent;
    border-bottom: 8px solid var(--color);
  }

  /* The stretch between two cuts the playhead is in. */
  .between {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--cut) 16%, transparent);
    pointer-events: none;
  }

  /* A stretch selected in the transcript, waiting for Clip. */
  .selection {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--pick) 20%, transparent);
    pointer-events: none;
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
