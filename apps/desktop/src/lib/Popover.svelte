<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import { portal } from "./portal";
  import { swallowClick } from "./swallowClick";

  // A panel opened from a button: Settings, the search's From filter, the
  // Timeline layers. It lives at the end of <body> (`portal`) and is placed
  // against the window, so nothing can clip it or draw over it (a scrolling
  // area, a sticky header, a playing video in WebKit). It sits below its
  // button, right edges aligned (shifted to stay inside the window), or above
  // it when there's more room there (or `above` it when asked, as a menu
  // from the player's bar, where it has the room), and scrolls within the
  // space it has. A click outside it or its button, or Escape, closes it.
  // A menu (Menu.svelte) closes as a press outside it begins instead
  // (`closeOn="press"`), its button included, and as the window loses the
  // focus. That press does nothing else (swallowClick.ts), but a right press
  // goes on, so a right click on another row opens that row's menu. On macOS
  // a right click on a window in the back opens the menu there, and the click
  // that brings the window to the front never reaches the page: the window's
  // coming to the front closes a menu opened that way.
  // Opened at a point instead (`at`, a streamer's card at the pointer), it
  // sits below and to the right of it, or above it, by the same rules.
  let {
    open = $bindable(),
    anchor,
    label,
    width,
    padding = 12,
    above = false,
    at = null,
    grow = false,
    closeOn = "click",
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
    /** A point in the window to open at, its left edge there, rather than against the button. */
    at?: { x: number; y: number } | null;
    /** As wide as what's in it, `width` at the least (a menu, its longest choice). */
    grow?: boolean;
    /** What outside it closes it: a click, or a press as it begins (a menu's). */
    closeOn?: "click" | "press";
    children: Snippet;
  } = $props();

  let panel = $state<HTMLElement>();
  // Below the button with at least this much room, even if there's more above.
  const ROOM = 280;
  // Less than this either side (a short window): as tall as the window, over the button.
  const LEAST = 160;
  let place = $state<{ top?: number; bottom?: number; right: number; maxHeight: number } | null>(null);
  function fit() {
    if (!open || (!anchor && !at)) return;
    const box = at ? new DOMRect(at.x, at.y, 0, 0) : anchor!.getBoundingClientRect();
    // Right edges aligned (from a point, its left edge there), but never past the window's left edge (a button near it).
    const wide = Math.max(width, panel?.offsetWidth ?? 0);
    const edge = at ? window.innerWidth - box.left - wide : window.innerWidth - box.right;
    const right = Math.max(8, Math.min(edge, window.innerWidth - wide - 8));
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
    void at;
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

  // Listened for on the document as events start (capture), so nothing
  // under the pointer that stops a press can keep it open.
  $effect(() => {
    if (!open || closeOn !== "press") return;
    const onDown = (e: PointerEvent) => {
      if (panel?.contains(e.target as Node)) return;
      open = false;
      if (e.button !== 2) swallowClick(e);
    };
    const onBlur = () => (open = false);
    // Elsewhere the click that focuses the window is pressed in the page too
    // and closes it as above; one opened in a focused window never gets this.
    const inBack = !document.hasFocus();
    const onFocus = () => {
      if (inBack) open = false;
    };
    document.addEventListener("pointerdown", onDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
    };
  });

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
