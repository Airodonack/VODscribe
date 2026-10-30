<script lang="ts">
  // A chapter's title or a highlight's note, written in a panel by its mark
  // on the timeline (Scrubber.svelte): opened by a double-click, Enter on a
  // selected one, or a new mark (cuts.svelte.ts `editing`).
  import { untrack } from "svelte";
  import { cuts } from "./cuts.svelte";
  import { formatTime } from "./platform";
  import { copyChapters as copy } from "./actions";
  import Popover from "./Popover.svelte";

  /** The timeline, where the marks are drawn; `fallback` if one isn't (yet). */
  let { root, fallback }: { root: HTMLElement | undefined; fallback: HTMLElement | undefined } = $props();

  // The title (a chapter's) or note (a highlight's) being written: in a
  // panel by it, saved when it closes, kept as it was on Escape.
  const editing = $derived(cuts.all.find((m) => m.id === cuts.editing) ?? null);
  let draft = $state("");
  let keep = true;
  let anchor = $state<HTMLElement>();
  // Set up when a mark's box opens: not again when the marks reload (an undo,
  // a save that failed), which would undo what's being written.
  $effect(() => {
    if (cuts.editing === null) return;
    const m = untrack(() => editing);
    if (!m) return;
    untrack(() => {
      draft = m.text ?? "";
      keep = true;
      copied = null;
    });
    // Once it's drawn (a chapter just added makes the lane).
    queueMicrotask(() => {
      anchor = root?.querySelector<HTMLElement>(`[data-mark="${m.id}"]`) ?? fallback;
    });
  });
  // Every chapter, for a YouTube video's description, with the title being written.
  let copied = $state<string | null>(null);
  async function copyChapters() {
    const id = editing?.id;
    copied = await copy(cuts.chapters.map((c) => (c.id === id ? { ...c, text: draft.trim() || null } : c)));
  }

  // Enter, or a click away, keeps what's written; Escape doesn't, and takes
  // away a mark just added (its key pressed by mistake).
  function closeEditor() {
    if (editing && keep) cuts.setText(editing.id, draft);
    else if (editing && editing.id === cuts.fresh) cuts.cancel(editing.id);
    cuts.editing = null;
    cuts.fresh = null;
  }
  const focus = (el: HTMLInputElement) => {
    requestAnimationFrame(() => el.focus());
  };
</script>

<svelte:window
  onkeydowncapture={(e) => {
    // Escape while a mark's box is open, wherever it's pressed: what's written
    // isn't kept, and a mark just added goes (closeEditor). Before the box's
    // own Escape (Popover) closes it.
    if (e.key === "Escape" && editing) keep = false;
  }}
/>

<Popover
  bind:open={() => !!editing, (open) => !open && closeEditor()}
  {anchor}
  label={editing?.kind === "chapter" ? "Chapter title" : "Highlight note"}
  width={280}
  padding={10}
>
  {#if editing}
    <form
      class="mark-editor"
      onsubmit={(e) => {
        e.preventDefault();
        closeEditor();
      }}
    >
      <input
        use:focus
        bind:value={draft}
        maxlength="200"
        autocorrect="off"
        autocapitalize="off"
        spellcheck="false"
        placeholder={editing.kind === "chapter" ? "Chapter title" : "What happens here (optional)"}
      />
      <p class="muted">
        {editing.kind === "chapter" ? "Chapter" : "Highlight"} at {formatTime(editing.start)} · Enter saves
      </p>
      {#if editing.kind === "chapter"}
        <button type="button" class="link accent" onclick={copyChapters}>
          Copy {cuts.chapters.length === 1 ? "it" : `all ${cuts.chapters.length} chapters`} as YouTube chapters
        </button>
        {#if copied}<p class="muted">{copied}</p>{/if}
      {/if}
    </form>
  {/if}
</Popover>

<style>
  .mark-editor {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .mark-editor .link {
    align-self: flex-start;
    font-size: 12px;
  }

  .mark-editor p {
    margin: 0;
    font-size: 12px;
  }
</style>
