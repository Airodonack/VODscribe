<script lang="ts">
  // The mark: a papyrus roll with a play triangle cut from its face.
  // `tile` draws it white on the app-icon gradient.
  let { size = 18, tile = false }: { size?: number; tile?: boolean } = $props();
</script>

{#if tile}
  <span class="tile" style:width="{size}px" style:height="{size}px" style:border-radius="{size * 0.22}px">
    <svg width={size * 0.72} height={size * 0.72} viewBox="0 0 64 64" fill="#fff" aria-hidden="true">
      <path fill-rule="evenodd" d="M12 18 h40 a6 6 0 0 1 6 6 v16 a6 6 0 0 1 -6 6 h-40 a6 6 0 0 1 -6 -6 v-16 a6 6 0 0 1 6 -6 z M27 24 v16 l13 -8 z"/>
      <path d="M6 14 a6 6 0 0 1 6 6 v24 a6 6 0 0 1 -6 6 a6 6 0 0 1 -4 -6 v-24 a6 6 0 0 1 4 -6 z"/>
      <path d="M58 14 a6 6 0 0 1 4 6 v24 a6 6 0 0 1 -4 6 a6 6 0 0 1 -6 -6 v-24 a6 6 0 0 1 6 -6 z"/>
    </svg>
  </span>
{:else}
  <svg width={size} height={size} viewBox="0 0 64 64" fill="currentColor" aria-hidden="true">
    <path fill-rule="evenodd" d="M12 18 h40 a6 6 0 0 1 6 6 v16 a6 6 0 0 1 -6 6 h-40 a6 6 0 0 1 -6 -6 v-16 a6 6 0 0 1 6 -6 z M27 24 v16 l13 -8 z"/>
    <path d="M6 14 a6 6 0 0 1 6 6 v24 a6 6 0 0 1 -6 6 a6 6 0 0 1 -4 -6 v-24 a6 6 0 0 1 4 -6 z"/>
    <path d="M58 14 a6 6 0 0 1 4 6 v24 a6 6 0 0 1 -4 6 a6 6 0 0 1 -6 -6 v-24 a6 6 0 0 1 6 -6 z"/>
  </svg>
{/if}

<style>
  svg {
    display: block;
    flex: none;
  }

  .tile {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, #4f46e5, #a78bfa);
  }
</style>
