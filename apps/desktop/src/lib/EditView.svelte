<script lang="ts">
  // An edit (edits.svelte.ts), opened from its row in the library's list, in the
  // player's place and laid out like it: the picture (editPlayer.ts, the app's
  // own player playing its pieces one after another), the timeline in the edit's own time with its
  // pieces' waveforms and the joins between them, and the same controls,
  // where Split cuts the piece at the playhead in two and Remove takes it
  // out (Delete, too), the playhead's piece being the one acted on; Render, at the top beside the
  // rendered video, makes it. Its pieces
  // are bands in the timeline's lane (Scrubber.svelte): clicked to go to,
  // dragged to move (pulled off the timeline, carried out), their edges dragged to resize. Under it, the edit's
  // transcript, piece by piece: a click on a line strikes it out (the edit
  // jumps over it) or brings it back.

  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onDestroy, onMount, untrack } from "svelte";
  import { api, type Cue, type Doc, type EditPiece, type PlayerStretch, type Vod } from "./api";
  import { ask } from "./ask";
  import { keysForApp } from "./demo";
  import { editShortcuts, keyOf, transportKey } from "./playerKeys";
  import Shortcuts from "./Shortcuts.svelte";
  import { DRAG_SLOP, carry, type CarryHome } from "./dragout";
  import { playsHere } from "./embed";
  import { EditPlayer, type EditPlayback } from "./editPlayer";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { DEFAULT_FPS, PEAKS_RATE, cueAt, playedSpans } from "./media";
  import { prefs } from "./playerPrefs.svelte";
  import PlayerBar, { toggleMute } from "./PlayerBar.svelte";
  import { formatFraction, os } from "./platform";
  import Scrubber, { type LanePiece } from "./Scrubber.svelte";
  import Spinner from "./Spinner.svelte";
  import { toast } from "./toasts.svelte";
  import { nav } from "./nav.svelte";
  import * as overlay from "./screenOverlay";

  let {
    vods,
    onopenvod,
  }: {
    /** The library's VODs, for carrying a piece out (its ghost, its VOD). */
    vods: Map<number, Vod>;
    /** Opens VOD `vodId` with the playhead at `at` (a source link was followed). */
    onopenvod: (vodId: number, at: number) => void;
  } = $props();

  const edit = $derived(edits.open);
  const pieces = $derived(edits.pieces);

  // The name, written in place: put back when the saved name changes, not
  // whenever the edit's news comes (a render's), which would undo typing.
  let title = $state("");
  // (Keyed by the edit too: another edit of the same name gets its own.)
  const saved = $derived(`${edit?.id ?? ""}\u0000${edit?.title ?? ""}`);
  $effect(() => {
    void saved;
    title = untrack(() => edit?.title ?? "");
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

  // Captions, on the edit's timeline, from its pieces' transcripts: asked
  // for again at each change to the edit.
  let cues = $state.raw<Cue[]>([]);
  $effect(() => {
    const id = edit?.id;
    void edit?.revision;
    if (id === undefined) return;
    let stale = false;
    api.editCaptions(id).then(
      (c) => !stale && (cues = c),
      (e) => console.error("captions", e),
    );
    return () => (stale = true);
  });

  // The player, and what the controls show, read from it every frame.
  let host = $state<HTMLDivElement>();
  let player = $state<EditPlayback | null>(null);
  let playError = $state<string | null>(null);
  let now = $state(0);
  // Where the frame on screen is, where the player knows (the app's own).
  let shown = $state<number | null>(null);
  let playing = $state(false);
  let volume = $state(1);
  let muted = $state(false);
  let rate = $state(1);
  let fps = $state(DEFAULT_FPS);
  const cue = $derived(prefs.captions ? cueAt(cues, now) : null);
  // The app's own player (editPlayer.ts), drawing on the window's screen;
  // where the screen couldn't be put in the window, the edit can't play here.
  onMount(() => {
    let gone = false;
    const show = (message: string | null) => (playError = message);
    api.playerAvailable().then(
      (native) => {
        if (gone) return;
        if (native) player = new EditPlayer(host!, show);
        else playError = "Edits can't play in this window. Render the edit to watch it.";
      },
      (e) => (edits.error = String(e)),
    );
    return () => {
      gone = true;
    };
  });
  // How far the section downloading of each VOD can be used already.
  const usable = (vodId: number) => {
    const f = edits.fetching.get(vodId);
    return f ? (f.usable ?? f.from + f.progress * (f.to - f.from) - 1) : null;
  };
  // How many tens of seconds of the sections downloading are in: each ten
  // more, the edit is planned again, so the player plays further into them.
  const reached = $derived(
    (edit?.vod_ids ?? []).reduce((sum, vodId) => sum + Math.floor(Math.max(0, usable(vodId) ?? 0) / 10), 0),
  );
  const EMPTY =
    `Empty for now. Carry clips onto this edit's row in the list on the left (drag a clip's pill, or ${os === "mac" ? "Option" : "Alt"}-drag a ` +
    "stretch of a VOD's timeline), or use a clip's “Add to an edit”. What's put in an edit is its own copy.";

  // The player's plan, asked for again whenever the pieces change, footage
  // of their VODs comes in, or what's downloading reaches more of them.
  let planSeq = 0;
  let planned = $state.raw<PlayerStretch[]>([]);
  $effect(() => {
    void segments;
    void reached;
    // Where it opens, the first time (it plays on from wherever it is after).
    const from = untrack(() => now);
    for (const vodId of edit?.vod_ids ?? []) void edits.footage.get(vodId);
    const p = player;
    const id = edit?.id;
    if (!p || id === undefined) return;
    const mine = ++planSeq;
    p.plan(id, from).then(
      (plan) => {
        if (mine !== planSeq) return;
        planned = plan;
        // Where its playhead was left (or its start), once there's a plan to seek in.
        if (bringTime !== null && plan.length) {
          p.seek(bringTime, false);
          now = bringTime;
          bringTime = null;
          playsEdit = id;
        }
      },
      (e) => (edits.error = String(e)),
    );
  });
  // Another edit opened in this view: the one playing pauses, and the new one
  // starts where its playhead was left this session (nav.svelte.ts), paused.
  let bringTime: number | null = null;
  /** The edit the player's playhead is of: none while it still shows the one before. */
  let playsEdit: number | null = null;
  // Only the edit's id: the edit itself is replaced each time it's loaded again.
  const editId = $derived(edit?.id);
  $effect(() => {
    const id = editId;
    if (id === undefined) return;
    untrack(() => player?.playing() && player.togglePlay());
    playsEdit = null;
    const doc = { kind: "edit" as const, id };
    const left = untrack(() => nav.bringBack(doc));
    // (An edit not seen yet starts at its start, not where the last one was.)
    bringTime = left?.time ?? untrack(() => nav.lastSeen(doc))?.time ?? 0;
  });
  let poll = requestAnimationFrame(function read() {
    poll = requestAnimationFrame(read);
    const p = player;
    if (!p) return;
    now = p.time();
    shown = p.shown?.() ?? null;
    // Only once it's this edit's playhead (after the seek to where it was left).
    if (edit && playsEdit === edit.id) nav.note({ kind: "edit", id: edit.id }, { time: now });
    playing = p.playing();
    fps = p.fps() ?? DEFAULT_FPS;
    [volume, muted] = [p.volume.get(), p.volume.muted()];
    rate = p.rate?.get() ?? 1;
  });

  const seekTo = (time: number) => {
    now = time;
    // Still waiting to seek to where it was left (no plan yet to seek in):
    // this is where it goes instead, and it's what's remembered meanwhile.
    if (bringTime !== null && edit) {
      bringTime = time;
      nav.note({ kind: "edit", id: edit.id }, { time });
      return;
    }
    player?.seek(time, false);
  };

  /** The group opened (double-clicked): its pieces show, move and go one by
   *  one, among themselves. */
  let openGroup = $state<number | null>(null);
  // Gone (removed, undone): closed.
  $effect(() => {
    if (openGroup !== null && !pieces.some((p) => p.group?.id === openGroup)) openGroup = null;
  });
  /** What `piece` moves, shows and goes as: the group it's in (a clip
   *  dropped into the edit), unless that's open, or itself. */
  const unitOf = (piece: EditPiece) => (piece.group && piece.group.id !== openGroup ? piece.group.id : piece.id);
  /** The pieces `piece` moves among: its open group's, or the edit's bands. */
  const unitsBeside = (piece: EditPiece) =>
    piece.group && piece.group.id === openGroup
      ? pieces.filter((p) => p.group?.id === openGroup).map((p) => p.id)
      : [...new Set(pieces.map(unitOf))];
  /** What `piece` is called: its own title, or its group's. */
  const nameOf = (piece: EditPiece) => piece.title || piece.group?.title || "";

  // The piece at the playhead.
  const atNow = $derived.by(() => {
    let found: Played | undefined;
    for (const s of segments) if (s.at <= now + 1e-6) found = s;
    return found;
  });
  const context = $derived.by(() => {
    if (!atNow) return "";
    const n = pieces.findIndex((p) => p.id === atNow.piece.id) + 1;
    const group = atNow.piece.group;
    const where = group ? ` · in “${group.title}”${group.id === openGroup ? " (open: Escape closes it)" : ""}` : "";
    const hole = atNow.piece.hole ? " · its video was deleted: it plays as black and silence" : "";
    return `Piece ${n} of ${pieces.length} · ${nameOf(atNow.piece)} · ${atNow.piece.vod_title}${where}${hole}`;
  });

  /** Remove is hovered: the piece it would take out lights up on the timeline. */
  let removeHover = $state(false);

  /** Remove: the piece at the playhead out of the edit (the one on screen,
   *  as for Split: there's no other selection to keep track of). */
  function removeCurrent() {
    const id = edit?.id;
    if (!atNow || id === undefined) return;
    edits
      .removePiece(unitOf(atNow.piece))
      .then(
        (removed) => removed && toast.show("Piece removed", { label: "Undo", run: () => undoIn({ kind: "edit", id }) }),
      );
  }

  /** The lines selected in the transcript, out of the edit (struck from the
   *  pieces they're in); the transcript then has them no more. */
  function removeSelection() {
    const id = edit?.id;
    if (id === undefined) return;
    edits
      .removeSelection()
      .then(
        (removed) => removed && toast.show("Lines removed", { label: "Undo", run: () => undoIn({ kind: "edit", id }) }),
      );
  }

  // A line clicked in the transcript: the edit plays from there.
  $effect(() => {
    const wanted = edits.wanted;
    if (!wanted) return;
    untrack(() => {
      edits.wanted = null;
      if (bringTime !== null || !player) seekTo(wanted.time);
      else {
        now = wanted.time;
        player.seek(wanted.time, true);
      }
    });
  });

  /** Undoes the newest change in `doc`'s history (from the toast's Undo). */
  function undoIn(doc: Doc) {
    api.undo(doc).then(
      (done) => done && toast.show(`Undone: ${done.label}`),
      (e) => toast.show(`Couldn't undo: ${e}`),
    );
  }

  /** Moves the piece at the playhead a place earlier (-1) or later (1), the
   *  playhead with it, so pressing again moves it on. */
  async function moveCurrent(by: -1 | 1) {
    const piece = atNow?.piece;
    if (!piece) return;
    const unit = unitOf(piece);
    const units = unitsBeside(piece);
    const index = units.indexOf(unit);
    if (index + by < 0 || index + by >= units.length) return;
    const into = now - (segments.find((s) => unitOf(s.piece) === unit)?.at ?? now);
    if (await edits.move(unit, index + by)) {
      const first = segments.find((s) => unitOf(s.piece) === unit);
      if (first) seekTo(first.at + into);
    }
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

  // The keys the VOD's player has, where they mean the same (playerKeys.ts);
  // B splits, E exports (renders).
  let showKeys = $state(false);
  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (e.altKey || !keysForApp()) return;
    if (target?.closest("input, textarea, select, [contenteditable], [role=dialog]")) return;
    const key = keyOf(e);
    // E renders even where edits can't play here: then it's the way to watch one.
    if (key === "e" && !player) {
      if (!e.repeat && edit && !snippet && edit.status !== "cutting" && segments.length) edits.render(edit.id);
      e.preventDefault();
      return;
    }
    if (!player) return;
    // Shift with a letter is the player's own: captions (C) and mute (M).
    if (e.shiftKey && (key === "c" || key === "m") && !e.ctrlKey && !e.metaKey) {
      if (key === "c") prefs.toggleCaptions();
      else toggleMute(player);
      e.preventDefault();
      return;
    }
    if (key === "b" || key === "e") {
      if (!e.repeat) {
        if (key === "b") split();
        else if (edit && !snippet && edit.status !== "cutting" && segments.length) edits.render(edit.id);
      }
      e.preventDefault();
      return;
    }
    // Ctrl+← and → (⌘ on a Mac) move the piece at the playhead, as Alt+↑/↓
    // moves a row in the library (Alt+← and → are back and forward).
    if ((os === "mac" ? e.metaKey : e.ctrlKey) && !e.shiftKey && (key === "ArrowLeft" || key === "ArrowRight")) {
      if (!e.repeat) moveCurrent(key === "ArrowLeft" ? -1 : 1);
      e.preventDefault();
      return;
    }
    if (e.ctrlKey || e.metaKey) return;
    const p = player;
    if (key === "Escape" && edits.selection) {
      edits.selection = null;
    } else if (key === "Escape" && openGroup !== null) {
      openGroup = null;
    } else if (key === "Delete" || key === "Backspace") {
      // Held down, it's one press: one piece, not one after another. Lines
      // selected in the transcript go, or else the piece at the playhead.
      if (e.repeat) {
        // Nothing more.
      } else if (edits.selection) removeSelection();
      else if (atNow) removeCurrent();
    } else if (
      !transportKey(key, e.shiftKey, {
        togglePlay: () => p.togglePlay(),
        seekBy: (step) => seekTo(Math.max(0, (bringTime ?? p.time()) + step)),
        step: p.step && ((n) => p.step!(n)),
        fullscreen,
        setFullscreen,
      })
    )
      return;
    e.preventDefault();
  }

  // The VODs the edit's pieces come from, in the order they're first used,
  // each a link back to it; the one hovered lights up its pieces.
  let hoveredSource = $state<number | null>(null);
  const sources = $derived.by(() => {
    const out: { vodId: number; title: string; count: number; first: number }[] = [];
    for (const p of pieces) {
      const known = out.find((s) => s.vodId === p.vod_id);
      if (known) known.count++;
      else out.push({ vodId: p.vod_id, title: vods.get(p.vod_id)?.title ?? p.vod_title, count: 1, first: p.start });
    }
    return out;
  });

  // The pieces as bands in the timeline's lane: what each says, where it is.
  const lane = $derived.by(() => {
    const out: LanePiece[] = [];
    let at = 0;
    // How much each band waits for of downloads, and how much of that is in.
    const waiting = new Map<number, { wanted: number; done: number }>();
    for (const piece of pieces) {
      const length = pieceLength(piece);
      const unit = unitOf(piece);
      for (const w of pending.filter((w) => w.pieceId === piece.id)) {
        const sum = waiting.get(unit) ?? { wanted: 0, done: 0 };
        waiting.set(unit, { wanted: sum.wanted + w.end - w.start, done: sum.done + w.done });
      }
      const sum = waiting.get(unit);
      const loading = sum && sum.wanted > 0 ? sum.done / sum.wanted : undefined;
      const lit = hoveredSource === piece.vod_id;
      const last = out[out.length - 1];
      if (piece.group && last?.id === unit) {
        // A group is one band: its pieces one after another.
        Object.assign(last, { end: at + length, loading, lit: last.lit || lit, hole: last.hole && piece.hole });
      } else {
        // A group's band isn't resized (open it to change what's in it).
        const group = !!piece.group && piece.group.id !== openGroup;
        const inGroup = !!piece.group && piece.group.id === openGroup;
        const range = group
          ? undefined
          : { start: piece.start, end: piece.end, max: vods.get(piece.vod_id)?.duration ?? null };
        out.push({
          id: unit,
          start: at,
          end: at + length,
          label: nameOf(piece),
          loading,
          range,
          lit,
          hole: piece.hole,
          group,
          inGroup,
        });
      }
      at += length;
    }
    return out;
  });

  // Where the edit waits for footage still to download (a VOD from a site):
  // each such stretch of it, and how far into it the download has come (a
  // section comes in from its start on).
  const pending = $derived.by(() => {
    const out: { start: number; end: number; done: number; pieceId: number }[] = [];
    for (const seg of planned) {
      if (seg.footage || seg.piece_id === null) continue;
      // A hole has nothing to wait for.
      if (pieces.find((p) => p.id === seg.piece_id)?.hole) continue;
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
  const edgeLabel = $derived(
    edgeFrame
      ? `${edgeFrame.label}${edgeFrame.missing ? " · not downloaded yet; it's fetched once you let go" : ""}`
      : "",
  );

  // With the app's own player, nothing of the page shows over the picture:
  // the hint and a dragged edge's frame are drawn over it by its screen
  // (screenOverlay.ts) instead.
  let hostSize = $state({ width: 0, height: 0 });
  $effect(() => {
    const el = host;
    if (!el) return;
    const sizes = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      hostSize = { width: r.width, height: r.height };
    });
    sizes.observe(el);
    return () => sizes.disconnect();
  });
  let edgePicture = $state.raw<HTMLImageElement | null>(null);
  $effect(() => {
    const url = edgeFrame?.url;
    if (!url) {
      edgePicture = null;
      return;
    }
    const picture = new Image();
    picture.onload = () => (edgePicture = picture);
    picture.src = url;
  });
  $effect(() => {
    if (!(player instanceof EditPlayer)) return;
    const { width, height } = hostSize;
    if (!width || !height) return;
    if (edgeFrame) overlay.show(overlay.framed(edgePicture, edgeLabel, width, height));
    else if (!segments.length) overlay.show(overlay.hint(EMPTY, width, height));
    else if (playError) overlay.show(overlay.hint(playError, width, height));
    else if (cue) overlay.show(overlay.caption(cue.text, width, height, fullscreen));
    else overlay.show(null);
  });
  onDestroy(() => overlay.show(null));
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
    const label = `New ${side === "start" ? "first" : "last"} frame · ${formatFraction(at, 1)}`;
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
    move(id: number, index: number) {
      // In the open group: among its pieces, from where they start in the lane.
      const piece = pieces.find((p) => p.id === id);
      if (piece?.group && piece.group.id === openGroup) {
        const first = lane.findIndex((b) => b.inGroup);
        const count = pieces.filter((p) => p.group?.id === openGroup).length;
        index = Math.max(0, Math.min(count - 1, index - first));
      }
      edits.move(id, index);
    },
    open(id: number) {
      const piece = pieces.find((p) => p.id === id);
      if (piece?.group && piece.group.id === openGroup) openGroup = null;
      else if (pieces.some((p) => p.group?.id === id)) openGroup = id;
    },
    resize: (id: number, start: number, end: number) => edits.trim(id, start, end),
    resizing: showEdge,
    carry(id: number, x: number, y: number, from: DOMRect, home: CarryHome) {
      // A piece, or a group of one: a group of more doesn't go out as one clip.
      const of = pieces.filter((p) => unitOf(p) === id);
      const piece = of.length === 1 ? of[0] : undefined;
      const vod = piece && vods.get(piece.vod_id);
      if (!piece || !vod) return;
      const { start, end } = piece;
      carry({ vod, start, end, title: nameOf(piece), ownPlayer: playsHere(vod), from, x, y, home });
    },
  };

  const pieceLength = (p: EditPiece) => playedSpans(p.start, p.end, p.struck).reduce((sum, [a, b]) => sum + b - a, 0);

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
  /** A clip of a clip bar, opened here: it's made again by itself as it
   *  changes, and is deleted from its clip bar. */
  const snippet = $derived(edit?.snippet_of != null);
</script>

<svelte:window onkeydown={onKey} onpointermove={onOutMove} onpointerup={() => (outPress = null)} />

{#if edit}
  <section class="edit-view" class:fullscreen>
    <div class="head">
      <span class="kind"><Icon name="film" size={14} /></span>
      <input
        class="title"
        bind:value={title}
        onblur={rename}
        onkeydown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        aria-label={snippet ? "Clip name" : "Edit name"}
        autocorrect="off"
        autocapitalize="off"
        spellcheck="false"
      />
      <span class="grow"></span>
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
      <!-- Making the video: after the link to it, so it stays put when that appears. -->
      {#if edit.status === "cutting"}
        <span class="rendering muted">
          <Spinner size={12} /> Rendering{progress ? ` ${Math.round(progress * 100)}%` : "…"}
        </span>
        <button onclick={() => edits.stop(edit.id)}>Stop</button>
      {:else if !snippet}
        {@const again = edit.status === "ready"}
        <button
          class="make"
          onclick={() => edits.render(edit.id)}
          disabled={!segments.length}
          title="{again
            ? 'Render the edit again'
            : 'Render the edit'} into one video, in Videos/VODscribe/Edits (E: export)"
        >
          <Icon name={again ? "redo" : "clapperboard"} size={14} />
          Render <kbd>E</kbd>
        </button>
      {/if}
      <button
        class="icon"
        class:on={showKeys}
        onclick={() => (showKeys = !showKeys)}
        aria-expanded={showKeys}
        aria-label="Keyboard shortcuts"
        title="Keyboard shortcuts"><Icon name="keyboard" size={16} /></button
      >
      {#if !snippet}
        <button class="icon" onclick={remove} title="Delete edit" aria-label="Delete edit">
          <Icon name="trash" size={16} />
        </button>
      {/if}
    </div>
    {#if showKeys}
      <Shortcuts shortcuts={editShortcuts(!!player?.step, os === "mac")} />
    {/if}

    <div class="stage">
      <!-- A click on the picture plays or pauses, as on a VOD's player. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="frame" bind:this={host} onclick={() => player?.togglePlay()}></div>
      {#if !segments.length}
        <p class="hint">{EMPTY}</p>
      {:else if playError}
        <p class="hint">{playError}</p>
      {/if}
      <!-- Over the player and its notes while an edge is dragged. -->
      {#if edgeFrame}
        <div class="edge-frame">
          {#if edgeFrame.url}<img src={edgeFrame.url} alt="" />{/if}
          <span>{edgeLabel}</span>
        </div>
      {/if}
    </div>

    <div class="controls">
      {#if total}
        <Scrubber
          memo={edit ? { kind: "edit", id: edit.id } : undefined}
          {waveform}
          pieces={lane}
          {pending}
          selectedPiece={atNow ? unitOf(atNow.piece) : null}
          picked={edits.selection}
          removing={removeHover && atNow ? (lane.find((p) => p.id === unitOf(atNow.piece)) ?? null) : null}
          {onpiece}
          duration={total}
          time={now}
          {shown}
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
        captionsAvailable={cues.length > 0}
        captionsOn={prefs.captions}
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
            onclick={removeCurrent}
            onmouseenter={() => (removeHover = true)}
            onmouseleave={() => (removeHover = false)}
            disabled={!atNow}
            title="Take the piece at the playhead out of the edit; its VOD and clips aren't touched (Delete)"
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
    {#if sources.length}
      <div class="sources" aria-label="Where its pieces come from">
        {#each sources as source (source.vodId)}
          <button
            class="source"
            onclick={() => {
              hoveredSource = null;
              if (edit) edits.lit = { editId: edit.id, vodId: source.vodId };
              onopenvod(source.vodId, source.first);
            }}
            onmouseenter={() => (hoveredSource = source.vodId)}
            onmouseleave={() => hoveredSource === source.vodId && (hoveredSource = null)}
            title="{source.count === 1
              ? 'A piece comes'
              : `${source.count} pieces come`} from “{source.title}”, lit up in the timeline · open it there"
          >
            <Icon name="play" size={12} />
            <span class="name">{source.title}</span>
            <span class="count muted">· {source.count} {source.count === 1 ? "piece" : "pieces"}</span>
            <Icon name="external" size={11} />
          </button>
        {/each}
      </div>
    {/if}
  </section>
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
  .fullscreen :global(.legend) {
    display: none;
  }

  .fullscreen .stage {
    flex: 1;
    min-height: 0;
  }

  .fullscreen .frame {
    height: 100%;
    max-width: none;
    aspect-ratio: auto;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px 6px 16px;
    min-height: 40px;
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
    background: #000;
  }

  /* As large as the VOD's player. */
  .frame {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    /* Capped by a width, not max-height: macOS 12's WebKit (17.6) turned
       max-height and aspect-ratio together into a max-width, and the box
       shrank to the left. */
    max-width: calc(var(--player-max-height, 44vh) * 16 / 9);
    margin: 0 auto;
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

  /* Links back to the VODs its pieces come from, as a VOD's clip bar links to its edits. */
  .sources {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 40px;
    padding: 6px 16px;
    overflow-x: auto;
    scrollbar-width: thin;
  }

  .source {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 320px;
    height: 28px;
    padding: 0 10px;
    border: 1px dashed color-mix(in srgb, var(--accent) 55%, var(--border));
    border-radius: 14px;
    background: var(--accent-soft);
    color: var(--text);
    font-size: 12px;
  }

  .source :global(svg) {
    flex: none;
    color: var(--accent);
  }

  .source .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .source .count {
    flex: none;
  }

  .source:hover {
    border-style: solid;
  }

  .make :global(svg) {
    color: var(--accent);
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
</style>
