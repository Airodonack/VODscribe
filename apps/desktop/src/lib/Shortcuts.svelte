<script lang="ts">
  // A player's keyboard shortcuts, as a line of key caps and what they do
  // (the lists are in playerKeys.ts).
  import type { Shortcut } from "./playerKeys";

  let { shortcuts }: { shortcuts: Shortcut[] } = $props();
</script>

<p class="legend muted">
  {#each shortcuts as s, i (s.does)}
    {#if i}<span class="sep">·</span>{/if}
    <span class="keys"
      >{#each s.keys as k, j (j)}{#if k === "+" || k === "or"}{k === "or" ? " or " : "+"}{:else}<kbd>{k}</kbd
          >{/if}{/each}</span
    >
    {s.does}
  {/each}
</p>

<style>
  .legend {
    margin: 0;
    padding: 0 16px 8px;
    font-size: 12px;
  }

  .keys {
    white-space: nowrap;
  }

  kbd + kbd {
    margin-left: 2px;
  }
</style>
