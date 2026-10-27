<script lang="ts">
  // A side column's inner edge, the full height of the window, as a button
  // that hides the column and shows it again (App.svelte). At rest it is the
  // column's divider; pointed at, it tints and shows which way it goes. It
  // leaves the scrollbar beside it a few pixels, so aiming at the scrollbar
  // doesn't hide the column. Hidden, it stays at the window's edge.
  import Icon from "./Icon.svelte";

  let {
    side,
    open,
    label,
    hint,
    mid,
    ontoggle,
  }: {
    /** Which column it is the edge of: the library on the left, or the pane's on the right. */
    side: "left" | "right";
    open: boolean;
    /** What a press does, as the button's name and its tip's first words. */
    label: string;
    /** The key's part of the tip (keyHint). */
    hint: string;
    /** Where the chevron goes: the player's middle, in pixels from the window's top; null, the window's middle. */
    mid: number | null;
    ontoggle: () => void;
  } = $props();

  // Towards where the column goes, or back from it.
  const towards = $derived((side === "left") === open ? "chevron-left" : "chevron-right");

  // A press that moves before its release is a drag (to be the column's
  // resize one day), not a click.
  const DRAG = 4;
  let down: { x: number; y: number } | null = null;
  function onclick(e: MouseEvent) {
    const from = down;
    down = null;
    // A click from the keyboard has no press (detail 0).
    if (e.detail && from && Math.hypot(e.clientX - from.x, e.clientY - from.y) > DRAG) return;
    ontoggle();
  }
</script>

<div class="edge-strip {side}" class:open>
  <button
    class="edge {side === 'left' ? 'library-edge' : 'pane-edge'}"
    aria-label={label}
    aria-expanded={open}
    title="{label}{hint}"
    style:--mid={mid === null ? null : `${mid}px`}
    onpointerdown={(e) => (down = { x: e.clientX, y: e.clientY })}
    {onclick}
  >
    <span class="chevron"><Icon name={towards} size={14} /></span>
  </button>
</div>

<style>
  /* A column of the window's grid (App.svelte), in the side column's colour,
     its line on the centre's side, where the column's own border was. */
  .edge-strip {
    position: relative;
    min-height: 0;
    background: var(--surface);
  }

  .edge-strip.left {
    border-right: 1px solid var(--border);
  }

  .edge-strip.right {
    border-left: 1px solid var(--border);
  }

  /* The line under the column's head and the toolbar, carried across (not
     once the column is hidden: then there's only the toolbar beside it, of
     a height of its own). */
  .edge-strip.open::after {
    content: "";
    position: absolute;
    top: 52px;
    left: 0;
    right: 0;
    height: 1px;
    background: var(--border);
    pointer-events: none;
  }

  .edge {
    position: absolute;
    top: 0;
    bottom: 0;
    /* Clear of the scrollbar on the left: the library's, or the transcript's. */
    left: 3px;
    right: 0;
    padding: 0;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }

  /* The library hidden, its edge is the window's: nothing to keep clear of,
     and the pointer flung to the window's edge lands on it. */
  .edge-strip.left:not(.open) .edge {
    left: 0;
  }

  .edge:hover:not(:disabled) {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .edge:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .chevron {
    position: absolute;
    left: 50%;
    top: var(--mid, 50%);
    display: flex;
    transform: translate(-50%, -50%);
    opacity: 0;
    pointer-events: none;
  }

  /* Shown when pointed at or focused; the column hidden, always, so the way
     back is in sight. */
  .edge:hover .chevron,
  .edge:focus-visible .chevron,
  .edge-strip:not(.open) .chevron {
    opacity: 1;
  }
</style>
