<script lang="ts">
  // Extra detail for whatever it wraps, shown on hover and on keyboard focus,
  // and read out by screen readers as the wrapped text's description.
  import type { Snippet } from "svelte";

  let { text, children }: { text: string; children: Snippet } = $props();
  const id = `tip-${Math.random().toString(36).slice(2, 9)}`;
</script>

<span class="tip-anchor">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <span class="target" tabindex="0" aria-describedby={id}>{@render children()}</span>
  <span class="tip" role="tooltip" {id}>{text}</span>
</span>

<style>
  .tip-anchor {
    position: relative;
    display: inline-flex;
  }

  .target {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    cursor: help;
    border-radius: 3px;
  }

  .tip {
    position: absolute;
    left: 0;
    top: calc(100% + 6px);
    z-index: 20;
    width: max-content;
    max-width: 320px;
    padding: 6px 9px;
    border-radius: var(--radius-sm);
    background: var(--text);
    color: var(--surface);
    font-size: 12px;
    font-weight: 400;
    line-height: 1.4;
    white-space: normal;
    box-shadow: var(--shadow);
    opacity: 0;
    visibility: hidden;
    transition: opacity 0.12s;
    pointer-events: none;
  }

  .tip-anchor:hover .tip,
  .target:focus + .tip {
    opacity: 1;
    visibility: visible;
  }
</style>
