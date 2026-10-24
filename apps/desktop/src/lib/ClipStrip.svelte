<script lang="ts">
  // The clip bar: the open VOD's clips as pills under the player. Clicking one
  // plays it; double-clicking its name (or Enter or F2 on it) renames it; once its file is made, it
  // drags out of the window (into a video editor or a folder), or onto an
  // edit in the library's list (or on "Make new edit" there). "Add to an edit"
  // picks one from a list. Each edit using this VOD is a link here too.
  // Hovering a pill lights up its stretch on the timeline, or for an edit's
  // link, what the edit plays of the VOD. An edit's clip bar (no `vod`)
  // has its clips of stretches of the edit: they drag out as files.
  //
  // Ctrl-click (⌘ on a Mac) or Shift-click pills to pick several, or drag a
  // box over them from empty space in the bar (selection.svelte.ts); what's
  // picked, with the stretches picked on the timeline, shows first, with
  // what can be done with it all ("Add to edit…" among it). A picked pill
  // dragged carries them all (bulk.ts `carryChosen`).

  import { api, type Clip, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import { DRAG_SLOP, carry, dragFileOut } from "./dragout";
  import Icon from "./Icon.svelte";
  import { formatTime } from "./platform";
  import Spinner from "./Spinner.svelte";
  import Popover from "./Popover.svelte";
  import { edits } from "./edits.svelte";
  import { keyHint, labelOf } from "./keys.svelte";
  import AddToEdit from "./AddToEdit.svelte";
  import { carryChosen, chosenClips, chosenPuts, chosenSegments, clipSegments, removeChosenClips } from "./bulk";
  import { count, dragBox, overlaps, picks, snippets, type Snippet } from "./selection.svelte";

  let {
    vod,
    ownPlayer = false,
    onplay,
    onremove = removeChosenClips,
  }: {
    vod?: Vod;
    ownPlayer?: boolean;
    onplay?: (time: number) => void;
    /** Removes what's picked (an edit's own clip bar takes its pieces too). */
    onremove?: () => void;
  } = $props();

  const itemOf = (clip: Clip): Snippet => ({ kind: "clip", id: clip.id });
  /** A Ctrl- or Shift-click on a pill picks it (or the run to it), rather than playing it. */
  function onChipClick(e: MouseEvent, clip: Clip) {
    if ((e.target as HTMLElement).closest(".act, input")) return;
    if (!picks(e)) return void snippets.plain(itemOf(clip));
    e.preventDefault();
    e.stopPropagation();
    if (e.shiftKey) snippets.rangeTo(itemOf(clip), clips.shown.map(itemOf));
    else snippets.toggle(itemOf(clip));
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

  // What's picked: how much, and what can be done with it.
  const segmentsPicked = $derived(chosenSegments().length);
  const clipsPicked = $derived(chosenClips().length);
  /** Remove: the clips picked, and in an edit the stretches too (a VOD's are its splits). */
  const removable = $derived(clipsPicked > 0 || (!vod && segmentsPicked > 0));

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

  // A made clip's chip drags out once the pointer moves with the button down
  // (a picked one, made or not, carries what's picked); the click that may
  // follow a drag that didn't start doesn't play it.
  let press: { clip: Clip; x: number; y: number } | null = null;
  let swallowClick = false;
  function onChipDown(e: PointerEvent, clip: Clip) {
    const target = e.target as HTMLElement;
    // Picked, it carries what's picked, made or not.
    const ready = clip.status === "ready" || snippets.has(itemOf(clip));
    if (e.button !== 0 || !ready || target.closest(".act, input")) return;
    press = { clip, x: e.clientX, y: e.clientY };
  }
  function onPointerMove(e: PointerEvent) {
    if (!press || Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_SLOP) return;
    const { clip, x, y } = press;
    press = null;
    swallowClick = true;
    setTimeout(() => (swallowClick = false), 500);
    const from = document.querySelector(`.chip[data-clip="${clip.id}"]`)?.getBoundingClientRect() ?? new DOMRect(x, y);
    // One picked: all of what's picked goes, wherever it's dropped.
    if (snippets.has(itemOf(clip))) return carryChosen(from, e.clientX, e.clientY);
    if (!vod) return void dragFileOut([clip]);
    carry({ vod, start: clip.start, end: clip.end, title: clip.title, ownPlayer, from, x: e.clientX, y: e.clientY });
  }
  function play(time: number) {
    if (!swallowClick) onplay?.(time);
    swallowClick = false;
  }

  async function save(clip: Clip) {
    try {
      await api.saveClip(clip.id);
    } catch (e) {
      clips.error = String(e);
    }
  }

  // "Add to edit…" for what's picked: the edits, then where in one.
  let addingAll = $state(false);
  let addAllButton = $state<HTMLElement>();

  // "Add to edit": the edits to pick from, by the clip's button.
  let adding = $state<{ clip: Clip; anchor: HTMLElement } | null>(null);
  let added = $state<string | null>(null);
  async function addTo(editId: number | null) {
    const clip = adding?.clip;
    adding = null;
    if (!clip) return;
    const edit = editId === null ? await edits.create() : edits.list.find((e) => e.id === editId);
    if (!edit) return;
    await edits.add(edit.id, clip.id);
    added = edits.error ? null : `Added to “${edit.title}”`;
    setTimeout(() => (added = null), 2500);
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
</script>

<svelte:window onpointermove={onPointerMove} onpointerup={() => (press = null)} />

<div
  class="strip clip-bar"
  class:over={clips.overBar}
  role="group"
  aria-label="Clips"
  bind:this={strip}
  use:dragBox={{ set: snippets, hits: boxed, onempty: () => snippets.clear() }}
>
  {#if snippets.size}
    <!-- What's picked here and on the timeline, and what can be done with it. -->
    <div class="picked" role="status">
      <span class="count"
        >{count([
          [segmentsPicked, "stretch", "stretches"],
          [clipsPicked, "clip", "clips"],
        ])}</span
      >
      {#if segmentsPicked}
        <button
          class="link"
          onclick={clipSegments}
          title="A clip of each stretch picked, in their order{keyHint('clip.make')}"
          >Clip {#if labelOf("clip.make")}<kbd>{labelOf("clip.make")}</kbd>{/if}</button
        >
      {/if}
      <button
        class="link"
        bind:this={addAllButton}
        onclick={() => (addingAll = !addingAll)}
        title="Put them in an edit, in the order picked: on its timeline or its clip bar">Add to edit…</button
      >
      {#if removable}
        <button
          class="link"
          onclick={onremove}
          title="Remove what's picked{keyHint(vod ? 'clips.remove' : 'edit.remove')}">Remove</button
        >
      {/if}
      <button
        class="act"
        onclick={() => snippets.clear()}
        title="Let go of them (Esc)"
        aria-label="Clear the selection"
      >
        <Icon name="x" size={13} />
      </button>
    </div>
  {/if}
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
        : ''}"
      onpointerdown={(e) => onChipDown(e, clip)}
      onclickcapture={(e) => onChipClick(e, clip)}
      onmouseenter={() => vod && (clips.hovered = { start: clip.start, end: clip.end })}
      onmouseleave={() => (clips.hovered = null)}
    >
      <button class="play" onclick={() => play(clip.start)} aria-label="Play {clip.title}">
        {#if n !== null}<span class="order">{n}</span>{/if}
        <span class="length">{formatTime(clip.length ?? clip.end - clip.start)}</span>
      </button>
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
        <!-- Double-click opens it in the editor; Enter or F2 on it renames it. -->
        <span
          class="name"
          role="button"
          tabindex="0"
          title="Double-click to open it in the editor · Enter or F2 renames it"
          ondblclick={() => edits.openEdit(clip.id)}
          onkeydown={(e) => {
            if (e.key !== "Enter" && e.key !== "F2") return;
            startRename(clip);
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
        <button class="act" onclick={() => save(clip)} title="Save a copy…" aria-label="Save a copy">
          <Icon name="download" size={13} />
        </button>
        <button class="act" onclick={() => clips.show(clip.id)} title="Show in folder" aria-label="Show in folder">
          <Icon name="folder" size={13} />
        </button>
      {/if}
      <button
        class="act"
        onclick={(e) => (adding = { clip, anchor: e.currentTarget as HTMLElement })}
        title="Add to an edit"
        aria-label="Add to an edit"
      >
        <Icon name="plus" size={13} />
      </button>
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
  {#if clips.error}
    <span class="error-text">{clips.error}</span>
  {/if}
  {#if added}
    <span class="muted added">{added}</span>
  {/if}
</div>

<AddToEdit bind:open={addingAll} anchor={addAllButton} items={chosenPuts} />

<Popover
  bind:open={() => !!adding, (open) => !open && (adding = null)}
  anchor={adding?.anchor}
  label="Add to an edit"
  width={240}
  padding={6}
>
  <div class="pick">
    {#each edits.list as edit (edit.id)}
      <button onclick={() => addTo(edit.id)}>{edit.title}</button>
    {/each}
    <button class="new" onclick={() => addTo(null)}><Icon name="plus" size={13} /> New edit</button>
  </div>
</Popover>

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
    overflow-x: auto;
    scrollbar-width: thin;
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

  /* Picked with others (Ctrl- or Shift-click, or a box). */
  .chip.chosen {
    border-color: var(--accent);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .play .order {
    margin-right: 5px;
  }

  /* What's picked, first in the bar. */
  .picked {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 4px 0 2px;
    font-size: 12px;
    white-space: nowrap;
  }

  .picked .count {
    font-weight: 600;
  }

  .picked .link {
    color: var(--accent);
    font-size: 12px;
  }

  .chip.error {
    border-color: var(--danger);
  }

  .chip.waiting .name {
    color: var(--muted);
  }

  .play {
    flex: none;
    height: 100%;
    padding: 0 8px 0 10px;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--accent);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .play:hover {
    background: var(--surface-3);
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

  .error-text {
    font-size: 12px;
    white-space: nowrap;
  }

  .added {
    font-size: 12px;
    align-self: center;
  }

  .pick {
    display: flex;
    flex-direction: column;
  }

  .pick button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    font-size: 13px;
  }

  .pick button:hover {
    background: var(--surface-2);
  }

  .pick .new {
    color: var(--accent);
  }
</style>
