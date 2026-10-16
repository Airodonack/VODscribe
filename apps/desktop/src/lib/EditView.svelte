<script lang="ts">
  // An edit (edits.svelte.ts), open in the main area: its pieces in order
  // (dragged to reorder), a preview that plays them one after another
  // jumping over struck lines, the selected piece's transcript (a click on a
  // line strikes it out or brings it back), and Render, which makes the video.

  import { onDestroy } from "svelte";
  import { api, type EditPiece, type Line } from "./api";
  import { ask } from "./ask";
  import { DRAG_SLOP } from "./dragout";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { playedSpans } from "./media";
  import { formatTime } from "./platform";
  import Spinner from "./Spinner.svelte";

  const edit = $derived(edits.open);
  const pieces = $derived(edits.pieces);

  // The title, written in place.
  let title = $state("");
  $effect(() => {
    title = edit?.title ?? "";
  });
  function rename() {
    if (edit && title.trim() && title.trim() !== edit.title) edits.rename(edit.id, title);
    else title = edit?.title ?? "";
  }

  // What plays: each piece's spans, in its clip's file (whose 0 is the clip's start).
  type Segment = { piece: EditPiece; from: number; to: number; at: number };
  const segments = $derived.by(() => {
    const out: Segment[] = [];
    let at = 0;
    for (const piece of pieces) {
      const start = Math.max(piece.start, piece.clip.start);
      const end = Math.min(piece.end, piece.clip.end);
      for (const [a, b] of playedSpans(start, end, piece.struck)) {
        out.push({ piece, from: a - piece.clip.start, to: b - piece.clip.start, at });
        at += b - a;
      }
    }
    return out;
  });
  const total = $derived(segments.reduce((sum, s) => sum + (s.to - s.from), 0));
  const pieceLength = (p: EditPiece) =>
    playedSpans(Math.max(p.start, p.clip.start), Math.min(p.end, p.clip.end), p.struck).reduce(
      (sum, [a, b]) => sum + b - a,
      0,
    );

  // The preview: one <video>, given each segment's clip in turn.
  let video = $state<HTMLVideoElement>();
  let index = $state(0);
  let position = $state(0);
  let playing = $state(false);
  let previewError = $state<string | null>(null);
  const urls = new Map<number, Promise<string>>();
  const urlOf = (clipId: number) => {
    let url = urls.get(clipId);
    if (!url) urls.set(clipId, (url = api.clipUrl(clipId)));
    return url;
  };
  let loaded: number | null = null;

  /** Shows segment `i`, `into` seconds in, playing if `play`. */
  async function show(i: number, into: number, play: boolean) {
    const seg = segments[i];
    if (!video || !seg) return;
    if (seg.piece.clip.status !== "ready") {
      previewError = `“${seg.piece.clip.title}” isn't made yet.`;
      video.pause();
      return;
    }
    previewError = null;
    index = i;
    if (loaded !== seg.piece.clip.id) {
      loaded = seg.piece.clip.id;
      video.src = await urlOf(seg.piece.clip.id);
    }
    video.currentTime = seg.from + into;
    position = seg.at + into;
    if (play) await video.play().catch(() => {});
  }

  let frame = 0;
  function follow() {
    cancelAnimationFrame(frame);
    const seg = segments[index];
    if (!video || !seg) return;
    const t = video.currentTime;
    position = seg.at + Math.max(0, t - seg.from);
    // Past this segment's end: on to the next (over what's struck), or done.
    if (!video.paused && t >= seg.to - 0.02) {
      if (index + 1 < segments.length) show(index + 1, 0, true);
      else {
        video.pause();
        position = total;
      }
    }
    if (!video.paused) frame = requestAnimationFrame(follow);
  }

  function togglePlay() {
    if (!video || !segments.length) return;
    if (!video.paused) return video.pause();
    if (position >= total - 0.05 || loaded === null) show(0, 0, true);
    else video.play().catch(() => {});
  }

  /** Goes to `time` seconds into the edit. */
  function seek(time: number) {
    const t = Math.max(0, Math.min(total, time));
    let i = 0;
    segments.forEach((s, k) => s.at <= t + 1e-6 && (i = k));
    if (segments[i]) show(i, Math.min(t - segments[i].at, segments[i].to - segments[i].from - 0.01), playing);
  }

  // A change to the pieces starts the preview over from the same place.
  $effect(() => {
    void segments;
    loaded = null;
  });

  let bar = $state<HTMLDivElement>();
  function onBarDown(e: PointerEvent) {
    if (!bar || !total) return;
    const box = bar.getBoundingClientRect();
    seek(((e.clientX - box.left) / box.width) * total);
  }

  // The piece selected: its transcript shows below.
  let selectedId = $state<number | null>(null);
  const selected = $derived(pieces.find((p) => p.id === selectedId) ?? pieces[0]);
  const linesOf = new Map<number, Promise<Line[]>>();
  let lines = $state.raw<Line[]>([]);
  $effect(() => {
    const piece = selected;
    if (!piece) return;
    const vodId = piece.clip.vod_id;
    let got = linesOf.get(vodId);
    if (!got) linesOf.set(vodId, (got = api.transcript(vodId)));
    got.then(
      (all) => {
        if (selected?.id !== piece.id) return;
        lines = all.filter((l, i) => {
          const end = l.end ?? all[i + 1]?.start ?? l.start + 5;
          return end > piece.clip.start && l.start < piece.clip.end;
        });
      },
      (e) => (edits.error = String(e)),
    );
  });
  const lineEnd = (i: number) => lines[i].end ?? lines[i + 1]?.start ?? lines[i].start + 5;
  /** Whether line `i` is struck out of `piece` (mostly covered by what's struck). */
  function isStruck(piece: EditPiece, i: number): boolean {
    // The part of the line within the clip.
    const [a, b] = [Math.max(lines[i].start, piece.clip.start), Math.min(lineEnd(i), piece.clip.end)];
    if (b <= a) return false;
    const covered = piece.struck.reduce((sum, [s, e]) => sum + Math.max(0, Math.min(b, e) - Math.max(a, s)), 0);
    return covered >= (b - a) / 2;
  }
  function toggleLine(piece: EditPiece, i: number) {
    const [a, b] = [Math.max(lines[i].start, piece.clip.start), Math.min(lineEnd(i), piece.clip.end)];
    const struck = isStruck(piece, i)
      ? piece.struck.filter(([s, e]) => !(Math.min(b, e) - Math.max(a, s) > 0))
      : [...piece.struck, [a, b] as [number, number]];
    edits.strike(piece.id, struck);
  }
  /** Plays the preview from line `i` of `piece`, if it isn't struck. */
  function playLine(piece: EditPiece, i: number) {
    const t = lines[i].start;
    const k = segments.findIndex((s) => s.piece.id === piece.id && t < s.to + piece.clip.start);
    if (k < 0) return;
    const seg = segments[k];
    show(k, Math.max(0, t - piece.clip.start - seg.from), true);
  }

  // Reordering: a piece dragged along the strip goes where it's let go.
  let strip = $state<HTMLDivElement>();
  let press: { piece: EditPiece; x: number; y: number } | null = null;
  let dragging = $state<{ id: number; to: number; x: number } | null>(null);
  function onPieceDown(e: PointerEvent, piece: EditPiece) {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".act")) return;
    press = { piece, x: e.clientX, y: e.clientY };
  }
  function onMove(e: PointerEvent) {
    if (!press || !strip) return;
    if (!dragging && Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_SLOP) return;
    // Where it would go: before the first other piece whose middle is right of the pointer.
    const others = [...strip.querySelectorAll<HTMLElement>(".piece")].filter(
      (el) => Number(el.dataset.piece) !== press!.piece.id,
    );
    const to = others.findIndex((el) => {
      const box = el.getBoundingClientRect();
      return e.clientX < box.left + box.width / 2;
    });
    const at = to < 0 ? others.length : to;
    const ref = others[at]?.getBoundingClientRect() ?? others[others.length - 1]?.getBoundingClientRect();
    const box = strip.getBoundingClientRect();
    const x = ref ? (others[at] ? ref.left : ref.right) - box.left + strip.scrollLeft : 0;
    dragging = { id: press.piece.id, to: at, x };
  }
  function onUp() {
    const done = dragging;
    const pressed = press;
    press = null;
    dragging = null;
    if (done && pressed) {
      const now = pieces.findIndex((p) => p.id === done.id);
      if (now !== done.to) edits.move(done.id, done.to);
    } else if (pressed) {
      selectedId = pressed.piece.id;
      const k = segments.findIndex((s) => s.piece.id === pressed.piece.id);
      if (k >= 0) show(k, 0, false);
    }
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

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (target?.closest("input, textarea, select, [contenteditable], [role=dialog]")) return;
    if (e.key === " " || e.key === "k") {
      togglePlay();
      e.preventDefault();
    }
  }

  onDestroy(() => {
    cancelAnimationFrame(frame);
    video?.removeAttribute("src");
    video?.load();
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
  <section class="edit-view">
    <div class="head">
      <input
        class="title"
        bind:value={title}
        onblur={rename}
        onkeydown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        aria-label="Edit name"
      />
      <span class="muted meta">{edit.pieces} piece{edit.pieces === 1 ? "" : "s"} · {formatTime(total)}</span>
      <span class="grow"></span>
      {#if edit.status === "cutting"}
        <span class="rendering muted">
          <Spinner size={12} /> Rendering{progress ? ` ${Math.round(progress * 100)}%` : "…"}
        </span>
        <button onclick={() => edits.stop(edit.id)}>Stop</button>
      {:else}
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
        <button class="primary" disabled={!segments.length} onclick={() => edits.render(edit.id)}>
          {edit.status === "ready" ? "Render again" : "Render"}
        </button>
      {/if}
      <button class="icon" onclick={remove} title="Delete edit" aria-label="Delete edit">
        <Icon name="trash" size={16} />
      </button>
    </div>
    {#if edit.status === "error" && edit.error}
      <p class="error-text pad">Couldn't render: {edit.error}</p>
    {/if}
    {#if edits.error}
      <p class="error-text pad">{edits.error}</p>
    {/if}

    <div class="preview">
      <!-- svelte-ignore a11y_media_has_caption -->
      <video
        bind:this={video}
        playsinline
        onplay={() => {
          playing = true;
          follow();
        }}
        onpause={() => {
          playing = false;
          follow();
        }}
        onseeked={follow}
        onclick={togglePlay}
      ></video>
      {#if !segments.length}
        <p class="hint muted">
          Drop clips here from the left: drag a clip's pill from under a player (or Alt-drag a stretch of its timeline)
          onto this edit in the list, or use a clip's “Add to edit”.
        </p>
      {:else if previewError}
        <p class="hint">{previewError}</p>
      {/if}
    </div>
    <div class="controls">
      <button class="icon" onclick={togglePlay} disabled={!segments.length} aria-label={playing ? "Pause" : "Play"}>
        <Icon name={playing ? "pause" : "play"} size={16} />
      </button>
      <span class="time">{formatTime(position)} / {formatTime(total)}</span>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="bar" bind:this={bar} onpointerdown={onBarDown}>
        {#each segments as s, i (i)}
          {#if s.at > 0}<span class="join" style:left="{(s.at / total) * 100}%"></span>{/if}
        {/each}
        <span class="played" style:width="{total ? (position / total) * 100 : 0}%"></span>
      </div>
    </div>

    <div class="strip" bind:this={strip} class:dragging={!!dragging}>
      {#each pieces as piece (piece.id)}
        {@const length = pieceLength(piece)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="piece {piece.clip.status}"
          class:selected={selected?.id === piece.id}
          class:lifted={dragging?.id === piece.id}
          data-piece={piece.id}
          style:flex-grow={Math.max(1, length)}
          onpointerdown={(e) => onPieceDown(e, piece)}
          title="{piece.clip.title} · {piece.vod_title}"
        >
          <span class="name">{piece.clip.title}</span>
          <span class="muted small">
            {formatTime(length)} · {piece.vod_title}
            {#if piece.clip.status === "cutting" || piece.clip.status === "waiting"}· being made{/if}
            {#if piece.clip.status === "error"}· <span class="bad">couldn't be made</span>{/if}
          </span>
          <button
            class="act"
            onclick={() => edits.removePiece(piece.id)}
            title="Take it out of the edit (the clip stays)"
            aria-label="Take it out of the edit"><Icon name="x" size={12} /></button
          >
        </div>
      {/each}
      {#if dragging}
        <span class="insert" style:left="{dragging.x}px"></span>
      {/if}
    </div>

    {#if selected}
      <div class="lines">
        <p class="muted small lines-head">
          {selected.clip.title} · {selected.vod_title} · click a line to strike it out, again to bring it back
        </p>
        {#each lines as line, i (line.start)}
          {@const struck = isStruck(selected, i)}
          <div class="line" class:struck>
            <button class="time-link" onclick={() => playLine(selected, i)} title="Play from here">
              {formatTime(line.start)}
            </button>
            <button
              class="text"
              onclick={() => toggleLine(selected, i)}
              title={struck ? "Bring it back" : "Strike it out"}>{line.text}</button
            >
          </div>
        {:else}
          <p class="muted small">No transcript in this clip.</p>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  .edit-view {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    overflow-y: auto;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
  }

  .title {
    font-weight: 600;
    font-size: 15px;
    border: 1px solid transparent;
    background: none;
    padding: 4px 6px;
    margin-left: -6px;
    min-width: 0;
    width: 260px;
  }

  .title:hover,
  .title:focus {
    border-color: var(--border);
    background: var(--surface);
  }

  .meta {
    font-size: 12px;
    white-space: nowrap;
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

  .pad {
    margin: 8px 16px 0;
  }

  .preview {
    position: relative;
    flex: none;
    width: 100%;
    height: min(44vh, 56vw);
    background: #000;
  }

  video {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
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

  .controls {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px;
  }

  .time {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .bar {
    position: relative;
    flex: 1;
    height: 8px;
    border-radius: 4px;
    background: var(--surface-2);
    cursor: pointer;
    overflow: hidden;
  }

  .played {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--accent);
    pointer-events: none;
  }

  .join {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--surface);
    z-index: 1;
    pointer-events: none;
  }

  .strip {
    position: relative;
    display: flex;
    gap: 6px;
    padding: 4px 16px 10px;
    overflow-x: auto;
    user-select: none;
    -webkit-user-select: none;
  }

  .piece {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex-basis: 0;
    min-width: 120px;
    padding: 6px 26px 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    cursor: grab;
  }

  .piece.selected {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .piece.lifted {
    opacity: 0.5;
  }

  .piece.error {
    border-color: var(--danger);
  }

  .dragging .piece {
    cursor: grabbing;
  }

  .name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 13px;
  }

  .small {
    font-size: 11px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .bad {
    color: var(--danger);
  }

  .act {
    position: absolute;
    top: 4px;
    right: 4px;
    display: flex;
    padding: 2px;
    border: none;
    background: none;
    color: var(--muted);
  }

  .insert {
    position: absolute;
    top: 4px;
    bottom: 10px;
    width: 2px;
    margin-left: -4px;
    background: var(--accent);
  }

  .lines {
    padding: 4px 16px 16px;
  }

  .lines-head {
    margin: 4px 0 8px;
  }

  .line {
    display: flex;
    gap: 12px;
    align-items: baseline;
  }

  .time-link {
    display: block;
    flex: none;
    min-width: 48px;
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
</style>
