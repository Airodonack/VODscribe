<script lang="ts">
  // The live demo: the app's own interface (apps/desktop/src/lib/App.svelte),
  // answering from two VODs transcribed ahead of time. Controls that only work
  // in the app carry data-demo (see demo.ts there): they're grayed out, explain
  // themselves on hover or focus, and ignore clicks.
  import { onMount, type Component } from "svelte";

  let App = $state<Component | null>(null);
  let root = $state<HTMLElement>();
  // Phones get a screenshot instead (+page.svelte), so the app's code and the
  // demo's data only load once the window is wide enough to show it.
  onMount(() => {
    const wide = matchMedia("(min-width: 821px)");
    const load = async () => {
      if (wide.matches && !App) App = (await import("$desktop/App.svelte")).default;
    };
    load();
    wide.addEventListener("change", load);
    return () => wide.removeEventListener("change", load);
  });

  // Keys (Space, arrows, Ctrl+F) go to the demo only after a click into it,
  // and back to the page after a click outside (keysForApp in demo.ts).
  let active = $state(false);
  const onWindowPointerDown = (e: PointerEvent) => (active = !!root?.contains(e.target as Node));

  const EXAMPLES = ["far side of the moon", "heat shield", "toilet", "chat"];

  /** Types `phrase` into the demo's search box, as if the visitor had. */
  function tryPhrase(phrase: string) {
    const input = root?.querySelector<HTMLInputElement>('input[type="search"]');
    if (!input) return;
    input.value = phrase;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.focus();
  }

  let tip = $state<{ text: string; x: number; y: number } | null>(null);
  function showTip(e: Event) {
    const el = (e.target as Element).closest<HTMLElement>("[data-demo]");
    if (!el || !root) return void (tip = null);
    const box = el.getBoundingClientRect();
    const frame = root.getBoundingClientRect();
    tip = {
      text: el.dataset.demo ?? "",
      x: Math.min(Math.max(box.left + box.width / 2 - frame.left, 130), frame.width - 130),
      y: box.bottom - frame.top + 8,
    };
  }

  // Capture runs before the app's own handlers, so they never see these.
  // A form is blocked when the button submitting it is app-only.
  function block(e: Event) {
    const target = e instanceof SubmitEvent ? e.submitter : (e.target as Element);
    if (target?.closest("[data-demo]")) {
      e.preventDefault();
      e.stopPropagation();
    }
  }
</script>

<div class="examples">
  <span class="muted">Try searching</span>
  {#each EXAMPLES as phrase (phrase)}
    <button class="chip" onclick={() => tryPhrase(phrase)} disabled={!App}>{phrase}</button>
  {/each}
</div>

<svelte:window onpointerdown={onWindowPointerDown} />

<div
  class="window"
  data-demo-root
  data-active={active ? "" : undefined}
  bind:this={root}
  onpointerover={showTip}
  onfocusin={showTip}
  onpointerleave={() => (tip = null)}
  onfocusout={() => (tip = null)}
  onclickcapture={block}
  onsubmitcapture={block}
  role="region"
  aria-label="VODscribe demo"
>
  {#if App}
    <App />
  {:else}
    <div class="loading muted">Loading the demo…</div>
  {/if}
  {#if tip}
    <div class="tip" role="tooltip" style:left="{tip.x}px" style:top="{tip.y}px">{tip.text}</div>
  {/if}
</div>

<style>
  .examples {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 8px;
    margin-bottom: 14px;
    font-size: 14px;
  }

  .chip {
    border-radius: 999px;
    padding: 4px 12px;
    font-size: 13px;
  }

  .window {
    position: relative;
    /* The app fills this height (App.svelte). */
    --app-height: min(680px, 82vh);
    height: var(--app-height);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
    background: var(--bg);
    box-shadow: var(--shadow);
    text-align: left;
  }

  .loading {
    display: grid;
    place-items: center;
    height: 100%;
  }

  /* App-only controls. */
  .window :global([data-demo]) {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .tip {
    position: absolute;
    z-index: 30;
    transform: translateX(-50%);
    width: max-content;
    max-width: 260px;
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    background: var(--text);
    color: var(--surface);
    font-size: 12px;
    line-height: 1.4;
    box-shadow: var(--shadow);
    pointer-events: none;
  }
</style>
