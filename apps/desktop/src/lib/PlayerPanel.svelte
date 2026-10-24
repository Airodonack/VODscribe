<script lang="ts">
  // The player's panel, the same for a VOD (Player.svelte) and an edit
  // (EditView.svelte): a header (what's open, and what's done with it), the
  // picture's stage, the timeline and controls, the keyboard legend under
  // them (there so the picture never moves when it opens or closes; its
  // button is at the controls' right, PlayerBar.svelte), and what goes under
  // that; full screen, which is the window's with the
  // panel laid over the page (WebKitGTK ends the app on the page's own,
  // requestFullscreen). What fills each part is the view's.

  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onDestroy, type Snippet } from "svelte";
  import { settings } from "./appSettings.svelte";
  import type { ShortcutGroup } from "./playerKeys";
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
    /** What the keyboard legend lists. */
    shortcuts: ShortcutGroup[];
    /** Names the panel (an edit's is `edit-view`, for what looks for it). */
    label?: string;
    /** What's open: its name, and what's done with it. */
    head: Snippet;
    /** At the header's right end. */
    headEnd?: Snippet;
    stage?: Snippet;
    /** The timeline and the bar of controls (with the legend's button); none
     *  while there's no bar, and then no legend either. */
    controls?: Snippet;
    below?: Snippet;
  } = $props();

  // Open or closed is a setting (toggled by the button in the bar):
  // someone learning the keys keeps the legend open across VODs, edits and
  // launches.
  const showKeys = $derived(settings.value?.show_keys ?? false);

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
    {@render headEnd?.()}
  </div>
  {@render stage?.()}
  {#if controls}
    <!-- The timeline over the controls: the same for every player. -->
    <div class="controls">{@render controls()}</div>
  {/if}
  {#if controls && showKeys}
    <Shortcuts groups={shortcuts} />
  {/if}
  {@render below?.()}
</section>

<style>
  /* A column within the window's (App.svelte's .main): each part as tall as
     it is, but where the window is too short for everything, the legend
     gives up room and scrolls inside itself, rather than the panel running
     past the window's bottom. */
  section {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  section > :global(*) {
    flex: none;
  }

  /* At least its headings and a couple of lines, so it never shrinks away. */
  section > :global(.legend) {
    flex: 0 1 auto;
    min-height: 64px;
    overflow-y: auto;
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
