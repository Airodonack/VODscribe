<script lang="ts">
  // The clip bar: the open VOD's clips as pills under the player. Clicking one
  // plays it; double-clicking its name renames it; once its file is made, it
  // drags out of the window (into a video editor or a folder), or onto an
  // edit's pill. The edits with clips of this VOD come first, as pills that
  // open them; while a clip is carried, each shows a + to drop it on, and
  // "Make new edit" pops up above the bar. "Add to an edit" picks one from a list.

  import { api, type Clip, type Vod } from "./api";
  import { clips } from "./clips.svelte";
  import { DRAG_SLOP, carry } from "./dragout";
  import Icon from "./Icon.svelte";
  import { formatTime } from "./platform";
  import Spinner from "./Spinner.svelte";
  import Popover from "./Popover.svelte";
  import { edits } from "./edits.svelte";

  let { vod, ownPlayer, onplay }: { vod: Vod; ownPlayer: boolean; onplay: (time: number) => void } = $props();

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

  // A made clip's chip drags out once the pointer moves with the button down;
  // the click that may follow a drag that didn't start doesn't play it.
  let press: { clip: Clip; x: number; y: number } | null = null;
  let swallowClick = false;
  function onChipDown(e: PointerEvent, clip: Clip) {
    const target = e.target as HTMLElement;
    if (e.button !== 0 || clip.status !== "ready" || target.closest(".act, input")) return;
    press = { clip, x: e.clientX, y: e.clientY };
  }
  function onPointerMove(e: PointerEvent) {
    if (!press || Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_SLOP) return;
    const { clip, x, y } = press;
    press = null;
    swallowClick = true;
    setTimeout(() => (swallowClick = false), 500);
    const from = document.querySelector(`.chip[data-clip="${clip.id}"]`)?.getBoundingClientRect() ?? new DOMRect(x, y);
    carry({ vod, start: clip.start, end: clip.end, ownPlayer, from, x: e.clientX, y: e.clientY });
  }
  function play(time: number) {
    if (!swallowClick) onplay(time);
    swallowClick = false;
  }

  async function save(clip: Clip) {
    try {
      await api.saveClip(clip.id);
    } catch (e) {
      clips.error = String(e);
    }
  }

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

  const span = (start: number, end: number) => `${formatTime(start)}–${formatTime(end)}`;

  function describe(clip: Clip): string {
    if (clip.status === "error")
      return `Couldn't make this clip: ${clip.error ?? "unknown error"}. Export it again to try again.`;
    if (clip.status === "cutting") return "Making the clip…";
    if (clip.status === "waiting") return "Waiting to be made";
    return clip.title;
  }
</script>

<svelte:window onpointermove={onPointerMove} onpointerup={() => (press = null)} />

<div class="clip-bar-wrap">
  {#if clips.carrying}
    <div class="new-edit" class:target={edits.dropTarget === "new"} data-new-edit>
      <Icon name="plus" size={14} /> Make new edit
    </div>
  {/if}
  <div class="strip clip-bar" class:over={clips.overBar} aria-label="Clips">
    {#each edits.forVod(vod.id) as edit (edit.id)}
      <button
        class="chip edit-chip"
        class:carrying={!!clips.carrying}
        class:target={edits.dropTarget === edit.id}
        class:open={edits.openId === edit.id}
        data-edit-drop={edit.id}
        onclick={() => edits.openEdit(edit.id)}
        title={clips.carrying
          ? `Drop to add it to “${edit.title}”`
          : `Edit: ${edit.title} · ${edit.pieces} piece${edit.pieces === 1 ? "" : "s"} · open it`}
      >
        <Icon name="film" size={13} />
        <span class="name">{edit.title}</span>
        <span class="length">{formatTime(edit.length)}</span>
        {#if clips.carrying}<span class="plus"><Icon name="plus" size={16} /></span>{/if}
      </button>
    {/each}
    {#each clips.shown as clip (clip.id)}
      {@const progress = clips.progress.get(clip.id)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="chip {clip.status}"
        data-clip={clip.id}
        title="{span(clip.start, clip.end)} · {describe(clip)}{clip.status === 'ready'
          ? ' · drag it into a video editor or a folder'
          : ''}"
        onpointerdown={(e) => onChipDown(e, clip)}
      >
        <button class="play" onclick={() => play(clip.start)} aria-label="Play {clip.title}">
          <span class="length">{formatTime(clip.end - clip.start)}</span>
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
          />
        {:else}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span class="name" ondblclick={() => startRename(clip)}>{clip.title}</span>
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
    {/if}
    {#if clips.error}
      <span class="error-text">{clips.error}</span>
    {/if}
    {#if added}
      <span class="muted added">{added}</span>
    {/if}
  </div>
</div>

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
  .clip-bar-wrap {
    position: relative;
  }

  /* While a clip is carried: a place above the bar to start an edit with it. */
  .new-edit {
    position: absolute;
    bottom: calc(100% + 4px);
    left: 16px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border: 1px dashed var(--accent);
    border-radius: 14px;
    background: var(--surface);
    box-shadow: var(--shadow);
    color: var(--accent);
    font-size: 13px;
  }

  .new-edit.target {
    border-style: solid;
    background: color-mix(in srgb, var(--accent) 14%, var(--surface));
  }

  /* An edit: opens it; while a clip is carried, somewhere to drop it. */
  .edit-chip {
    gap: 6px;
    padding: 0 12px 0 10px;
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
    background: var(--surface);
    color: var(--text);
    font-size: 12px;
  }

  .edit-chip :global(svg) {
    flex: none;
    color: var(--accent);
  }

  .edit-chip .length {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .edit-chip.open {
    background: var(--accent-soft);
  }

  .edit-chip .plus {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--surface) 55%, transparent);
    color: var(--accent);
  }

  .edit-chip.target {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    box-shadow: inset 0 0 0 1px var(--accent);
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
