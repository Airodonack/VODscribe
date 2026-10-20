<script lang="ts">
  // The player's panel, the same for a VOD (Player.svelte) and an edit
  // (EditView.svelte): a header (what's open, then the keyboard button and
  // its list of shortcuts), the picture's stage, the timeline and controls,
  // and what goes under them; full screen, which is the window's with the
  // panel laid over the page (WebKitGTK ends the app on the page's own,
  // requestFullscreen). What fills each part is the view's.

  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onDestroy, type Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { Shortcut } from "./playerKeys";
  import Shortcuts from "./Shortcuts.svelte";

  let {
    fullscreen = $bindable(false),
    shortcuts,
    label,
    head,
    headEnd,
    stage,
    controls,
    below,
  }: {
    /** The window full screen, the panel over all of it. */
    fullscreen?: boolean;
    /** What the keyboard button lists. */
    shortcuts: Shortcut[];
    /** Names the panel (an edit's is `edit-view`, for what looks for it). */
    label?: string;
    /** What's open: its name, and what's done with it. */
    head: Snippet;
    /** After the keyboard button. */
    headEnd?: Snippet;
    stage?: Snippet;
    /** The timeline and the bar of controls. */
    controls?: Snippet;
    below?: Snippet;
  } = $props();

  let showKeys = $state(false);

  // The window follows: full screen as asked, and not once the panel's gone.
  let windowFull = false;
  $effect(() => {
    if (fullscreen === windowFull) return;
    windowFull = fullscreen;
    getCurrentWindow()
      .setFullscreen(fullscreen)
      .catch((e) => console.error("full screen", e));
  });
  onDestroy(() => {
    if (windowFull)
      getCurrentWindow()
        .setFullscreen(false)
        .catch(() => {});
  });
</script>

<section class={label} class:fullscreen>
  <div class="head">
    {@render head()}
    <button
      class="icon"
      class:on={showKeys}
      onclick={() => (showKeys = !showKeys)}
      aria-expanded={showKeys}
      aria-label="Keyboard shortcuts"
      title="Keyboard shortcuts"><Icon name="keyboard" size={16} /></button
    >
    {@render headEnd?.()}
  </div>
  {#if showKeys}
    <Shortcuts {shortcuts} />
  {/if}
  {@render stage?.()}
  {#if controls}
    <!-- The timeline over the controls: the same for every player. -->
    <div class="controls">{@render controls()}</div>
  {/if}
  {@render below?.()}
</section>

<style>
  section {
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  /* Full screen: the panel over the whole window, the picture as large as fits. */
  section.fullscreen {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    border: none;
  }

  .fullscreen .head,
  .fullscreen :global(.legend) {
    display: none;
  }

  .fullscreen :global(.stage) {
    flex: 1;
    min-height: 0;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px 6px 16px;
    min-height: 40px;
  }

  .head :global(.icon) {
    width: 28px;
    height: 28px;
  }

  .controls {
    padding: 8px 16px 2px;
  }
</style>
