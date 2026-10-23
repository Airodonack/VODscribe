<script lang="ts">
  // The timeline under every player: the whole VOD to start, zoomable to a
  // few seconds, with its waveform where there is one, its marks (cuts.svelte.ts)
  // and the clips made (an underline). A left click or drag only ever moves the
  // playhead; a mark's flag selects it and drags it (a frame at a time); an
  // Alt-drag (Option on a Mac) makes the stretch under the pointer a clip and
  // drags it out of the window (dragout.ts). Holding the middle button and
  // dragging pans (sideways) and zooms (up and down) about where it was
  // pressed; a middle click fits the whole VOD. The wheel zooms around the
  // pointer and Shift+wheel (or sideways) pans; zoomed in, it follows the
  // playhead until panned. A VOD from a site has its waveform only where its
  // audio was fetched to transcribe; elsewhere the bar is flat.
  //
  // Chapters (cuts.svelte.ts) show in a lane above it once there are any,
  // each from its start to the next one's; highlights are stars on its top
  // edge. Hovering one shows its title or note, a click goes to it and selects
  // it (to move like a cut), and a double click opens its title to write.
  //
  // An edit's timeline (EditView.svelte) is the same without any of a VOD's
  // marks or clips: its own time, its pieces' waveforms end to end (`waveform`)
  // and the joins between them. Its pieces are bands in a lane above it: a
  // click goes to one (the piece at the playhead is the one an edit's
  // Split and Remove act on, outlined), and dragging one (or Alt-dragging the
  // waveform under it) moves it, with a marker where it
  // would go; pulled well above or below the timeline, it's carried out
  // instead, as a stretch is carried from a VOD's. Dragging a band's edge
  // moves that end of the piece in its VOD, a frame at a time, to make it
  // shorter or longer: only that piece, the pieces past that edge moving
  // aside as it goes, and the player shows the frame the edge is on. Footage still downloading
  // (a VOD from a site) is darker until it's in, and its piece's band shows
  // a spinner and how much is.
  //
  // Hovering, scrubbing or dragging a selection's edge shows a thumbnail of
  // that moment beside the time tip (ScrubFrame.svelte), for what plays from
  // a file here: a selection's start shows its first frame, its end the
  // frame before it (the last one in), as the picture does while it's
  // dragged. Not for a finger, which has no hover.

  import { onMount, untrack } from "svelte";
  import { api, events, type Stretch, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import { cuts } from "./cuts.svelte";
  import {
    columns,
    edgeIndex,
    edgeShows,
    frameStart,
    inFrame as frameAfter,
    PEAKS_RATE,
    tickStep,
    waveformGaps,
  } from "./media";
  import { DRAG_SLOP, carry, stretchAt, type CarryHome } from "./dragout";
  import { formatFraction, formatTime, os, playsFromFile } from "./platform";
  import MarkEditor from "./MarkEditor.svelte";
  import Spinner from "./Spinner.svelte";
  import ScrubFrame from "./ScrubFrame.svelte";
  import { nearWithin, place } from "./scrubFrame";
  import { ExactFrame } from "./exactFrame.svelte";
  import type { Mark } from "./api";

  /** An edit's piece on its timeline (in the edit's time). */
  export interface LanePiece {
    id: number;
    start: number;
    end: number;
    label: string;
    /** While its footage downloads: how much is in, 0..1. */
    loading?: number;
    /** What it is of its VOD (start, end), and the VOD's length if known: for resizing. */
    range?: { start: number; end: number; max: number | null };
    /** Lit up: its VOD's source link is hovered. */
    lit?: boolean;
    /** Its video is gone for good: it plays as black and silence. */
    hole?: boolean;
    /** A group (a clip dropped in): double-click opens it. */
    group?: boolean;
    /** In the group open: its pieces show one by one. */
    inGroup?: boolean;
  }
  import { theme } from "./theme.svelte";
  import { nav } from "./nav.svelte";
  import type { Doc } from "./api";

  let {
    vod,
    memo,
    waveform,
    pieces,
    joins: splits,
    pending = [],
    selectedPiece = null,
    onpiece,
    duration,
    time,
    shown = null,
    fps,
    highlight = null,
    removing = null,
    picked = null,
    marks = false,
    ranges = [],
    current = null,
    ownPlayer = false,
    onseek,
    onpause,
  }: {
    /** The VOD shown, with its marks and clips; none for an edit. */
    vod?: Vod;
    /** What it's the timeline of, so its zoom is kept for going back to it (nav.svelte.ts). */
    memo?: Doc;
    /** An edit's waveform, given rather than fetched. */
    waveform?: Uint8Array;
    /** An edit's pieces, in its time, with what their bands say. */
    pieces?: LanePiece[];
    /** An edit's real splits, where what plays jumps (else where its pieces meet). */
    joins?: number[];
    /** Stretches of an edit whose footage isn't downloaded yet, and how much of each (from its start) is. */
    pending?: { start: number; end: number; done: number }[];
    /** The piece acted on (an edit's: the one at the playhead), outlined. */
    selectedPiece?: number | null;
    /** What's done with an edit's pieces in the lane. */
    onpiece?: {
      move: (id: number, index: number) => void;
      /** Makes it `start`..`end` of its VOD (an edge was dragged). */
      resize: (id: number, start: number, end: number) => void;
      /** While an edge is dragged: where it is in its VOD (null once it's let go). */
      resizing?: (id: number, side: "start" | "end", at: number | null) => void;
      /** Carries it out; `home` is the timeline, where coming back moves it again. */
      carry: (id: number, x: number, y: number, from: DOMRect, home: CarryHome) => void;
      /** Double-clicked: a group opens (or closes). */
      open?: (id: number) => void;
    };
    duration: number;
    /** Where the player is asked to be, or runs on to between frames. */
    time: number;
    /** Where the frame on screen is, where the player knows (the app's
     *  own): the playhead, solid. While `time` is apart from it (a seek still
     *  finding its frame, frames late), `time` shows faint. */
    shown?: number | null;
    /** The video's frame rate, for marks (assumed, for players that don't say). */
    fps: number;
    /** A stretch to light up: the one the Clip button would make a clip of, while it's hovered. */
    highlight?: { start: number; end: number } | null;
    /** A stretch to light up as going: the piece Remove would take out, while it's hovered. */
    removing?: { start: number; end: number } | null;
    /** An edit's lines selected in its transcript: the stretch Delete takes out. */
    picked?: { start: number; end: number } | null;
    /** An edit's chapters and highlights show (cuts.svelte.ts has the edit open). */
    marks?: boolean;
    /** Stretches lit up another way: what an edit plays of the VOD, while its link is hovered. */
    ranges?: { start: number; end: number }[];
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
  let fetched = $state.raw<Uint8Array>(new Uint8Array());
  const levels = $derived(waveform ?? fetched);
  // A VOD's marks and clips, and what's done with them; an edit has none.
  const marking = $derived(!!vod);
  // Chapters and highlights: a VOD's, or an edit's.
  const named = $derived(marking || marks);

  // Where a VOD from a site has no levels yet (media.ts `waveformGaps`), once
  // its stretches are known. A file, or a video kept here, has all of its
  // waveform read from that.
  let stretches = $state.raw<Stretch[] | null>(null);
  const gaps = $derived(vod && stretches && !playsFromFile(vod) ? waveformGaps(stretches, duration) : []);

  // Asked again as the VOD changes: a stretch's levels arrive once its audio is fetched.
  onMount(() => {
    if (!vod) return;
    const id = vod.id;
    let asked = 0;
    const load = () => {
      const mine = ++asked;
      api.peaks(id).then(
        (l) => mine === asked && (fetched = l),
        (e) => console.error("waveform", e),
      );
      api.transcriptStretches(id).then(
        (s) => mine === asked && (stretches = s),
        (e) => console.error("waveform", e),
      );
    };
    load();
    const unlisten = events.vod((v) => v.id === id && load());
    return () => void unlisten.then((u) => u());
  });

  // A VOD whose length was unknown (or wrong) until the player said, or an
  // edit's that changed: keep the view inside it, and all of it if it showed
  // all of it before (an edit grown, a stretch taken out brought back).
  let shownLength = 0;
  $effect(() => {
    void duration;
    untrack(() => {
      const whole = span >= shownLength - 1e-6 || span >= duration;
      shownLength = duration;
      setView(viewStart, whole ? duration : span);
    });
  });

  // Zoomed in, the view keeps the playhead in sight while it plays.
  $effect(() => {
    const now = time;
    untrack(() => {
      if (follow && span < duration && (now < viewStart || now > viewStart + span * 0.92)) setView(now - span * 0.08);
    });
  });

  // Its zoom kept as it changes, and brought back with the view (back and forward).
  const memoKey = $derived(memo ? `${memo.kind}:${memo.id}` : null);
  $effect(() => {
    void memoKey;
    const left = untrack(() => memo && nav.bringBack(memo)?.zoom);
    if (left) {
      follow = false;
      untrack(() => setView(left.start, left.span));
    }
  });
  $effect(() => {
    const zoom = { start: viewStart, span };
    if (memo) untrack(() => nav.note(memo, { zoom }));
  });

  function setView(start: number, length = span) {
    span = Math.min(duration, Math.max(Math.min(SHORTEST, duration), length));
    viewStart = Math.min(Math.max(0, start), Math.max(0, duration - span));
  }

  const x = (t: number) => ((t - viewStart) / span) * width;
  const pct = (t: number) => `${((t - viewStart) / span) * 100}%`;
  // The playhead where the picture really is, and where it's going while
  // that's apart from it by more than a few frames' worth.
  const actual = $derived(shown ?? time);
  const going = $derived(shown !== null && Math.abs(time - shown) > 0.1 ? time : null);
  const widthPct = (a: number, b: number) => `${((x(b) - x(a)) / (width || 1)) * 100}%`;

  let bar = $state<HTMLDivElement>();
  function timeAt(clientX: number): number {
    const box = bar!.getBoundingClientRect();
    return Math.min(duration, Math.max(0, viewStart + ((clientX - box.left) / box.width) * span));
  }

  // The waveform of the stretch shown: played (left of the playhead) in the
  // accent, the rest muted; a plain track where there's no waveform, or a
  // gap in it.
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
    const at = x(actual) * ratio;
    const mid = h / 2 + 6 * ratio;
    // The plain track from `from` to `to` (in pixels).
    const flat = (from: number, to: number) => {
      ctx.fillStyle = rest;
      ctx.fillRect(from, mid - 2 * ratio, to - from, 4 * ratio);
      ctx.fillStyle = played;
      ctx.fillRect(from, mid - 2 * ratio, Math.max(0, Math.min(to, at) - from), 4 * ratio);
    };
    if (!levels.length) return flat(0, w);
    // Only the stretch shown: the levels from its start, as many as it holds.
    const first = Math.floor(viewStart * PEAKS_RATE);
    const shown = levels.subarray(first, Math.ceil((viewStart + span) * PEAKS_RATE) + 1);
    const bars = columns(shown, span, Math.floor(width / 2));
    const seen = gaps.filter((g) => g.end > viewStart && g.start < viewStart + span);
    for (let i = 0; i < bars.length; i++) {
      // A column whose middle is in a gap is the gap's.
      const t = viewStart + ((i + 0.5) / bars.length) * span;
      if (seen.some((g) => t >= g.start && t < g.end)) continue;
      const px = i * 2 * ratio;
      const b = Math.max(ratio, bars[i] * (h - 16 * ratio));
      ctx.fillStyle = px < at ? played : rest;
      ctx.fillRect(px, mid - b / 2, ratio, b);
    }
    for (const g of seen) flat(Math.max(0, x(g.start) * ratio), Math.min(w, x(g.end) * ratio));
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
    | { kind: "piece"; id: number; x: number; dx: number; to: number; moved: boolean }
    | {
        kind: "resize";
        id: number;
        side: "start" | "end";
        x: number;
        /** The piece's range in its VOD when pressed, and as dragged. */
        from: { start: number; end: number; max: number | null };
        start: number;
        end: number;
        moved: boolean;
      }
    | { kind: "edge"; edge: "start" | "end"; at: number; x: number; moved: boolean }
    | { kind: "mark"; id: number; at: number; x: number; moved: boolean; away: boolean }
    | { kind: "view"; x: number; y: number; at: number; share: number; span: number; moved: boolean };
  let drag = $state<Drag | null>(null);
  let hover = $state<number | null>(null);

  // The stretch being taken out of the window, lit up while it's made.
  let taking = $state<{ start: number; end: number } | null>(null);

  /** The modifier that drags a stretch out: Alt (Option on a Mac). Ctrl stays free for selecting. */
  const dragsOut = (e: PointerEvent) => e.altKey;

  function onDown(e: PointerEvent) {
    if (e.button === 1) {
      const at = timeAt(e.clientX);
      drag = { kind: "view", x: e.clientX, y: e.clientY, at, share: (at - viewStart) / span, span, moved: false };
      e.preventDefault();
      return;
    }
    touch = e.pointerType === "touch";
    if (e.button !== 0) return;
    const onMark = named && !!(e.target as HTMLElement).closest("[data-mark], [data-chapter]");
    if (!marking && !onMark) {
      // Anywhere but a mark lets go of the one selected (as on a VOD's), so
      // Delete and Remove act on the piece or stretch, not on it.
      if (named) cuts.selected = null;
      // A band's edge: resizing that piece.
      const grip = (e.target as HTMLElement).closest<HTMLElement>("[data-grip]");
      const gripped =
        grip && pieces?.find((p) => p.id === Number(grip.closest<HTMLElement>("[data-piece]")?.dataset.piece));
      if (grip && gripped?.range) {
        e.preventDefault();
        const { start, end } = gripped.range;
        const side = grip.dataset.grip as "start" | "end";
        drag = { kind: "resize", id: gripped.id, side, x: e.clientX, from: gripped.range, start, end, moved: false };
        onpause();
        return;
      }
      const band = (e.target as HTMLElement).closest<HTMLElement>("[data-piece]");
      // The piece pressed on: its band, or where the waveform is.
      const piece = band
        ? pieces?.find((p) => p.id === Number(band.dataset.piece))
        : pieces?.find((p) => timeAt(e.clientX) >= p.start && timeAt(e.clientX) < p.end);
      e.preventDefault();
      // A band, or the piece anywhere with Alt: the same
      // drag, which moves it in the timeline and carries it out past it.
      if (piece && (band || dragsOut(e))) {
        const index = pieces!.indexOf(piece);
        drag = { kind: "piece", id: piece.id, x: e.clientX, dx: 0, to: index, moved: false };
        return;
      }
      drag = { kind: "scrub" };
      follow = true;
      onseek(timeAt(e.clientX));
      return;
    }
    if (marking && dragsOut(e)) {
      // With no cuts, the whole video.
      const range = stretchAt(timeAt(e.clientX), duration);
      if (range) {
        e.preventDefault();
        takeOut(range, e.clientX, e.clientY);
      }
      return;
    }
    const flag = (e.target as HTMLElement).closest<HTMLElement>("[data-mark]");
    const mark = flag && cuts.all.find((m) => m.id === Number(flag.dataset.mark));
    const chapter = (e.target as HTMLElement).closest<HTMLElement>("[data-chapter]");
    const band = chapter && cuts.all.find((m) => m.id === Number(chapter.dataset.chapter));
    const edge = (e.target as HTMLElement).closest<HTMLElement>("[data-edge]")?.dataset.edge as
      "start" | "end" | undefined;
    if (edge && cuts.selection) {
      // An edge of the selection: selected, to drag or move a frame at a time.
      drag = { kind: "edge", edge, at: cuts.selection[edge], x: e.clientX, moved: false };
      onpause();
      cuts.selectedEdge = edge;
      cuts.selected = null;
    } else if (band && !mark) {
      // A chapter's band: to its start, selected.
      cuts.selected = band.id;
      cuts.clearSelection();
      onseek(inFrame(band.start));
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

  /** Whether the pointer moving is a finger: a touch has no hover to share. */
  let touch = $state(false);

  function onMove(e: PointerEvent) {
    touch = e.pointerType === "touch";
    const over = (e.target as HTMLElement | null)?.closest?.<HTMLElement>("[data-mark], [data-chapter]");
    const id = over && root?.contains(over) ? Number(over.dataset.mark ?? over.dataset.chapter) : null;
    hoverMark = id === null ? null : (cuts.all.find((m) => m.id === id && m.kind !== "cut") ?? null);
    if (bar) {
      const box = bar.getBoundingClientRect();
      const inside = e.clientX >= box.left && e.clientX <= box.right && e.clientY >= box.top && e.clientY <= box.bottom;
      const top = (markLane ?? lane)?.getBoundingClientRect().top;
      const inLane = top !== undefined && e.clientY >= top && e.clientY < box.top;
      const across = e.clientX >= box.left && e.clientX <= box.right;
      hover = inside || (inLane && across) || drag ? Math.min(box.width, Math.max(0, e.clientX - box.left)) : null;
      // Resizing a piece: its own label says the times.
      if (drag?.kind === "resize") hover = null;
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
    if (drag.kind === "resize") {
      if (!drag.moved && Math.abs(e.clientX - drag.x) < DRAG_SLOP) return;
      drag.moved = true;
      // As far as the pointer went, in seconds of the view, on a frame.
      const by = ((e.clientX - drag.x) / bar!.getBoundingClientRect().width) * span;
      const { start, end, max } = drag.from;
      const onFrame = (t: number) => edgeIndex(t, fps) / fps;
      if (drag.side === "start") drag.start = Math.min(end - SHORTEST_PIECE, Math.max(0, onFrame(start + by)));
      else drag.end = Math.max(start + SHORTEST_PIECE, Math.min(max ?? Infinity, onFrame(end + by)));
      onpiece?.resizing?.(drag.id, drag.side, drag.side === "start" ? drag.start : drag.end);
      return;
    }
    const t = timeAt(e.clientX);
    if (drag.kind === "piece") {
      // Pulled well above or below the timeline: carried out instead.
      const box = root!.getBoundingClientRect();
      if (e.clientY < box.top - CARRY_OFF || e.clientY > box.bottom + CARRY_OFF) {
        const { id, x: pressX } = drag;
        const piece = pieces?.find((p) => p.id === id);
        drag = null;
        if (piece) {
          const bounds = bar!.getBoundingClientRect();
          const from = new DOMRect(
            bounds.left + Math.max(0, x(piece.start)),
            bounds.top,
            Math.max(4, Math.min(width, x(piece.end)) - Math.max(0, x(piece.start))),
            HEIGHT,
          );
          // Back over the timeline, the same press moves it again.
          const back = (cx: number) => {
            drag = { kind: "piece", id, x: pressX, dx: cx - pressX, to: 0, moved: true };
            drag.to = insertionAt(id, timeAt(cx));
          };
          onpiece?.carry(id, e.clientX, e.clientY, from, { el: root!, back });
        }
        return;
      }
      if (!drag.moved && Math.abs(e.clientX - drag.x) < DRAG_SLOP) return;
      drag.moved = true;
      drag.dx = e.clientX - drag.x;
      drag.to = insertionAt(drag.id, t);
      return;
    }
    if (drag.kind === "scrub") {
      onseek(t);
    } else if (drag.kind === "edge") {
      if (!drag.moved && Math.abs(e.clientX - drag.x) < DRAG_SLOP) return;
      drag.moved = true;
      cuts.setEdge(drag.edge, frameStart(t, fps));
      drag.edge = cuts.selectedEdge ?? drag.edge;
      // The picture shows what the edge keeps: for an end, the frame before it.
      onseek(inFrame(edgeShows(drag.edge, frameStart(t, fps), fps)));
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
      if (vod) await carry({ vod, start: range.start, end: range.end, ownPlayer, levels, from, x: px, y: py });
    } finally {
      taking = null;
    }
  }

  /** How far off the timeline a cut is dragged to remove it. */
  const AWAY = 60;
  /** Where piece `id` would go, dropped at `t`: before the first other piece whose middle is past it. */
  function insertionAt(id: number, t: number): number {
    const others = (pieces ?? []).filter((p) => p.id !== id);
    const to = others.findIndex((p) => t < (p.start + p.end) / 2);
    return to < 0 ? others.length : to;
  }

  /** How far above or below an edit's timeline a piece is pulled to carry it out. */
  const CARRY_OFF = 40;
  /** The shortest a piece is resized to, in seconds. */
  const SHORTEST_PIECE = 0.5;

  // While an edge is dragged, the pieces past it move aside by as much as it
  // moved (later ones for an end, earlier ones for a start), in pixels.
  const resized = $derived(drag?.kind === "resize" && drag.moved ? drag : null);
  function aside(id: number): number {
    const r = resized;
    if (!r || !pieces || !width) return 0;
    const at = pieces.findIndex((p) => p.id === r.id);
    const i = pieces.findIndex((p) => p.id === id);
    const perSecond = width / span;
    if (r.side === "end" && i > at) return (r.end - r.from.end) * perSecond;
    if (r.side === "start" && i < at) return (r.start - r.from.start) * perSecond;
    return 0;
  }

  /** A time in a VOD to a tenth of a second: 1:02.4. */
  const tenths = (t: number) => formatFraction(t, 1);

  const inFrame = (cut: number) => frameAfter(cut, fps, duration);

  function onUp() {
    const done = drag;
    drag = null;
    if (done?.kind === "resize") {
      onpiece?.resizing?.(done.id, done.side, null);
      if (done.moved && (done.start !== done.from.start || done.end !== done.from.end))
        onpiece?.resize(done.id, done.start, done.end);
    }
    if (done?.kind === "piece") {
      // A click on a band goes to the piece; a drag moves it.
      const piece = pieces?.find((p) => p.id === done.id);
      if (done.moved && pieces?.findIndex((p) => p.id === done.id) !== done.to) onpiece?.move(done.id, done.to);
      else if (!done.moved && piece) onseek(piece.start);
    }
    if (done?.kind === "view" && !done.moved) {
      follow = true;
      setView(0, duration);
    }
    if (done?.kind === "edge" && !done.moved) onseek(inFrame(edgeShows(done.edge, done.at, fps)));
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
  // Where an edit's pieces meet, and where one being moved would go.
  const joins = $derived(splits ?? (pieces ?? []).slice(1).map((p) => p.start));
  const insertAt = $derived.by(() => {
    if (drag?.kind !== "piece" || !drag.moved) return null;
    const id = drag.id;
    const others = (pieces ?? []).filter((p) => p.id !== id);
    return others[drag.to]?.start ?? others.at(-1)?.end ?? 0;
  });
  let hoverMark = $state<Mark | null>(null);

  // The moment the tip and the thumbnail show: the pointer's, or a
  // selection's edge being dragged (its end: the frame before it).
  const tipTime = $derived.by(() => {
    const s = cuts.selection;
    if (marking && drag?.kind === "edge" && drag.moved && s) return edgeShows(drag.edge, s[drag.edge], fps);
    return hoverTime;
  });
  // A thumbnail where the footage plays from a file here (a file, a kept
  // video, an edit's pieces), while hovering, scrubbing or dragging an edge.
  const framed = $derived(
    !!memo &&
      (vod ? ownPlayer : true) &&
      !touch &&
      !hoverMark &&
      (!drag || drag.kind === "scrub" || drag.kind === "edge"),
  );
  // Its exact frame, from the app's own player, once the pointer rests.
  const exact = new ExactFrame();
  $effect(() => {
    const doc = framed && hover !== null ? memo : undefined;
    const at = tipTime;
    untrack(() => exact.want(doc, at));
  });
  $effect(() => () => exact.clear());
  let frameWidth = $state(0);
  let tipWidth = $state(0);
  const beside = $derived(
    place(hover ?? 0, width, tipWidth, framed && hoverTime !== null && hover !== null ? frameWidth : 0),
  );

  // The moment under the pointer, marked on the transcript's minimap; and the
  // moment of the line or chat message under it there, marked here, a thin
  // line (cuts.svelte.ts `pointed`). Only on the VOD's or edit's own timeline.
  const mine = $derived(cuts.isOf(memo));
  $effect(() => {
    if (!mine || hoverTime === null || touch) return;
    cuts.pointed = { time: hoverTime, on: "timeline" };
    return () => {
      if (cuts.pointed?.on === "timeline") cuts.pointed = null;
    };
  });
  const pointedAt = $derived(mine && cuts.pointed?.on === "transcript" ? cuts.pointed.time : null);

  /** Where mark `m` is drawn: where it's being dragged to, while it is. */
  const placed = (m: Mark) => (drag?.kind === "mark" && drag.id === m.id ? drag.at : m.start);

  // Each chapter from its start to the next one's (or the end), as drawn.
  const chapterSpans = $derived.by(() => {
    const starts = cuts.chapters.map((c) => ({ chapter: c, start: placed(c) })).sort((a, b) => a.start - b.start);
    return starts.map((c, i) => ({ ...c, end: starts[i + 1]?.start ?? duration }));
  });
  const nowChapter = $derived(chapterSpans.findIndex((c) => time >= c.start && time < c.end));
  const label = (m: Mark) => m.text ?? (m.kind === "chapter" ? "Untitled chapter" : "Highlight");

  let root = $state<HTMLDivElement>();
  let lane = $state<HTMLDivElement>();
  let markLane = $state<HTMLDivElement>();
</script>

<svelte:window
  onpointermove={onMove}
  onpointerup={onUp}
  onpointercancel={() => {
    if (drag?.kind === "resize") onpiece?.resizing?.(drag.id, drag.side, null);
    drag = null;
  }}
/>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="timeline" bind:this={root}>
  {#if pieces?.length}
    <div
      class="chapters lane"
      class:resizing={!!resized}
      class:windows={os === "windows"}
      bind:this={lane}
      onpointerdown={onDown}
    >
      {#each pieces as piece (piece.id)}
        {#if piece.end > viewStart && piece.start < viewStart + span}
          {@const moving = drag?.kind === "piece" && drag.id === piece.id && drag.moved ? drag : null}
          {@const sizing = drag?.kind === "resize" && drag.id === piece.id && drag.moved ? drag : null}
          <!-- Being resized: its dragged edge where the pointer took it. -->
          {@const from = sizing ? piece.start + (sizing.start - sizing.from.start) : piece.start}
          {@const to = sizing ? piece.end + (sizing.end - sizing.from.end) : piece.end}
          <div
            class="chapter piece"
            class:current={time >= piece.start && time < piece.end}
            class:selected={selectedPiece === piece.id}
            class:lifted={!!moving}
            class:sizing={!!sizing}
            class:lit={piece.lit}
            class:hole={piece.hole}
            class:group={piece.group}
            class:in-group={piece.inGroup}
            data-piece={piece.id}
            ondblclick={() => onpiece?.open?.(piece.id)}
            style:left={pct(Math.max(from, viewStart))}
            style:width={widthPct(Math.max(from, viewStart), Math.min(to, viewStart + span))}
            style:transform={moving
              ? `translateX(${moving.dx}px)`
              : aside(piece.id)
                ? `translateX(${aside(piece.id)}px)`
                : null}
            title="{piece.label}{piece.hole
              ? ' · its video was deleted: it plays as black and silence'
              : ''}{piece.group ? ' · double-click to open it' : ''}{piece.inGroup
              ? ' · double-click to close its group'
              : ''} · drag to move it (pull it off the timeline to carry it out), drag an edge to resize it"
          >
            {#if piece.range}
              <span class="grip start" data-grip="start"></span>
              <span class="grip end" data-grip="end"></span>
            {/if}
            {#if sizing}
              <span class="sizes" class:at-end={sizing.side === "end"}
                >{tenths(sizing.start)} – {tenths(sizing.end)} · {(sizing.end - sizing.start).toFixed(1)} s</span
              >
            {/if}
            {#if piece.loading !== undefined}
              <span class="loading" title="Downloading its footage">
                <Spinner size={10} />{Math.round(piece.loading * 100)}%
              </span>
            {/if}
            <span class="label">{piece.label}</span>
          </div>
        {/if}
      {/each}
      {#if insertAt !== null}
        <span class="insert" style:left={pct(insertAt)}></span>
      {/if}
    </div>
  {/if}
  {#if named && chapterSpans.length}
    <div class="chapters" bind:this={markLane} onpointerdown={onDown}>
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
    class:resizing={!!resized}
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
    {#if named && current && current.end > viewStart && current.start < viewStart + span}
      <div
        class="between"
        style:left={pct(Math.max(current.start, viewStart))}
        style:width={widthPct(Math.max(current.start, viewStart), Math.min(current.end, viewStart + span))}
      ></div>
    {/if}
    {#if marking && cuts.selection && cuts.selection.end > viewStart && cuts.selection.start < viewStart + span}
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
    {#if !marking && picked && picked.end > viewStart && picked.start < viewStart + span}
      <div
        class="selection"
        style:left={pct(Math.max(picked.start, viewStart))}
        style:width={widthPct(Math.max(picked.start, viewStart), Math.min(picked.end, viewStart + span))}
        title="Selected: Delete takes it out"
      ></div>
    {/if}
    {#each pending as w, i (i)}
      {@const from = Math.max(w.start + w.done, viewStart)}
      {@const to = Math.min(w.end, viewStart + span)}
      {#if to > from}
        <!-- Not downloaded yet: darker, until the download reaches it. -->
        <div class="pending" style:left={pct(from)} style:width={widthPct(from, to)}></div>
      {/if}
    {/each}
    {#each ranges as r, i (i)}
      {#if r.end > viewStart && r.start < viewStart + span}
        <div
          class="lit used"
          style:left={pct(Math.max(r.start, viewStart))}
          style:width={widthPct(Math.max(r.start, viewStart), Math.min(r.end, viewStart + span))}
        ></div>
      {/if}
    {/each}
    {#each [highlight, taking, removing] as lit, i (i)}
      {#if lit && lit.end > viewStart && lit.start < viewStart + span}
        <div
          class="lit"
          class:removing={i === 2}
          style:left={pct(Math.max(lit.start, viewStart))}
          style:width={widthPct(Math.max(lit.start, viewStart), Math.min(lit.end, viewStart + span))}
        ></div>
      {/if}
    {/each}
    {#each marking ? clips.list : [] as clip (clip.id)}
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
    {#if going !== null && going >= viewStart && going <= viewStart + span}
      <span class="playhead going" style:left={pct(going)}></span>
    {/if}
    {#if actual >= viewStart && actual <= viewStart + span}
      <span class="playhead" style:left={pct(actual)}></span>
    {/if}
    {#each joins ?? [] as join (join)}
      {#if join > viewStart && join < viewStart + span}
        <span class="join" style:left={pct(join)}></span>
      {/if}
    {/each}
    {#each named ? cuts.list : [] as mark (mark.id)}
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
    {#each named ? cuts.highlights : [] as h (h.id)}
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
    {#if pointedAt !== null && pointedAt >= viewStart && pointedAt <= viewStart + span}
      <span class="pointed" style:left={pct(pointedAt)}></span>
    {/if}
  </div>
  {#if hoverTime !== null && hover !== null}
    {#if framed && memo && tipTime !== null}
      <ScrubFrame
        doc={memo}
        time={tipTime}
        within={nearWithin(span, width)}
        left={beside.frame ?? 0}
        dragging={!!drag}
        exact={exact.frame}
        bind:shown={frameWidth}
      />
    {/if}
    <span
      class="tip"
      class:wide={!!hoverMark}
      class:beside={beside.frame !== null}
      bind:offsetWidth={tipWidth}
      style:left={beside.frame !== null ? `${beside.tip}px` : `${Math.min(Math.max(hover, 28), width - 28)}px`}
      >{#if hoverMark}<strong>{label(hoverMark)}</strong> {formatTime(placed(hoverMark))}{:else}{formatTime(
          tipTime ?? hoverTime,
        )}{/if}</span
    >
  {/if}
</div>

<MarkEditor {root} fallback={bar} />

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

  /* An edit's pieces: bands to select and move. */
  .piece {
    cursor: grab;
    padding-right: 2px;
    box-shadow: inset 2px 0 0 var(--cut);
  }

  .piece.lit {
    background: var(--accent-soft);
    box-shadow:
      inset 2px 0 0 var(--cut),
      inset 0 0 0 1px var(--accent);
  }

  /* A group: a clip dropped in, shown as one. */
  .piece.group {
    box-shadow:
      inset 2px 0 0 var(--cut),
      inset 0 -3px 0 color-mix(in srgb, var(--accent) 45%, transparent);
  }

  /* The group open: its pieces, one by one. */
  .piece.in-group {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  /* Its video deleted: plays as black and silence. */
  .piece.hole {
    background: repeating-linear-gradient(
      -45deg,
      transparent 0 4px,
      color-mix(in srgb, var(--cut) 30%, transparent) 4px 8px
    );
  }

  .piece.lifted {
    z-index: 2;
    opacity: 0.85;
    cursor: grabbing;
    box-shadow:
      inset 2px 0 0 var(--cut),
      var(--shadow);
  }

  .piece .label {
    flex: 1;
    min-width: 0;
  }

  /* A band's edges, to resize it by. */
  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 6px;
    z-index: 1;
  }

  /* One-way cursors: the system's, which follow the user's cursor theme. */
  .grip.start {
    left: 0;
    cursor: w-resize;
  }

  .grip.end {
    right: 0;
    cursor: e-resize;
  }

  /* Windows has none (its w-resize and e-resize are two-way arrows), so
     they're drawn: an arrow from inside the piece pointing at its edge, |<-
     and ->|, white with a black outline like Windows' own pointer, and its
     size: at the top of a 32 px box, about 11 px tall, the hotspot on the
     bar's middle (Windows' IDC_SIZEWE: 23 x 9 at the top, hotspot 11,4). */
  .lane.windows .grip.start {
    cursor:
      url("data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='32'%20height='32'%3E%3Cg%20stroke='black'%20stroke-width='4'%20stroke-linecap='square'%3E%3Cpath%20d='M3%202V8'/%3E%3Cpath%20d='M10%205H18'/%3E%3C/g%3E%3Cpath%20d='M5.5%205L10.5%200.5V9.5Z'%20fill='white'%20stroke='black'/%3E%3Cg%20stroke='white'%20stroke-width='2'%20stroke-linecap='square'%3E%3Cpath%20d='M3%202V8'/%3E%3Cpath%20d='M10%205H18'/%3E%3C/g%3E%3C/svg%3E")
        3 5,
      w-resize;
  }

  .lane.windows .grip.end {
    cursor:
      url("data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='32'%20height='32'%3E%3Cg%20stroke='black'%20stroke-width='4'%20stroke-linecap='square'%3E%3Cpath%20d='M19%202V8'/%3E%3Cpath%20d='M4%205H12'/%3E%3C/g%3E%3Cpath%20d='M16.5%205L11.5%200.5V9.5Z'%20fill='white'%20stroke='black'/%3E%3Cg%20stroke='white'%20stroke-width='2'%20stroke-linecap='square'%3E%3Cpath%20d='M19%202V8'/%3E%3Cpath%20d='M4%205H12'/%3E%3C/g%3E%3C/svg%3E")
        19 5,
      e-resize;
  }

  .piece:hover .grip,
  .piece.sizing .grip {
    background: color-mix(in srgb, var(--cut) 45%, transparent);
  }

  .piece.sizing {
    z-index: 2;
    overflow: visible;
  }

  /* While resizing, pieces moved aside stay within the timeline's sides (the
     label above the band still shows). */
  .lane.resizing {
    clip-path: inset(-40px 0 0 0);
  }

  /* While resizing, the waveform is as it was until it's let go: dimmed. */
  .scrubber.resizing canvas {
    opacity: 0.35;
  }

  /* While resizing: the piece's new stretch of its VOD, above the band. */
  .sizes {
    position: absolute;
    bottom: calc(100% + 4px);
    left: 0;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    background: var(--text);
    color: var(--bg);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    pointer-events: none;
  }

  .sizes.at-end {
    left: auto;
    right: 0;
  }

  .lane .insert {
    position: absolute;
    top: -2px;
    bottom: -2px;
    width: 2px;
    margin-left: -1px;
    background: var(--accent);
    z-index: 3;
  }

  /* A chapter's start, to grab and move. */
  .chapter-start {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 9px;
    margin-left: -4px;
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

  /* Beside the thumbnail, rather than centred on the hover line. */
  .tip.beside {
    transform: none;
  }

  .tip.wide {
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tip strong {
    font-weight: 600;
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

  /* The stretch Clip would take, or being taken out. */
  .lit {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    box-shadow: inset 0 0 0 1px var(--accent);
    border-radius: 3px;
    pointer-events: none;
  }

  /* An edit's footage not downloaded yet. */
  .pending {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, #000 38%, transparent);
    pointer-events: none;
  }

  .piece .loading {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 3px;
    margin-right: 4px;
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  /* What an edit plays of the VOD, while its link is hovered. */
  .lit.removing {
    background: color-mix(in srgb, var(--danger) 18%, transparent);
    box-shadow: inset 0 0 0 1px var(--danger);
  }

  .lit.used {
    background: color-mix(in srgb, var(--chapter) 22%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--chapter) 80%, transparent);
    border-radius: 2px;
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

  /* Where an edit's pieces meet. */
  .join {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--cut);
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

  /* Where it's going, while the picture isn't there yet. */
  .playhead.going {
    opacity: 0.35;
  }

  .hover {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: color-mix(in srgb, var(--text) 35%, transparent);
    pointer-events: none;
  }

  /* The moment of the line or chat message the pointer is over in the transcript. */
  .pointed {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--accent);
    pointer-events: none;
  }

  /* Inside the timeline's top edge rather than above it: above, it would
     rise into the picture's box, where the app's own player draws over it. */
  .tip {
    position: absolute;
    top: 2px;
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
