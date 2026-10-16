<script lang="ts">
  // An edit (edits.svelte.ts), opened from its pill in a clip bar, in the
  // player's place and laid out like it: the picture (editPlayer.ts, its
  // pieces one after another), the timeline in the edit's own time with its
  // pieces' waveforms and the joins between them, and the same controls,
  // where Split cuts the piece at the playhead in two and Render makes the
  // video. Under them, the pieces in order (dragged to reorder), then the
  // edit's transcript, piece by piece: a click on a line strikes it out (the
  // edit jumps over it) or brings it back.

  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onDestroy, onMount, untrack } from "svelte";
  import { api, type EditPiece, type Line } from "./api";
  import { ask } from "./ask";
  import { keysForApp } from "./demo";
  import { DRAG_SLOP } from "./dragout";
  import { EditPlayer, type Segment } from "./editPlayer";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { DEFAULT_FPS, PEAKS_RATE, playedSpans } from "./media";
  import PlayerBar, { toggleMute } from "./PlayerBar.svelte";
  import { formatTime } from "./platform";
  import Scrubber from "./Scrubber.svelte";
  import Spinner from "./Spinner.svelte";

  let { backTo, onback }: { backTo?: string; onback: () => void } = $props();

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

  // Where each clip's file is served from (media.rs), known before it plays.
  let urls = $state.raw(new Map<number, string>());
  const asked = new Set<number>();
  $effect(() => {
    for (const piece of pieces) {
      const id = piece.clip.id;
      if (asked.has(id)) continue;
      asked.add(id);
      api.clipUrl(id).then((url) => (urls = new Map(urls).set(id, url)));
    }
  });

  // What plays: each piece's spans (its VOD's time), as stretches of its clip's file.
  type Played = Segment & { piece: EditPiece; vodFrom: number };
  const segments = $derived.by(() => {
    const out: Played[] = [];
    let at = 0;
    for (const piece of pieces) {
      const start = Math.max(piece.start, piece.clip.start);
      const end = Math.min(piece.end, piece.clip.end);
      for (const [a, b] of playedSpans(start, end, piece.struck)) {
        out.push({
          clipId: piece.clip.id,
          url: piece.clip.status === "ready" ? (urls.get(piece.clip.id) ?? null) : null,
          title: piece.clip.title,
          from: a - piece.clip.start,
          to: b - piece.clip.start,
          at,
          piece,
          vodFrom: a,
        });
        at += b - a;
      }
    }
    return out;
  });
  const total = $derived(segments.reduce((sum, s) => sum + (s.to - s.from), 0));
  // Where pieces meet, in the edit's time.
  const joins = $derived.by(() => {
    const out: number[] = [];
    segments.forEach((s, i) => i > 0 && s.piece.id !== segments[i - 1].piece.id && out.push(s.at));
    return out;
  });

  // The waveform: each segment's stretch of its VOD's, end to end.
  const peaksOf = new Map<number, Promise<Uint8Array>>();
  let waveform = $state.raw<Uint8Array>(new Uint8Array());
  let waveSeq = 0;
  $effect(() => {
    const segs = segments;
    const mine = ++waveSeq;
    Promise.all(
      segs.map((s) => {
        const vodId = s.piece.clip.vod_id;
        let got = peaksOf.get(vodId);
        if (!got) peaksOf.set(vodId, (got = api.peaks(vodId).catch(() => new Uint8Array())));
        return got.then((levels) => {
          const n = Math.max(0, Math.round((s.to - s.from) * PEAKS_RATE));
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
  let player = $state<EditPlayer | null>(null);
  let previewError = $state<string | null>(null);
  let now = $state(0);
  let playing = $state(false);
  let volume = $state(1);
  let muted = $state(false);
  let rate = $state(1);
  let fps = $state(DEFAULT_FPS);
  onMount(() => {
    player = new EditPlayer(host!, (message) => (previewError = message));
  });
  // The pieces' frame rate: their first VOD's, where it's known.
  let frameRate = $state<number | null>(null);
  $effect(() => {
    const vodId = pieces[0]?.clip.vod_id;
    if (vodId === undefined) return;
    api.frameRate(vodId).then(
      (f) => (frameRate = f),
      () => (frameRate = null),
    );
  });
  $effect(() => {
    const segs = segments;
    const f = frameRate;
    const p = player;
    untrack(() => p?.setSegments(segs, f));
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
    return `Piece ${n} of ${pieces.length} · ${atNow.piece.clip.title} · ${atNow.piece.vod_title}`;
  });

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
    if (key === " " || key === "k") player.togglePlay();
    else if (key === "m") toggleMute(player);
    else if (key === "f" || (key === "Escape" && fullscreen)) setFullscreen(key === "f" ? !fullscreen : false);
    else if (key === "," || key === ".") player.step(key === "," ? -1 : 1);
    else if (key in STEPS) {
      const step = e.shiftKey && key.startsWith("Arrow") ? Math.sign(STEPS[key]) : STEPS[key];
      player.seek(Math.max(0, player.time() + step), false);
    } else return;
    e.preventDefault();
  }

  // Reordering: a piece's pill dragged along the bar goes where it's let go;
  // a click goes to it.
  let bar = $state<HTMLDivElement>();
  let press: { piece: EditPiece; x: number; y: number } | null = null;
  let dragging = $state<{ id: number; to: number; x: number } | null>(null);
  function onPieceDown(e: PointerEvent, piece: EditPiece) {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".act")) return;
    press = { piece, x: e.clientX, y: e.clientY };
    e.preventDefault();
  }
  function onMove(e: PointerEvent) {
    if (!press || !bar) return;
    if (!dragging && Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_SLOP) return;
    const others = [...bar.querySelectorAll<HTMLElement>(".piece")].filter(
      (el) => Number(el.dataset.piece) !== press!.piece.id,
    );
    const to = others.findIndex((el) => {
      const box = el.getBoundingClientRect();
      return e.clientX < box.left + box.width / 2;
    });
    const at = to < 0 ? others.length : to;
    const ref = (others[at] ?? others[others.length - 1])?.getBoundingClientRect();
    const box = bar.getBoundingClientRect();
    const x = ref ? (others[at] ? ref.left - 4 : ref.right + 2) - box.left + bar.scrollLeft : 0;
    dragging = { id: press.piece.id, to: at, x };
  }
  function onUp() {
    const done = dragging;
    const pressed = press;
    press = null;
    dragging = null;
    if (done) {
      if (pieces.findIndex((p) => p.id === done.id) !== done.to) edits.move(done.id, done.to);
    } else if (pressed) {
      const s = segments.find((seg) => seg.piece.id === pressed.piece.id);
      if (s) seekTo(s.at);
    }
  }
  const pieceLength = (p: EditPiece) =>
    playedSpans(Math.max(p.start, p.clip.start), Math.min(p.end, p.clip.end), p.struck).reduce(
      (sum, [a, b]) => sum + b - a,
      0,
    );

  // The transcript: each piece's lines, within it.
  const linesOf = new Map<number, Promise<Line[]>>();
  let transcripts = $state.raw<Map<number, Line[]>>(new Map());
  $effect(() => {
    for (const vodId of new Set(pieces.map((p) => p.clip.vod_id))) {
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
    const all = transcripts.get(piece.clip.vod_id) ?? [];
    const from = Math.max(piece.start, piece.clip.start);
    const to = Math.min(piece.end, piece.clip.end);
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
      const end = s.vodFrom + (s.to - s.from);
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
      `Delete the edit “${edit.title}”${edit.file_path ? " and its video" : ""}? Its clips stay.`,
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

<svelte:window
  onkeydown={onKey}
  onpointermove={(e) => {
    onMove(e);
    onOutMove(e);
  }}
  onpointerup={() => {
    onUp();
    outPress = null;
  }}
/>

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
      {:else if edit.status === "ready" && edit.file_path}
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
          Empty for now. Drop clips on this edit's pill in a VOD's clip bar, or use a clip's “Add to an edit”.
        </p>
      {:else if previewError}
        <p class="hint">{previewError}</p>
      {/if}
    </div>

    <div class="controls">
      {#if total}
        <Scrubber {waveform} {joins} duration={total} time={now} {fps} onseek={seekTo} onpause={() => {}} />
      {/if}
      <PlayerBar
        {player}
        {playing}
        {now}
        duration={total}
        {volume}
        {muted}
        {rate}
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
            class="action render"
            onclick={() => edit && edits.render(edit.id)}
            disabled={!segments.length || edit?.status === "cutting"}
            title="Render the edit into one video, in Videos/VODscribe/Edits (E)"
          >
            <Icon name="film" size={14} />
            {edit?.status === "ready" ? "Render again" : "Render"} <kbd>E</kbd>
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

    <div class="pieces" bind:this={bar} class:dragging={!!dragging} aria-label="Pieces">
      {#each pieces as piece (piece.id)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="piece {piece.clip.status}"
          class:current={atNow?.piece.id === piece.id}
          class:lifted={dragging?.id === piece.id}
          data-piece={piece.id}
          onpointerdown={(e) => onPieceDown(e, piece)}
          title="{piece.clip.title} · {piece.vod_title} · drag to move it"
        >
          <span class="length">{formatTime(pieceLength(piece))}</span>
          <span class="name">{piece.clip.title}</span>
          {#if piece.clip.status === "cutting" || piece.clip.status === "waiting"}
            <Spinner size={11} />
          {/if}
          <button
            class="act"
            onclick={() => edits.removePiece(piece.id)}
            title="Take it out of the edit (the clip stays)"
            aria-label="Take it out of the edit"><Icon name="x" size={13} /></button
          >
        </div>
      {/each}
      {#if dragging}
        <span class="insert" style:left="{dragging.x}px"></span>
      {/if}
    </div>
  </section>

  <div class="lines">
    {#each pieces as piece, n (piece.id)}
      <p class="piece-head muted">
        <span class="num">{n + 1}</span>
        {piece.clip.title} · {piece.vod_title}
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

  .fullscreen .head,
  .fullscreen .pieces {
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

  .render :global(svg) {
    color: var(--accent);
  }

  /* The pieces, like the clip bar's pills. */
  .pieces {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 40px;
    padding: 6px 16px;
    overflow-x: auto;
    user-select: none;
    -webkit-user-select: none;
  }

  .piece {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 260px;
    height: 28px;
    padding: 0 2px 0 10px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface-2);
    cursor: grab;
    font-size: 12px;
  }

  .piece.current {
    border-color: var(--accent);
  }

  .piece.lifted {
    opacity: 0.45;
  }

  .piece.error {
    border-color: var(--danger);
  }

  .dragging .piece {
    cursor: grabbing;
  }

  .length {
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }

  .name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .act {
    display: flex;
    padding: 3px;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--muted);
  }

  .act:hover {
    color: var(--text);
  }

  .insert {
    position: absolute;
    top: 6px;
    bottom: 6px;
    width: 2px;
    background: var(--accent);
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
