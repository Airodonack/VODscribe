<script lang="ts">
  // An edit's row in the library's list, among the VODs (Library.svelte),
  // shaped like theirs (VodRow.svelte): it opens the edit. While a clip is
  // carried (dragout.ts), it's somewhere to drop it: a + over it, lit up when
  // the clip is over it.

  import { api, type Edit } from "./api";
  import { ask } from "./ask";
  import { clips } from "./clips.svelte";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { formatTime } from "./platform";
  import Spinner from "./Spinner.svelte";

  let {
    edit,
    reorderable,
    onopen,
    onkey,
  }: {
    edit: Edit;
    reorderable: boolean;
    onopen: () => void;
    onkey: (e: KeyboardEvent) => void;
  } = $props();

  const progress = $derived(edits.progress.get(edit.id));
  const meta = $derived.by(() => {
    if (edit.pieces === 0) return "Empty";
    const what = `${edit.pieces} piece${edit.pieces === 1 ? "" : "s"} · ${formatTime(edit.length)}`;
    if (edit.status === "cutting") return `${what} · rendering${progress ? ` ${Math.round(progress * 100)}%` : "…"}`;
    if (edit.status === "error") return `${what} · couldn't render`;
    if (edit.status === "ready") return `${what} · ${edit.current ? "rendered" : "changed since rendered"}`;
    return what;
  });

  async function remove() {
    const sure = await ask(
      `Delete the edit “${edit.title}”${edit.file_path ? " and its video" : ""}? Its clips stay.`,
      "Delete",
      "Keep",
    );
    if (sure) edits.remove(edit.id);
  }
</script>

<button
  class="main"
  data-edit-drop={edit.id}
  onclick={onopen}
  onkeydown={onkey}
  title={clips.carrying ? `Drop to add it to “${edit.title}”` : reorderable ? "Drag to reorder (Alt+↑/↓)" : undefined}
>
  <span class="thumb">
    <Icon name="film" size={18} />
    {#if edit.length}<span class="duration">{formatTime(edit.length)}</span>{/if}
  </span>
  <span class="text">
    <span class="title">{edit.title}</span>
    <span class="meta muted" class:bad={edit.status === "error"}>
      <span class="kind">Edit</span><span class="sep">·</span>{meta}
    </span>
  </span>
  {#if edit.status === "cutting"}<Spinner size={12} />{/if}
  {#if clips.carrying}
    <span class="drop" class:target={edits.dropTarget === edit.id}><Icon name="plus" size={22} /></span>
  {/if}
</button>
<div class="actions">
  {#if edit.status === "ready" && edit.file_path}
    <button
      class="act"
      onclick={() => api.showEdit(edit.id).catch((e) => (edits.error = String(e)))}
      title="Show the rendered video in its folder"
      aria-label="Show in folder"><Icon name="folder" size={15} /></button
    >
  {/if}
  <button class="act" onclick={remove} title="Delete edit" aria-label="Delete edit">
    <Icon name="trash" size={15} />
  </button>
</div>

<style>
  .main {
    position: relative;
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
    background: color-mix(in srgb, var(--accent) 16%, var(--surface-3));
    color: var(--accent);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .duration {
    position: absolute;
    right: 3px;
    bottom: 3px;
    padding: 0 4px;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.75);
    color: #fff;
    font-size: 10px;
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .meta.bad {
    color: var(--danger);
  }

  .kind {
    color: var(--accent);
  }

  .sep {
    margin: 0 4px;
  }

  /* While a clip is carried: a + over the row, solid when the clip is over it. */
  .drop {
    position: absolute;
    inset: -4px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px dashed color-mix(in srgb, var(--accent) 60%, transparent);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--surface) 55%, transparent);
    color: var(--accent);
  }

  .drop.target {
    border-style: solid;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .actions {
    display: flex;
    gap: 2px;
    justify-content: flex-end;
    margin-top: 4px;
    margin-right: -6px;
  }

  .act {
    width: 26px;
    height: 24px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    border-radius: var(--radius-sm);
    opacity: 0.75;
  }

  :global(.vod:hover) .act,
  :global(.vod.selected) .act {
    opacity: 1;
  }

  .act:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
</style>
