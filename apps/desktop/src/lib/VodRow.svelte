<script lang="ts">
  import { settings as appSettings } from "./appSettings.svelte";
  import { hideNote, noteHidden } from "./permission";
  import PermissionNote from "./PermissionNote.svelte";
  import Popover from "./Popover.svelte";
  import { videoDownloads } from "./videos.svelte";
  import { edits } from "./edits.svelte";
  // The contents of one library row: thumbnail, title, status and actions. The
  // row element itself (selection, dragging) belongs to Library, since
  // animate:flip has to sit directly inside its keyed list.
  import { api, type Stretch, type Vod } from "./api";
  import { demoOff } from "./demo";
  import { ask, choose } from "./ask";
  import Icon from "./Icon.svelte";
  import { DEFAULT_HEIGHT, HEIGHTS, formatTime, hasSitePlayer, os, siteName, videoSize } from "./platform";
  import Progress from "./Progress.svelte";
  import Spinner from "./Spinner.svelte";
  import { covered as coveredBy, requestDetail, statusLabel } from "./stretches";
  import { DELETE, REMOVE, inEdits as inEditsOf, removeVod, removing as removals } from "./removal.svelte";
  import Thumb from "./Thumb.svelte";
  import { transcribe } from "./bulk";

  let {
    vod,
    reorderable,
    ontoggle,
    onkey,
    onremove,
    onerror,
    together,
  }: {
    vod: Vod;
    /** Whether the row can be dragged (there's more than one). */
    reorderable: boolean;
    ontoggle: () => void;
    onkey: (e: KeyboardEvent) => void;
    /** Called before the VOD is removed. */
    onremove: () => void;
    /** An action's error, or null when a new action starts. */
    onerror: (message: string | null) => void;
    /** Picked with others to transcribe (Library.svelte): how many, lighting them up, and transcribing them all. */
    together?: { count: number; lit: (on: boolean) => void; run: () => void };
  } = $props();

  // Nothing asked of it, and not all of it transcribed.
  const untouched = $derived(vod.status === "none");
  // Its stretches, read when the row says something of them: how much is
  // transcribed (for "some"), or the range of the request it shows. Read
  // again when what's asked changes, not at each progress event.
  let stretches = $state.raw<Stretch[]>([]);
  const wanted = $derived(vod.stretch !== null || (untouched && vod.transcribed === "some"));
  const asked = $derived(`${vod.stretch} ${vod.status === "none"} ${vod.transcribed} ${vod.waiting}`);
  $effect(() => {
    void asked;
    if (!wanted) return;
    let current = true;
    api.transcriptStretches(vod.id).then(
      (s) => current && (stretches = s),
      (e) => console.error("stretches", e),
    );
    return () => (current = false);
  });
  const covered = $derived.by(() => {
    const end = vod.duration ?? Infinity;
    return coveredBy(stretches).reduce((sum, [a, b]) => sum + Math.max(0, Math.min(b, end) - a), 0);
  });
  // After the request's state: its range, unless it's all of it, and how many more wait.
  const detail = $derived(
    requestDetail(stretches.find((s) => s.id === vod.stretch) ?? null, vod.duration, vod.waiting, formatTime),
  );
  const active = $derived(vod.status === "downloading" || vod.status === "transcribing");
  const showBar = $derived(active || (vod.status === "paused" && vod.progress > 0));
  const platform = $derived(siteName(vod));
  // A kept video can be deleted unless it's about to be used: done, paused or
  // failed (a failed transcription still has its download).
  const keptVideo = $derived(
    !!vod.media &&
      (vod.status === "done" || vod.status === "paused" || vod.status === "error" || vod.status === "none"),
  );

  // The whole video of a YouTube, Twitch or Kick VOD, downloaded on request to
  // play and clip here (videos.svelte.ts); other sites keep theirs anyway.
  videoDownloads.start();
  // Sites without their own player only play here once it's downloaded.
  const canKeep = $derived(vod.platform !== "local");
  const keeping = $derived(videoDownloads.progress.get(vod.id));
  const keepError = $derived(videoDownloads.errors.get(vod.id));
  const height = $derived(appSettings.value?.video_height ?? DEFAULT_HEIGHT);
  const sizeAt = (h: number) => (vod.duration ? `about ${videoSize(h, vod.duration / 3600)}` : "");
  const keepSize = $derived(sizeAt(height));
  // The quality picked in the note, for this download only.
  let pickedHeight = $state(DEFAULT_HEIGHT);
  let keepButton = $state<HTMLElement>();
  let asking = $state(false);
  function keep() {
    if (keeping !== undefined) {
      stopKeeping();
    } else if (noteHidden()) {
      videoDownloads.keep(vod.id);
    } else {
      pickedHeight = height;
      asking = true;
    }
  }
  function answer(download: boolean, hide: boolean) {
    asking = false;
    if (!download) return;
    // Not asked again: the quality picked becomes the one downloads use.
    if (hide) {
      hideNote();
      if (pickedHeight !== height) appSettings.update({ video_height: pickedHeight });
    }
    videoDownloads.keep(vod.id, pickedHeight);
  }
  /** A sentence about the edits with this VOD's clips, between `before` and `after`; empty if none. */
  const inEdits = (before: string, after: string) => inEditsOf(vod.id, before, after);

  async function stopKeeping() {
    if (
      await ask(
        `Stop downloading the video of “${vod.title}”? What's downloaded so far is deleted.`,
        "Stop",
        "Keep going",
      )
    ) {
      videoDownloads.stop(vod.id);
    }
  }

  /** Runs an action, reporting its error if it fails. */
  function act(action: Promise<unknown>) {
    onerror(null);
    action.catch((e) => onerror(String(e)));
  }

  // Only some of it transcribed, some of that by an older version: those
  // parts again, each a request, their lines replaced as the new ones come.
  async function retranscribeOutdated() {
    if (
      await ask(
        `Transcribe the parts of “${vod.title}” made with an older version again? Their lines will be replaced, and the audio may need to download again.`,
        "Transcribe again",
      )
    ) {
      act(api.retranscribeVod(vod.id, true));
    }
  }

  // The new transcript replaces the old one as it comes.
  async function retranscribe() {
    if (
      await ask(
        `Transcribe “${vod.title}” again? Its current transcript will be replaced, and the audio may need to download again.`,
        "Transcribe again",
      )
    ) {
      act(api.retranscribeVod(vod.id));
    }
  }

  // A kept video can be deleted to free up space. A site with its own player
  // plays it from there instead; any other site's plays here only once downloaded again.
  async function deleteVideo() {
    const after = hasSitePlayer(vod)
      ? `the video plays on ${platform} instead`
      : `the video plays here again once it's downloaded again (${platform} has no player VODscribe can show)`;
    const sure = await ask(
      `Delete the video of “${vod.title}” to free up space? The transcript stays, and ${after}.${inEdits(
        " Its clips in ",
        " stay as they are, but can't be made longer without downloading again.",
      )}`,
      "Delete video",
      "Keep",
    );
    if (sure) act(api.deleteVideo(vod.id));
  }

  async function cancel() {
    if (
      await ask(
        `Stop transcribing “${vod.title}”? What's transcribed so far is kept; what's downloaded for the rest is deleted.`,
        "Stop",
        "Keep going",
      )
    ) {
      act(api.cancelVod(vod.id));
    }
  }

  // Removing a VOD with a long transcript and busy chat takes a few seconds
  // (its text is erased, not just dropped); the row says so meanwhile, and
  // goes when it's done (removal.svelte.ts).
  const removing = $derived(removals.has(vod.id));
  // Removing a VOD that edits use asks whether its video goes too: then the
  // stretches they use are kept as videos of their own first.
  async function remove() {
    let deleteVideo = false;
    if (edits.forVod(vod.id).length) {
      const answer = await choose(
        `Remove “${vod.title}”? Its clips leave VODscribe (their files stay in your Videos folder).${inEdits(
          " Its pieces in ",
          " stay: removed from the library, its video stays on this computer for them; deleted, only the stretches they use are kept.",
        )}`,
        REMOVE,
        DELETE,
        "Keep",
      );
      if (!answer) return;
      deleteVideo = answer === DELETE;
    } else if (
      !(await ask(
        `Remove “${vod.title}”? Its transcript and any downloaded audio or video are erased from this computer, and its clips leave VODscribe (their files stay in your Videos folder).`,
        "Remove",
        "Keep",
      ))
    ) {
      return;
    }
    onremove();
    onerror(null);
    await removeVod(vod, deleteVideo, onerror);
  }
</script>

<button
  class="main"
  class:removing
  onclick={ontoggle}
  onkeydown={onkey}
  title={reorderable ? `Drag to reorder (${os === "mac" ? "Option" : "Alt"}+↑/↓)` : undefined}
>
  <span class="thumb {vod.platform}">
    <!-- The platform tile shows until (and unless) the thumbnail covers it. -->
    <Icon name="play" size={14} />
    {#if vod.thumb}<Thumb name={vod.thumb} />{/if}
    {#if vod.duration}<span class="duration">{formatTime(vod.duration)}</span>{/if}
  </span>
  <span class="text">
    <span class="title">{vod.title}</span>
    <span class="meta muted">
      <span class="platform {vod.platform}">{platform}</span>
      {#if vod.channel}<span class="sep">·</span>{vod.channel}{/if}
    </span>
  </span>
</button>

{#if removing}
  <div class="status-area">
    <span class="status">
      <Spinner label="Removing" />
      Removing…
    </span>
  </div>
{:else if vod.status !== "done" || showBar || vod.error || vod.outdated || vod.note}
  <div class="status-area">
    {#if untouched}
      <span class="status muted">
        {vod.transcribed === "some" && vod.duration
          ? `Transcribed ${formatTime(covered)} of ${formatTime(vod.duration)}`
          : vod.transcribed === "some"
            ? "Partly transcribed"
            : "Not transcribed"}
      </span>
    {:else if vod.status !== "done"}
      <span class="status" class:active class:error-text={vod.status === "error"}>
        {#if active}<span class="pulse" aria-hidden="true"></span>{/if}
        {#if vod.status === "error"}<Icon name="error" size={13} />{/if}
        {statusLabel[vod.status]}{#if showBar}<span class="pct">{Math.round(vod.progress * 100)}%</span>{/if}{detail}
      </span>
    {/if}
    {#if showBar}
      <Progress value={vod.progress} paused={vod.status === "paused"} thin />
    {/if}
    {#if vod.status === "error" && vod.error}
      <span class="note error-text" title={vod.error}>{vod.error}</span>
    {:else if vod.outdated}
      <span
        class="note warn-text"
        title="VODscribe's speech recognition has improved since this was made. Transcribe it again for a better transcript."
      >
        <Icon name="warning" size={13} />
        {vod.status === "done" || untouched ? "Made with an older version" : "Will restart with the newer version"}
      </span>
    {:else if vod.note}
      <span class="note muted" title={vod.note}>{vod.note}</span>
    {/if}
  </div>
{/if}

<div class="actions" class:removing inert={removing}>
  {#if untouched}
    <!-- Clicking is the decision: no question. The rest is each part not transcribed. -->
    {@const label = together
      ? `Transcribe all ${together.count} picked`
      : vod.transcribed === "some"
        ? "Transcribe the rest"
        : "Transcribe"}
    <!-- Picked with others: hovered, all it would transcribe light up; clicked, they're all asked for. -->
    <button
      class="row-act"
      onclick={() => (together ? together.run() : act(transcribe(vod)))}
      onmouseenter={() => together?.lit(true)}
      onmouseleave={() => together?.lit(false)}
      onfocus={() => together?.lit(true)}
      onblur={() => together?.lit(false)}
      title={label}
      aria-label={label}
      {...demoOff("The web demo can't transcribe. In the app, this transcribes the VOD.")}
    >
      <Icon name="captions" size={15} />
    </button>
    {#if vod.outdated}
      <button
        class="row-act warn"
        onclick={retranscribeOutdated}
        title="Transcribe again with the newer version: the parts made with an older one"
        aria-label="Transcribe again"
        {...demoOff("The web demo can't transcribe. In the app, this transcribes the VOD again.")}
      >
        <Icon name="redo" size={15} />
      </button>
    {/if}
  {:else if vod.status === "error"}
    <button class="row-act" onclick={() => act(api.retryVod(vod.id))} title="Retry" aria-label="Retry">
      <Icon name="redo" size={15} />
    </button>
  {:else if vod.status === "paused"}
    <!-- Resuming an outdated transcript starts over, like transcribing again. -->
    <button
      class="row-act"
      class:warn={vod.outdated}
      onclick={() => act(api.resumeVod(vod.id))}
      title={vod.outdated ? "Resume (starts over with the newer version)" : "Resume"}
      aria-label="Resume"
    >
      <Icon name="play" size={15} />
    </button>
    <button class="row-act" onclick={cancel} title="Cancel" aria-label="Cancel">
      <Icon name="x" size={15} />
    </button>
  {:else if vod.status !== "done"}
    <button class="row-act" onclick={() => act(api.pauseVod(vod.id))} title="Pause" aria-label="Pause">
      <Icon name="pause" size={15} />
    </button>
    <button class="row-act" onclick={cancel} title="Cancel" aria-label="Cancel">
      <Icon name="x" size={15} />
    </button>
  {:else}
    <button
      class="row-act"
      class:warn={vod.outdated}
      onclick={retranscribe}
      title={vod.outdated ? "Transcribe again with the newer version" : "Transcribe again"}
      aria-label="Transcribe again"
      {...demoOff("The web demo can't transcribe. In the app, this transcribes the VOD again.")}
    >
      <Icon name="redo" size={15} />
    </button>
  {/if}
  {#if canKeep && !vod.media}
    <button
      class="row-act keep"
      class:failed={keepError}
      bind:this={keepButton}
      onclick={keep}
      title={keeping !== undefined
        ? `Downloading the video: ${Math.round(keeping * 100)}%. Click to stop.`
        : keepError
          ? `The video couldn't be downloaded: ${keepError} Click to try again.`
          : hasSitePlayer(vod)
            ? `Download the video (up to ${height}p, ${keepSize}) to play it in VODscribe's own player, where frames can be stepped through exactly. Splitting and clipping work without it.`
            : `Download the video (up to ${height}p, ${keepSize}): ${platform} has no player VODscribe can show, so it plays here, as it downloads.`}
      aria-label={keeping !== undefined ? "Stop downloading the video" : "Download the video"}
      {...demoOff("The web demo can't download videos. In the app, this downloads the VOD's video to clip it.")}
    >
      {#if keeping !== undefined}
        <svg class="ring" viewBox="0 0 20 20" aria-hidden="true">
          <circle cx="10" cy="10" r="7.5" class="track" />
          <circle cx="10" cy="10" r="7.5" class="done" style:stroke-dasharray="{keeping * 47.1} 47.1" />
        </svg>
      {:else}
        <Icon name={keepError ? "warning" : "download"} size={15} />
      {/if}
    </button>
  {/if}
  {#if keptVideo}
    <button
      class="row-act"
      onclick={deleteVideo}
      title="Delete the video kept on this computer (the transcript stays)"
      aria-label="Delete video"
    >
      <Icon name="video-off" size={15} />
    </button>
  {/if}
  <button
    class="row-act"
    onclick={remove}
    title="Remove"
    aria-label="Remove"
    {...demoOff("The web demo keeps its VODs. In the app, this removes a VOD and its transcript.")}
  >
    <Icon name="trash" size={15} />
  </button>
</div>

{#if canKeep}
  <Popover bind:open={asking} anchor={keepButton} label="Before downloading the video" width={340}>
    <PermissionNote
      action="Download video"
      hideLabel="Don't show this again{pickedHeight !== height
        ? ` (and download at ${pickedHeight}p from now on)`
        : ''}"
      onanswer={answer}
    >
      <div class="quality" role="radiogroup" aria-label="Video quality">
        {#each HEIGHTS as h (h)}
          <button
            role="radio"
            aria-checked={pickedHeight === h}
            class:on={pickedHeight === h}
            onclick={() => (pickedHeight = h)}>{h}p</button
          >
        {/each}
      </div>
      <p class="muted">
        Up to {pickedHeight}p{sizeAt(pickedHeight) ? `, ${sizeAt(pickedHeight)}` : ""}. It's kept until you delete it.
      </p>
    </PermissionNote>
  </Popover>
{/if}

<style>
  /* Being removed: faded, and its actions out of reach. */
  .removing {
    opacity: 0.5;
  }

  .main {
    display: flex;
    gap: 10px;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    padding: 0;
    white-space: normal;
    align-items: flex-start;
  }

  .thumb {
    position: relative;
    flex: none;
    width: 80px;
    aspect-ratio: 16 / 9;
    border-radius: var(--radius-sm);
    background: var(--surface-3);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .thumb.youtube {
    background: color-mix(in srgb, var(--youtube) 18%, var(--surface-3));
  }

  .thumb.twitch {
    background: color-mix(in srgb, var(--twitch) 22%, var(--surface-3));
  }

  .thumb.kick {
    background: color-mix(in srgb, var(--kick) 20%, var(--surface-3));
  }

  .duration {
    position: absolute;
    right: 3px;
    bottom: 3px;
    padding: 0 4px;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.75);
    color: #fff;
    font-family: var(--mono);
    font-size: 10px;
    line-height: 16px;
    font-variant-numeric: tabular-nums;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .title {
    font-weight: 600;
    font-size: 13px;
    line-height: 1.35;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .meta {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .status-area {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 7px;
    font-size: 12px;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .pct {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .pulse {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1.6s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  /* Notes wrap, so an error reads in full. */
  .note {
    display: flex;
    align-items: flex-start;
    gap: 5px;
    min-width: 0;
    line-height: 1.4;
  }

  .note :global(svg) {
    flex: none;
  }

  .actions {
    display: flex;
    gap: 2px;
    justify-content: flex-end;
    margin-top: 4px;
    margin-right: -6px;
  }

  /* The video's download: where Delete video is once it's here. */
  .row-act.keep {
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .row-act.failed {
    color: var(--warn);
  }

  .ring {
    width: 15px;
    height: 15px;
    transform: rotate(-90deg);
  }

  .ring circle {
    fill: none;
    stroke-width: 2.5;
  }

  .ring .track {
    stroke: var(--surface-3);
  }

  .ring .done {
    stroke: var(--accent);
    transition: stroke-dasharray 0.25s linear;
  }

  .quality {
    display: flex;
    gap: 6px;
  }

  .quality button {
    flex: 1;
    height: 28px;
    font-size: 12px;
  }

  .quality button.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
  }

  .row-act.warn {
    color: var(--warn);
  }
</style>
