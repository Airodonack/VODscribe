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
  // ↑ and ↓ (Home, End) move it, Enter picks, and the focus goes back where
  // it was before once it closes. Escape closes it, as the window losing
  // the focus does and a press anywhere outside it, the row it's of
  // included. That press does nothing else, as in a file manager: a click
  // on a row closes the menu without opening or picking the row. A right
  // press goes on, so a right click on another row opens that row's menu.
  // It's as wide as its longest choice, `width` at the least.
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

  // Listened for on the document as events start (capture), so nothing
  // under the pointer that stops a press can keep the menu open.
  $effect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (menu?.closest(".popover")?.contains(e.target as Node)) return;
      open = false;
      if (e.button !== 2) swallowClick(e);
    };
    const onBlur = () => (open = false);
    document.addEventListener("pointerdown", onDown, true);
    window.addEventListener("blur", onBlur);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("blur", onBlur);
    };
  });

  /** Keeps press `down`, and the rest of its click, from the page. */
  function swallowClick(down: PointerEvent) {
    const rest = ["mousedown", "pointerup", "mouseup", "click"];
    const end = () => rest.forEach((type) => document.removeEventListener(type, stop, true));
    function stop(e: Event) {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.type === "click") end();
      // Let go away from where it was pressed, there's no click: it ends here.
      else if (e.type === "pointerup" || e.type === "mouseup") setTimeout(end);
    }
    rest.forEach((type) => document.addEventListener(type, stop, true));
    stop(down);
  }

  function pick(item: MenuItem) {
    open = false;
    // Once the click is done with: one that opens a panel of its own (Add to
    // edit…) would otherwise reach the window as a click outside that panel,
    // which closes it at once.
    setTimeout(item.run);
  }
</script>

<Popover bind:open {anchor} {label} {width} padding={4} grow>
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
