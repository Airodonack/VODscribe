<script lang="ts">
  // An edit (edits.svelte.ts), opened from its row in the library's list, in the
  // player's place and laid out like it: the picture (editPlayer.ts, the app's
  // own player playing its pieces one after another), the timeline in the edit's own time with its
  // pieces' waveforms and the joins between them, and the same controls,
  // where Split puts a cut at the playhead (as on a VOD) and Remove
  // takes out the stretch there between cuts, which become a real
  // split, or else the piece (Delete, too); Render, at the top beside the
  // rendered video, makes it. Its pieces
  // are bands in the timeline's lane (Scrubber.svelte): clicked to go to,
  // dragged to move (pulled off the timeline, carried out), their edges dragged to resize. Under it, the edit's
  // transcript, only what plays: a click on a line plays from it, and lines
  // selected are a slice of the timeline that Delete takes out as any other.

  import { onDestroy, onMount, untrack } from "svelte";
  import { api, type Cue, type Doc, type EditPiece, type PlayerStretch, type Vod } from "./api";
  import { ask } from "./ask";
  import { keysForApp } from "./demo";
  import { bindingsOf, is, keyHint, labelOf } from "./keys.svelte";
  import { editShortcuts, forPlayer, inOut, isEscape, transportKey } from "./playerKeys";
  import { DRAG_SLOP, carry, dropsInEdit, type CarryHome, type Insertion } from "./dragout";
  import { playsFromFile } from "./platform";
  import { EditPlayer, type EditPlayback } from "./editPlayer";
  import { clips } from "./clips.svelte";
  import ClipStrip from "./ClipStrip.svelte";
  import { cuts, markKey } from "./cuts.svelte";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { PEAKS_RATE, cueAt, frameStart, playedSpans, stretchIn } from "./media";
  import { prefs } from "./playerPrefs.svelte";
  import PlayerBar, { toggleMute } from "./PlayerBar.svelte";
  import PlayerPanel from "./PlayerPanel.svelte";
  import { PlayerReading } from "./playerReading.svelte";
  import { formatFraction, os } from "./platform";
  import Scrubber, { type LanePiece } from "./Scrubber.svelte";
  import Spinner from "./Spinner.svelte";
  import { toast } from "./toasts.svelte";
  import { nav } from "./nav.svelte";
  import * as overlay from "./screenOverlay";
  import { carryChosen, chosenClips, chosenSegments, clipSegments } from "./bulk";
  import { count, snippets } from "./selection.svelte";

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
  // The real splits: wherever what plays jumps, between pieces or over a
  // stretch struck out.
  const joins = $derived(segments.slice(1).map((s) => s.at));

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
  // What the controls show (playerReading.svelte.ts).
  const reading = new PlayerReading();
  const now = $derived(reading.now);
  const fps = $derived(reading.fps);
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
    "stretch of a VOD's timeline). What's put in an edit is its own copy.";

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
          reading.at(bringTime);
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
    reading.read(p);
    // Still to seek to where it was left (no plan yet): the player's time is
    // where it opened, not where the page has put the playhead meanwhile
    // (`seekTo`), which reading it would put back, under Split and Delete.
    if (bringTime !== null) reading.at(bringTime);
    // Only once it's this edit's playhead (after the seek to where it was left).
    if (edit && playsEdit === edit.id) nav.note({ kind: "edit", id: edit.id }, { time: reading.now });
  });

  // The transcript follows the playhead (cuts.svelte.ts), while its marks are this edit's.
  $effect(() => {
    if (!edit || !cuts.isOf({ kind: "edit", id: edit.id })) return;
    cuts.now = reading.actual;
    cuts.playing = reading.playing;
  });

  const seekTo = (time: number) => {
    reading.at(time);
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

  /** Remove: the piece at the playhead out of the edit (the one on screen). */
  function removeCurrent() {
    const id = edit?.id;
    if (!atNow || id === undefined) return;
    edits
      .removePiece(unitOf(atNow.piece))
      .then(
        (removed) => removed && toast.show("Piece removed", { label: "Undo", run: () => undoIn({ kind: "edit", id }) }),
      );
  }

  /** The selection, or else the stretch at the playhead (between split
   *  marks, or the piece), a clip on the edit's clip bar. */
  function clipNow() {
    if (chosenSegments().length) return void clipSegments();
    const range = edits.selection ?? stretchNow;
    if (range) void clips.makeOfEdit(range.start, range.end);
  }

  /** Delete with nothing selected: the stretch at the playhead between split
   *  marks (which become a real split), or else the piece. */
  function removeHere() {
    const id = edit?.id;
    const s = stretchNow;
    if (id === undefined || !s) return;
    if (s.whole) return removeCurrent();
    edits
      .removeStretch(s.start, s.end)
      .then(
        (removed) =>
          removed && toast.show("Stretch removed", { label: "Undo", run: () => undoIn({ kind: "edit", id }) }),
      );
  }

  /** What's picked on the timeline and in the clip bar (selection.svelte.ts)
   *  out of the edit, as one change: a stretch that's a whole band goes as
   *  that piece (or group), as Remove takes one; the others as stretches. */
  function removeChosen() {
    const id = edit?.id;
    const picked = chosenSegments();
    const clipIds = chosenClips();
    if (id === undefined || (!picked.length && !clipIds.length)) return;
    const near = (a: number, b: number) => Math.abs(a - b) < 1e-6;
    const units: number[] = [];
    const stretches: [number, number][] = [];
    for (const s of picked) {
      const band = lane.find((b) => near(b.start, s.start) && near(b.end, s.end));
      if (band) units.push(band.id);
      else stretches.push([s.start, s.end]);
    }
    snippets.clear();
    const what = count([
      [units.length + stretches.length, "piece", "pieces"],
      [clipIds.length, "clip", "clips"],
    ]);
    edits
      .removeFrom(units, stretches, clipIds)
      .then(
        (removed) =>
          removed && toast.show(`${what} removed`, { label: "Undo", run: () => undoIn({ kind: "edit", id }) }),
      );
  }

  /** The lines selected in the transcript: their slice of the timeline out
   *  of the edit (the pieces split at its ends); the transcript then has them no more. */
  function removeSelection() {
    const id = edit?.id;
    if (id === undefined) return;
    edits
      .removeSelection()
      .then(
        (removed) => removed && toast.show("Lines removed", { label: "Undo", run: () => undoIn({ kind: "edit", id }) }),
      );
  }

  // The edit's chapters and highlights (cuts.svelte.ts), on its timeline:
  // placed again whenever it changes, since its pieces place them.
  $effect(() => {
    const id = edit?.id;
    void edit?.revision;
    if (id === undefined) return;
    untrack(() => {
      const doc = { kind: "edit" as const, id };
      if (cuts.doc?.kind === "edit" && cuts.doc.id === id) cuts.reload(doc);
      else cuts.openDoc(doc);
      // Its clip bar: a clip of the bar open in the editor has none.
      if (!snippet) clips.openEdit(id);
    });
  });

  // A line clicked in the transcript: the edit plays from there.
  $effect(() => {
    const wanted = edits.wanted;
    if (!wanted) return;
    untrack(() => {
      edits.wanted = null;
      if (bringTime !== null || !player) seekTo(wanted.time);
      else {
        reading.at(wanted.time);
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

  /** Split: a cut at both ends of the selection, else one here (or
   *  the one here taken away), as on a VOD. A delete at one makes it real. */
  async function split() {
    const picked = edits.selection;
    if (!picked) return void cuts.toggle(now, fps, !reading.playing);
    edits.selection = null;
    for (const t of [picked.start, picked.end])
      if (t > 0 && t < total && !cuts.at(t, fps)) await cuts.toggle(t, fps, false);
  }

  // Full screen (the panel's, PlayerPanel.svelte).
  let fullscreen = $state(false);
  const setFullscreen = (on: boolean) => (fullscreen = on);

  // The keys the VOD's player has, where they mean the same (playerKeys.ts);
  // B splits, E exports (renders). Which key does what is the map's
  // (keys.svelte.ts); Escape is always Escape.
  function onKey(e: KeyboardEvent) {
    if (!forPlayer(e) || !keysForApp()) return;
    const render = is(e, "edit.render");
    // E renders even where edits can't play here: then it's the way to watch one.
    if (render && !player) {
      if (!e.repeat && renderable !== null) edits.render(renderable);
      e.preventDefault();
      return;
    }
    if (!player) return;
    // Shift with a letter is the player's own: captions (C) and mute (M).
    if (is(e, "captions.toggle") || is(e, "mute.toggle")) {
      if (is(e, "captions.toggle")) prefs.toggleCaptions();
      else toggleMute(player);
      e.preventDefault();
      return;
    }
    if (is(e, "cut.here") || render) {
      if (!e.repeat) {
        if (!render) split();
        else if (renderable !== null) edits.render(renderable);
      }
      e.preventDefault();
      return;
    }
    // Ctrl+← and → (⌘ on a Mac) move the piece at the playhead, as Alt+↑/↓
    // moves a row in the library (Alt+← and → are back and forward).
    const earlier = is(e, "edit.pieceEarlier");
    if (earlier || is(e, "edit.pieceLater")) {
      if (!e.repeat) moveCurrent(earlier ? -1 : 1);
      e.preventDefault();
      return;
    }
    const p = player;
    // I and O put the selection's in and out points at the playhead (as on
    // a VOD); with none, I or O begins one, up to a cut or a piece's end.
    const end = is(e, "selection.in") ? "i" : is(e, "selection.out") ? "o" : null;
    if (end) {
      const stops = [...lane.flatMap((u) => [u.start, u.end]), ...cuts.list.map((m) => m.start)];
      const picked = total ? inOut(end, frameStart(now, fps), edits.selection, stops, total) : null;
      if (picked) {
        edits.selection = picked;
        // The arrows go on moving the playhead, not a mark.
        cuts.selected = null;
      }
      e.preventDefault();
      return;
    }
    // M marks a highlight here, N starts a chapter, each with its title to
    // write; Enter renames the selected one (as on a VOD).
    // C makes a clip of the selection, or of the piece at the playhead, on
    // the edit's clip bar (bare: Ctrl+C is copy).
    if (is(e, "clip.make")) {
      if (!e.repeat && !snippet) clipNow();
      e.preventDefault();
      return;
    }
    const highlight = is(e, "mark.highlight");
    if (highlight || is(e, "mark.chapter")) {
      if (!e.repeat && edit && segments.length) cuts.add(highlight ? "highlight" : "chapter", now, fps);
      e.preventDefault();
      return;
    }
    // The selected mark: renamed, moved, taken away (as on a VOD).
    if (markKey(e, { fps, length: total, pause: () => p.playing() && p.togglePlay(), seekTo })) {
      e.preventDefault();
      return;
    }
    if (isEscape(e) && edits.selection) {
      edits.selection = null;
    } else if (isEscape(e) && openGroup !== null) {
      openGroup = null;
    } else if ((is(e, "edit.remove") || is(e, "clips.remove")) && snippets.size) {
      // What's picked on the timeline and in the clip bar goes, at once.
      if (!e.repeat) removeChosen();
    } else if (is(e, "edit.remove")) {
      // Held down, it's one press: one piece, not one after another. Lines
      // selected in the transcript go, or else the piece at the playhead.
      if (e.repeat) {
        // Nothing more.
      } else if (edits.selection) removeSelection();
      else removeHere();
    } else if (
      !transportKey(
        e,
        {
          togglePlay: () => p.togglePlay(),
          seekBy: (step) => seekTo(Math.max(0, (bringTime ?? p.time()) + step)),
          step: p.step && ((n) => p.step!(n)),
          fullscreen,
          setFullscreen,
        },
        is,
      )
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

  // The band at the playhead, and the stretch of it between its cuts
  // (as a VOD's between its cuts) that the playhead is in: all of it if it has none.
  const unitNow = $derived(atNow ? lane.find((p) => p.id === unitOf(atNow.piece)) : undefined);
  const stretchNow = $derived.by(() => {
    const unit = unitNow;
    if (!unit) return undefined;
    const inside = cuts.list.map((m) => m.start - unit.start).filter((t) => t > 0 && t < unit.end - unit.start);
    const s = stretchIn(inside, unit.end - unit.start, now - unit.start);
    return s && { start: unit.start + s.start, end: unit.start + s.end, whole: inside.length === 0 };
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
      // A band picked with others: all of what's picked goes together.
      const band = lane.find((b) => b.id === id);
      if (band && snippets.has({ kind: "segment", start: band.start, end: band.end }))
        return void carryChosen(from, x, y);
      // A piece, or a group of one: a group of more doesn't go out as one clip.
      const of = pieces.filter((p) => unitOf(p) === id);
      const piece = of.length === 1 ? of[0] : undefined;
      const vod = piece && vods.get(piece.vod_id);
      if (!piece || !vod) return;
      const { start, end } = piece;
      carry({ vod, start, end, title: nameOf(piece), ownPlayer: playsFromFile(vod), from, x, y, home });
    },
  };

  const pieceLength = (p: EditPiece) => playedSpans(p.start, p.end, p.struck).reduce((sum, [a, b]) => sum + b - a, 0);

  /** Where something carried over the timeline at `x` (on screen) would go
   *  (dragout.ts): before the first of the edit's pieces (a group as one)
   *  whose middle is past it, at its start. */
  function insertionAt(x: number): Insertion {
    const bar = document.querySelector<HTMLElement>(".drop-in .scrubber");
    const box = bar?.getBoundingClientRect();
    const viewStart = Number(bar?.dataset.viewStart ?? 0);
    const span = Number(bar?.dataset.span ?? total);
    const t = box?.width ? viewStart + ((x - box.left) / box.width) * span : 0;
    const tops: { id: number; start: number; end: number }[] = [];
    let at = 0;
    for (const piece of pieces) {
      const id = piece.group?.id ?? piece.id;
      const last = tops.at(-1);
      const end = at + pieceLength(piece);
      if (last?.id === id) last.end = end;
      else tops.push({ id, start: at, end });
      at = end;
    }
    const index = tops.findIndex((p) => t < (p.start + p.end) / 2);
    return index < 0 ? { index: tops.length, time: total } : { index, time: tops[index].start };
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
    player?.destroy();
  });

  const progress = $derived(edit ? edits.progress.get(edit.id) : undefined);
  /** A clip of a clip bar, opened here: it's made again by itself as it
   *  changes, and is deleted from its clip bar. */
  const snippet = $derived(edit?.snippet_of != null);
  /** The edit E renders, if it can be: one with pieces, not a snippet, not rendering already. */
  const renderable = $derived(edit && !snippet && edit.status !== "cutting" && segments.length ? edit.id : null);
</script>

<svelte:window onkeydown={onKey} onpointermove={onOutMove} onpointerup={() => (outPress = null)} />

{#if edit}
  <PlayerPanel
    bind:fullscreen
    label="edit-view"
    shortcuts={editShortcuts(!!player?.step, os, bindingsOf)}
    stage={editStage}
    controls={editControls}
    below={editBelow}
  >
    {#snippet head()}
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
            : 'Render the edit'} into one video, in Videos/VODscribe/Edits{labelOf('edit.render')
            ? ` (${labelOf('edit.render')}: export)`
            : ''}"
        >
          <Icon name={again ? "redo" : "clapperboard"} size={14} />
          Render {#if labelOf("edit.render")}<kbd>{labelOf("edit.render")}</kbd>{/if}
        </button>
      {/if}
    {/snippet}
    {#snippet headEnd()}
      {#if !snippet}
        <button class="icon" onclick={remove} title="Delete edit" aria-label="Delete edit">
          <Icon name="trash" size={16} />
        </button>
      {/if}
    {/snippet}
  </PlayerPanel>
{/if}

{#snippet editStage()}
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
{/snippet}

{#snippet editControls()}
  {#if total}
    <!-- Several things picked, carried here, go in where the marker shows (dragout.ts). -->
    <div class="drop-in" use:dropsInEdit={insertionAt}>
      <Scrubber
        memo={edit ? { kind: "edit", id: edit.id } : undefined}
        {waveform}
        pieces={lane}
        {pending}
        selectedPiece={atNow ? unitOf(atNow.piece) : null}
        picked={edits.selection}
        marks
        removing={removeHover ? (stretchNow ?? null) : null}
        {joins}
        current={stretchNow && !stretchNow.whole ? stretchNow : null}
        {onpiece}
        duration={total}
        time={now}
        shown={reading.shown}
        {fps}
        onseek={seekTo}
        onpause={() => player?.playing() && player.togglePlay()}
        inserting={edits.inserting}
      />
    </div>
  {/if}
  <PlayerBar
    {player}
    playing={reading.playing}
    now={reading.actual}
    duration={total}
    volume={reading.volume}
    muted={reading.muted}
    rate={reading.rate}
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
        title="Put a split here, or take away the one here; a delete beside it makes it real{keyHint('cut.here')}"
      >
        <Icon name="scissors" size={14} /> Split {#if labelOf("cut.here")}<kbd>{labelOf("cut.here")}</kbd>{/if}
      </button>
      <button
        class="action remove"
        onclick={removeHere}
        onmouseenter={() => (removeHover = true)}
        onmouseleave={() => (removeHover = false)}
        disabled={!atNow}
        title="Take the stretch at the playhead (between splits, or else its piece) out of the edit; its VOD and clips aren't touched{keyHint(
          'edit.remove',
        )}"
      >
        <Icon name="circle-minus" size={14} /> Remove {#if labelOf("edit.remove")}<kbd>{labelOf("edit.remove")}</kbd
          >{/if}
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
{/snippet}

{#snippet editBelow()}
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
  {#if !snippet && clips.doc?.kind === "edit" && (clips.shown.length || clips.error || snippets.size || clips.carryingAny)}
    <ClipStrip />
  {/if}
{/snippet}

<style>
  /* Full screen: the picture as large as fits (PlayerPanel.svelte). */
  :global(.fullscreen) .frame {
    height: 100%;
    max-width: none;
    aspect-ratio: auto;
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

  .drop-in {
    display: contents;
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
