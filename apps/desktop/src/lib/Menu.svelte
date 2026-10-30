<script lang="ts" module>
  import type { ComponentProps } from "svelte";
  import type Icon from "./Icon.svelte";

  /** A choice in a menu: what it says, its sign and key, and what it does. */
  export type MenuItem = {
    label: string;
    icon?: ComponentProps<typeof Icon>["name"];
    /** Its key, shown at the end ("A"). */
    key?: string;
    /** It deletes or removes something: in the danger colour. */
    danger?: boolean;
    run: () => void;
  };
</script>

<script lang="ts">
  // A menu opened by a right click (or the menu key) on a row: the
  // library's VODs and edits (Library.svelte), the highlights pane's rows
  // (AnnotationsPane.svelte). Its first choice has the focus as it opens;
  // ↑ and ↓ (Home, End) move it, Enter picks, Escape or a click elsewhere
  // closes it, and the focus goes back where it was before.
  import { tick } from "svelte";
  import IconView from "./Icon.svelte";
  import Popover from "./Popover.svelte";

  let {
    open = $bindable(),
    anchor,
    label,
    items,
    width = 240,
  }: {
    open: boolean;
    /** What it's the menu of: it opens beside it. */
    anchor: HTMLElement | undefined;
    label: string;
    items: MenuItem[];
    width?: number;
  } = $props();

  let menu = $state<HTMLElement>();
  /** What had the focus before it opened, to give it back. */
  let before: HTMLElement | null = null;
  $effect(() => {
    if (!open) return;
    before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    void tick().then(() => menu?.querySelector<HTMLElement>("button")?.focus());
    return () => {
      // Closed with its focus gone (the panel went with it): back where it was.
      const to = before;
      void tick().then(() => {
        if (to?.isConnected && (!document.activeElement || document.activeElement === document.body)) to.focus();
      });
    };
  });

  function onKey(e: KeyboardEvent) {
    const buttons = [...(menu?.querySelectorAll<HTMLElement>("button") ?? [])];
    const at = buttons.indexOf(document.activeElement as HTMLElement);
    const to =
      e.key === "ArrowDown"
        ? (at + 1) % buttons.length
        : e.key === "ArrowUp"
          ? (at - 1 + buttons.length) % buttons.length
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? buttons.length - 1
              : null;
    if (to === null || !buttons.length) return;
    e.preventDefault();
    buttons[to].focus();
  }

  function pick(item: MenuItem) {
    open = false;
    // Once the click is done with: one that opens a panel of its own (Add to
    // edit…) would otherwise reach the window as a click outside that panel,
    // which closes it at once.
    setTimeout(item.run);
  }
</script>

<Popover bind:open {anchor} {label} {width} padding={4}>
  <div class="menu" role="menu" tabindex="-1" bind:this={menu} onkeydown={onKey}>
    {#each items as item (item.label)}
      <button role="menuitem" class:danger={item.danger} onclick={() => pick(item)}>
        {#if item.icon}<IconView name={item.icon} size={14} />{/if}
        {item.label}
        {#if item.key}<kbd>{item.key}</kbd>{/if}
      </button>
    {/each}
  </div>
</Popover>

<style>
  .menu {
    display: flex;
    flex-direction: column;
    outline: none;
  }

  .menu button {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-start;
    border: none;
    background: none;
    text-align: left;
  }

  .menu button:hover,
  .menu button:focus-visible {
    background: var(--surface-2);
  }

  .menu button.danger {
    color: var(--danger);
  }

  .menu kbd {
    margin-left: auto;
  }
</style>
