<script lang="ts">
  // "Add to edit…" for several things picked (selection.svelte.ts), without
  // a mouse to drag them: the edits to pick from (or a new one), then where
  // in it, at the end of its timeline or on its clip bar, as a drop there
  // would put them (bulk.ts `addTo`).

  import type { Put } from "./api";
  import { addTo } from "./bulk";
  import { edits } from "./edits.svelte";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";

  let {
    open = $bindable(),
    anchor,
    items,
  }: {
    open: boolean;
    anchor: HTMLElement | undefined;
    /** What's picked, as an edit takes it, in the order picked; asked once an edit is picked. */
    items: () => Put[];
  } = $props();

  /** The edit picked (`new`: one to make), where in it is asked next. A
   *  new one is made only once that's answered: let go before, nothing is. */
  let into = $state<{ id: number; title: string } | "new" | null>(null);
  $effect(() => {
    if (!open) into = null;
  });

  function pick(editId: number | null) {
    const edit = editId === null ? null : edits.list.find((e) => e.id === editId);
    into = edit ? { id: edit.id, title: edit.title } : editId === null ? "new" : null;
  }

  async function put(bar: boolean) {
    const picked = into;
    open = false;
    // What's picked now, at the choice: it may change while a new edit is made.
    const what = items();
    const edit = picked === "new" ? await edits.create() : picked;
    if (edit) void addTo(edit.id, edit.title, bar, what);
  }
</script>

<Popover bind:open {anchor} label="Add to an edit" width={240} padding={6}>
  <div class="pick">
    {#if into}
      <span class="muted head">{into === "new" ? "Into a new edit:" : `Into “${into.title}”:`}</span>
      <button onclick={() => put(false)}><Icon name="film" size={13} /> Timeline, at the end</button>
      <button onclick={() => put(true)}><Icon name="plus" size={13} /> Clip bar</button>
    {:else}
      {#each edits.list as edit (edit.id)}
        <button onclick={() => pick(edit.id)}>{edit.title}</button>
      {/each}
      <button class="new" onclick={() => pick(null)}><Icon name="plus" size={13} /> New edit</button>
    {/if}
  </div>
</Popover>

<style>
  .pick {
    display: flex;
    flex-direction: column;
  }

  .head {
    padding: 4px 8px;
    font-size: 12px;
  }

  .pick button {
    display: flex;
    align-items: center;
    justify-content: flex-start;
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
