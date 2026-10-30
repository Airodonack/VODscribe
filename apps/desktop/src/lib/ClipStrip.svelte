<script lang="ts">
  // The clip bar: the open VOD's clips as pills under the player. Clicking one
  // picks it, as in a file manager; its play button plays the VOD from the
  // clip's start; double-clicking it (or Enter on its name, picked) opens it
  // in the editor; F2 on its name renames it. Once its file is made, it drags
  // out of the window (into a video editor or a folder: that saves a copy),
  // or onto an edit in the library's list (or on "Make new edit" there). Each
  // edit using this VOD is a link here too; on an edit's bar, the links are
  // the VODs its pieces come from (`links`).
  // The wheel scrolls the bar sideways, and so does a drag with the middle
  // button pressed, anywhere on it.
  // Hovering a pill lights up its stretch on the timeline, or for an edit's
  // link, what the edit plays of the VOD. An edit's clip bar (no `vod`)
  // has its clips of stretches of the edit: dragged, they go as picked ones
  // do (bulk.ts `carrySnippets`), into an edit or out as files.
  //
  // Ctrl-click (⌘ on a Mac) or Shift-click pills to pick several, or drag a
  // box over them from empty space in the bar (selection.svelte.ts). The
  // keys and buttons are the same for several as for one: C or Clip makes a
  // clip of each stretch picked, Delete takes what's picked, and a pill
  // picked with others carries them all when dragged (bulk.ts `carryChosen`).
  // Without a mouse, A asks which edit what's picked (pills and stretches of
  // the timeline) goes in (AddToEdit.svelte), as on a highlight's row.

  import type { Clip, Vod } from "./api";
  import { clips } from "./clips.svelte";
  import { carry } from "./dragout";
  import { holds, pressToDrag, type Pressed } from "./pressToDrag";
  import Icon from "./Icon.svelte";
  import { formatTime } from "./platform";
  import Spinner from "./Spinner.svelte";
  import { edits } from "./edits.svelte";
  import { keyHint } from "./keys.svelte";
  import { carryChosen, carrySnippets, chosenPuts } from "./bulk";
  import AddToEdit from "./AddToEdit.svelte";
  import { forPlayer } from "./playerKeys";
  import { dragBox, howOf, overlaps, snippets, type Snippet } from "./selection.svelte";

  let {
    vod,
    ownPlayer = false,
    onplay,
    links,
  }: {
    vod?: Vod;
    ownPlayer?: boolean;
    /** Plays the VOD from `time` (a pill's play button; a VOD's clip bar only). */
    onplay?: (time: number) => void;
    /** Links of the bar's owner's own, where a VOD's bar has its edits' links. */
    links?: import("svelte").Snippet;
  } = $props();

  const itemOf = (clip: Clip): Snippet => ({ kind: "clip", id: clip.id });
  /** A click on a pill picks it alone (as in a file manager; on the one
   *  picked alone, lets it go), a Ctrl-click with the others, a Shift-click
   *  the run to it (Ctrl+Shift-click adding the run). Its buttons and its
   *  name being edited keep their clicks. */
  function onChipClick(e: MouseEvent, clip: Clip) {
    if ((e.target as HTMLElement).closest(".act, input")) return;
    // A double-click's second click doesn't pick (the double-click opens it).
    // (The click a drag leaves behind never comes here: pressToDrag.)
    if (e.detail > 1) return;
    const how = howOf(e);
    if (how === "plain") snippets.only(itemOf(clip));
    else snippets.click(itemOf(clip), how, clips.shown.map(itemOf));
  }
  function onChipDoubleClick(e: MouseEvent, clip: Clip) {
    if (!(e.target as HTMLElement).closest(".act, input")) edits.openEdit(clip.id);
  }
  /** The pills a box drawn over the bar touches, in their order. */
  const boxed = (box: DOMRect) =>
    clips.shown
      .filter((c) => {
        const pill = strip?.querySelector(`.chip[data-clip="${c.id}"]`);
        return pill && overlaps(box, pill.getBoundingClientRect());
      })
      .map(itemOf);
  let strip = $state<HTMLElement>();
  // A clip picked that's gone another way (its own ×) goes from the set too.
  $effect(() => {
    const ids = new Set(clips.shown.map((c) => c.id));
    snippets.keep((i) => i.kind !== "clip" || ids.has(i.id));
  });

  let renaming = $state<number | null>(null);
  let draft = $state("");

  function startRename(clip: Clip) {
    renaming = clip.id;
    draft = clip.title;
  }

  function finishRename(clip: Clip) {
    if (renaming !== clip.id) return;
    renaming = null;
    if (draft.trim() && draft.trim() !== clip.title) clips.rename(clip.id, draft);
  }

  function focus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  /** Picked with others (more clips, or stretches on the timeline): a drag of it carries them all. */
  const withOthers = (clip: Clip) => snippets.size > 1 && snippets.has(itemOf(clip));

  // A made clip's chip drags out once the pointer moves with the button down
  // (one picked with others, made or not, carries what's picked; picked
  // alone, it goes as it would unpicked); the click that may follow a drag
  // doesn't pick it (pressToDrag).
  function canDrag(e: PointerEvent, clip: Clip) {
    const ready = clip.status === "ready" || withOthers(clip);
    return ready && !(e.target as HTMLElement).closest(".act, input");
  }

  // The middle button pressed anywhere on the bar (a pill too) and dragged:
  // the bar scrolls with the pointer, as if held. Nothing's picked or carried.
  let pan = $state<{ x: number; left: number } | null>(null);
  function onBarDown(e: PointerEvent) {
    if (e.button !== 1 || !strip || (e.target as HTMLElement).closest("input")) return;
    pan = { x: e.clientX, left: strip.scrollLeft };
  }
  // Not the system's own use of the middle button: scrolling by the
  // pointer's distance (Windows), or pasting.
  function onBarMouseDown(e: MouseEvent) {
    if (e.button === 1 && !(e.target as HTMLElement).closest("input")) e.preventDefault();
  }

  // The wheel over the bar scrolls it sideways (it has nowhere to go up or
  // down); one turned sideways (or Shift held, where that's how it's sent)
  // scrolls it as ever. Ctrl and the wheel stay the system's.
  function onWheel(e: WheelEvent) {
    if (!strip || e.ctrlKey || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    if (strip.scrollWidth <= strip.clientWidth) return;
    const unit =
      e.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? 40
        : e.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? strip.clientWidth
          : 1;
    strip.scrollLeft += e.deltaY * unit;
    e.preventDefault();
  }

  function onPointerMove(e: PointerEvent) {
    if (!pan || !strip) return;
    // Its release missed (it happened outside the window): over.
    if (!holds(e, 1)) pan = null;
    else strip.scrollLeft = pan.left - (e.clientX - pan.x);
  }
  function onChipDrag(e: PointerEvent, clip: Clip, { x, y }: Pressed) {
    const from = document.querySelector(`.chip[data-clip="${clip.id}"]`)?.getBoundingClientRect() ?? new DOMRect(x, y);
    // Picked with others: all of what's picked goes, wherever it's dropped.
    if (withOthers(clip)) return carryChosen(from, e.clientX, e.clientY);
    // On an edit's clip bar, as what's picked would go: into an edit, or out of the window as its file.
    if (!vod) return carrySnippets([itemOf(clip)], from, e.clientX, e.clientY);
    carry({
      vod,
      start: clip.start,
      end: clip.end,
      title: clip.title,
      clip: clip.id,
      ownPlayer,
      from,
      x: e.clientX,
      y: e.clientY,
    });
  }
  // Each edit with pieces of this VOD: a link to it. An edit's pieces are
  // copies, so the pills are the bar's own, and deleting one changes no edit.
  const linked = $derived(vod ? edits.forVod(vod.id) : []);

  const span = (start: number, end: number) => `${formatTime(start)}–${formatTime(end)}`;

  function describe(clip: Clip): string {
    if (clip.status === "error")
      return `Couldn't make this clip: ${clip.error ?? "unknown error"}. Clip the stretch again${keyHint("clip.make")} to retry.`;
    if (clip.status === "cutting") return "Making the clip…";
    if (clip.status === "waiting") return "Waiting to be made";
    return clip.title;
  }

  // A, with something picked: which edit it goes in.
  let adding = $state(false);
  function onKey(e: KeyboardEvent) {
    if (e.key.toLowerCase() !== "a" || e.ctrlKey || e.metaKey || e.altKey || !snippets.size || !forPlayer(e)) return;
    e.preventDefault();
    if (!e.repeat) adding = true;
  }
</script>

<svelte:window onkeydown={onKey} onpointermove={onPointerMove} onpointerup={() => (pan = null)} />

<!-- The pointer's own ways of scrolling it (the wheel, a middle-button drag); the keys are its pills'. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="strip clip-bar"
  class:over={clips.overBar}
  class:panning={pan !== null}
  role="group"
  aria-label="Clips"
  bind:this={strip}
  onpointerdown={onBarDown}
  onmousedown={onBarMouseDown}
  onwheel={onWheel}
  use:dragBox={{ set: snippets, hits: boxed, onempty: () => snippets.clear() }}
>
  {#each linked as edit (edit.id)}
    <button
      class="chip edit-link"
      onclick={() => edits.openEdit(edit.id)}
      onmouseenter={() => (edits.hovered = edit.id)}
      onmouseleave={() => edits.hovered === edit.id && (edits.hovered = null)}
      title="“{edit.title}” uses this VOD where it lights up on the timeline · open the edit"
    >
      <Icon name="film" size={13} />
      <span class="name">{edit.title}</span>
      <Icon name="external" size={11} />
    </button>
  {/each}
  {@render links?.()}
  {#each clips.shown as clip (clip.id)}
    {@const progress = clips.progress.get(clip.id)}
    {@const picked = snippets.has(itemOf(clip))}
    {@const n = snippets.number(itemOf(clip))}
    <!-- Its buttons have the focus and the keys. -->
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      class="chip {clip.status}"
      class:chosen={picked}
      role="option"
      aria-selected={picked}
      data-clip={clip.id}
      title="{vod ? span(clip.start, clip.end) : formatTime(clip.length ?? 0)} · {describe(clip)}{clip.status ===
      'ready'
        ? ' · drag it into a video editor or a folder'
        : ''} · double-click to open it in the editor"
      use:pressToDrag={{ press: (e) => canDrag(e, clip), start: (e, at) => onChipDrag(e, clip, at) }}
      onclickcapture={(e) => onChipClick(e, clip)}
      ondblclick={(e) => onChipDoubleClick(e, clip)}
      onmouseenter={() => vod && (clips.hovered = { start: clip.start, end: clip.end })}
      onmouseleave={() => (clips.hovered = null)}
    >
      {#if onplay}
        <button
          class="act play"
          onclick={() => onplay(clip.start)}
          title="Play from the clip's start"
          aria-label="Play {clip.title}"
        >
          <Icon name="play" size={12} />
        </button>
      {/if}
      <span class="time" class:after-play={!!onplay}>
        {#if n !== null}<span class="order">{n}</span>{/if}
        <span class="length">{formatTime(clip.length ?? clip.end - clip.start)}</span>
      </span>
      {#if renaming === clip.id}
        <input
          class="name"
          bind:value={draft}
          use:focus
          onblur={() => finishRename(clip)}
          onkeydown={(e) => {
            if (e.key === "Enter") finishRename(clip);
            else if (e.key === "Escape") renaming = null;
            e.stopPropagation();
          }}
          aria-label="Clip name"
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
        />
      {:else}
        <!-- Clicked, it has the keys: Enter opens the clip in the editor once
             it's picked (as a click picks it), else renames it, as F2 does. -->
        <span
          class="name"
          role="button"
          tabindex="0"
          title="Double-click to open it in the editor · F2 renames it"
          onkeydown={(e) => {
            if (e.key === "Enter" && picked) edits.openEdit(clip.id);
            else if (e.key === "Enter" || e.key === "F2") startRename(clip);
            else return;
            e.preventDefault();
            e.stopPropagation();
          }}>{clip.title}</span
        >
      {/if}
      {#if clip.status === "cutting"}
        <Spinner size={11} />
      {:else if clip.status === "error"}
        <span class="bad"><Icon name="error" size={13} /></span>
      {:else if clip.status === "ready"}
        <button class="act" onclick={() => clips.show(clip.id)} title="Show in folder" aria-label="Show in folder">
          <Icon name="folder" size={13} />
        </button>
      {/if}
      <button class="act" onclick={() => clips.remove(clip.id)} title="Delete clip" aria-label="Delete clip">
        <Icon name="x" size={13} />
      </button>
      {#if progress !== undefined && progress > 0}
        <span class="progress" style:width="{progress * 100}%"></span>
      {:else if clip.status === "cutting"}
        <!-- Under way, how far not known yet: a stripe that moves. -->
        <span class="progress busy"></span>
      {/if}
    </div>
  {/each}
  {#if clips.carrying && !clips.shown.some((c) => c.start === clips.carrying?.start && c.end === clips.carrying?.end)}
    <span class="drop muted">Drop here to keep it as a clip</span>
  {:else if clips.carried && clips.overBar}
    <span class="drop muted">Let go to keep them as clips</span>
  {/if}
</div>

<AddToEdit bind:open={adding} anchor={strip} items={chosenPuts} />

<style>
  /* A link to an edit with pieces of this VOD: tinted, dashed, with an arrow. */
  .chip.edit-link {
    gap: 6px;
    padding: 0 10px;
    border-style: dashed;
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    background: var(--accent-soft);
    color: var(--text);
    font-size: 12px;
    cursor: pointer;
  }

  .chip.edit-link :global(svg) {
    flex: none;
    color: var(--accent);
  }

  .chip.edit-link:hover {
    border-style: solid;
  }

  .strip.over {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .drop {
    font-size: 12px;
    padding: 0 6px;
    white-space: nowrap;
  }

  .strip {
    user-select: none;
    -webkit-user-select: none;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 40px;
    padding: 6px 16px;
    /* The page's own scrollbar (app.css), below the pills. Not
       scrollbar-width: thin, which WebKitGTK draws as an overlay whose hit
       area covers the lower half of the bar: a pill's middle, and its
       buttons, then reach the bar instead of the pill. */
    overflow-x: auto;
  }

  .chip {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    max-width: 260px;
    height: 28px;
    padding: 0 2px 0 0;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .chip.ready {
    cursor: grab;
  }

  /* Scrolled by a middle-button drag: held, wherever the pointer is. */
  .strip.panning,
  .strip.panning :global(*) {
    cursor: grabbing;
  }

  /* Picked (a click; Ctrl- or Shift-click, or a box, for several). */
  .chip.chosen {
    border-color: var(--accent);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .time .order {
    margin-right: 5px;
  }

  .chip.error {
    border-color: var(--danger);
  }

  .chip.waiting .name {
    color: var(--muted);
  }

  .time {
    flex: none;
    display: flex;
    align-items: center;
    height: 100%;
    padding: 0 8px 0 10px;
    color: var(--accent);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  /* The play button first, at the pill's round end; the length close by it. */
  .act.play {
    margin-left: 3px;
    color: var(--accent);
  }

  .time.after-play {
    padding-left: 2px;
  }

  .name {
    min-width: 0;
    padding: 0 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }

  input.name {
    user-select: text;
    -webkit-user-select: text;
    width: 160px;
    height: 22px;
    padding: 0 6px;
  }

  .act {
    flex: none;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .act:hover {
    background: var(--surface-3);
    color: var(--text);
  }

  .bad {
    display: inline-flex;
    color: var(--danger);
    padding: 0 2px;
  }

  .progress {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    background: var(--accent);
    transition: width 0.25s linear;
  }

  .progress.busy {
    width: 30%;
    animation: busy 1.2s ease-in-out infinite;
  }

  @keyframes busy {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }
</style>
