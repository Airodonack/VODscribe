<script lang="ts">
  // The hint for DaVinci Resolve on Linux (formatHint.svelte.ts), at the top of the library.
  import { formatHint } from "./formatHint.svelte";
  import Icon from "./Icon.svelte";
</script>

{#if formatHint.shown}
  <div class="banner" role="status">
    <p>
      <Icon name="film" size={14} />
      <span
        ><strong>Using DaVinci Resolve?</strong> On Linux it can't read MP4 clips: they come in blank and silent. Clips made
        as MOV (DNxHR, about 350 MB a minute at 1080p) work there.</span
      >
    </p>
    <div class="actions">
      <button class="primary" onclick={() => formatHint.answer(true)}>Make clips as MOV</button>
      <button class="link" onclick={() => formatHint.answer(false)}>Keep MP4</button>
    </div>
    <p class="muted small">You can change this any time in Settings → Storage.</p>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 8px 12px 0;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--border));
    border-radius: var(--radius);
    background: var(--accent-soft);
    font-size: 13px;
  }

  p {
    display: flex;
    gap: 8px;
    margin: 0;
  }

  p :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--accent);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .small {
    font-size: 11px;
  }
</style>
