<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, tick, untrack } from "svelte";
  import { api, events, type Cue, type Vod } from "./api";
  import { ask } from "./ask";
  import { clips } from "./clips.svelte";
  import { edits } from "./edits.svelte";
  import ClipStrip from "./ClipStrip.svelte";
  import { cuts, markKey } from "./cuts.svelte";
  import { DRAG_SLOP, carry, exportStretch, stretchAt } from "./dragout";
  import { cueAt, frameStart, isUnplayable } from "./media";
  import PermissionNote from "./PermissionNote.svelte";
  import Popover from "./Popover.svelte";
  import PlayerBar, { toggleMute, type BarControls } from "./PlayerBar.svelte";
  import PlayerPanel from "./PlayerPanel.svelte";
  import { PlayerReading } from "./playerReading.svelte";
  import { prefs } from "./playerPrefs.svelte";
  import Scrubber from "./Scrubber.svelte";
  import Icon from "./Icon.svelte";
  import { Hold } from "./hold.svelte";
  import Thumb from "./Thumb.svelte";
  import TranscribeFrom from "./TranscribeFrom.svelte";
  import { demoOff, keysForApp } from "./demo";
  import { bindingsOf, is, keyHint, labelOf } from "./keys.svelte";
  import { forPlayer, inOut, isEscape, transportKey, vodShortcuts } from "./playerKeys";
  import { canEmbed, createPlayer, drawsOwnCaptions, playsHere, type EmbeddedPlayer } from "./embed";
  import { ownPlayer } from "./ownPlayer.svelte";
  import {
    formatFraction,
    formatTime,
    keepsVideo,
    os,
    playbackHint,
    playsFromFile,
    siteName,
    watchUrl,
  } from "./platform";
  import { nav } from "./nav.svelte";
  import { hideNote, noteHidden } from "./permission";
  import { videoDownloads } from "./videos.svelte";
  import { chosenClips, chosenSegments, clipSegments, removeChosenClips } from "./bulk";
  import { snippets } from "./selection.svelte";
  import { gaps, transcribeRest } from "./stretches";

  // One instance per VOD (the parent keys it by id). `at` is a jump (a search
  // result, a transcript line, a source link), a fresh object each time, so
  // jumping to the same timestamp twice still seeks; null opens it where its
  // playhead was left this session (nav.svelte.ts), or at its start.
  let { vod, at }: { vod: Vod; at: { time: number; play: boolean } | null } = $props();
  const start = untrack(() => {
    const doc = { kind: "vod" as const, id: vod.id };
    return at?.time ?? nav.bringBack(doc)?.time ?? nav.lastSeen(doc)?.time ?? 0;
  });
  // Where it opened, or was last jumped to: the moment the header links to.
  let from = $state(start);

  // A site without a player the app can show plays its VODs only here, once
  // their video is downloaded: Play asks first (the rights note), as Keep
  // video does, then downloads it; it plays once it's in (`vod.media`).
  videoDownloads.start();
  const download = $derived(keepsVideo(vod) && !vod.media);
  const downloading = $derived(videoDownloads.progress.get(vod.id));
  const downloadError = $derived(videoDownloads.errors.get(vod.id));
  let playButton = $state<HTMLElement>();
  let askingDownload = $state(false);
  function playByDownloading() {
    if (downloading !== undefined) return;
    if (noteHidden()) startDownload();
    else askingDownload = true;
  }
  function answerDownload(yes: boolean, hide: boolean) {
    askingDownload = false;
    if (!yes) return;
    if (hide) hideNote();
    startDownload();
  }
  /** Downloads its video and plays it as it comes in: the app's own player
   *  takes it from now (embed.ts), waiting where it hasn't come yet. */
  async function startDownload() {
    void videoDownloads.keep(vod.id);
    await tick();
    if (canEmbed(vod)) activate(from);
  }

  // Collapsing hides the video but keeps the player loaded; a jump reopens it.
  let collapsed = $state(false);

  // Clicking the timestamp copies a link to that moment on the VOD's site.
  let copied = $state<"ok" | "failed" | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  async function copyLink() {
    copied = await writeText(watchUrl(vod, from)).then(
      () => "ok" as const,
      (e) => (console.error("copy failed", e), "failed" as const),
    );
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copied = null), 1500);
  }

  const site = $derived(siteName(vod));

  const isLocal = $derived(vod.source_path !== null);
  let error = $state<string | null>(null);
  // Audio alone (a podcast, a song): a bar with the controls, not a 16:9 frame.
  // Known from the file type; a file whose type didn't say is found out once loaded.
  let detected = $state(false);
  // Follows the VOD: one opened while still downloading learns once it's done.
  let audioOnly = $derived(vod.audio_only || detected);
  // The app's own player, for a file played here, steps a frame at a time.
  const framed = $derived(playsHere(vod) && !audioOnly);

  // The player loads on demand: until then the VOD's (local) thumbnail stands in,
  // so selecting a VOD contacts nobody. Clicking it, a timestamp, or play starts it.
  let active = $state(false);
  let host = $state<HTMLDivElement>();
  let player = $state.raw<EmbeddedPlayer | null>(null);
  // A jump requested while the player is still loading, applied once it's ready.
  let pending: { time: number; play: boolean } | null = null;
  let destroyed = false;

  async function activate(time: number, play = true) {
    if (active) return;
    active = true;
    await tick();
    if (!host || destroyed) return;
    createPlayer(host, vod, time, play, (e) => (error = e))?.then(
      (p) => {
        // Closed while loading: don't leave an invisible player running.
        if (destroyed) return p.destroy();
        player = p;
        // Now, not at the next frame's reading: a strip shown for a frame would move what's under it.
        drawsCaptions = p.drawsCaptions?.() ?? false;
        // Muted on the bar before it was made: it starts muted (its volume
        // and speed it takes from what was set, playerPrefs.svelte.ts).
        if (firstMuted) p.volume?.setMuted(true);
        firstMuted = false;
        if (pending) p.seek(pending.time, pending.play);
        const video = host?.querySelector("video, audio");
        video?.addEventListener("loadedmetadata", () => {
          if (video instanceof HTMLVideoElement && video.videoWidth === 0) detected = true;
        });
      },
      (e) => (error = String(e)),
    );
  }

  // Opened by a jump (a search result or transcript line): start straight away.
  // Later jumps arrive as a new `at` object.
  let handled = untrack(() => at);
  if (handled?.play && untrack(() => canEmbed(vod))) activate(handled.time);
  // A file or a kept video opens in the app's own player at once, paused:
  // the stage shows the frame at the playhead, and Play starts from the
  // frames already decoded, with no wait. One VOD is open at a time, so
  // one decoder; closing the VOD lets it go. A site's VOD still waits for
  // Play (nothing of the site's loads before), and so does sound alone.
  if (untrack(() => playsFromFile(vod) && playsHere(vod) && !vod.audio_only))
    void ownPlayer.ask().then((own) => {
      if (own && !destroyed) activate(from, false);
    });
  $effect(() => {
    if (at === handled) return;
    handled = at;
    if (!at) return;
    const { time, play } = at;
    from = time;
    collapsed = false;
    if (!active) activate(time, play);
    else if (player) player.seek(time, play);
    else pending = { time, play };
  });

  cuts.open(untrack(() => vod.id));
  clips.open(
    untrack(() => vod.id),
    untrack(() => !playsFromFile(vod)),
  );

  /** Plays from `time` (a clip's play button), starting the player if it isn't yet. */
  function playFrom(time: number) {
    collapsed = false;
    reading.at(time);
    if (!active) activate(time);
    else if (player) player.seek(time, true);
    else pending = { time, play: true };
  }

  // A VOD whose video has just been downloaded switches to the app's own
  // player, where it was and as it was (playing or not).
  let here = untrack(() => playsHere(vod));
  $effect(() => {
    const now = playsHere(vod);
    // Still loading: looked at again once it's in (this reads `player`).
    if (active && !player) return;
    if (now === here) return;
    here = now;
    // Its clips are cut from the file now (or downloaded again, once it's deleted).
    clips.open(vod.id, !playsFromFile(vod));
    if (!active || !player) return;
    const time = player.time();
    const play = player.playing();
    firstMuted = player.volume?.muted() ?? false;
    player.destroy();
    player = null;
    active = false;
    error = null;
    activate(time, play);
  });

  // A VOD played in the app's own player shows its sound and speed on the
  // bar from the moment it opens, though the player is made a moment
  // after (a file's as it opens; one played in the web view on the first
  // Play): what's set meanwhile is what it starts with. The volume and speed are
  // every player's (playerPrefs.svelte.ts); muted is kept till it's made
  // (and from one player to the next, as a download switches it). A
  // site's player has none of this till it's made: nothing of the site's
  // is loaded before Play.
  let firstMuted = $state(false);
  const first: BarControls = {
    volume: {
      get: () => prefs.volume() ?? 1,
      muted: () => firstMuted,
      set(volume) {
        prefs.setVolume(volume);
        firstMuted = volume === 0;
      },
      setMuted: (muted) => (firstMuted = muted),
    },
    rate: { get: () => prefs.speed() ?? 1, set: (rate) => prefs.setSpeed(rate) },
  };
  const controls = $derived(player ?? (playsHere(vod) ? first : null));

  // The transcript's lines as captions, for the captions button (greyed
  // till a part is transcribed) and, where the player doesn't draw them
  // over its picture (a site's own: embed.ts `drawsCaptions`), for the
  // strip under it. Asked again as the VOD changes: a stretch transcribed
  // brings its captions.
  let cues = $state.raw<Cue[]>([]);
  function fetchCues() {
    api.captions(vod.id).then(
      (c) => !destroyed && (cues = c),
      (e) => console.error("captions", e),
    );
  }
  fetchCues();
  const changed = events.vod((v) => v.id === vod.id && fetchCues());

  // What the controls show, read from the player every frame (playerReading.svelte.ts).
  const reading = new PlayerReading(start);
  // Whether the player draws the captions itself, over its picture: known
  // before it opens (and then read from it), so under a site's player the
  // strip's room is kept from the start, and nothing below moves on Play.
  let drawsCaptions = $state(untrack(() => playsHere(vod)));
  drawsOwnCaptions(untrack(() => vod)).then((draws) => !player && !destroyed && (drawsCaptions = draws));
  let playerDuration = $state(0);
  let poll = 0;
  function read() {
    poll = requestAnimationFrame(read);
    const p = player;
    if (!p) {
      if (controls) reading.readSound(controls);
      return;
    }
    reading.read(p);
    nav.note({ kind: "vod", id: vod.id }, { time: reading.now });
    playerDuration = p.duration?.() ?? 0;
    cuts.fps = reading.fps;
    drawsCaptions = p.drawsCaptions?.() ?? false;
  }
  // The caption in the strip under a site's player (never over it); the
  // strip, empty, before the player opens.
  const stripCue = $derived(
    !drawsCaptions && prefs.captions && cues.length ? (player ? (cueAt(cues, reading.now)?.text ?? "") : "") : null,
  );
  const now = $derived(reading.now);
  // The chapter playing, named in the header where there's room for it.
  const chapterNow = $derived(cuts.isOf({ kind: "vod", id: vod.id }) ? cuts.chapterAt(now) : undefined);
  const playing = $derived(reading.playing);
  const fps = $derived(reading.fps);
  poll = requestAnimationFrame(read);
  // The transcript follows the playhead (cuts.svelte.ts), while its marks are this VOD's.
  $effect(() => {
    if (!cuts.isOf({ kind: "vod", id: vod.id })) return;
    cuts.now = reading.actual;
    cuts.playing = reading.playing;
  });
  // The VOD's length as the site said, or as the player found when the site didn't say.
  const duration = $derived(vod.duration || playerDuration);

  /** Moves the playhead, starting the player there (paused) if it isn't yet. */
  function seekTo(time: number) {
    reading.at(time);
    if (player) player.seek(time, false);
    else if (active) pending = { time, play: false };
    else activate(time, false);
  }

  function togglePlay() {
    if (player) player.togglePlay();
    else if (!active) activate(now);
  }

  // What a pill hovered in the clip bar (ClipStrip.svelte) is of this VOD: a
  // clip's stretch, or what an edit plays of it; or, with neither, what the
  // edit whose source link led here plays of it.
  const editRanges = $derived.by(() => {
    if (clips.hovered) return [clips.hovered];
    const lit = edits.lit?.vodId === vod.id ? edits.lit.editId : null;
    const id = edits.hovered ?? lit;
    return id === null ? [] : (edits.list.find((e) => e.id === id)?.ranges.filter((r) => r.vod_id === vod.id) ?? []);
  });

  // Clip (C): the stretch the playhead is in becomes a clip, a pill in the clip
  // bar. Hovering the button lights the stretch up on the timeline.
  let clipButton = $state<HTMLElement>();
  let clipHover = $state(false);
  const stretch = $derived(duration ? stretchAt(now, duration) : undefined);

  /** A time to the hundredth of a second, for marks: 1:02:03.25. */
  const precise = (t: number) => formatFraction(t, 2);

  // The line beside the controls: a fact or two (the shortcuts list says how,
  // and the line is kept for what's to come). ≈ marks times a site's player
  // can only show roughly.
  const context = $derived.by(() => {
    const approx = playsHere(vod) ? "" : "≈ ";
    const span = (a: number, b: number) => `${approx}${formatTime(a)}–${formatTime(b)} (${formatTime(b - a)})`;
    const picked = cuts.selection;
    if (picked && cuts.selectedEdge) return `${approx}${precise(picked[cuts.selectedEdge])}`;
    if (picked) return span(picked.start, picked.end);
    const selected = cuts.all.find((m) => m.id === cuts.selected);
    if (selected) return `${approx}${precise(selected.start)}${selected.text ? ` · ${selected.text}` : ""}`;
    if (stretch && between) return span(stretch.start, stretch.end);
    return "";
  });
  // Whether the playhead's stretch lies between two cuts (not the VOD's start or end).
  const between = $derived(
    !!stretch &&
      cuts.list.some((m) => Math.abs(m.start - stretch.start) < 1e-6) &&
      cuts.list.some((m) => Math.abs(m.start - stretch.end) < 1e-6),
  );

  /** Split: cuts both ends of the selection, else cuts here (or takes away the cut here). */
  function split() {
    if (cuts.selection) cuts.cutSelection();
    // Selected only when paused: cuts placed while watching leave the keys to the playhead.
    else cuts.toggle(now, fps, !playing);
  }

  /** Clip: the stretches picked on the timeline, each a clip (bulk.ts); else
   *  the selection, else the stretch the playhead is in (with no cuts, the
   *  whole video), as a clip in the clip bar. */
  async function makeClip() {
    if (chosenSegments().length) return clipSegments();
    const range = cuts.selection ? await cuts.cutSelection() : stretch;
    if (range) await exportStretch(range.start, range.end);
  }
  const canClip = $derived(!!cuts.selection || !!stretch || chosenSegments().length > 0);

  // Transcribe (T), at the bar's right beside the captions: the selection;
  // else the whole VOD, or what of it isn't transcribed yet (stretches.ts
  // `transcribeRest`), or all of it again once all of it is, after asking
  // (as the library row's Transcribe again: it replaces it all). Pressed and
  // held, a menu of how much from the playhead instead (TranscribeFrom.svelte,
  // hold.svelte.ts). Hovering it lights up on the timeline what it would
  // transcribe, as Clip does.
  let transcribeButton = $state<HTMLElement>();
  // Where the menu was opened: the stretch starts there, though the video plays on.
  let transcribeFrom = $state(0);
  const transcribeHold = new Hold(
    "[data-choice]",
    (item) => item.click(),
    () => (transcribeFrom = now),
  );
  let transcribeError = $state<string | null>(null);
  let transcribeHover = $state(false);
  // The parts not transcribed nor asked for yet: read as the button's hovered, for the rest to light up.
  let untranscribed = $state.raw<{ start: number; end: number }[]>([]);
  // Read for none transcribed too: the whole VOD asked for already (T pressed
  // once) leaves nothing to light, as nothing's left to ask.
  function onTranscribeEnter() {
    transcribeHover = true;
    if (vod.transcribed === "all") return;
    const id = vod.id;
    api.transcriptStretches(id).then(
      (s) => {
        if (id !== vod.id) return;
        const end = vod.duration ?? duration;
        untranscribed = gaps(s, vod.duration)
          .filter((g) => !g.request)
          .map((g) => ({ start: g.start, end: g.end ?? end }));
      },
      (e) => console.error("stretches", e),
    );
  }
  const transcribes = $derived.by(() => {
    if (cuts.selection) return [cuts.selection];
    if (vod.transcribed !== "all") return untranscribed;
    return duration ? [{ start: 0, end: duration }] : [];
  });
  // On the timeline, while Clip or Transcribe is hovered: what it would act on.
  const lit = $derived.by(() => {
    if (clipHover && canClip && !chosenSegments().length) {
      const range = cuts.selection ?? stretch;
      return range ? [range] : [];
    }
    return transcribeHover ? transcribes : [];
  });
  const transcribeTitle = $derived(
    `${
      cuts.selection
        ? "Transcribe the selection, replacing any lines there"
        : vod.transcribed === "some"
          ? "Transcribe the rest: what isn't transcribed yet"
          : vod.transcribed === "all"
            ? "Transcribe again…"
            : "Transcribe the whole VOD"
    }${keyHint("transcribe.here")}. Hold for how much from here.`,
  );
  function onTranscribeClick() {
    if (!transcribeHold.held) transcribe();
  }
  async function transcribe() {
    transcribeError = null;
    const picked = cuts.selection;
    try {
      if (picked) await api.transcribeStretch(vod.id, picked.start, picked.end);
      // The rest: of a VOD with none transcribed, all of it, unless that's
      // asked for already (pressed again while it waits).
      else if (vod.transcribed !== "all") await transcribeRest(vod, api);
      else if (
        await ask(
          `Transcribe “${vod.title}” again? Its current transcript will be replaced, and the audio may need to download again.`,
          "Transcribe again",
        )
      )
        await api.retranscribeVod(vod.id);
    } catch (e) {
      transcribeError = String(e);
    }
  }

  // Clip dragged rather than clicked: the lit-up stretch carried, as a clip's
  // pill is (dragout.ts carry), with no key held: onto an edit or the clip
  // bar, or past the window's edge into another app. Pressed and let go in
  // place, it's a click.
  let clipPress: { x: number; y: number } | null = null;
  let clipDragged = false;
  function onClipDown(e: PointerEvent) {
    if (e.button === 0 && canClip) clipPress = { x: e.clientX, y: e.clientY };
  }
  function onClipMove(e: PointerEvent) {
    if (!clipPress || Math.hypot(e.clientX - clipPress.x, e.clientY - clipPress.y) < DRAG_SLOP) return;
    // Its release missed (it happened elsewhere): a press no more.
    if (!(e.buttons & 1)) return void (clipPress = null);
    clipPress = null;
    const picked = cuts.selection;
    const range = picked ? { start: frameStart(picked.start, fps), end: frameStart(picked.end, fps) } : stretch;
    if (!range || range.end <= range.start) return;
    // The let-go may land on the button again: not a click too (the click
    // comes just after the carry ends).
    clipDragged = true;
    clipHover = false;
    const from = clipButton?.getBoundingClientRect() ?? new DOMRect(e.clientX, e.clientY);
    carry({
      vod,
      start: range.start,
      end: range.end,
      ownPlayer: playsFromFile(vod),
      from,
      x: e.clientX,
      y: e.clientY,
    }).then(() => setTimeout(() => (clipDragged = false)));
  }
  function onClipClick() {
    if (clipDragged) clipDragged = false;
    else makeClip();
  }

  // Full screen (the panel's, PlayerPanel.svelte): the player shown.
  let fullscreen = $state(false);
  function setFullscreen(on: boolean) {
    fullscreen = on;
    if (on) collapsed = false;
  }

  onDestroy(() => {
    destroyed = true;
    cancelAnimationFrame(poll);
    clearTimeout(copiedTimer);
    void changed.then((u) => u());
    player?.destroy();
    // The note asks about this VOD's clip.
    clips.answer(false);
  });

  // The note is shown with the controls.
  $effect(() => {
    if (collapsed) untrack(() => clips.answer(false));
  });

  // Keyboard control for whichever player is open. Keys pressed while the
  // player's own iframe has focus go to that player instead. Which key does
  // what is the map's (keys.svelte.ts); Escape is always Escape.
  function onKey(e: KeyboardEvent) {
    if (!forPlayer(e) || !canEmbed(vod) || !keysForApp()) return;
    // B cuts, as in video editors (also with Ctrl, or ⌘ on a Mac), C
    // makes a clip (bare: Ctrl+C is copy), and T transcribes, as the
    // button does.
    const cut = is(e, "cut.here");
    const clip = is(e, "clip.make");
    if (cut || clip || is(e, "transcribe.here")) {
      // Held down, it's one press.
      if (e.repeat) return e.preventDefault();
      collapsed = false;
      if (cut) split();
      else if (clip) makeClip();
      else transcribe();
      e.preventDefault();
      return;
    }
    // I and O put the selection's in and out points at the playhead, as in
    // video editors; with none, I or O begins one.
    const end = is(e, "selection.in") ? "i" : is(e, "selection.out") ? "o" : null;
    if (end) {
      const stops = cuts.list.map((m) => m.start);
      const picked = duration ? inOut(end, frameStart(now, fps), cuts.selection, stops, duration) : null;
      // The arrows go on moving the playhead, not an edge or a mark.
      if (picked) {
        cuts.selection = picked;
        cuts.selectedEdge = null;
        cuts.selected = null;
      }
      e.preventDefault();
      return;
    }
    // Shift with a letter is the player's own: captions (C) and mute (M).
    if (is(e, "captions.toggle") || is(e, "mute.toggle")) {
      if (is(e, "captions.toggle")) prefs.toggleCaptions();
      else toggleMute(controls);
      e.preventDefault();
      return;
    }
    // M marks a highlight here (a marker, as in video editors), N starts a
    // chapter, each with its title to write.
    const highlight = is(e, "mark.highlight");
    if (highlight || is(e, "mark.chapter")) {
      if (!e.repeat) cuts.add(highlight ? "highlight" : "chapter", now, fps);
      collapsed = false;
      e.preventDefault();
      return;
    }
    // The selected mark (or the selection's edge): renamed, moved, taken away.
    const pause = () => player?.playing() && player.togglePlay();
    if (markKey(e, { fps, length: duration, pause, seekTo })) {
      e.preventDefault();
      return;
    }
    // The clips picked in the clip bar go (a VOD's stretches picked aren't
    // removable: they're splits).
    if (is(e, "clips.remove") && chosenClips().length) {
      if (!e.repeat) void removeChosenClips();
      e.preventDefault();
      return;
    }
    if (!active && is(e, "play.toggle")) {
      activate(from);
      e.preventDefault();
      return;
    }
    const p = player;
    // No player yet (loading, or failed): only full screen is for us; Space
    // and the arrows keep their usual meaning (a focused button, scrolling).
    if (!p && !is(e, "fullscreen.toggle") && !(isEscape(e) && fullscreen)) return;
    const done = transportKey(
      e,
      {
        togglePlay: () => p?.togglePlay(),
        seekBy: (s) => p?.seek(Math.max(0, p.time() + s), false),
        step: p?.step && ((n) => p.step!(n)),
        fullscreen,
        setFullscreen,
      },
      is,
    );
    // preventDefault also stops Space from clicking a focused button, such
    // as the search result that was just clicked.
    if (done) e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} onpointermove={onClipMove} onpointerup={() => (clipPress = null)} />

<PlayerPanel
  bind:fullscreen
  shortcuts={vodShortcuts(framed, os, bindingsOf)}
  controls={canEmbed(vod) && !collapsed ? vodControls : undefined}
  {stage}
  {below}
>
  {#snippet head()}
    <div class="info">
      <strong title={vod.title}>{vod.title}</strong>
      <span class="meta muted">
        <span class="platform {vod.platform}">{site}</span>
        {#if vod.channel}<span class="sep">·</span>{vod.channel}{/if}
        {#if vod.duration}<span class="sep">·</span>{formatTime(vod.duration)}{/if}
      </span>
      {#if chapterNow?.text}
        <span class="chapter-now" title="Chapter: {chapterNow.text}"
          >{#if chapterNow.icon}<span class="icon" aria-hidden="true">{chapterNow.icon}</span
            >{/if}{chapterNow.text}</span
        >
      {/if}
      {#if from > 0 && isLocal}
        <span class="stamp"><span class="time">{formatTime(from)}</span></span>
      {:else if from > 0}
        <button
          class="stamp"
          class:copied={copied === "ok"}
          class:failed={copied === "failed"}
          onclick={copyLink}
          title="Copy a link to this moment"
        >
          <span class="time">{formatTime(from)}</span>
          <Icon name={copied === "ok" ? "check" : copied === "failed" ? "error" : "copy"} size={12} />
          {#if copied}<span class="copied-text">{copied === "ok" ? "Copied" : "Couldn't copy"}</span>{/if}
        </button>
      {/if}
    </div>
  {/snippet}
  {#snippet headEnd()}
    {#if isLocal}
      <button class="link open" onclick={() => api.showFile(vod.id).catch((e) => (error = String(e)))}>
        Show in folder
      </button>
    {:else}
      <button class="link open" onclick={() => openUrl(watchUrl(vod, from))}>
        Open on {site}
        <Icon name="external" size={12} />
      </button>
    {/if}
    {#if canEmbed(vod)}
      <button
        class="icon"
        onclick={() => (collapsed = !collapsed)}
        aria-expanded={!collapsed}
        aria-label={collapsed ? "Show player" : "Hide player"}
        title={collapsed ? "Show player" : "Hide player"}
      >
        <Icon name={collapsed ? "chevron-down" : "chevron-up"} size={16} />
      </button>
    {/if}
  {/snippet}
</PlayerPanel>

{#snippet stage()}
  {#if canEmbed(vod)}
    <div class="stage" class:collapsed class:audio={audioOnly}>
      <div class="frame" class:audio={audioOnly} class:own={active && playsHere(vod)}>
        {#if active}
          <div class="host" bind:this={host}></div>
        {:else if audioOnly}
          <button class="poster bar" onclick={() => activate(from)} aria-label="Play {vod.title}">
            <Icon name="play" size={16} /> Play from {formatTime(from)}
          </button>
        {:else}
          <button class="poster" onclick={() => activate(from)} aria-label="Play {vod.title}">
            {#if vod.thumb}<Thumb name={vod.thumb} />{/if}
            <span class="play"><Icon name="play" size={26} /></span>
          </button>
        {/if}
      </div>
    </div>
    {#if stripCue !== null && !collapsed}
      <!-- A site's player can't be drawn over: its captions show under it,
           a line kept for them (so nothing below moves as they change). -->
      <p class="caption-strip" aria-live="off">{stripCue}</p>
    {/if}
    {#if !collapsed}
      <Popover
        bind:open={() => !!clips.asking, (open) => !open && clips.answer(false)}
        anchor={clipButton}
        label="Before downloading a clip"
        width={340}
      >
        <PermissionNote action="Download clip" onanswer={(download, hide) => clips.answer(download, hide)} />
      </Popover>
    {/if}
  {:else if download}
    <div class="stage">
      <div class="frame">
        <button
          class="poster"
          bind:this={playButton}
          onclick={playByDownloading}
          aria-label="Play {vod.title}"
          disabled={downloading !== undefined}
        >
          {#if vod.thumb}<Thumb name={vod.thumb} />{/if}
          <span class="play"><Icon name={downloading !== undefined ? "download" : "play"} size={26} /></span>
        </button>
      </div>
    </div>
    <Popover bind:open={askingDownload} anchor={playButton} label="Before downloading the video" width={340}>
      <PermissionNote action="Download video" onanswer={answerDownload}>
        <p class="muted">
          {site} has no player VODscribe can show, so its video can only be downloaded: it plays here as it comes in. It's
          kept until you delete it.
        </p>
      </PermissionNote>
    </Popover>
  {/if}
{/snippet}

{#snippet vodControls()}
  {#if duration}
    <Scrubber
      {vod}
      memo={{ kind: "vod", id: vod.id }}
      {duration}
      time={now}
      shown={reading.shown}
      {fps}
      highlight={lit}
      ranges={editRanges}
      current={between ? stretch : null}
      ownPlayer={playsFromFile(vod)}
      onpause={() => player?.playing() && player.togglePlay()}
      onseek={seekTo}
    />
  {/if}
  <PlayerBar
    player={controls}
    {playing}
    now={reading.actual}
    {duration}
    volume={reading.volume}
    muted={reading.muted}
    rate={reading.rate}
    {fps}
    captionsAvailable={cues.length > 0}
    captionsNone={cues.length === 0}
    captionsOn={prefs.captions}
    {fullscreen}
    canFullscreen
    ontoggleplay={togglePlay}
    onfullscreen={setFullscreen}
  >
    {#snippet actions()}
      <button
        class="action split"
        class:picking={!!cuts.selection}
        onclick={split}
        title={cuts.selection
          ? `Cut both ends of the selection${keyHint("cut.here")}`
          : `Cut here, between this frame and the one before; each stretch between cuts can become a clip${keyHint("cut.here")}`}
      >
        <Icon name="scissors" size={14} /> Split {#if labelOf("cut.here")}<kbd>{labelOf("cut.here")}</kbd>{/if}
      </button>
      <button
        class="action clip"
        class:picking={!!cuts.selection}
        bind:this={clipButton}
        onpointerdown={onClipDown}
        onclick={onClipClick}
        onmouseenter={() => (clipHover = true)}
        onmouseleave={() => (clipHover = false)}
        disabled={!canClip}
        {...demoOff(
          "The web demo can't make clips. In the app, this cuts the lit-up stretch into a video file of its own.",
        )}
        title="Clip the lit-up stretch (or each one selected), to the clip bar{keyHint(
          'clip.make',
        )}: with no cuts, the whole video. Or drag it: onto an edit, or straight into your video editor."
      >
        <Icon name="film" size={14} /> Clip {#if labelOf("clip.make")}<kbd>{labelOf("clip.make")}</kbd>{/if}
      </button>
      {#if cuts.error}
        <span class="context error-text" title={cuts.error}>{cuts.error}</span>
      {:else if transcribeError}
        <span class="context error-text" title={transcribeError}>{transcribeError}</span>
      {:else}
        <span class="context muted" title={context}>{context}</span>
      {/if}
    {/snippet}
    {#snippet beside()}
      <button
        class="action transcribe"
        class:picking={!!cuts.selection}
        bind:this={transcribeButton}
        onpointerdown={transcribeHold.down}
        onclick={onTranscribeClick}
        onmouseenter={onTranscribeEnter}
        onmouseleave={() => (transcribeHover = false)}
        aria-expanded={transcribeHold.open}
        {...demoOff("The web demo can't transcribe. In the app, this transcribes the VOD, or the part selected.")}
        title={transcribeTitle}
      >
        <Icon name="audio-lines" size={14} /> Transcribe {#if labelOf("transcribe.here")}<kbd
            >{labelOf("transcribe.here")}</kbd
          >{/if}
      </button>
      <TranscribeFrom
        bind:open={transcribeHold.open}
        anchor={transcribeButton}
        {vod}
        from={transcribeFrom}
        above
        onerror={(e) => (transcribeError = e)}
      />
    {/snippet}
  </PlayerBar>
{/snippet}

{#snippet below()}
  {#if canEmbed(vod)}
    {#if download && downloading !== undefined}
      <p class="muted note">
        Downloading the video ({Math.round(downloading * 100)}%): it plays as far as it's in, and waits where it hasn't
        come yet.
      </p>
    {/if}
    {#if !collapsed && (clips.shown.length || clips.carryingAny || clips.error || edits.forVod(vod.id).length || snippets.size)}
      <ClipStrip {vod} ownPlayer={playsFromFile(vod)} onplay={playFrom} />
    {/if}
    {#if error}
      <p class="error-text note">
        {error}
        {#if isLocal}
          <button class="link" onclick={() => api.openFile(vod.id).catch((e) => (error = String(e)))}
            >Open it in your video player</button
          >
        {:else if vod.media && isUnplayable(error)}
          {playbackHint()} Meanwhile, use “Open on {site}”.
        {/if}
      </p>
    {/if}
  {:else}
    <!-- One short line: the note before downloading says why it's a
         download, and “Open on …” is in the header. -->
    <p class="muted note">
      {#if downloading !== undefined || (keepsVideo(vod) && vod.status === "downloading")}
        Downloading the video{downloading ? ` (${Math.round(downloading * 100)}%)` : ""}: it plays here once it's in.
      {:else if downloadError}
        <span class="error-text">The video couldn't be downloaded: {downloadError}</span> Press Play to try again.
      {:else if keepsVideo(vod)}
        Press Play to download the video and watch it here.
      {:else}
        {site} doesn't offer an embeddable VOD player. Use “Open on {site}” to watch from {formatTime(from)}.
      {/if}
    </p>
  {/if}
{/snippet}

<style>
  /* Full screen: the picture as large as fits (PlayerPanel.svelte). */
  :global(.fullscreen) .stage {
    display: flex;
  }

  :global(.fullscreen) .frame {
    flex: 1;
    width: 100%;
    max-height: none;
    aspect-ratio: auto;
  }

  .split :global(svg) {
    color: var(--cut);
  }

  /* A selection (made on the timeline or in the transcript) waits to be cut
     or clipped (or transcribed): Split, Clip and Transcribe take its color. (:not(:disabled) to win
     over the bar's own style for its buttons.) */
  .split.picking:not(:disabled),
  .clip.picking:not(:disabled),
  .transcribe.picking:not(:disabled) {
    color: var(--pick);
    background: color-mix(in srgb, var(--pick) 14%, transparent);
  }

  .split.picking :global(svg),
  .clip.picking :global(svg),
  .transcribe.picking :global(svg) {
    color: var(--pick);
  }

  .clip :global(svg) {
    color: var(--accent);
  }

  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 10px;
    align-items: baseline;
  }

  .info strong {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .meta {
    flex: none;
    font-size: 12px;
    white-space: nowrap;
  }

  /* A phone (the website's VOD page): the title over its details, and the
     actions without their keys, which a phone hasn't got. */
  @media (max-width: 560px) {
    .info {
      flex-wrap: wrap;
      row-gap: 2px;
    }

    .info strong {
      flex-basis: 100%;
    }

    .action kbd {
      display: none;
    }
  }

  /* The chapter playing: the first to give way when the header is narrow. */
  .chapter-now {
    flex: 0 4 auto;
    min-width: 0;
    font-size: 12px;
    color: var(--chapter);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chapter-now .icon {
    margin-right: 4px;
    font-family: "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", sans-serif;
  }

  .stamp {
    flex: none;
    gap: 4px;
    height: 22px;
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 12px;
    border-radius: var(--radius-sm);
  }

  .stamp:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }

  .stamp.copied {
    color: var(--ok);
  }

  .stamp.failed {
    color: var(--danger);
  }

  .open {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .stage {
    background: var(--stage);
  }

  /* Hidden, not unmounted: the player keeps its state and keeps playing. */
  .stage.collapsed {
    display: none;
  }

  .frame {
    aspect-ratio: 16 / 9;
    /* The website's demo, a window within a page, sets a smaller share of its own height. */
    max-height: var(--player-max-height, 44vh);
    margin: 0 auto;
  }

  /* The app's own player sizes its picture itself (LocalPlayer.svelte). */
  .frame.own {
    aspect-ratio: auto;
    max-height: none;
  }

  .host {
    width: 100%;
    height: 100%;
  }

  /* Audio alone: the controls in the panel's own colors, at their own height,
     rather than floating on a video's black stage. */
  .stage.audio {
    background: var(--surface);
    padding: 0 16px 8px;
  }

  .frame.audio {
    aspect-ratio: auto;
    height: auto;
    max-height: none;
  }

  .frame.audio .host {
    height: auto;
  }

  /* Audio alone, full screen: black, its captions large in the middle (LocalPlayer.svelte). */
  :global(.fullscreen) .stage.audio {
    background: #000;
    padding: 0;
  }

  :global(.fullscreen) .frame.audio .host {
    height: 100%;
  }

  .poster.bar {
    height: 40px;
    border-radius: var(--radius);
    align-items: center;
    justify-content: center;
    gap: 8px;
    background: var(--surface-2);
    color: var(--text);
    font-size: 13px;
  }

  .poster.bar:hover {
    background: var(--accent);
    color: var(--accent-text);
  }

  .poster {
    position: relative;
    display: flex;
    width: 100%;
    height: 100%;
    padding: 0;
    border: none;
    border-radius: 0;
    background: #000;
    overflow: hidden;
  }

  .poster :global(img) {
    opacity: 0.85;
    transition: opacity 0.15s;
  }

  .poster:hover :global(img) {
    opacity: 1;
  }

  .play {
    position: relative;
    z-index: 1;
    margin: auto;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding-left: 4px;
    background: rgb(0 0 0 / 0.6);
    color: #fff;
    transition:
      transform 0.15s,
      background 0.15s;
  }

  .poster:hover .play {
    transform: scale(1.06);
    background: var(--accent);
  }

  .note {
    margin: 0;
    padding: 0 16px 12px;
  }

  /* Two lines' room, the longest cut short: on the stage's dark, as the
     captions over the app's own picture are. */
  .caption-strip {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    box-sizing: content-box;
    height: calc(2 * 1.35em);
    margin: 0;
    padding: 6px 16px;
    background: var(--stage);
    color: var(--stage-text);
    font-size: 14px;
    line-height: 1.35;
    text-align: center;
  }

  :global(.fullscreen) .caption-strip {
    font-size: 22px;
  }
</style>
