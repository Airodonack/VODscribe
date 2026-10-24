<script lang="ts">
  // A player's keyboard legend, under its controls: a few short columns
  // (keymap.ts LEGEND_GROUPS), each line key caps and what they do (the lists
  // are playerKeys.ts's), and at the end a link to Settings › Controls, where
  // every key is and each can be changed.
  import { demoOff } from "./demo";
  import type { ShortcutGroup } from "./playerKeys";
  import { settingsWindow } from "./settingsWindow.svelte";

  let { groups }: { groups: ShortcutGroup[] } = $props();
</script>

<div class="legend">
  <div class="columns">
    {#each groups as g, i (g.group)}
      <div class="group" role="group" aria-label={g.heading}>
        <p class="heading eyebrow">{g.heading}</p>
        {#each g.shortcuts as s (s.does)}
          <div class="row">
            <span class="keys"
              >{#each s.keys as k, j (j)}{#if k === "+" || k === "or"}{k === "or" ? " or " : "+"}{:else}<kbd>{k}</kbd
                  >{/if}{/each}</span
            >
            <span class="does">{s.does}</span>
          </div>
        {/each}
        {#if i === groups.length - 1}
          <button
            class="link all"
            onclick={() => settingsWindow.show("controls")}
            {...demoOff("The web demo has no settings. In the app: every keyboard control, each one yours to change.")}
            >All controls ›</button
          >
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .legend {
    container-type: inline-size;
    padding: 4px 16px 8px;
    font-size: 12px;
  }

  /* The columns side by side, each as wide as its lines need of the room
     there is; where that's too little, two columns a row, then one: never
     a sideways scroll. */
  .columns {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: auto;
    gap: 8px 16px;
  }

  @container (max-width: 680px) {
    .columns {
      grid-auto-flow: row;
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @container (max-width: 360px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  /* The keys lined up in a column of their own, what they do beside them. */
  .group {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 2px 8px;
    align-content: start;
  }

  .row {
    display: contents;
  }

  .heading,
  .all {
    grid-column: 1 / -1;
  }

  .heading {
    margin: 0 0 1px;
  }

  .keys {
    white-space: nowrap;
  }

  .all {
    justify-self: start;
    margin-top: 2px;
  }

  /* Caps a little smaller than elsewhere: eight lines fit under the
     controls of a window of the default height. */
  kbd {
    font-size: 11px;
    line-height: 14px;
  }

  kbd + kbd {
    margin-left: 2px;
  }
</style>
