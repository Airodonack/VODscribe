<script lang="ts">
  // What toasts.svelte.ts shows, at the bottom of the window.
  import Icon from "./Icon.svelte";
  import { toast } from "./toasts.svelte";
</script>

{#if toast.shown}
  {@const shown = toast.shown}
  <div class="toast" role="status">
    <span class="message">{shown.message}</span>
    {#if shown.action}
      <button
        class="act"
        onclick={() => {
          // Taken first: hiding it empties `shown`.
          const run = shown.action?.run;
          toast.hide();
          run?.();
        }}>{shown.action.label}</button
      >
    {/if}
    <button class="icon close" onclick={() => toast.hide()} title="Dismiss" aria-label="Dismiss">
      <Icon name="x" size={14} />
    </button>
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    left: 50%;
    bottom: 20px;
    transform: translateX(-50%);
    z-index: 50;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: min(560px, calc(100vw - 32px));
    padding: 6px 6px 6px 14px;
    border-radius: var(--radius);
    background: var(--text);
    color: var(--bg);
    font-size: 13px;
    box-shadow: var(--shadow);
  }

  .message {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .act {
    padding: 4px 10px;
    border: none;
    background: none;
    color: var(--accent-soft);
    font-weight: 600;
  }

  .act:hover:not(:disabled) {
    background: color-mix(in srgb, var(--bg) 15%, transparent);
  }

  .close {
    width: 26px;
    height: 26px;
    color: inherit;
    opacity: 0.7;
  }

  .close:hover:not(:disabled) {
    background: color-mix(in srgb, var(--bg) 15%, transparent);
    color: inherit;
    opacity: 1;
  }
</style>
