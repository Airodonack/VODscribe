<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import { portal } from "./portal";

  // A window of its own over the app (Settings): the rest is dimmed and out of
  // reach until it closes. It lives at the end of <body> (`portal`), as a
  // Popover does, so nothing clips it or draws over it. Escape, the close
  // button or a click on the dimmed backdrop closes it. Focus moves into it
  // when it opens, Tab goes round inside it, and focus goes back to what had
  // it (the button that opened it) when it closes. The backdrop is marked
  // data-over-screen, so a player's picture drawn over the page (Screen.svelte)
  // steps behind it and is dimmed with the rest.
  let {
    open = $bindable(),
    label,
    small = false,
    children,
  }: {
    open: boolean;
    label: string;
    /** A short question rather than a window: as tall as what it says. */
    small?: boolean;
    children: Snippet;
  } = $props();

  let panel = $state<HTMLElement>();
  const FOCUSABLE =
    'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
  const focusables = (within: HTMLElement | null | undefined = panel) =>
    within ? [...within.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null) : [];

  // Where the focus was before it opened, to give it back on close.
  let opener: HTMLElement | null = null;
  $effect(() => {
    if (!open) return;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // What the content marks data-autofocus (Settings' chosen section), or
    // else the first thing in it, rather than the close button.
    void tick().then(() => {
      const body = panel?.querySelector<HTMLElement>(".body");
      (body?.querySelector<HTMLElement>("[data-autofocus]") ?? focusables(body)[0] ?? panel)?.focus();
    });
    return () => {
      if (opener?.isConnected) opener.focus();
      opener = null;
    };
  });

  // Keys are taken on the window: focus can't be outside the panel while it's
  // open, but a press on the backdrop that isn't a click leaves it on <body>.
  function onKey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
      return;
    }
    if (e.key !== "Tab") return;
    const all = focusables();
    if (all.length === 0) {
      e.preventDefault();
      return;
    }
    const [first, last] = [all[0], all[all.length - 1]];
    const at = document.activeElement;
    if (e.shiftKey && (at === first || !panel?.contains(at))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (at === last || !panel?.contains(at))) {
      e.preventDefault();
      first.focus();
    }
  }

  // Closed by a click that started and ended on the backdrop: a drag that
  // selects text in the panel and lets go outside it isn't one.
  let downOnBackdrop = false;
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <div
    use:portal
    class="backdrop"
    data-over-screen
    role="presentation"
    onpointerdown={(e) => (downOnBackdrop = e.target === e.currentTarget)}
    onclick={(e) => {
      if (downOnBackdrop && e.target === e.currentTarget) open = false;
      downOnBackdrop = false;
    }}
  >
    <div bind:this={panel} class="modal" class:small role="dialog" aria-modal="true" aria-label={label} tabindex="-1">
      <header>
        <h2>{label}</h2>
        <button class="icon" onclick={() => (open = false)} aria-label="Close" title="Close (Esc)"
          ><Icon name="x" size={16} /></button
        >
      </header>
      <div class="body">
        {@render children()}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    /* Above everything: it's at the end of <body> (portal.ts). */
    z-index: 1000;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.4);
  }

  .modal {
    display: flex;
    flex-direction: column;
    width: min(760px, calc(100vw - 32px));
    height: min(560px, calc(100vh - 32px));
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .modal.small {
    width: min(440px, calc(100vw - 32px));
    height: auto;
    max-height: calc(100vh - 32px);
  }

  .modal:focus {
    outline: none;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 8px 8px 16px;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
</style>
