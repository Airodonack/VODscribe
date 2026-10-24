<script lang="ts">
  import type { SearchQuery } from "./api";
  import Icon from "./Icon.svelte";
  import { chipsOf, shows } from "./queryChips";

  // Under the search box: how the typed text was read, as chips (queryChips.ts),
  // or what's wrong with it. A filter's chip has an × that takes it out of the
  // box. Nothing here takes focus, so typing goes on in the box.
  let {
    query,
    onremove,
  }: {
    query: SearchQuery;
    /** A filter's × clicked: its text, as the core writes it, to take out of the box. */
    onremove: (text: string) => void;
  } = $props();

  const chips = $derived(shows(query.tree) ? chipsOf(query.tree) : []);
  const keep = (e: MouseEvent) => e.preventDefault();
</script>

{#if query.error}
  <p class="grouping error-text" role="status">{query.error}</p>
{:else if chips.length}
  <div class="grouping" role="status" aria-label="How the search reads">
    {#each chips as chip, i (i)}
      {#if chip.kind === "phrase"}
        <span class="chip" title={chip.strict ? "These whole words" : "These words, in order"}
          >{chip.strict ? `“${chip.text}”` : chip.text}</span
        >
      {:else if chip.kind === "not"}
        <span class="chip not" title="Not near a match">{chip.strict ? `“${chip.text}”` : chip.text}</span>
      {:else if chip.kind === "filter"}
        <span class="chip filter"
          ><span class="key">{chip.key}:</span>{chip.value}<button
            class="remove"
            tabindex="-1"
            aria-label="Remove {chip.text} from the search"
            title="Remove from the search"
            onmousedown={keep}
            onclick={() => onremove(chip.text)}><Icon name="x" size={11} /></button
          ></span
        >
      {:else if chip.kind === "or"}
        <span class="or">OR</span>
      {:else}
        <span class="bracket">{chip.kind === "open" ? "(" : ")"}</span>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .grouping {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    margin: 0;
    padding: 6px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    font-size: 12px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    padding: 1px 7px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    white-space: pre;
  }

  .chip.not {
    color: var(--muted);
    text-decoration: line-through;
  }

  .chip.filter {
    padding-right: 0;
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .key {
    color: var(--muted);
  }

  .remove {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 16px;
    margin-left: 3px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
  }

  .remove:hover {
    color: var(--danger);
  }

  .or {
    padding: 0 3px;
    color: var(--accent);
    font-size: 11px;
    font-weight: 600;
  }

  .bracket {
    color: var(--muted);
    font-size: 15px;
    line-height: 1;
  }
</style>
