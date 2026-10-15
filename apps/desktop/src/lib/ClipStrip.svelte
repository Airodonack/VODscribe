<script lang="ts">
  // The open VOD's clips as chips under the player, and the range picked but
  // not kept yet as a dashed one. Clicking a chip plays it; double-clicking its
  // name renames it.

  import { api, type Clip } from "./api";
  import { clips } from "./clips.svelte";
  import { dragOut } from "./dragout";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";
  import { formatTime } from "./platform";
  import Spinner from "./Spinner.svelte";

  let { onplay }: { onplay: (time: number) => void } = $props();

  let renaming = $state<number | null>(null);
  let ghostChip = $state<HTMLElement>();
  let hideNote = $state(false);
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
    if (!press || Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return;
    const clip = press.clip;
    press = null;
    swallowClick = true;
    setTimeout(() => (swallowClick = false), 500);
    dragOut(clip);
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

  const span = (start: number, end: number) => `${formatTime(start)}–${formatTime(end)}`;

  function describe(clip: Clip): string {
    if (clip.status === "error") return `Couldn't make this clip: ${clip.error ?? "unknown error"}`;
    if (clip.status === "cutting") return "Making the clip…";
    if (clip.status === "waiting") return "Waiting to be made";
    return clip.title;
  }
</script>

<svelte:window onpointermove={onPointerMove} onpointerup={() => (press = null)} />

<div class="strip" aria-label="Clips">
  {#each clips.list as clip (clip.id)}
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
      <button class="act" onclick={() => clips.remove(clip.id)} title="Delete clip" aria-label="Delete clip">
        <Icon name="x" size={13} />
      </button>
      {#if progress !== undefined}
        <span class="progress" style:width="{progress * 100}%"></span>
      {/if}
    </div>
  {/each}
  {#if clips.ghost}
    {@const ghost = clips.ghost}
    <div class="chip ghost" bind:this={ghostChip}>
      <button class="play" onclick={() => onplay(ghost.start)} aria-label="Play the range picked">
        <span class="length">{formatTime(ghost.end - ghost.start)}</span>
      </button>
      <span class="name">{span(ghost.start, ghost.end)}</span>
      <button class="keep" onclick={() => clips.keep()} title="Keep this clip (Enter)">
        <Icon name="scissors" size={13} /> Keep
      </button>
      <button class="act" onclick={() => (clips.ghost = null)} title="Drop (Esc)" aria-label="Drop the range">
        <Icon name="x" size={13} />
      </button>
    </div>
  {/if}
  {#if !clips.list.length && !clips.ghost}
    <span class="hint muted">
      <Icon name="scissors" size={13} /> To clip, drag on the bar above or across lines of the transcript, or press
      <kbd>I</kbd> where it starts and <kbd>O</kbd> where it ends.
    </span>
  {/if}
  <Popover
    bind:open={() => !!clips.asking, (open) => !open && clips.answer(false)}
    anchor={ghostChip}
    label="Before downloading a clip"
    width={340}
  >
    <div class="note">
      <p>Only download clips you have permission to use, such as from your own channel or with the streamer's okay.</p>
      <p class="muted">Streamers can also download their whole VODs from their channel's video manager.</p>
      <label><input type="checkbox" bind:checked={hideNote} /> Don't show this again</label>
      <div class="buttons">
        <button onclick={() => clips.answer(false)}>Cancel</button>
        <button class="primary" onclick={() => clips.answer(true, hideNote)}>Download clip</button>
      </div>
    </div>
  </Popover>
  {#if clips.error}
    <span class="error-text">{clips.error}</span>
  {/if}
</div>

<style>
  .strip {
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

  .chip.ghost {
    border: 1px dashed var(--accent);
    background: var(--accent-soft);
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

  .keep {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 8px;
    border: none;
    border-radius: 11px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 12px;
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

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    white-space: nowrap;
  }

  .hint kbd + kbd {
    margin-left: 0;
  }

  .note {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 13px;
  }

  .note p {
    margin: 0;
  }

  .note label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .error-text {
    font-size: 12px;
    white-space: nowrap;
  }
</style>
