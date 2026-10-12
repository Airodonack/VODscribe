<script lang="ts">
  // The live demo: the app's own interface (apps/desktop/src/lib/App.svelte),
  // answering from two VODs transcribed ahead of time. Controls that only work
  // in the app carry data-demo (see demo.ts there): they're grayed out, explain
  // themselves on hover, and ignore clicks.
  import { onMount, type Component } from "svelte";

  let App = $state<Component | null>(null);
  let root = $state<HTMLElement>();
  onMount(async () => {
    App = (await import("$desktop/App.svelte")).default;
  });

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
  function onPointerOver(e: PointerEvent) {
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
  function block(e: Event) {
    if ((e.target as Element).closest?.("[data-demo]")) {
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

<div
  class="window"
  data-demo-root
  bind:this={root}
  onpointerover={onPointerOver}
  onpointerleave={() => (tip = null)}
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
