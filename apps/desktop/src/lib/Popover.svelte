<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import { portal } from "./portal";

  // A panel opened from a button: Settings, the search's From filter, the
  // Timeline layers. It lives at the end of <body> (`portal`) and is placed
  // against the window, so nothing can clip it or draw over it (a scrolling
  // area, a sticky header, a playing video in WebKit). It sits below its
  // button, right edges aligned (shifted to stay inside the window), or above
  // it when there's more room there (or `above` it when asked, as a menu
  // from the player's bar, where it has the room), and scrolls within the
  // space it has. A click outside it or its button, or Escape, closes it.
  let {
    open = $bindable(),
    anchor,
    label,
    width,
    padding = 12,
    above = false,
    grow = false,
    children,
  }: {
    open: boolean;
    /** The button and what's around it: clicks in here don't close the panel. */
    anchor: HTMLElement | undefined;
    label: string;
    width: number;
    padding?: number;
    /** Above the button if it fits there, rather than below. */
    above?: boolean;
    /** As wide as what's in it, `width` at the least (a menu, its longest choice). */
    grow?: boolean;
    children: Snippet;
  } = $props();

  let panel = $state<HTMLElement>();
  // Below the button with at least this much room, even if there's more above.
  const ROOM = 280;
  // Less than this either side (a short window): as tall as the window, over the button.
  const LEAST = 160;
  let place = $state<{ top?: number; bottom?: number; right: number; maxHeight: number } | null>(null);
  function fit() {
    if (!open || !anchor) return;
    const box = anchor.getBoundingClientRect();
    // Right edges aligned, but never past the window's left edge (a button near it).
    const wide = Math.max(width, panel?.offsetWidth ?? 0);
    const right = Math.max(8, Math.min(window.innerWidth - box.right, window.innerWidth - wide - 8));
    const below = window.innerHeight - box.bottom - 16;
    const up = box.top - 16;
    place = (above ? up < LEAST && below >= LEAST : below >= ROOM || (below >= up && below >= LEAST))
      ? { top: box.bottom + 8, right, maxHeight: below - 8 }
      : up >= LEAST
        ? { bottom: window.innerHeight - box.top + 8, right, maxHeight: up - 8 }
        : { top: 8, right, maxHeight: window.innerHeight - 16 };
  }
  $effect(() => {
    // Again once it's drawn, to know its width.
    void panel;
    if (open) fit();
  });

  // A click is a press and its release: one whose press was held as the panel
  // opened (a clip pulled out of the window asks the note on permission, and
  // is let go after) isn't a click outside it.
  let pressed = false;
  let pressedBefore = false;
  $effect(() => {
    if (open) pressedBefore = untrack(() => pressed);
  });
  function onWindowPointerDown() {
    pressed = true;
    pressedBefore = false;
  }

  // By the click's path when it happened: a button inside that removes itself
  // (a layer's pill, say) is gone from the page before this runs, and that's
  // still a click inside.
  function onWindowClick(e: MouseEvent) {
    if (!open) return;
    if (pressedBefore) {
      pressedBefore = false;
      return;
    }
    const path = e.composedPath();
    if (!(anchor && path.includes(anchor)) && !(panel && path.includes(panel))) open = false;
  }
</script>

<svelte:window
  onpointerdowncapture={onWindowPointerDown}
  onpointerupcapture={() => (pressed = false)}
  onclick={onWindowClick}
  onkeydown={(e) => e.key === "Escape" && (open = false)}
  onresize={fit}
  onscrollcapture={fit}
/>

{#if open}
  <div
    use:portal
    bind:this={panel}
    class="popover"
    role="dialog"
    aria-label={label}
    style:width={grow ? "max-content" : `${width}px`}
    style:min-width={grow ? `${width}px` : null}
    style:padding="{padding}px"
    style:top={place?.top !== undefined ? `${place.top}px` : null}
    style:bottom={place?.bottom !== undefined ? `${place.bottom}px` : null}
    style:right="{place?.right ?? 0}px"
    style:max-height="{place?.maxHeight ?? 0}px"
  >
    {@render children()}
  </div>
{/if}

<style>
  .popover {
    position: fixed;
    /* Above everything: it's at the end of <body> (portal.ts). */
    z-index: 1000;
    max-width: calc(100vw - 16px);
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
  }
</style>
