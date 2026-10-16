<script lang="ts">
  // The edits, under the VODs in the sidebar: each opens in the Edit view, and
  // a clip carried from a player's clip bar or timeline (dragout.ts) is put in
  // the one it's dropped on.

  import { clips } from "./clips.svelte";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import { demoOff } from "./demo";
  import { formatTime } from "./platform";
  import Spinner from "./Spinner.svelte";

  let { onopen }: { onopen: (id: number) => void } = $props();

  edits.start();

  async function create() {
    const edit = await edits.create();
    if (edit) onopen(edit.id);
  }

  const meta = (pieces: number, length: number) =>
    pieces === 0 ? "Empty" : `${pieces} piece${pieces === 1 ? "" : "s"} · ${formatTime(length)}`;
</script>

<section class="edits" class:carrying={!!clips.carrying}>
  <div class="head">
    <span class="label">Edits</span>
    <button
      class="link accent new"
      onclick={create}
      {...demoOff("The web demo can't make videos. In the app, an edit joins clips into one video.")}
    >
      <Icon name="plus" size={13} /> New edit
    </button>
  </div>
  <div class="list">
    {#each edits.list as edit (edit.id)}
      {@const progress = edits.progress.get(edit.id)}
      <button
        class="edit"
        class:open={edits.openId === edit.id}
        class:target={edits.dropTarget === edit.id}
        data-edit-drop={edit.id}
        onclick={() => onopen(edit.id)}
        title={clips.carrying ? `Drop to add it to “${edit.title}”` : edit.title}
      >
        <span class="kind"><Icon name="film" size={15} /></span>
        <span class="text">
          <span class="title">{edit.title}</span>
          <span class="meta muted">
            {meta(edit.pieces, edit.length)}
            {#if edit.status === "cutting"}
              · rendering{progress ? ` ${Math.round(progress * 100)}%` : "…"}
            {:else if edit.status === "ready" && !edit.current}
              · changed since rendered
            {:else if edit.status === "error"}
              · <span class="bad">couldn't render</span>
            {/if}
          </span>
        </span>
        {#if edit.status === "cutting"}
          <Spinner size={12} />
        {:else if edit.status === "ready" && edit.current}
          <span class="done" title="Rendered"><Icon name="check" size={14} /></span>
        {/if}
      </button>
    {:else}
      <p class="muted empty">
        {clips.carrying ? "Make a new edit, then drop clips on it." : "Join clips into one video: make an edit."}
      </p>
    {/each}
  </div>
</section>

<style>
  .edits {
    flex: none;
    display: flex;
    flex-direction: column;
    max-height: 38%;
    border-top: 1px solid var(--border);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 14px 4px 16px;
  }

  .label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .new {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
  }

  .list {
    overflow-y: auto;
    padding: 0 8px 8px;
  }

  .edit {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    justify-content: flex-start;
  }

  .edit:hover {
    background: var(--surface-2);
  }

  .edit.open {
    background: var(--accent-soft);
  }

  /* While a clip is carried, every edit is somewhere to drop it. */
  .carrying .edit {
    border-style: dashed;
    border-color: var(--border);
  }

  .carrying .edit.target {
    border-style: solid;
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .kind {
    display: flex;
    color: var(--muted);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .title {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 13px;
  }

  .meta {
    font-size: 11px;
  }

  .bad {
    color: var(--danger);
  }

  .done {
    display: flex;
    color: var(--accent);
  }

  .empty {
    margin: 2px 8px 4px;
    font-size: 12px;
  }
</style>
