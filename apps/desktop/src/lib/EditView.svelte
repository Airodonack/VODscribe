<script lang="ts">
  // An edit (edits.svelte.ts), opened from its row in the library's list, in the
  // player's place and laid out like it: the picture (previewPlayer.ts, its
  // pieces one after another), the timeline in the edit's own time with its
  // pieces' waveforms and the joins between them, and the same controls,
  // where Split cuts the piece at the playhead in two and Remove takes the
  // selected piece out (Delete, too); Render, at the top beside the
  // rendered video, makes it. Its pieces
  // are bands in the timeline's lane (Scrubber.svelte): clicked to select,
  // dragged to move, their edges dragged to resize, Alt-dragged to carry out. Under it, the edit's
  // transcript, piece by piece: a click on a line strikes it out (the edit
  // jumps over it) or brings it back.

  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onDestroy, onMount } from "svelte";
  import { api, events, type EditPiece, type Line, type PreviewSegment, type Vod } from "./api";
  import { ask } from "./ask";
  import { keysForApp } from "./demo";
  import { DRAG_SLOP, carry, type CarryHome } from "./dragout";
  import { playsHere } from "./embed";
  import { PreviewPlayer } from "./previewPlayer";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { DEFAULT_FPS, PEAKS_RATE, playedSpans } from "./media";
  import PlayerBar, { toggleMute } from "./PlayerBar.svelte";
  import { formatTime } from "./platform";
  import Scrubber, { type LanePiece } from "./Scrubber.svelte";
  import Spinner from "./Spinner.svelte";
  import { load, save } from "./stored";

  let {
    backTo,
    vods,
    onback,
  }: {
    backTo?: string;
    /** The library's VODs, for carrying a piece out (its ghost, its VOD). */
    vods: Map<number, Vod>;
    onback: () => void;
  } = $props();

  const edit = $derived(edits.open);
  const pieces = $derived(edits.pieces);

  // The name, written in place.
  let title = $state("");
  $effect(() => {
    title = edit?.title ?? "";
  });
  function rename() {
    if (edit && title.trim() && title.trim() !== edit.title) edits.rename(edit.id, title);
    else title = edit?.title ?? "";
  }

  // What plays: each piece's spans (its VOD's time), one after another.
  type Played = { piece: EditPiece; at: number; vodFrom: number; length: number };
  const segments = $derived.by(() => {
    const out: Played[] = [];
    let at = 0;
    for (const piece of pieces) {
      for (const [a, b] of playedSpans(piece.start, piece.end, piece.struck)) {
        out.push({ piece, at, vodFrom: a, length: b - a });
        at += b - a;
      }
    }
    return out;
  });
  const total = $derived(segments.reduce((sum, s) => sum + s.length, 0));

  // The waveform: each segment's stretch of its VOD's, end to end.
  const peaksOf = new Map<number, Promise<Uint8Array>>();
  let waveform = $state.raw<Uint8Array>(new Uint8Array());
  let waveSeq = 0;
  $effect(() => {
    const segs = segments;
    const mine = ++waveSeq;
    Promise.all(
      segs.map((s) => {
        const vodId = s.piece.vod_id;
        let got = peaksOf.get(vodId);
        if (!got) peaksOf.set(vodId, (got = api.peaks(vodId).catch(() => new Uint8Array())));
        return got.then((levels) => {
          const n = Math.max(0, Math.round(s.length * PEAKS_RATE));
          const first = Math.floor(s.vodFrom * PEAKS_RATE);
          const part = new Uint8Array(n);
          part.set(levels.subarray(first, first + n));
          return part;
        });
      }),
    ).then((parts) => {
      if (mine !== waveSeq) return;
      const all = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
      let at = 0;
      for (const p of parts) {
        all.set(p, at);
        at += p.length;
      }
      waveform = all.some((l) => l > 0) ? all : new Uint8Array();
    });
  });

  // The player, and what the controls show, read from it every frame.
  let host = $state<HTMLDivElement>();
  let player = $state<PreviewPlayer | null>(null);
  let previewError = $state<string | null>(null);
  let now = $state(0);
  let playing = $state(false);
  let volume = $state(1);
  let muted = $state(false);
  let rate = $state(1);
  let fps = $state(DEFAULT_FPS);
  // The preview's segments (previewPlayer.ts), where the webview can play them.
  onMount(() => {
    let gone = false;
    let stop: (() => void) | undefined;
    api.previewCodecs().then(
      (codecs) => {
        if (gone) return;
        const show = (message: string | null) => (previewError = message);
        if (PreviewPlayer.supported(codecs)) {
          const p = new PreviewPlayer(host!, codecs, show);
          player = p;
          events
            .previewMade(({ key, error }) => (error ? (previewError = error) : p.made(key)))
            .then((u) => (gone ? u() : (stop = u)));
        } else previewError = "This computer's web view can't play the preview. Render the edit to watch it.";
      },
      (e) => (edits.error = String(e)),
    );
    return () => {
      gone = true;
      stop?.();
    };
  });
  // How far the section downloading of each VOD can be used already.
  const usable = (vodId: number) => {
    const f = edits.fetching.get(vodId);
    return f ? (f.usable ?? f.from + f.progress * (f.to - f.from) - 1) : null;
  };
  // How many tens of seconds (preview segments' longest, crates/core
  // `preview::LONGEST`) of the sections downloading are in: each more may
  // make one more segment playable.
  const reached = $derived(
    (edit?.vod_ids ?? []).reduce((sum, vodId) => sum + Math.floor(Math.max(0, usable(vodId) ?? 0) / 10), 0),
  );
  // The minute the playhead is in: segments are made from its start on, so
  // there's at least a minute's ahead of the playhead.
  const minute = $derived(Math.floor(now / 60));
  // The note that the preview is lower quality than the render, until it's dismissed.
  const QUALITY_NOTE = "vodscribe.previewQualityNoted";
  let qualityNoted = $state(load(QUALITY_NOTE) === "1");
  function noteQuality() {
    qualityNoted = true;
    save(QUALITY_NOTE, "1");
  }

  // The preview's plan, asked for again whenever the pieces change, footage
  // of their VODs comes in, what's downloading reaches more of them, or the
  // playhead moves to another minute.
  let planSeq = 0;
  let planned = $state.raw<PreviewSegment[]>([]);
  $effect(() => {
    void segments;
    void reached;
    const from = minute * 60;
    for (const vodId of edit?.vod_ids ?? []) void edits.footage.get(vodId);
    const p = player;
    const id = edit?.id;
    if (!p || id === undefined) return;
    const mine = ++planSeq;
    api.previewPlan(id, from).then(
      (plan) => {
        if (mine !== planSeq) return;
        planned = plan;
        p.setPlan(plan);
      },
      (e) => (edits.error = String(e)),
    );
  });
  let poll = requestAnimationFrame(function read() {
    poll = requestAnimationFrame(read);
    const p = player;
    if (!p) return;
    now = p.time();
    playing = p.playing();
    fps = p.fps() ?? DEFAULT_FPS;
    [volume, muted] = [p.volume.get(), p.volume.muted()];
    rate = p.rate.get();
  });

  const seekTo = (time: number) => {
    now = time;
    player?.seek(time, false);
  };

  // The piece at the playhead.
  const atNow = $derived.by(() => {
    let found: Played | undefined;
    for (const s of segments) if (s.at <= now + 1e-6) found = s;
    return found;
  });
  const context = $derived.by(() => {
    if (!atNow) return "";
    const n = pieces.findIndex((p) => p.id === atNow.piece.id) + 1;
    return `Piece ${n} of ${pieces.length} · ${atNow.piece.title} · ${atNow.piece.vod_title}`;
  });

  /** Remove: the selected piece out of the edit. */
  function removeSelected() {
    if (selectedPiece === null) return;
    edits.removePiece(selectedPiece);
    selectedPiece = null;
  }

  /** Split: the piece at the playhead in two, there. */
  function split() {
    const s = atNow;
    if (!s) return;
    edits.split(s.piece.id, s.vodFrom + (now - s.at));
  }

  // Full screen is the window's, as for the VOD's player.
  let fullscreen = $state(false);
  function setFullscreen(on: boolean) {
    fullscreen = on;
    getCurrentWindow()
      .setFullscreen(on)
      .catch((e) => console.error("full screen", e));
  }

  // The keys the VOD's player has, where they mean the same; B splits, E renders.
  const STEPS: Record<string, number> = { ArrowLeft: -5, ArrowRight: 5, j: -10, l: 10 };
  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (e.altKey || !keysForApp() || !player) return;
    if (target?.closest("input, textarea, select, [contenteditable], [role=dialog]")) return;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (key === "b" || key === "e") {
      if (!e.repeat) {
        if (key === "b") split();
        else if (edit && edit.status !== "cutting" && segments.length) edits.render(edit.id);
      }
      e.preventDefault();
      return;
    }
    if (e.ctrlKey || e.metaKey) return;
    if ((key === "Delete" || key === "Backspace") && selectedPiece !== null) removeSelected();
    else if (key === "Escape" && selectedPiece !== null && !fullscreen) selectedPiece = null;
    else if (key === " " || key === "k") player.togglePlay();
    else if (key === "m") toggleMute(player);
    else if (key === "f" || (key === "Escape" && fullscreen)) setFullscreen(key === "f" ? !fullscreen : false);
    else if (key === "," || key === ".") player.step(key === "," ? -1 : 1);
    else if (key in STEPS) {
      const step = e.shiftKey && key.startsWith("Arrow") ? Math.sign(STEPS[key]) : STEPS[key];
      player.seek(Math.max(0, player.time() + step), false);
    } else return;
    e.preventDefault();
  }

  // The pieces as bands in the timeline's lane: what each says, where it is.
  let selectedPiece = $state<number | null>(null);
  const lane = $derived.by(() => {
    const out: LanePiece[] = [];
    let at = 0;
    for (const piece of pieces) {
      const length = pieceLength(piece);
      // Still downloading: how much of what it waits for is in.
      const waits = pending.filter((w) => w.pieceId === piece.id);
      const wanted = waits.reduce((sum, w) => sum + w.end - w.start, 0);
      const loading = wanted > 0 ? waits.reduce((sum, w) => sum + w.done, 0) / wanted : undefined;
      const range = { start: piece.start, end: piece.end, max: vods.get(piece.vod_id)?.duration ?? null };
      out.push({ id: piece.id, start: at, end: at + length, label: piece.title, loading, range });
      at += length;
    }
    return out;
  });

  // Where the edit waits for footage still to download (a VOD from a site):
  // each such stretch of it, and how far into it the download has come (a
  // section comes in from its start on).
  // (A stretch is planned as segments of up to 10 s; those next to each
  // other in a piece are one here.)
  const pending = $derived.by(() => {
    const out: { start: number; end: number; done: number; pieceId: number }[] = [];
    for (const seg of planned) {
      if (seg.key) continue;
      // The stretch it's of, and so where it is in the VOD.
      const s = segments.find((x) => x.piece.id === seg.piece_id && x.at <= seg.at + 1e-6 && seg.at < x.at + x.length);
      const vodFrom = s ? s.vodFrom + (seg.at - s.at) : 0;
      const f = s && edits.fetching.get(s.piece.vod_id);
      const upTo = s && usable(s.piece.vod_id);
      let done = 0;
      if (s && f && upTo != null && f.from <= vodFrom + 1e-3 && f.to >= vodFrom + seg.length - 1e-3)
        done = Math.max(0, Math.min(seg.length, upTo - vodFrom));
      const last = out[out.length - 1];
      if (last && last.pieceId === seg.piece_id && Math.abs(last.end - seg.at) < 1e-6) {
        last.end = seg.at + seg.length;
        last.done += done;
      } else out.push({ start: seg.at, end: seg.at + seg.length, done, pieceId: seg.piece_id });
    }
    return out;
  });

  // While a piece's edge is dragged: the frame it's on, over the player, and
  // what it is. One frame is asked for at a time, the latest edge's next.
  let edgeFrame = $state<{ url: string | null; label: string; missing?: boolean } | null>(null);
  let edgeWanted: { vodId: number; at: number; label: string } | null = null;
  let edgeAsking = false;
  function showEdge(id: number, side: "start" | "end", at: number | null) {
    const piece = pieces.find((p) => p.id === id);
    if (at === null || !piece) {
      edgeWanted = null;
      if (edgeFrame?.url) URL.revokeObjectURL(edgeFrame.url);
      edgeFrame = null;
      return;
    }
    // The last frame is the one just before the end.
    const frameAt = side === "start" ? at : Math.max(0, at - 1 / fps);
    const label = `New ${side === "start" ? "first" : "last"} frame · ${formatTime(at)}.${Math.floor((at % 1) * 10 + 1e-6)}`;
    edgeWanted = { vodId: piece.vod_id, at: frameAt, label };
    if (!edgeFrame) edgeFrame = { url: null, label };
    else edgeFrame.label = label;
    askEdge();
  }
  async function askEdge() {
    if (edgeAsking || !edgeWanted) return;
    edgeAsking = true;
    const want = edgeWanted;
    try {
      const bytes = await api.vodFrame(want.vodId, want.at, 960);
      if (!edgeWanted || !edgeFrame) return;
      const url = bytes.length ? URL.createObjectURL(new Blob([bytes as BlobPart], { type: "image/jpeg" })) : null;
      if (edgeFrame.url) URL.revokeObjectURL(edgeFrame.url);
      edgeFrame.url = url;
      edgeFrame.missing = !url;
    } catch (e) {
      console.error("edge frame", e);
    } finally {
      edgeAsking = false;
      if (edgeWanted && edgeWanted !== want) askEdge();
    }
  }

  const onpiece = {
    select: (id: number) => (selectedPiece = id),
    move: (id: number, index: number) => edits.move(id, index),
    resize: (id: number, start: number, end: number) => edits.trim(id, start, end),
    resizing: showEdge,
    carry(id: number, x: number, y: number, from: DOMRect, home: CarryHome) {
      const piece = pieces.find((p) => p.id === id);
      const vod = piece && vods.get(piece.vod_id);
      if (!piece || !vod) return;
      const { start, end, title } = piece;
      carry({ vod, start, end, title, ownPlayer: playsHere(vod), from, x, y, home });
    },
  };

  const pieceLength = (p: EditPiece) => playedSpans(p.start, p.end, p.struck).reduce((sum, [a, b]) => sum + b - a, 0);

  // The transcript: each piece's lines, within it.
  const linesOf = new Map<number, Promise<Line[]>>();
  let transcripts = $state.raw<Map<number, Line[]>>(new Map());
  $effect(() => {
    for (const vodId of new Set(pieces.map((p) => p.vod_id))) {
      if (linesOf.has(vodId)) continue;
      const got = api.transcript(vodId);
      linesOf.set(vodId, got);
      got.then(
        (lines) => (transcripts = new Map(transcripts).set(vodId, lines)),
        (e) => (edits.error = String(e)),
      );
    }
  });
  type PieceLine = { start: number; end: number; text: string };
  function linesIn(piece: EditPiece): PieceLine[] {
    const all = transcripts.get(piece.vod_id) ?? [];
    const { start: from, end: to } = piece;
    const out: PieceLine[] = [];
    all.forEach((l, i) => {
      const end = l.end ?? all[i + 1]?.start ?? l.start + 5;
      if (end > from && l.start < to)
        out.push({ start: Math.max(l.start, from), end: Math.min(end, to), text: l.text });
    });
    return out;
  }
  /** Whether `line` is struck out of `piece`: mostly covered by what's struck. */
  function isStruck(piece: EditPiece, line: PieceLine): boolean {
    const covered = piece.struck.reduce(
      (sum, [s, e]) => sum + Math.max(0, Math.min(line.end, e) - Math.max(line.start, s)),
      0,
    );
    return covered >= (line.end - line.start) / 2;
  }
  function toggleLine(piece: EditPiece, line: PieceLine) {
    const struck = isStruck(piece, line)
      ? piece.struck.filter(([s, e]) => !(Math.min(line.end, e) - Math.max(line.start, s) > 0))
      : [...piece.struck, [line.start, line.end] as [number, number]];
    edits.strike(piece.id, struck);
  }
  /** Where `line` of `piece` plays in the edit, if it isn't struck. */
  function editTimeOf(piece: EditPiece, line: PieceLine): number | null {
    for (const s of segments) {
      if (s.piece.id !== piece.id) continue;
      const end = s.vodFrom + s.length;
      if (line.start < end && line.end > s.vodFrom) return s.at + Math.max(0, line.start - s.vodFrom);
    }
    return null;
  }

  // The rendered video, carried out of the window into an editor or a folder.
  let outPress: { x: number; y: number } | null = null;
  function onOutMove(e: PointerEvent) {
    if (!outPress || !edit || Math.hypot(e.clientX - outPress.x, e.clientY - outPress.y) < DRAG_SLOP) return;
    outPress = null;
    api.dragEdit(edit.id).catch((err) => (edits.error = String(err)));
  }

  async function remove() {
    if (!edit) return;
    const sure = await ask(
      `Delete the edit “${edit.title}”${edit.file_path ? " and its video" : ""}?`,
      "Delete",
      "Keep",
    );
    if (sure) edits.remove(edit.id);
  }

  onDestroy(() => {
    cancelAnimationFrame(poll);
    if (fullscreen) setFullscreen(false);
    player?.destroy();
  });

  const progress = $derived(edit ? edits.progress.get(edit.id) : undefined);
</script>

<svelte:window onkeydown={onKey} onpointermove={onOutMove} onpointerup={() => (outPress = null)} />

{#if edit}
  <section class="edit-view" class:fullscreen>
    <div class="head">
      <button class="back link" onclick={onback} title="Back to {backTo ?? 'the VOD'}">
        <Icon name="chevron-down" size={14} />{backTo ?? "Back"}
      </button>
      <span class="sep muted">/</span>
      <span class="kind"><Icon name="film" size={14} /></span>
      <input
        class="title"
        bind:value={title}
        onblur={rename}
        onkeydown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        aria-label="Edit name"
      />
      <span class="grow"></span>
      {#if edit.status === "cutting"}
        <span class="rendering muted">
          <Spinner size={12} /> Rendering{progress ? ` ${Math.round(progress * 100)}%` : "…"}
        </span>
        <button onclick={() => edits.stop(edit.id)}>Stop</button>
      {:else}
        <!-- The edit's output: made here, primary while there's something new to make. -->
        {@const fresh = edit.status === "ready" && !!edit.file_path && edit.current}
        <button
          class="make"
          class:primary={!fresh}
          onclick={() => edits.render(edit.id)}
          disabled={!segments.length}
          title="Render the edit into one video, in Videos/VODscribe/Edits (E)"
        >
          {edit.status === "ready" ? "Render again" : "Render"} <kbd>E</kbd>
        </button>
      {/if}
      {#if edit.status === "ready" && edit.file_path}
        <button
          class="out"
          onpointerdown={(e) => e.button === 0 && (outPress = { x: e.clientX, y: e.clientY })}
          onclick={() => api.showEdit(edit.id).catch((err) => (edits.error = String(err)))}
          title="Drag it into a video editor or a folder, or click to show it in its folder"
        >
          <Icon name="film" size={14} />
          {edit.current ? "Rendered" : "Rendered (changed since)"}
        </button>
      {/if}
      <button class="icon" onclick={remove} title="Delete edit" aria-label="Delete edit">
        <Icon name="trash" size={16} />
      </button>
    </div>

    <div class="stage">
      <div class="frame" bind:this={host}></div>
      {#if !segments.length}
        <p class="hint">
          Empty for now. Carry clips onto this edit's row in the list on the left (drag a clip's pill, or Alt-drag a
          stretch of a VOD's timeline), or use a clip's “Add to an edit”. What's put in an edit is its own copy.
        </p>
      {:else if previewError}
        <p class="hint">{previewError}</p>
      {:else if !qualityNoted}
        <span
          class="quality"
          title="The preview is made quickly, at a lower quality, so the edit plays as you change it. The rendered video is full quality."
          >Preview · lower quality
          <button class="icon" onclick={noteQuality} title="Dismiss" aria-label="Dismiss"
            ><Icon name="x" size={11} /></button
          ></span
        >
      {/if}
      <!-- Over the player and its notes while an edge is dragged. -->
      {#if edgeFrame}
        <div class="edge-frame">
          {#if edgeFrame.url}<img src={edgeFrame.url} alt="" />{/if}
          <span>{edgeFrame.label}{edgeFrame.missing ? " · not downloaded yet; it's fetched once you let go" : ""}</span>
        </div>
      {/if}
    </div>

    <div class="controls">
      {#if total}
        <Scrubber
          {waveform}
          pieces={lane}
          {pending}
          {selectedPiece}
          {onpiece}
          duration={total}
          time={now}
          {fps}
          onseek={seekTo}
          onpause={() => player?.playing() && player.togglePlay()}
        />
      {/if}
      <PlayerBar
        {player}
        {playing}
        {now}
        duration={total}
        {volume}
        {muted}
        {rate}
        {fps}
        captionsAvailable={false}
        captionsOn={false}
        {fullscreen}
        canFullscreen
        ontoggleplay={() => player?.togglePlay()}
        onfullscreen={setFullscreen}
      >
        {#snippet actions()}
          <button
            class="action split"
            onclick={split}
            disabled={!atNow}
            title="Split the piece at the playhead in two (B)"
          >
            <Icon name="scissors" size={14} /> Split <kbd>B</kbd>
          </button>
          <button
            class="action remove"
            onclick={removeSelected}
            disabled={selectedPiece === null}
            title={selectedPiece === null
              ? "Select a piece (click it in the timeline) to take it out of the edit"
              : "Take the selected piece out of the edit; its VOD and clips aren't touched (Delete)"}
          >
            <Icon name="circle-minus" size={14} /> Remove <kbd>Del</kbd>
          </button>
          {#if edits.error}
            <span class="context error-text" title={edits.error}>{edits.error}</span>
          {:else if edit?.status === "error" && edit.error}
            <span class="context error-text" title={edit.error}>Couldn't render: {edit.error}</span>
          {:else}
            <span class="context muted" title={context}>{context}</span>
          {/if}
        {/snippet}
      </PlayerBar>
    </div>
  </section>

  <div class="lines">
    {#each pieces as piece, n (piece.id)}
      <p class="piece-head muted">
        <span class="num">{n + 1}</span>
        {piece.title} · {piece.vod_title}
      </p>
      {#each linesIn(piece) as line (line.start)}
        {@const struck = isStruck(piece, line)}
        {@const at = editTimeOf(piece, line)}
        <div class="line" class:struck>
          <button class="time-link" disabled={at === null} onclick={() => at !== null && player?.seek(at, true)}>
            {at === null ? "–" : formatTime(at)}
          </button>
          <button
            class="text"
            onclick={() => toggleLine(piece, line)}
            title={struck ? "Bring it back" : "Strike it out: the edit jumps over it"}>{line.text}</button
          >
        </div>
      {:else}
        <p class="muted small">No speech in this piece.</p>
      {/each}
    {/each}
  </div>
{/if}

<style>
  section {
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  section.fullscreen {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    border: none;
  }

  .fullscreen .head {
    display: none;
  }

  .fullscreen .stage {
    flex: 1;
    min-height: 0;
  }

  .fullscreen .frame {
    height: 100%;
    max-height: none;
    aspect-ratio: auto;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px 6px 16px;
    min-height: 40px;
  }

  .back {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 30%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 13px;
  }

  .back :global(svg) {
    transform: rotate(90deg);
  }

  .sep {
    font-size: 13px;
  }

  .kind {
    display: flex;
    color: var(--accent);
  }

  .title {
    font-weight: 600;
    font-size: 14px;
    border: 1px solid transparent;
    background: none;
    padding: 3px 6px;
    margin-left: -4px;
    min-width: 0;
    width: 240px;
  }

  .title:hover,
  .title:focus {
    border-color: var(--border);
    background: var(--surface);
  }

  .grow {
    flex: 1;
  }

  .rendering {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }

  .out {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: grab;
  }

  .stage {
    position: relative;
  }

  /* As large as the VOD's player. */
  .frame {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    max-height: var(--player-max-height, 44vh);
    background: #000;
  }

  /* The frame a dragged edge is on, over the player. */
  .edge-frame {
    position: absolute;
    inset: 0;
    background: #000;
    display: grid;
    place-items: center;
    overflow: hidden;
  }

  .edge-frame img {
    grid-area: 1 / 1;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .edge-frame span {
    grid-area: 1 / 1;
    align-self: start;
    margin-top: 12px;
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    background: rgb(0 0 0 / 0.7);
    color: #fff;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .quality {
    position: absolute;
    top: 8px;
    left: 8px;
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    background: rgb(0 0 0 / 0.55);
    color: rgb(255 255 255 / 0.85);
    font-size: 11px;
    cursor: default;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  /* Small, so the note stays one line high. */
  .quality button {
    width: 14px;
    height: 14px;
    display: grid;
    place-items: center;
    color: inherit;
    background: none;
  }

  .quality button.icon:hover:not(:disabled) {
    color: #fff;
    background: rgb(255 255 255 / 0.15);
  }

  .hint {
    position: absolute;
    inset: auto 24px 24px;
    margin: 0;
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    background: rgb(0 0 0 / 0.7);
    color: #fff;
    font-size: 13px;
    text-align: center;
  }

  /* The timeline over the controls, as for every player. */
  .controls {
    padding: 8px 16px 2px;
  }

  .split :global(svg) {
    color: var(--cut);
  }

  .make kbd {
    font: inherit;
    font-size: 10px;
    padding: 0 4px;
    border: 1px solid currentColor;
    border-radius: 3px;
    opacity: 0.7;
  }

  /* Taking a piece out: red, softened towards the muted text so it reads
     as removing something, not as an error. */
  .remove :global(svg) {
    color: color-mix(in srgb, var(--danger) 85%, var(--muted));
  }

  .lines {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 24px 24px;
  }

  .piece-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 14px 0 4px;
    font-size: 12px;
  }

  .num {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    border-radius: 9px;
    background: var(--surface-2);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .line {
    display: flex;
    gap: 12px;
    align-items: baseline;
  }

  .time-link {
    display: block;
    flex: none;
    min-width: 56px;
    padding: 3px 0;
    border: none;
    background: none;
    color: var(--accent);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .text {
    display: block;
    flex: 1;
    white-space: normal;
    padding: 3px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    line-height: 1.5;
  }

  .text:hover {
    background: var(--surface-2);
  }

  .struck .text {
    text-decoration: line-through;
    color: var(--muted);
  }

  .small {
    font-size: 12px;
  }
</style>
