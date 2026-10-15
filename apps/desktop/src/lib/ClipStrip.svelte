<script lang="ts">
  // The open VOD's clips as chips under the player, and the range picked but
  // not kept yet as a dashed one. Clicking a chip plays it; double-clicking its
  // name renames it.

  import type { Clip } from "./api";
  import { clips } from "./clips.svelte";
  import Icon from "./Icon.svelte";
  import { formatTime } from "./platform";
  import Spinner from "./Spinner.svelte";

  let { onplay, canCut }: { onplay: (time: number) => void; canCut: boolean } = $props();

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

  const span = (start: number, end: number) => `${formatTime(start)}–${formatTime(end)}`;

  function describe(clip: Clip): string {
    if (clip.status === "error") return `Couldn't make this clip: ${clip.error ?? "unknown error"}`;
    if (clip.status === "cutting") return "Making the clip…";
    if (clip.status === "waiting")
      return canCut ? "Waiting to be made" : "Clips of this VOD can't be made into files yet";
    return clip.title;
  }
</script>

<div class="strip" aria-label="Clips">
  {#each clips.list as clip (clip.id)}
    {@const progress = clips.progress.get(clip.id)}
    <div class="chip {clip.status}" title="{span(clip.start, clip.end)} · {describe(clip)}">
      <button class="play" onclick={() => onplay(clip.start)} aria-label="Play {clip.title}">
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
    <div class="chip ghost">
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
      <Icon name="scissors" size={13} /> To clip, press <kbd>I</kbd> where it starts and <kbd>O</kbd> where it ends.
    </span>
  {/if}
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

  .error-text {
    font-size: 12px;
    white-space: nowrap;
  }
</style>
